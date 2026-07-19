//! Memory map, peripheral wiring, IF/IE, OAM DMA, CGB extras.
//! Interconnect work package.
//!
//! Implements [`crate::cpu::Bus`]. Each `read`/`write`/`tick` advances every
//! peripheral by one M-cycle (PPU: 4 dots, 2 in CGB double speed) and then
//! performs the access. Owns: WRAM (banked on CGB), HRAM, IF/IE, OAM DMA
//! engine (bus conflicts included), CGB regs (KEY1 speed switch, VBK, SVBK,
//! HDMA/GDMA, BCPS/BCPD/OCPS/OCPD routing, OPRI, FF72-FF77), and the
//! per-model post-boot hardware state.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::cpu::Bus;
use crate::cycle_clock::{Conflict, CycleClock};
use crate::joypad::Joypad;
use crate::model::Model;
use crate::ppu::{OamBugKind, Ppu};
use crate::serial::Serial;
use crate::timer::Timer;

// Behavior-preserving submodules (each a second `impl Interconnect` block —
// except `bus`, which holds the `impl Bus for Interconnect` trait impl, and
// `phase`, which holds the eighth-grid sub-dot access machinery —
// EdgeKind/event_phase/edge_eighth/stamp_blocks/ACCESS_PHASE — as free
// items, re-exported below). The struct, its fields and the free helpers
// stay here.
mod accessors;
mod boot;
mod boot_rom;
mod bus;
mod cycle;
mod debug;
mod hdma;
mod link;
mod memory;
mod oam_dma;
mod phase;
mod speed;
mod state;
mod tick;

// Private re-export of the sub-dot phase machinery so the parent's own
// references and the sibling submodules' `use super::*` keep resolving these
// as unqualified names.
use phase::*;

pub use debug::CdlRange;

/// The five implemented interrupt sources: IF/IE bits 0-4 (VBlank, STAT,
/// timer, serial, joypad). Bits 5-7 of FF0F/FFFF are unmapped (Pan Docs
/// "Interrupts").
const IF_MASK: u8 = 0x1F;
/// IF bit 1 (STAT), for the line-0 OAM-rise dispatch-late mask.
const IF_STAT_BIT: u8 = 0x02;

/// OAM DMA source classes (gambatte-core memptrs.h `OamDmaSrc`, classified
/// from the FF46 page by memory.cpp `oamDmaInitSetup`). The class decides
/// what the engine reads and which address pages a CPU access conflicts
/// with while a byte is in flight (gbctr "OAM DMA": the DMA controller
/// drives the bus it reads from).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmaSrcKind {
    /// FF46 $00-$7F.
    Rom,
    /// FF46 $80-$9F.
    Vram,
    /// FF46 $A0-$BF.
    Sram,
    /// FF46 $C0-$DF, plus $E0-$FF on the DMG family (incomplete address
    /// decoding re-reads WRAM there, acceptance/oam_dma/sources-GS).
    Wram,
    /// FF46 $E0-$FF on CGB/AGB: the engine reads the idle bus value $FF
    /// (gambatte memory.cpp `oamDmaSrcPtr` → `rdisabledRam`).
    Invalid,
}

/// An OAM DMA transfer in progress: `idx` is the next byte to copy (one per
/// M-cycle).
#[derive(Clone, Copy)]
struct OamDmaRun {
    src: u16,
    idx: u8,
}

/// Conflict state of the current M-cycle: a DMA byte was copied, and CPU
/// accesses to conflicting pages observe or derail it (see
/// [`Interconnect::read_no_tick`] / [`Interconnect::write_no_tick`]).
#[derive(Clone, Copy)]
struct DmaConflict {
    kind: DmaSrcKind,
    /// FF46 of the running transfer; bit 4 selects the WRAM page of the
    /// CGB WRAM-region redirect.
    src_hi: u8,
    /// OAM index the byte was committed to this cycle.
    idx: u8,
    /// The byte the DMA engine drove onto the bus (= the byte committed).
    byte: u8,
}

/// A freshly written FF46 value waiting out its 1 M-cycle setup delay
/// (acceptance/oam_dma_start: the cycle after the write still reads OAM).
#[derive(Clone)]
struct OamDmaStart {
    src: u16,
    delay: u8,
}

/// HBlank DMA arming, mirroring gambatte-core's `memevent_hdma` time
/// encoding (video.cpp `enableHdma`/`disableHdma`/`lcdcChange`):
/// `disabled_time` = off, `disabled_time - 1` = armed with the LCD off,
/// a real mode-0 time = armed with the LCD on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HdmaMode {
    Disabled,
    ArmedLcdOff,
    ArmedLcdOn,
}

/// A flagged VRAM-DMA request, serviced at the head of the CPU's next bus
/// operation (gambatte-core `flagHdmaReq`/`flagGdmaReq` set the
/// `intevent_dma` event; see [`Interconnect::service_vram_dma`] for the
/// exact seam the `_1`/`_2` ROM pairs pin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VramDmaReq {
    /// One 16-byte HBlank block.
    Hblank,
    /// One 16-byte HBlank block re-flagged by a halt/stop wake: it skips
    /// the teardown M-cycle (gambatte Memory::event `intevent_dma`:
    /// `cc -= 4` when `haltHdmaState_ == hdma_requested`).
    HblankUnhalt,
    /// The whole remaining length at once.
    Gdma,
}

/// HBlank-DMA bookkeeping across a halt (HALT, deep STOP, or the
/// speed-switch pause) — gambatte-core memory.h `HdmaState`. While the
/// core clock is gated the LCD's mode-0 entries do not flag block requests
/// (video.h `EventTimes::flagHdmaReq` is suppressed while halted); this
/// records what the wake must re-evaluate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HaltHdmaState {
    /// Halt began outside the hblank window: a wake landing inside one
    /// fires a block.
    Low,
    /// Halt began *inside* the hblank window: the same hblank must not
    /// retrigger at wake.
    High,
    /// A flagged block request was deferred by the halt; the wake
    /// re-flags it ([`VramDmaReq::HblankUnhalt`]).
    Requested,
}

#[derive(Clone)]
pub struct Interconnect {
    model: Model,
    cart: Cartridge,
    ppu: Ppu,
    apu: Apu,
    timer: Timer,
    serial: Serial,
    joypad: Joypad,
    /// Elapsed T-cycles since power-on (normal-speed dots).
    cycles: u64,
    /// Of [`Self::cycles`], those elapsed while the CPU was HALT-gated — a
    /// read-only diagnostic for the frontend's GB-CPU-usage meter. Bumped in
    /// lockstep with `cycles` inside `tick_machine` when `cpu_halted`, so it
    /// cannot perturb emulation (no emulated value or control flow reads it);
    /// deliberately NOT serialized, so save-states stay byte-identical and it
    /// simply restarts from 0 on load.
    halt_cycles: u64,
    /// CPU-side deferred-commit clock (SameBoy `pending_cycles`,
    /// `sm83_cpu.c`). Every CPU-driven M-cycle (the five [`Bus`] access
    /// methods) parks its 4 T-cycles here, commits the previous M-cycle's debt
    /// at the leading edge, and drains at the instruction boundary via
    /// [`Bus::flush_pending`]. Counts pure CPU T-cycles (4 per M-cycle in
    /// *both* speeds — the double-speed factor lives in the PPU/APU domain, not
    /// here; `cycle_clock.rs` module doc). Advanced only by the CPU's own
    /// M-cycles, never by OAM-DMA / HDMA / STOP-pause stolen ticks (those call
    /// `tick_machine` directly, not through `Bus`).
    clock: CycleClock,
    /// A CGB single-speed WriteCpu-conflict engine write (FF41/FF0F/FF45) just
    /// committed one PPU dot into the next M-cycle (SameBoy `GB_CONFLICT_
    /// WRITE_CPU` lands the CPU value 1 T past the M-cycle boundary). The write
    /// borrowed that dot ahead of `write_no_tick`, so the next `tick_machine`
    /// ticks 3 PPU dots (skip cc 1) to restore CPU/PPU phase.
    eager_wr_borrow: bool,

    /// CGB hardware running a CGB-flagged cart. CGB hardware with a DMG
    /// cart runs in DMG compatibility mode: KEY1/SVBK/HDMA/RP/FF74 and the
    /// palette data ports are disabled (misc/boot_hwio-C).
    cgb_mode: bool,
    double_speed: bool,
    /// CPU↔PPU sub-dot phase for the cc-granular reclock: which cc's of the
    /// M-cycle tick a PPU dot in double speed (see [`dot_ticks_on_cc`]). 0 = the
    /// fixed even-cc {2,4} alignment (single speed is phase-independent); 1 =
    /// the odd-cc {1,3} offset a STOP speed switch can establish. Held at 0:
    /// setting it at the speed switch lifts zero gambatte DS rows
    /// (`cc_grid_matches_dot_loop` confirms 0 is bit-identical to the dot loop);
    /// the m3stat/speedchange `_2` reads need the pixel-pipe END dot to move,
    /// not the M-cycle sample phase.
    dot_phase: u8,
    /// KEY1 bit 0: speed switch armed for the next STOP.
    key1_armed: bool,

    /// 0x2000 bytes on DMG, 8 banks of 0x1000 on CGB.
    wram: Vec<u8>,
    /// SVBK as written (3 bits); bank 0 acts as bank 1.
    svbk: u8,
    hram: [u8; 0x7F],
    /// IF, low 5 bits (upper 3 read 1).
    intf: u8,
    /// IE, all 8 bits stored and readable.
    ie: u8,
    /// Timer IF bits committed in the *second half* of the current (most
    /// recent) M-cycle: the halt-exit sampling misses them until the next
    /// cycle, while IF reads and the running CPU's end-of-fetch sampling
    /// see them immediately ([`Bus::pending_halt_wake`]; `Timer::tick`'s
    /// `late`).
    if_late: u8,
    /// Inert deferred-clock scratch (the removed −1 TIMA-phase mode-0 STAT
    /// halt-wake delay). Never set on the current clock; kept for savestate
    /// stability.
    m0_halt_hold: u8,
    /// Inert deferred-clock scratch (the removed exact-T timer/serial
    /// ack-squash deadline; the live squash uses the `ack_squash_*` tick/dot
    /// counters below). Never set now; kept for savestate stability.
    ack_squash_deadline_t: u64,
    /// Inert deferred-clock scratch (the removed sub-M-cycle wake-skew clock
    /// offset). Never set now; kept for savestate stability.
    wake_skew: u32,
    /// Inert deferred-clock scratch (the removed per-T machine clock). Never
    /// set now; kept for savestate stability.
    machine_now: u64,
    /// Inert deferred-clock scratch (the removed pre-write VRAM-DMA-request
    /// latch). Never set now; kept for savestate stability.
    vram_dma_req_pre: bool,
    /// Inert deferred-clock scratch (the removed mode-0 STAT halt-wake
    /// visibility deadline in machine T). Never set now; kept for savestate
    /// stability.
    stat_vis_from_t: u64,
    /// Inert deferred-clock scratch (the removed post-mode-0-halt-wake FF44
    /// read-phase carry). Never set now; kept for savestate stability.
    halt_ly_phase: u8,
    /// STAT IF bit raised by the PPU in the *current* M-cycle's second
    /// half (line-0 OAM rise): readable via FF0F at once, but the CPU's
    /// interrupt sample for this cycle must not see it
    /// (`Ppu::take_stat_late`).
    if_stat_late: u8,
    /// The mode-3→mode-0 OAM accessibility unblock for the current M-cycle
    /// (`Ppu::take_m0_access_flip`): a CPU OAM read samples at the cc+2 MID
    /// phase, two dots before this M-cycle's end view, so it still reads mode 3
    /// ($FF) when the unblock lands late (gambatte `oam_access/postread_*`).
    /// Routes only the OAM read; every other access keeps the end view. Stamped
    /// with the flip's dot-END commit eighth ([`edge_eighth`]; `None` = no flip
    /// this M-cycle); a read blocks when its observer phase precedes the stamp
    /// ([`stamp_blocks`]).
    m0_access_edge: Option<u8>,
    /// As `m0_access_edge` but for the CGB palette-RAM unblock (anchored at
    /// the pipe end / `render_finished`, one dot after the m0 flip). Unlike
    /// OAM/VRAM, the palette unblock commits at the M-cycle END
    /// ([`event_phase`] gives `PalAccess` phase 8 = the whole-M-cycle block):
    /// a cc+2 MID-phase FF69/FF6B read reads $FF for the
    /// ENTIRE straddle M-cycle, not just its second half (gambatte
    /// `cgbpal_m3end` `scx2_1`/`scx5_1`/`scx5_ds_1`). See `Ppu::pal_access_flip`.
    pal_access_edge: Option<u8>,
    /// The mode-3→mode-0 STAT mode-bit flip's dot-END commit eighth, or
    /// `None` when no flip lands this M-cycle (`Ppu::take_m0_stat_flip`,
    /// gated to sprite-extended lines): a CPU FF41 read samples the mode bits
    /// at the cc+2 MID phase, so in double speed it still reads mode 3 when
    /// the flip lands late (gambatte sprites `m3stat_ds_1`). The FF41 override
    /// consults this in double speed only. See `Ppu::m0_stat_flip`.
    stat_mode_edge: Option<u8>,
    /// Dispatch-ack source sync-ahead (gambatte-core memory.cpp
    /// `Memory::ackIrq`): a dispatch's IF clear lands slightly *into* the
    /// low-push M-cycle, so it also consumes a re-set of the acked source just
    /// after the ack — `updateSerial(cc + 3 + isCgb())`,
    /// `updateTimaIrq(cc + 2 + isCgb())`, `lcd_.update(cc + 2)` run before
    /// `intreq_.ackIrq(bit)`. On the tick-then-access grid: a timer/serial set
    /// from the next tick(s) and a STAT/VBlank rise in the first 2 dots of the
    /// next tick are swallowed by the preceding [`Bus::ack`]. The single-speed
    /// 2-dot LCD window deliberately does NOT reach the line-anchored rises'
    /// second-half emission dots (m2int_m2irq_late_retrigger_1,
    /// late_m0irq_retrigger_scx1_1 pin the keeps); in double speed it spans the
    /// whole tick, flipping the `*_late_retrigger_ds_2` rows. Pinned by gambatte
    /// tima/tc00_irq_late_retrigger_2/3 (dmg08_outE4 vs cgb04c_outE0),
    /// serial/start_wait_trigger_int8_read_if_2/3, and the
    /// irq_precedence/m1/ly0/lyc153int `*_late_retrigger` rows.
    /// `ack_squash_mask` is the acked source's IF bit (only that source is
    /// consumed — others get *flagged*, which the per-tick OR already models);
    /// `ack_squash_ticks`/`ack_squash_dots` are the remaining windows.
    ack_squash_mask: u8,
    ack_squash_ticks: u8,
    ack_squash_dots: u8,

    /// Inert deferred-clock scratch (the removed −2 dispatch reclock's timer/
    /// serial squash latch); the live squash is recomputed per `tick_machine`
    /// call. Never set now; kept for savestate stability.
    deferred_squash: u8,

    /// FF46 readback is simply the last written value
    /// (acceptance/oam_dma/reg_read).
    dma_reg: u8,
    dma_run: Option<OamDmaRun>,
    dma_start: Option<OamDmaStart>,
    /// A transfer owned OAM at the head of the previous M-cycle — the
    /// one-cycle trailing edge of the PPU scan-disconnect level (see
    /// [`Self::oam_dma_tick`]).
    dma_oam_owned_prev: bool,
    /// The byte copied by this M-cycle's controller advance, committed to
    /// OAM at the head of the next one (gambatte's end-of-cycle copy
    /// timestamp — see [`Self::oam_dma_commit_pending`]). A conflicted
    /// CPU write derails into this slot before it lands.
    dma_pending_oam: Option<(u8, u8)>,
    /// CPU core clock gated off by HALT/STOP (see [`Self::set_cpu_halted`]).
    /// The OAM DMA controller shares that clock and freezes with it.
    cpu_halted: bool,
    /// Set while a DMA byte is copied during the current M-cycle.
    dma_conflict: Option<DmaConflict>,
    /// FEA0-FEFF on CPU CGB C ([`Model::Cgb`]): 24 bytes of extra OAM RAM,
    /// mirrored 4 times because low-address bits 3-4 don't decode (Pan
    /// Docs "FEA0-FEFF range" revisions 0-D; gambatte-core memory.cpp
    /// indexes `(addr - 0xFE00) & 0xE7`). AGB and the DMG family never
    /// touch this (see [`Self::prohibited_read`]).
    extra_oam: [u8; 24],

    // CGB VRAM DMA (FF51-FF55).
    /// Live source address counter as assembled from HDMA1/2 writes and
    /// advanced by the engine (gambatte memory.cpp `dmaSource_`).
    hdma_src: u16,
    /// Live destination counter, FULL 16 bits: only the VRAM write masks
    /// it to 0x1FFF, and the transfer terminates when the counter crosses
    /// 0x10000 (gambatte `dmaDestination_`; SameBoy `hdma_current_dest`).
    hdma_dst: u16,
    /// FF55 as the live register (gambatte `ioamhram_[0x155]`): bits 0-6 =
    /// remaining blocks - 1, bit 7 set = no HBlank transfer registered
    /// (completion, cancel, or halted abort). Reads back verbatim.
    hdma5: u8,
    /// HBlank DMA arming (see [`HdmaMode`]).
    hdma_mode: HdmaMode,
    /// Flagged block/GDMA request awaiting the next bus operation.
    vram_dma_req: Option<VramDmaReq>,
    /// HBlank-DMA state across a core-clock gate (see [`HaltHdmaState`]).
    halt_hdma: HaltHdmaState,
    /// Previous `hblank_active` level for the per-dot mode-0 edge detector.
    hdma_prev_hblank: bool,
    /// Re-entrancy guard: a VRAM DMA transfer is stalling the CPU and
    /// ticking the machine internally.
    vram_dma_stall: bool,
    /// Set for the stolen byte-copy M-cycles of a VRAM DMA service (not
    /// the teardown cycle): the VRAM DMA owns the bus, so the OAM DMA
    /// controller performs no source reads of its own — it advances by
    /// latching the VRAM DMA's bus traffic instead (gambatte memory.cpp
    /// `dma()` sets `lastOamDmaUpdate_ = disabled_time` around its copy
    /// loop and advances `oamDmaPos_` inline; see
    /// [`Self::oam_dma_bus_capture`]).
    vram_dma_owns_bus: bool,

    // CGB misc registers.
    /// FF56 RP bits 0/6/7 as written. No IR peer is modelled: bit 1
    /// ("received signal") always reads 1 (= not receiving).
    rp: u8,
    /// FF72/FF73: fully readable/writable scratch (exist on CGB in both
    /// modes, boot_hwio-C).
    ff72: u8,
    ff73: u8,
    /// FF74: scratch, CGB mode only (reads $FF in DMG mode).
    ff74: u8,
    /// FF75: bits 4-6 writable, others read 1.
    ff75: u8,

    // ---- Debugger-only state (all inert on every golden/test path) ----
    /// Memory watchpoints (bgb's "Set watchpoint", RM8). Empty by default —
    /// an empty list is a zero-overhead no-op in `check_access`, so the golden
    /// path is byte-identical.
    watchpoints: Vec<crate::Watchpoint>,
    /// The most recent watchpoint hit address, consumed by the run loop.
    watch_hit: Option<u16>,
    /// The execution profiler tally (bgb's logging mode, MB5). `None` (off) by
    /// default; a `None` tally makes `profile_pc` a no-op, so golden is
    /// byte-identical.
    prof: Option<std::collections::BTreeMap<u16, u64>>,
    /// FCEUX-style bank-aware code/data log: physical-offset flag buffer
    /// (R=1/W=2/X=4), sized to the machine (ROM|VRAM|SRAM|WRAM|tail — see
    /// `cdl_layout`), or `None` when off. A `None` log makes every CDL hook a
    /// no-op, so the golden path is byte-identical. Excluded from save-state
    /// (live UI state).
    cdl: Option<Box<[u8]>>,
    /// Profiler break mode: halt the free run on each address's first execution.
    prof_break: bool,
    /// The pending profiler break hit address, consumed by the run loop.
    prof_break_hit: Option<u16>,
    /// Exception-break mask (bgb's Options → Exceptions, the `EXC_*` bits).
    /// `0` (disarmed) by default — every exec/access check early-outs, so the
    /// golden path is byte-identical.
    exc_mask: u16,
    /// The pending exception-break hit address, consumed by the run loop.
    exc_hit: Option<u16>,

    // ---- Opt-in boot ROM (golden-safe: `boot_active` false by default) ----
    /// A boot ROM attached by `GameBoy::new_with_boot`, overlaying the low cart
    /// region until FF50. `None` on every `new` (no-boot) / golden path.
    boot_rom: Option<Vec<u8>>,
    /// Whether the boot ROM is currently mapped (false unless a boot ROM was
    /// attached and has not yet written FF50).
    boot_active: bool,
}

/// DMG-compat palettes installed by the CGB boot ROM for DMG carts. The
/// boot ROM consults its title-checksum lookup table only when the licensee
/// is Nintendo (old licensee byte $14B == $01, or $33 with "01" at
/// $144-$145); every other cart gets this *default* combination — entries
/// OBJ0=4, OBJ1=4, BG=29 of SameBoy's BootROMs/cgb_boot.asm palette tables
/// (Pan Docs "Compatibility palettes"). The per-game hash table is
/// deliberately not modelled; if it ever is, gate it on the licensee check
/// first or non-Nintendo homebrew will mis-color.
const CGB_COMPAT_BG_PALETTE: [u16; 4] = [0x7FFF, 0x1BEF, 0x6180, 0x0000];
const CGB_COMPAT_OBJ_PALETTE: [u16; 4] = [0x7FFF, 0x421F, 0x1CF2, 0x0000];

impl Interconnect {
    pub fn new(model: Model, cart: Cartridge) -> Self {
        // CGB mode iff the hardware is a CGB/AGB *and* the cart opts in via
        // header byte 0x143 bit 7 (same predicate `GameBoy::auto_model`
        // uses: `cartridge::cgb_flag`).
        let cgb_mode = model.is_cgb() && cart.supports_cgb();
        // SGB packet/multiplayer port: SGB-family hardware with a cart
        // whose header unlocks SGB functions (Pan Docs "SGB flag").
        let sgb_joypad = matches!(model, Model::Sgb | Model::Sgb2) && cart.supports_sgb();
        let mut ppu = Ppu::new(model);
        ppu.set_dmg_compat(model.is_cgb() && !cgb_mode);
        Self {
            model,
            cart,
            ppu,
            apu: Apu::new(model.is_cgb()),
            timer: Timer::new(),
            // The serial fast-clock bit (SC bit 1) exists in CGB mode only;
            // in DMG compatibility mode SC reads $7E (misc/boot_hwio-C).
            serial: Serial::new(cgb_mode),
            joypad: Joypad::new(sgb_joypad),
            cycles: 0,
            halt_cycles: 0,
            clock: CycleClock::new(),
            eager_wr_borrow: false,
            cgb_mode,
            double_speed: false,
            dot_phase: 0,
            key1_armed: false,
            wram: vec![0; if model.is_cgb() { 0x8000 } else { 0x2000 }],
            svbk: 0,
            hram: [0; 0x7F],
            intf: 0,
            ie: 0,
            if_late: 0,
            m0_halt_hold: 0,
            ack_squash_deadline_t: 0,
            wake_skew: 0,
            machine_now: 0,
            vram_dma_req_pre: false,
            stat_vis_from_t: 0,
            halt_ly_phase: 0,
            if_stat_late: 0,
            m0_access_edge: None,
            pal_access_edge: None,
            stat_mode_edge: None,
            ack_squash_mask: 0,
            ack_squash_ticks: 0,
            ack_squash_dots: 0,
            deferred_squash: 0,
            dma_reg: 0,
            dma_run: None,
            dma_start: None,
            dma_oam_owned_prev: false,
            dma_pending_oam: None,
            cpu_halted: false,
            dma_conflict: None,
            extra_oam: [0; 24],
            hdma_src: 0,
            hdma_dst: 0,
            hdma5: 0xFF,
            hdma_mode: HdmaMode::Disabled,
            vram_dma_req: None,
            halt_hdma: HaltHdmaState::Low,
            hdma_prev_hblank: false,
            vram_dma_stall: false,
            vram_dma_owns_bus: false,
            rp: 0,
            ff72: 0,
            ff73: 0,
            ff74: 0,
            ff75: 0,
            watchpoints: Vec::new(),
            watch_hit: None,
            prof: None,
            cdl: None,
            prof_break: false,
            prof_break_hit: None,
            exc_mask: 0,
            exc_hit: None,
            boot_rom: None,
            boot_active: false,
        }
    }
}

/// Zero bits among the bytes the SGB boot ROM transfers to the SNES: six
/// 16-byte packets, each a command byte ($F1 + 2×packet), a checksum byte
/// (8-bit sum of the payload) and 14 payload bytes from $0104 + 14×packet
/// (addresses ≥ $0150 read as $00).
///
/// Each zero bit costs one extra M-cycle of boot time relative to a one
/// bit. Derivation — the SGB boot ROM's per-bit send loop, $0095-$00A5 in
/// the dumped ROM (sha1 aa2f50a77dfb4823da96ba99309085a3c6278515), clocks
/// each bit out to the SNES via JOYP P14/P15:
///
/// ```text
/// $0095  BIT 0,D      ; 2 M-cycles
/// $0097  LD A,$10     ; 2          (P15 low = "1" bit)
/// $0099  JR NZ,$009D  ; 3 taken (one bit) / 2 not taken (zero bit)
/// $009B  LD A,$20     ; 2          (P14 low = "0" bit), zero path only
/// $009D  LDH (C),A    ; pulse, then $30 restores both lines high
/// ```
///
/// A one bit takes 2+2+3 = 7 M-cycles to reach the pulse; a zero bit
/// takes 2+2+2+2 = 8 — exactly one M-cycle (4 DIV T-cycles) more. The
/// per-packet reset and stop pulses are unconditional and sit in the DIV
/// base value instead. Verified against acceptance/boot_div-S and
/// boot_div2-S, which differ only in the global checksum bytes
/// (`model::tests::sgb_div_base_matches_both_checksum_roms`).
fn sgb_header_zero_bits(cart: &Cartridge) -> u32 {
    let mut zeros = 0;
    for packet in 0..6u16 {
        let cmd = 0xF1 + 2 * packet as u8;
        let mut sum = 0u8;
        for i in 0..14 {
            let addr = 0x104 + 14 * packet + i;
            let byte = if addr < 0x150 { cart.read_rom(addr) } else { 0 };
            sum = sum.wrapping_add(byte);
            zeros += byte.count_zeros();
        }
        zeros += cmd.count_zeros() + sum.count_zeros();
    }
    zeros
}

#[cfg(test)]
#[path = "interconnect_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "interconnect_pcm_probe.rs"]
mod pcm_decay_probe;
