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
use crate::joypad::Joypad;
use crate::model::Model;
use crate::ppu::{OamBugKind, Ppu};
use crate::serial::Serial;
use crate::timer::Timer;

// Behavior-preserving submodules (each a second `impl Interconnect` block).
// The struct, its fields, the sub-dot access machinery (EdgeKind/event_phase/
// edge_eighth/stamp_blocks/ACCESS_PHASE) and the free helpers stay here.
mod boot;
mod debug;
mod hdma;
mod memory;
mod oam_dma;
mod state;
mod tick;

/// The five implemented interrupt sources: IF/IE bits 0-4 (VBlank, STAT,
/// timer, serial, joypad). Bits 5-7 of FF0F/FFFF are unmapped (Pan Docs
/// "Interrupts").
const IF_MASK: u8 = 0x1F;
/// IF bit 1 (STAT), for the line-0 OAM-rise dispatch-late mask.
const IF_STAT_BIT: u8 = 0x02;

/// Eighth-grid sub-cc phase model. An M-cycle spans 4 cc = 8 *eighths*; PPU
/// events commit and CPU observers sample at sub-cc phases within it.
/// `MID_PHASE` is the cc+2 observer phase (the M-cycle midpoint a
/// tick-then-access read effectively samples at — gambatte's access offset
/// two dots before our cc+4 end-sampled view, which is the full M-cycle = 8
/// eighths). See [`edge_eighth`] / [`obs_pre_edge`] and the dot-loop in
/// [`Interconnect::tick_machine`].
const MID_PHASE: u8 = 4;

/// The M-cycle END phase (cc+4 = 8 eighths) — [`edge_eighth`]'s last-dot value
/// for both speeds. An event committing here is past every observer (it blocks
/// the whole M-cycle and is visible only next M-cycle); the CGB palette unblock
/// commits here (`event_phase(EdgeKind::PalAccess, ..)`, INC-G3 task 5).
const END_PHASE: u8 = 8;

/// The dot-END commit phase (in eighths of an M-cycle) of an event that
/// fired on dot `i` of a `dots`-dot M-cycle (`dots` = 4 single speed / 2
/// double speed). Single speed → {2,4,6,8}; double speed → {4,8}. The
/// edge commits at the end of its dot, so a later increment adds a small
/// negative offset (e.g. −1 eighth) to model an edge that leads the dot end.
#[inline]
fn edge_eighth(i: u64, dots: u64) -> u8 {
    // `dots` is the PPU-dots-per-M-cycle, structurally 4 (single speed) or 2
    // (double speed); the eighth table {2,4,6,8}/{4,8} relies on it.
    debug_assert!(dots == 2 || dots == 4, "dots must be 2 or 4, got {dots}");
    ((i + 1) * 8 / dots) as u8
}

/// Whether a whole PPU dot ticks on cc `cc` (1..=4) of an M-cycle, at the
/// given speed and CPU↔PPU `dot_phase`. The cc-granular successor to the fixed
/// `for i in 0..dots` dot loop: single speed ticks one dot per cc (4 dots per
/// M-cycle, phase-independent — 1 cc = 1 dot); double speed ticks one dot per
/// 2 cc (2 dots per M-cycle). In double speed `phase`=0 ticks on the even cc
/// {2,4} — the alignment the old loop baked in — and `phase`=1 on the odd cc
/// {1,3}, the half-dot (1 cc) offset a STOP speed switch can establish because
/// the LCD dot clock runs on continuously across the switch while the CPU's
/// M-cycle grid is re-paced. Phase 0 is bit-identical to the dot loop
/// (`cc_grid_matches_dot_loop`).
#[inline]
fn dot_ticks_on_cc(cc: u8, ds: bool, phase: u8) -> bool {
    debug_assert!((1..=4).contains(&cc), "cc must be 1..=4, got {cc}");
    !ds || cc % 2 == phase % 2
}

/// The commit eighth (of 8 per M-cycle) of an event on the dot that ticks at
/// cc `cc` (1..=4). The cc grid IS the single-speed dot grid — cc is the
/// single-speed dot index + 1 — so the eighth is the single-speed dot-END
/// [`edge_eighth`]: `cc*2` → {2,4,6,8}. Double speed selects a 2-cc subset of
/// these per [`dot_ticks_on_cc`] (phase 0 → the even cc, eighths {4,8} = today;
/// phase 1 → the odd cc, eighths {2,6} = the half-dot offset the whole-dot loop
/// could never place). At `dot_phase` 0 the dot-tick cc's reproduce
/// [`edge_eighth`]'s per-`i` sequence exactly (`cc_grid_matches_dot_loop`).
#[inline]
fn cc_eighth(cc: u8) -> u8 {
    debug_assert!((1..=4).contains(&cc), "cc must be 1..=4, got {cc}");
    edge_eighth(u64::from(cc) - 1, 4)
}

/// Whether an observer sampling at phase `obs` (eighths) precedes the event
/// committing at phase `edge` — i.e. the observer sees the pre-commit state.
/// For accessibility/STAT reads that means "still blocked / pre-flip"; for
/// the mode-0 IRQ rise it means "the halt-exit sampler misses the rise this
/// M-cycle". Bit-identical to the legacy `2 * (i + 1) > dots` half-split when
/// `obs == MID_PHASE` (see `eighth_grid_predicate_matches_half_split`).
#[inline]
fn obs_pre_edge(obs: u8, edge: u8) -> bool {
    obs < edge
}

/// Whether a CPU read/write observing at phase `obs` (eighths) is still
/// blocked by a per-M-cycle accessibility/STAT edge stamped at its dot-END
/// commit eighth (`Some(edge)` from [`edge_eighth`]; `None` = no edge this
/// M-cycle). The edge-stamp replaces the old precomputed boolean: storing the
/// raw commit eighth (rather than `obs_pre_edge(MID_PHASE, edge)`) is what lets
/// an EVENT carry its own sub-dot position via [`event_phase`] — the INC-G3
/// discriminator between read chains, since every CPU access observes at the
/// one [`ACCESS_PHASE`] (the reverted G2c per-read-chain observer phase was the
/// wrong premise). `stamp_blocks(Some(edge), MID_PHASE)` is bit-identical to
/// the legacy half-split for every dot/speed (`stamp_blocks_matches_half_split`).
#[inline]
fn stamp_blocks(stamp: Option<u8>, obs: u8) -> bool {
    stamp.is_some_and(|edge| obs_pre_edge(obs, edge))
}

/// The boundary events that commit a per-M-cycle sub-cc edge. Each PPU edge
/// commits at its own dot-END eighth today ([`event_phase`] returns
/// [`edge_eighth`] for every kind — net-zero), so the kinds are
/// interchangeable; the enum is the seam a later INC-G3 increment uses to give
/// one event its own sub-dot lead/lag (the cc-exact boundary positions from
/// the gambatte xpos formulas — e.g. the CGB palette unblock trails the mode-0
/// IRQ rise by a half-dot, m0Time=xpos+7 vs IRQ+6) without recalibrating the
/// dot-clocked pixel pipe or the other events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeKind {
    /// The mode-0 STAT IRQ rise (consumed by the halt-exit sampler, not stamped).
    M0Rise,
    /// The OAM/VRAM mode-3→mode-0 accessibility unblock (`m0_access_edge`).
    M0Access,
    /// The CGB palette-RAM pipe-end unblock (`pal_access_edge`).
    PalAccess,
    /// The double-speed FF41 STAT mode-bit flip (`stat_mode_edge`); commits at
    /// the whole-M-cycle END phase (INC-G3 task 6), like `PalAccess`.
    StatMode,
}

/// The commit phase (eighths of an M-cycle) of boundary event `kind` on the
/// dot that ticks at cc `cc` (1..=4 — see [`dot_ticks_on_cc`]). Most kinds
/// commit at their dot-END eighth ([`cc_eighth`]); `PalAccess`/`StatMode` are
/// calibrated off it (INC-G3 tasks 5/6) — their per-event offset is exactly
/// what [`EdgeKind`] keys, so the others stay dot-clocked while one event
/// moves. cc-granular: the cc the event's dot ticks on already carries the
/// sub-dot (`dot_phase`) offset, so no separate `i`/`dots` parameter.
#[inline]
fn event_phase(kind: EdgeKind, cc: u8) -> u8 {
    match kind {
        // The CGB palette-RAM unblock commits at the M-cycle END (phase 8 =
        // cc+4), one observer grid later than OAM/VRAM's dot-split: a cc+2 MID
        // FF69/FF6B read stays blocked for the ENTIRE straddle M-cycle and reads
        // $FF until the next M-cycle, regardless of which dot lx==160 lands on.
        // The dot-split half-classification under-blocked the geometries where
        // lx==160 falls in the M-cycle's first half — gambatte cgbpal_m3end
        // scx2_1/scx5_1/scx5_ds_1 (out7) pin the late effect across SCX. The
        // palette unblock physically lags the pixel-pipe end (gambatte
        // cgbpAccessible vs m0Time), so it gets the whole-M-cycle block where
        // OAM/VRAM only get the second half. INC-G3 task 5.
        EdgeKind::PalAccess => END_PHASE,
        // The double-speed FF41 STAT mode-bit block also commits at the
        // M-cycle END (INC-G3 task 6): a sprite-line m3→m0 flip anywhere in the
        // straddle M-cycle holds the cc+2 read at the old mode 3, not only a
        // 2nd-half flip. The INC-DS-1 dot-END half-split caught the +43 rows
        // whose flip lands in the M-cycle's second half; promoting StatMode to
        // the whole-M-cycle block lifts the +84 residual `m3stat_ds_1` rows
        // whose flip lands in the FIRST half (gambatte sprites). The full-gbtr
        // ratchet measured +84/−3 (net floor −84): the only regressions are the
        // 3 `late_sizechange_sp00/01/39_ds_1` (out0, want mode 0) — a net-neutral
        // in-cluster A/B swap, since their `_ds_2` siblings (out3) are in the
        // lift. Whole-M-cycle forces both the size-change `_1` and `_2` reads on
        // the straddle line to mode 3; the `_2` want it, the `_1` do not, and no
        // `event_phase` offset separates two reads in the same M-cycle (the
        // parked multi-chain CPU↔PPU phase problem). Taken on the half-dot-grid
        // branch (net-positive trades OK); see the task-6 swap note in
        // tests/gbtr/baselines/gambatte.txt.
        EdgeKind::StatMode => END_PHASE,
        // Every other event commits at its dot-END eighth (net-zero scaffold).
        _ => cc_eighth(cc),
    }
}

/// The single sub-cc phase (eighths) at which every CPU bus access samples the
/// accessibility/STAT edge stamps. INC-G3 corrects the reverted G2c premise (a
/// per-read-chain observer phase): M-cycles are dot-aligned to the PPU, so all
/// CPU accesses sample at the SAME M-cycle cc-offset — the discriminator
/// between read chains is the EVENT's sub-dot position ([`event_phase`]), not
/// the observer's. Equals [`MID_PHASE`] (cc+2), so the scaffold is net-zero
/// (`access_phase_is_single_constant`).
const ACCESS_PHASE: u8 = MID_PHASE;

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
    /// Debugger memory watchpoints (RM8). Empty on every golden/test path, so
    /// [`Self::check_watch`] is a zero-cost no-op there (golden-safe).
    watchpoints: Vec<crate::Watchpoint>,
    /// Address of the most recent watchpoint-matching CPU access, consumed by
    /// the free-run loop ([`crate::GameBoy::run_frame_until_breakpoint`]).
    watch_hit: Option<u16>,
    /// Execution-profiler per-PC instruction tally (MB5). `None` on every
    /// golden/test path, so [`Bus::profile_pc`] is a single-branch no-op there
    /// (golden-safe); `Some` only while the live debugger's profiler logs.
    prof: Option<std::collections::BTreeMap<u16, u64>>,
    /// Profiler "break mode": flag the first execution of each address so the
    /// free run can halt there (bgb's coverage break — find unexecuted code).
    /// Only meaningful while `prof` is `Some`.
    prof_break: bool,
    /// A newly-seen address under break mode, consumed by the free-run loop
    /// ([`crate::GameBoy::run_frame_until_breakpoint`]).
    prof_break_hit: Option<u16>,
    /// Debugger exception-break mask (Options → Exceptions, the `EXC_*` bits).
    /// `0` on every golden/test path ⇒ the exec/access checks are single-branch
    /// no-ops (golden-safe). Set only by the live debugger.
    exc_mask: u16,
    /// PC/addr of the most recent armed-exception hit, consumed by the free-run
    /// loop ([`crate::GameBoy::run_frame_until_breakpoint`]).
    exc_hit: Option<u16>,
    /// Elapsed T-cycles since power-on (normal-speed dots).
    cycles: u64,

    /// CGB hardware running a CGB-flagged cart. CGB hardware with a DMG
    /// cart runs in DMG compatibility mode: KEY1/SVBK/HDMA/RP/FF74 and the
    /// palette data ports are disabled (misc/boot_hwio-C).
    cgb_mode: bool,
    double_speed: bool,
    /// CPU↔PPU sub-dot phase for the cc-granular reclock: which cc's of the
    /// M-cycle tick a PPU dot in double speed (see [`dot_ticks_on_cc`]). 0 =
    /// the fixed even-cc {2,4} alignment the old `for i in 0..dots` loop baked
    /// in (single speed is phase-independent); 1 = the odd-cc {1,3} half-dot
    /// offset a STOP speed switch can establish (the LCD dot clock runs on
    /// continuously across the switch while the CPU's M-cycle grid is re-paced).
    /// **Held at 0**: setting it at the speed switch was measured to lift ZERO
    /// gambatte DS rows (env probe `SLOPGB_DOTPHASE`, all of const-1 / cycle-
    /// parity candidates over 287 baselined rows). The m3stat / speedchange
    /// `_2` reads are served by the cc-invariant `END_PHASE` StatMode/PalAccess
    /// overrides, and their correct answer needs the pixel-pipe END *dot* to
    /// move (a full pixel-pipe reclock), not the M-cycle's sample phase — see
    /// docs/hardware-state/ppu-subdot-ladder.md. The field is the cc-granular
    /// substrate that reclock would drive; 0 is bit-identical to the dot loop
    /// (`cc_grid_matches_dot_loop`).
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
    /// STAT IF bit raised by the PPU in the *current* M-cycle's second
    /// half (line-0 OAM rise): readable via FF0F at once, but the CPU's
    /// interrupt sample for this cycle must not see it
    /// (`Ppu::take_stat_late`).
    if_stat_late: u8,
    /// The mode-3→mode-0 OAM accessibility unblock fired in the *second
    /// half* of the current M-cycle (`Ppu::take_m0_access_flip`
    /// half-classified against the dot-loop index): a CPU OAM read samples
    /// at the cc+2 MID phase, two dots before this M-cycle's end-sampled
    /// view, so it still reads mode 3 ($FF) when the unblock lands late.
    /// First wedge of the sub-dot event-phase model — routes only the OAM
    /// read; the m0 IRQ, mode-bit flip and every other access stay on the
    /// whole-dot end view, so this is net-zero except the straddle
    /// M-cycle (gambatte `oam_access/postread_*`). See `Ppu::m0_access_flip`.
    /// Stamped with the flip's dot-END commit eighth ([`edge_eighth`]; `None`
    /// = no flip this M-cycle); a read is blocked when its observer phase
    /// precedes the stamp ([`stamp_blocks`]).
    m0_access_edge: Option<u8>,
    /// As `m0_access_edge` but for the CGB palette-RAM unblock (anchored at
    /// the pipe end / `render_finished`, one dot after the m0 flip). Unlike
    /// OAM/VRAM, the palette unblock commits at the M-cycle END
    /// ([`event_phase`] gives `PalAccess` phase 8 = the whole-M-cycle block,
    /// INC-G3 task 5): a cc+2 MID-phase FF69/FF6B read reads $FF for the
    /// ENTIRE straddle M-cycle, not just its second half (gambatte
    /// `cgbpal_m3end` `scx2_1`/`scx5_1`/`scx5_ds_1`). See `Ppu::pal_access_flip`.
    pal_access_edge: Option<u8>,
    /// The mode-3→mode-0 STAT mode-bit flip's dot-END commit eighth, or
    /// `None` when no flip lands this M-cycle (`Ppu::take_m0_stat_flip`,
    /// gated to sprite-extended lines): a CPU FF41 read samples the mode bits
    /// at the cc+2 MID phase, so in double speed it still reads mode 3 when
    /// the flip lands late (gambatte sprites `m3stat_ds_1`). The FF41 override
    /// consults this only in double speed; the single-speed STAT-mode read,
    /// and the first-half/other-chain DS reads, are the parked multi-chain
    /// problem (see the dot-loop comment). See `Ppu::m0_stat_flip` (sub-dot
    /// event-phase model, increment INC-DS-1).
    stat_mode_edge: Option<u8>,
    /// Dispatch-ack source sync-ahead (gambatte-core memory.cpp
    /// `Memory::ackIrq`): the IF clear of an interrupt dispatch happens
    /// slightly *into* the low-push M-cycle on hardware, so it also
    /// consumes a hardware re-set of the acked source that lands just
    /// after the ack — `updateSerial(cc + 3 + isCgb())`,
    /// `updateTimaIrq(cc + 2 + isCgb())`, `lcd_.update(cc + 2)` run
    /// before `intreq_.ackIrq(bit)`. Translated to this core's
    /// tick-then-access grid: a timer/serial set produced by the next
    /// machine tick (the next two on CGB/AGB — the timer IF commits on
    /// the last T-substep, the serial IF on the DIV-edge boundary), and
    /// a STAT/VBlank rise in the first 2 dots of the next tick, are
    /// swallowed by the preceding [`Bus::ack`]. The 2-dot LCD window is
    /// deliberately NOT widened to the line-anchored rises' second-half
    /// emission dots at single speed (gambatte's `cc + 2` does not reach
    /// them — m2int_m2irq_late_retrigger_1 and late_m0irq_retrigger_scx1_1
    /// pin the keeps); in double speed the 2-dot window spans the whole
    /// tick, which is what flips the `*_late_retrigger_ds_2` rows.
    /// Pinned by gambatte tima/tc00_irq_late_retrigger_2/3 (dmg08_outE4
    /// vs cgb04c_outE0), serial/start_wait_trigger_int8_read_if_2/3 and
    /// the irq_precedence/m1/ly0/lyc153int `*_late_retrigger` rows.
    /// `ack_squash_mask` is the acked source's IF bit (only that source
    /// is consumed — others get *flagged*, which our per-tick OR already
    /// models); `ack_squash_ticks`/`ack_squash_dots` are the remaining
    /// windows.
    ack_squash_mask: u8,
    ack_squash_ticks: u8,
    ack_squash_dots: u8,

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
            watchpoints: Vec::new(),
            watch_hit: None,
            prof: None,
            prof_break: false,
            prof_break_hit: None,
            exc_mask: 0,
            exc_hit: None,
            cycles: 0,
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
            if_stat_late: 0,
            m0_access_edge: None,
            pal_access_edge: None,
            stat_mode_edge: None,
            ack_squash_mask: 0,
            ack_squash_ticks: 0,
            ack_squash_dots: 0,
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
        }
    }

    /// True when the machine runs in native CGB mode (CGB/AGB hardware with
    /// a CGB-flagged cart, as opposed to DMG compatibility mode).
    pub(crate) fn cgb_mode(&self) -> bool {
        self.cgb_mode
    }

    /// CGB double-speed (KEY1 bit 7) state, for the debugger registers panel.
    pub(crate) fn double_speed(&self) -> bool {
        self.double_speed
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    pub fn frame_count(&self) -> u64 {
        self.ppu.frame_count()
    }

    pub fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    pub fn apu(&self) -> &Apu {
        &self.apu
    }

    // The debugger watchpoint (RM8) + execution-profiler (MB5) inherent methods
    // live in the `debug` submodule (a second `impl Interconnect` block) —
    // including the per-access watchpoint + exception checks (`check_access`,
    // `check_exc_lcd`) and the per-opcode exception check (`exec_exception`).

    /// Debugger memory write: store `value` at `addr` with no M-cycle timing
    /// (the symmetric counterpart of [`Self::peek`] / the debug read path).
    /// Used by [`crate::GameBoy::debug_call`] to push a return address — a
    /// live-debugger-only `&mut` path, never on a golden/test run.
    pub fn debug_write(&mut self, addr: u16, value: u8) {
        self.write_no_tick(addr, value);
    }

    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    pub fn joypad_mut(&mut self) -> &mut Joypad {
        &mut self.joypad
    }

    /// Read-only joypad handle (debugger/UI; side-effect-free).
    #[must_use]
    pub fn joypad(&self) -> &Joypad {
        &self.joypad
    }

    pub fn cartridge(&self) -> &Cartridge {
        &self.cart
    }

    pub fn cartridge_mut(&mut self) -> &mut Cartridge {
        &mut self.cart
    }

    /// Drain captured serial output (test-harness hook; see
    /// `Serial::take_output`).
    pub(crate) fn take_serial_output(&mut self) -> Vec<u8> {
        self.serial.take_output()
    }

    // --- Link cable (frontend TCP peer; inert when disconnected) ------------

    /// Attach/detach a serial link peer (frontend path only).
    pub(crate) fn link_set_connected(&mut self, on: bool) {
        self.serial.set_link_connected(on);
    }

    /// Whether a link peer is attached.
    pub(crate) fn link_connected(&self) -> bool {
        self.serial.link_connected()
    }

    /// Provide the peer byte the next master transfer shifts in.
    pub(crate) fn link_push_recv(&mut self, byte: u8) {
        self.serial.push_link_in(byte);
    }

    /// Drain the byte a completed master transfer shifted out, for the peer.
    pub(crate) fn link_take_send(&mut self) -> Option<u8> {
        self.serial.take_link_send()
    }

    /// Complete a pending external-clock (slave) transfer with the peer's
    /// byte, folding the resulting serial interrupt into IF. Returns the
    /// slave's outgoing byte if it was armed, else `None` (a no-op).
    pub(crate) fn link_slave_transfer(&mut self, master_byte: u8) -> Option<u8> {
        let (out, iff) = self.serial.link_slave_transfer(master_byte);
        self.intf |= iff & IF_MASK;
        out
    }

    /// Side-effect-free, time-free view of memory for test harnesses:
    /// `&self` guarantees no peripheral ticks and no read side effects.
    ///
    /// Deliberately omniscient — unlike a CPU read it ignores PPU
    /// mode-based VRAM/OAM lockout and OAM DMA bus conflicts.
    /// ROM/VRAM/cart-RAM/WRAM follow the live banking; disabled cart RAM
    /// still reads $FF like a real access (`Cartridge::read_ram`). IO
    /// registers (FF00-FF7F) are *not* peekable — their values are
    /// computed from live peripheral state under the tick-then-access
    /// contract, and reading them out of band would mislead harnesses —
    /// and the FEA0-FEFF prohibited area has no stable content; both read
    /// $FF here.
    pub(crate) fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.vram_read_raw(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
            0xFE00..=0xFE9F => self.ppu.oam_read_raw(addr),
            0xFEA0..=0xFF7F => 0xFF,
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)],
            0xFFFF => self.ie,
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

impl Bus for Interconnect {
    fn read(&mut self, addr: u16) -> u8 {
        self.service_vram_dma();
        self.tick_machine();
        // A trigger inside this very cycle still steals the bus before
        // the read samples (see `service_vram_dma`: reads yield, writes
        // in flight commit first).
        self.service_vram_dma();
        self.maybe_oam_bug(addr, OamBugKind::Read);
        self.check_access(addr, false);
        self.read_no_tick(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.service_vram_dma();
        // The CPU drives the data bus during the second half of the write
        // M-cycle (gbctr "Memory access timing"), which the dot-clocked
        // pixel pipeline can observe mid-cycle: stage rendering-register
        // writes with the PPU before ticking. The architectural commit
        // below is unchanged — `Ppu::stage_write` affects only the
        // pipeline's register view (mealybug m3_* mid-mode-3 writes).
        if let 0xFF40 | 0xFF42 | 0xFF43 | 0xFF47..=0xFF4B = addr {
            let dots = if self.double_speed { 1 } else { 2 };
            self.ppu.stage_write(addr, value, dots);
        }
        self.tick_machine();
        // Corruption first, then the (mode-blocked) write attempt — during
        // the scan the CPU byte never lands (oam_write_blocked).
        self.maybe_oam_bug(addr, OamBugKind::Write);
        self.check_access(addr, true);
        // Exception break: disabling the LCD outside vblank — sample the *old*
        // LCDC (`write_no_tick` commits the new one below).
        self.check_exc_lcd(addr, value);
        self.write_no_tick(addr, value);
    }

    fn tick(&mut self) {
        self.service_vram_dma();
        self.tick_machine();
    }

    fn tick_addr(&mut self, value: u16) {
        self.service_vram_dma();
        self.tick_machine();
        self.maybe_oam_bug(value, OamBugKind::Write);
    }

    fn read_inc(&mut self, addr: u16) -> u8 {
        self.service_vram_dma();
        self.tick_machine();
        self.service_vram_dma(); // reads yield to a same-cycle trigger
        self.maybe_oam_bug(addr, OamBugKind::ReadIncrease);
        self.check_access(addr, false);
        self.read_no_tick(addr)
    }

    fn check_exec(&mut self, pc: u16, opcode: u8) {
        // Inert unless an opcode exception was armed (`exc_mask == 0` on every
        // golden/test path), so this is byte-identical there.
        self.exec_exception(pc, opcode);
    }

    fn profile_pc(&mut self, pc: u16) {
        // Inert unless the live debugger enabled profiling — `prof` is `None` on
        // every golden/test path, so this records nothing and the emulated
        // state (and the fingerprint) is byte-identical.
        if let Some(m) = &mut self.prof {
            let count = m.entry(pc).or_insert(0);
            let first_seen = *count == 0;
            *count += 1;
            // Break mode: remember an address's first execution so the free run
            // can halt there (consumed by `take_prof_break_hit`).
            if first_seen && self.prof_break {
                self.prof_break_hit = Some(pc);
            }
        }
    }

    fn pending(&self) -> u8 {
        self.intf & self.ie & IF_MASK & !self.if_stat_late
    }

    fn pending_halt_wake(&self) -> u8 {
        // The halt-exit logic samples IE & IF *within* the M-cycle, not at
        // its end (SameBoy sm83_cpu.c `GB_cpu_run`: DMG samples mid-cycle
        // after 2 of 4 T-cycles, CGB/AGB at the start of the cycle), so a
        // timer reload + IF commit — which lands on the last T-substep
        // under the hardware DIV phase (div ≡ 0 mod 4 at boundaries) — is
        // missed until the next cycle: the halt wake comes one M-cycle
        // later than a running-CPU dispatch would (gambatte tima/tc*_irq_*
        // on both models; wilbertpol timer_if rounds 5/6 vs 3/4).
        //
        // The STAT bit joins the mask per event, not wholesale: the PPU
        // flags its second-half IF commits (line-start OAM pulses, mode-0
        // rises on dots ≡ 3,0 mod 4) via `Ppu::take_stat_halt_late`, which
        // ORs IF_STAT into `if_late` for exactly those cycles — the
        // gbmicrotest int_oam_*/int_hblank_halt_scx* grids and the
        // mooneye/wilbertpol hblank halt groupings pin the law, while
        // first-half STAT commits and the vblank IF stay live
        // (halt_ime1_timing2-GS, vblank, DMG). The known unmodelled
        // remainder is the CGB/AGB start-of-cycle staleness for first-half
        // PPU commits (halt_ime1_timing2-GS's "fail: CGB, AGB, AGS";
        // gambatte halt/*_cgb04c split rows): landing it requires a
        // per-model widening of the halt-late mask, a separate work
        // package.
        (self.intf & !self.if_late) & self.ie & IF_MASK
    }

    fn ack(&mut self, bit: u8) {
        self.intf &= !(1 << bit);
        // gambatte Memory::ackIrq syncs the acked bit's source a few
        // T-cycles past the ack point before clearing, so a hardware
        // re-set landing just after the dispatch's IF clear is consumed
        // by it (see the `ack_squash_*` field docs for the window
        // derivation and the pinning ROMs).
        match bit {
            0 | 1 => {
                // lcd_.update(cc + 2), no isCgb term: 2 dots into the
                // next machine tick on both families and at both speeds
                // (in double speed that is the whole 2-dot tick). The
                // line-anchored rises' single-speed second-half emission
                // dots stay OUT of reach — see the field docs.
                self.ack_squash_mask = 1 << bit;
                self.ack_squash_ticks = 0;
                self.ack_squash_dots = 2;
            }
            2 | 3 => {
                // updateTimaIrq(cc + 2 + isCgb()) / updateSerial(cc + 3 +
                // isCgb()): with the timer IF on the last T-substep and
                // the serial IF on the DIV-edge boundary, both windows
                // cover the set produced by the next machine tick on the
                // DMG family and the next two on CGB/AGB.
                self.ack_squash_mask = 1 << bit;
                self.ack_squash_ticks = if self.model.is_cgb() { 2 } else { 1 };
                self.ack_squash_dots = 0;
            }
            _ => {}
        }
    }

    fn stop(&mut self, skipped_addr: u16, interrupt_pending: bool) -> bool {
        let switching = self.cgb_mode && self.key1_armed;
        let entering_ds = switching && !self.double_speed;
        // gambatte Memory::stop snapshots the HDMA situation at the
        // pre-read cc: a block request still pending when STOP executes
        // (flagged mid-instruction — no boundary came) is deferred when
        // leaving double speed (haltHdmaState_ = hdma_requested +
        // ackDmaReq) but stays flagged when entering it, firing *inside*
        // the pause where the gated core clock aborts the HBlank transfer
        // with the count latched (dma()'s halted path; pinned by
        // hdma_transition_speedchange_hdmalen*_hdma5 → $80|len vs
        // hdma_late_m3speedchange_hdma5_*_ds_1 → still active).
        let in_window = self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period();
        let pending_req = self.vram_dma_req.take();
        if switching && !entering_ds {
            // Leaving double speed: the PPU/APU re-pace from the cycle
            // right after the STOP opcode fetch (gambatte lcd_/psg_
            // .speedChange at cc_ = cc + 8 * !isDoubleSpeed(): offset 0
            // leaving, +8 entering), so the toggle precedes the
            // skipped-byte read below; entering double speed it lands
            // after the read + internal cycle instead.
            self.double_speed = false;
            self.ppu.set_double_speed(false);
        }
        if !interrupt_pending {
            // The skipped byte costs one real read M-cycle (SameBoy
            // stop(): `cycle_read(gb, gb->pc++)`, gated on no pending
            // interrupt). The value is discarded; the address still
            // drives the bus (OAM bug).
            self.tick_machine();
            self.maybe_oam_bug(skipped_addr, OamBugKind::Read);
            let _ = self.read_no_tick(skipped_addr);
        }
        // STOP resets DIV on every model (Pan Docs "FF04 — DIV"),
        // committing like a write occupying the skipped-byte read slot:
        // gambatte Memory::stop timestamps `nontrivial_ff_write(0x04, 0,
        // cc)` at the slot's *start* cc, and gambatte write timestamps are
        // start-of-cycle (cpu.cpp FF_WRITE advances cc afterwards) where
        // ours commit after the tick — so the reset lands here, after that
        // cycle's tick (the gambatte speedchange tima/div a/b phase pairs
        // pin the TIMA falling-edge quirk to this cell). Modelled as a DIV
        // write so the falling-edge effects apply (frame-sequencer edge
        // included, `Apu::div_write` — the speedchange ch2_nr52 families).
        self.apu.div_write(self.double_speed);
        self.timer.write(0xFF04, 0);
        if !switching {
            // Deep stop: hand a still-pending block request back — the
            // CPU's stop idle engages the halt gate, which defers it
            // (gambatte's non-switch stop path calls Memory::halt).
            self.vram_dma_req = pending_req.or(self.vram_dma_req);
            return false;
        }
        self.key1_armed = false;
        if interrupt_pending {
            // With IE & IF pending the switch is instantaneous: no
            // skipped-byte read, no pause (SameBoy stop() gates the halt
            // countdown on !interrupt_pending; age caution/
            // spsw-interrupts).
            if entering_ds {
                self.double_speed = true;
                self.ppu.set_double_speed(true);
            }
            self.vram_dma_req = pending_req.or(self.vram_dma_req);
            return true;
        }
        // The OAM DMA controller freezes after the read cycle (gambatte
        // Memory::stop: updateOamDma(cc + 4), then intreq_.halt()); the
        // halt-hdma snapshot below is installed first so the wake path
        // can re-evaluate it.
        self.halt_hdma = if pending_req.is_some() && !entering_ds {
            HaltHdmaState::Requested
        } else if in_window {
            HaltHdmaState::High
        } else {
            HaltHdmaState::Low
        };
        self.engage_halt_gate(true);
        // One internal M-cycle before the pause (gambatte Memory::stop
        // returns cc + 8: the operand read plus one cycle), still at the
        // old PPU/APU pace when entering double speed.
        self.tick_machine();
        if entering_ds {
            self.double_speed = true;
            self.ppu.set_double_speed(true);
        }
        // Mode-0 entries seen by the two cycles above never flag a block:
        // gambatte defers all LCD events into the pause, where the halted
        // gate suppresses the flag; the live window is re-checked at wake.
        self.vram_dma_req = None;
        // The pause: the CPU sleeps for 0x7FFF more M-cycles on the *new*
        // clock — with the read + internal cycles that totals 0x8001
        // M-cycles ≙ gambatte's unhalt event at cc + 0x20000 + 4 (cc
        // counts 4 per M-cycle at either speed) — while PPU/APU/timer run
        // on. IE & IF != 0 ends it early, exactly like halt mode
        // (gambatte's pause *is* a halt: the halted intevent_interrupts
        // path unhalts; SameBoy keeps gb->halted under
        // speed_switch_halt_countdown). SameBoy instead uses a flat
        // 0x20008 8-MHz-clock countdown — half the pause when leaving
        // double speed; gambatte's cgb04c expectations are this suite's
        // oracle, and the speedchange2/3/4/5 (DS→single) LY families
        // confirm its doubled length.
        let dots_per_m: u64 = if self.double_speed { 2 } else { 4 };
        let target = self.cycles + 0x7FFF * dots_per_m;
        if entering_ds && pending_req.is_some() {
            // The surviving block request fires at the first event check
            // inside the pause: the halted service aborts the transfer
            // (see run_vram_dma). Its stall counts toward the pause.
            self.vram_dma_req = pending_req;
            self.run_vram_dma();
        }
        while self.cycles < target && self.intf & self.ie & IF_MASK == 0 {
            self.tick_machine();
        }
        self.engage_halt_gate(false);
        self.vram_dma_unhalt();
        true
    }

    fn set_halted(&mut self, halted: bool) {
        self.set_cpu_halted(halted);
    }
}

#[cfg(test)]
#[path = "interconnect_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "interconnect_pcm_probe.rs"]
mod pcm_decay_probe;
