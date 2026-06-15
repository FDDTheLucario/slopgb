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

/// The commit phase (eighths of an M-cycle) of boundary event `kind` firing on
/// dot `i` of a `dots`-dot M-cycle. Most kinds commit at their dot-END eighth
/// ([`edge_eighth`]); `PalAccess` is calibrated off it (INC-G3 task 5) — its
/// per-event offset is exactly what [`EdgeKind`] keys, so the others stay
/// dot-clocked while one event moves.
#[inline]
fn event_phase(kind: EdgeKind, i: u64, dots: u64) -> u8 {
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
        _ => edge_eighth(i, dots),
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

    /// CGB hardware running a CGB-flagged cart. CGB hardware with a DMG
    /// cart runs in DMG compatibility mode: KEY1/SVBK/HDMA/RP/FF74 and the
    /// palette data ports are disabled (misc/boot_hwio-C).
    cgb_mode: bool,
    double_speed: bool,
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
            cycles: 0,
            cgb_mode,
            double_speed: false,
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

    /// Initialise hardware registers and DIV to the post-boot state of the
    /// model (called once from `GameBoy::new`).
    ///
    /// Special cases (everything else goes through the normal IO write
    /// paths):
    /// * LCD: the boot ROM turned the LCD on long before hand-off, so LCDC
    ///   is written first and the PPU is ticked through its glitched enable
    ///   line (70224-4 dots) plus `lcd_phase_dots` to reach the exact
    ///   mid-frame position `boot_hwio-*` measure. IF bits produced during
    ///   this warmup are discarded — the table's IF value ($E1) already
    ///   represents them.
    /// * FF46 is installed as a plain register value; an IO write would
    ///   start a transfer.
    /// * DIV is set directly (`Timer::set_div`); an FF04 write resets the
    ///   counter and can clock TIMA through the falling-edge detector.
    /// * CGB compat palettes are written through BCPS/BCPD before the mode
    ///   gate would block them (the boot ROM writes them while still in CGB
    ///   mode, then locks compatibility mode via KEY0).
    /// * Serial and APU get one seeding tick with the final DIV value so
    ///   their internal previous-DIV edge detectors start in phase
    ///   (boot_sclk_align-dmgABCmgb). A seeding tick from prev_div = 0
    ///   cannot produce a spurious falling edge.
    pub fn apply_post_boot_state(&mut self) {
        let s = self.model.post_boot_state();

        self.install_power_on_wram();
        self.install_boot_logo_vram();

        // The CGB/AGB boot ROM hands a CGB-flagged cart off 0x7D8
        // T-cycles (502 M-cycles) *earlier* than a DMG cart: the
        // DMG-compat path does its compatibility-palette work after the
        // logo. DIV and the LCD run uninterrupted through that tail, so
        // both shift together by exactly the DIV difference. The
        // CGB-cart DIV is pinned by gambatte div/start_inc_1/2 ($143=$C0
        // carts: FF04 reads $1E at +96 T right before an increment, $1F
        // at +100 → counter $1E9C = $2674 - $7D8) and cross-checked by
        // tima/tc00_start_1/2 ($1E9C + 356 ≡ 0 mod $400 puts the first
        // TIMA increment exactly between rounds); the DMG-cart values
        // stay pinned by mooneye misc/boot_div-cgbABCDE/-A (DMG carts).
        let cgb_cart_cut: u32 = if self.cgb_mode { 0x7D8 } else { 0 };

        // LCD warmup: glitched enable line (452 dots) + 153 normal lines
        // brings the PPU to line 0 dot 0; then advance to the hand-off
        // phase.
        self.ppu.write(0xFF40, 0x91);
        for _ in 0..(70224 - 4 + s.lcd_phase_dots - cgb_cart_cut) {
            self.ppu.tick();
        }

        if self.model.is_cgb() {
            // Compat palette: BG palette 0 (8 bytes) leaves BCPS = $88,
            // OBJ palettes 0+1 (16 bytes) leave OCPS = $90 — boot_hwio-C
            // reads $C8/$D0.
            self.ppu.write(0xFF68, 0x80);
            for c in CGB_COMPAT_BG_PALETTE {
                self.ppu.write(0xFF69, c as u8);
                self.ppu.write(0xFF69, (c >> 8) as u8);
            }
            self.ppu.write(0xFF6A, 0x80);
            for _ in 0..2 {
                for c in CGB_COMPAT_OBJ_PALETTE {
                    self.ppu.write(0xFF6B, c as u8);
                    self.ppu.write(0xFF6B, (c >> 8) as u8);
                }
            }
            // OPRI: DMG-compat mode uses DMG-style X priority (FF6C reads
            // $FF), CGB mode uses OAM-index priority ($FE).
            self.ppu.write(0xFF6C, u8::from(!self.cgb_mode));
        }

        for &(addr, value) in s.hwio {
            if addr == 0xFF46 {
                self.dma_reg = value;
            } else {
                self.write_no_tick(addr, value);
            }
        }

        // SGB boot duration depends on the cartridge header: the boot ROM
        // sends it to the SNES bit by bit, and the zero-bit branch of its
        // send loop is one M-cycle longer than the one-bit branch (see
        // `sgb_header_zero_bits` for the boot ROM derivation).
        let div = if matches!(self.model, Model::Sgb | Model::Sgb2) {
            s.div_counter
                .wrapping_add((4 * sgb_header_zero_bits(&self.cart)) as u16)
        } else {
            s.div_counter.wrapping_sub(cgb_cart_cut as u16)
        };
        self.timer.set_div(div);
        self.serial.tick(div);
        self.apu.tick(div, false);

        // APU warmup: the hwio replay above re-triggered the boot beep, but
        // on hardware the beep plays while the logo is shown, well before
        // hand-off, and its decaying envelope (NR12=$F3: 15 steps x 3
        // frame-sequencer envelope ticks = 45/64 s) has reached volume 0 by
        // PC=0x100 — channel 1 stays *enabled* (NR52 still reads $F1; volume
        // is not an enable condition), it just outputs digital 0, so the CGB
        // PCM12 register reads $00 at hand-off (oracle: misc/boot_hwio-C and
        // misc/bits/unused_hwio-C, which expect FF76 == $00). Run the APU
        // through one emulated second of synthetic DIV time to decay the
        // envelope; samples produced are discarded. The real DIV counter and
        // the serial/APU edge detectors are re-seeded afterwards.
        let mut warm_div = div;
        for _ in 0..1_048_576u32 {
            warm_div = warm_div.wrapping_add(4);
            self.apu.tick(warm_div, false);
        }
        self.apu.tick(div, false);
        let mut sink = Vec::new();
        self.apu.drain_samples(&mut sink);
    }

    /// WRAM as it powers up. Real DMG-family WRAM is not zeroed: it wakes
    /// in a deterministic stripe pattern — alternating $00/$FF half-pages
    /// (256 B) with the polarity inverted across each 2 KiB half, and
    /// C000-CFFF mirrored into D000-DFFF — plus a sprinkle of per-board
    /// bit noise. The pattern is the gambatte-core mem_dumps.h
    /// `setInitialDmgWram` hardware dump (captured on the same DMG-CPU-08
    /// board as the `dmg08` expectation corpus; every DMG emulator with a
    /// hardware dump agrees on the stripe base). The per-board
    /// `dmgWramDumpDiff` noise bytes are deliberately NOT vendored: no
    /// test in the corpus distinguishes them, and they are unit noise,
    /// not architecture. Pinned by gambatte `oamdma_srcFE00_*` (an OAM
    /// DMA from $FE00 reads the $DE00 WRAM echo page, which must read
    /// $FF). The boot ROM only clears VRAM/HRAM, never WRAM, so the
    /// pattern survives to PC=0x100; mooneye `boot_hwio-*` masks WRAM
    /// out. CGB WRAM keeps the zero fill: its power-on pattern differs
    /// per bank (gambatte `setInitialCgbWram`) and nothing in the corpus
    /// pins it.
    fn install_power_on_wram(&mut self) {
        if self.model.is_cgb() {
            return;
        }
        for (i, byte) in self.wram.iter_mut().enumerate() {
            // Half-page index 0..15 within the (mirrored) 4 KiB bank:
            // $00 for even half-pages in C000-C7FF and odd ones in
            // C800-CFFF, $FF otherwise.
            let half_page = (i >> 8) & 0x0F;
            let inverted = half_page >= 8;
            *byte = if (half_page & 1 == 0) != inverted {
                0x00
            } else {
                0xFF
            };
        }
    }

    /// VRAM tile data as the boot ROM leaves it: the Nintendo logo
    /// decompressed into tiles $01-$18 and the (R) trademark tile at $19
    /// — even (low-bitplane) bytes only, the boot routine writes one
    /// bitplane (DMG boot ROM "Graphic routine"; gambatte initstate.cpp
    /// setInitialVram models the same bytes). mealybug m3_scx_low_3_bits
    /// renders the leftover (R) tile straight out of this data.
    ///
    /// The boot ROM decompresses the logo from the cartridge header, but
    /// it also locks up unless the header bytes equal the canonical logo
    /// — so on hardware VRAM only ever holds the standard image, and the
    /// fixed constant below covers every cart (including the gambatte
    /// test ROMs, whose headers carry no logo at all; gambatte's
    /// initstate uses the same fixed dump).
    ///
    /// Deliberately NOT modelled: the DMG boot also leaves the logo's two
    /// tile-map rows at $9904/$9924 (+ the (R) entry at $9910). The
    /// pinned gambatte reference PNGs are emulator captures from before
    /// gambatte modelled initial VRAM — they encode a cleared map, and
    /// several otherwise-passing screens (scx_during_m3/old,
    /// bgtilemap/bgtiledata) show the logo rows if the entries are
    /// installed, while no test in the corpus needs them.
    fn install_boot_logo_vram(&mut self) {
        const NINTENDO_LOGO: [u8; 48] = [
            0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C,
            0x00, 0x0D, 0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6,
            0xDD, 0xDD, 0xD9, 0x99, 0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC,
            0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
        ];
        // Each logo nibble is bit-doubled into one tile-row byte, written
        // twice (two consecutive tile rows).
        let double = |n: u8| -> u8 {
            let mut out = 0u8;
            for bit in 0..4 {
                if n & (1 << bit) != 0 {
                    out |= 0b11 << (bit * 2);
                }
            }
            out
        };
        for (i, byte) in NINTENDO_LOGO.into_iter().enumerate() {
            let base = 0x8010 + 8 * i as u16;
            for (j, nibble) in [byte >> 4, byte & 0x0F].into_iter().enumerate() {
                let row = double(nibble);
                self.ppu.vram_write_raw(base + 4 * j as u16, row);
                self.ppu.vram_write_raw(base + 4 * j as u16 + 2, row);
            }
        }
        // The (R) trademark tile data lives in the boot ROM itself.
        const TRADEMARK: [u8; 8] = [0x3C, 0x42, 0xB9, 0xA5, 0xB9, 0xA5, 0x42, 0x3C];
        for (i, b) in TRADEMARK.into_iter().enumerate() {
            self.ppu.vram_write_raw(0x8190 + 2 * i as u16, b);
        }
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

    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    pub fn joypad_mut(&mut self) -> &mut Joypad {
        &mut self.joypad
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

    /// Advance the whole machine by one CPU M-cycle (docs/ARCHITECTURE.md
    /// §Timing: timer, OAM DMA engine, PPU dots, VRAM DMA, APU, serial,
    /// joypad; IF bits OR-ed in as produced).
    fn tick_machine(&mut self) {
        let dots: u64 = if self.double_speed { 2 } else { 4 };
        self.cycles += dots;
        // Dispatch-ack sync-ahead window for this tick (see `ack`):
        // timer/serial sets produced by an in-window tick are consumed
        // by the preceding ack instead of re-raising IF.
        let tick_squash = if self.ack_squash_ticks > 0 {
            self.ack_squash_ticks -= 1;
            self.ack_squash_mask & 0x0C
        } else {
            0
        };
        let t = self.timer.tick();
        // IF reads must see a second-half commit within its own cycle
        // (mooneye tima_reload access sequences) — only the halt-exit
        // sampling misses it, via the `if_late` mask.
        let t_iff = t.iff & IF_MASK & !tick_squash;
        self.intf |= t_iff;
        self.if_late = if t.late { t_iff } else { 0 };
        self.oam_dma_tick();
        self.if_stat_late = 0;
        self.m0_access_edge = None;
        self.pal_access_edge = None;
        self.stat_mode_edge = None;
        for i in 0..dots {
            // STAT/VBlank rises in the first 2 dots after the ack are
            // consumed too (gambatte ackIrq lcd_.update(cc + 2); in
            // double speed the window spans the whole tick — see `ack`).
            let dot_squash = if self.ack_squash_dots > 0 {
                self.ack_squash_dots -= 1;
                self.ack_squash_mask & 0x03
            } else {
                0
            };
            self.intf |= self.ppu.tick() & IF_MASK & !dot_squash;
            if self.ppu.take_stat_late() {
                // The line-0 OAM STAT rise sits in the second half of the
                // M-cycle: the IF bit is readable at once, but this
                // cycle's interrupt sample must not see it (see
                // Ppu::stat_events_tick; mealybug "line 0 timing is different
                // by 4 cycles").
                self.if_stat_late |= IF_STAT_BIT;
            }
            if self.ppu.take_stat_halt_late() {
                // Second-half STAT IF commit (line-start OAM pulses):
                // readable at once, but the halt-exit sampler misses it
                // for one cycle — the same shape as the timer's `if_late`
                // mask (SameBoy GB_cpu_run halt path; gbmicrotest
                // int_oam_* grids pin the law).
                self.if_late |= IF_STAT_BIT;
            }
            if self.ppu.take_m0_rise()
                && obs_pre_edge(MID_PHASE, event_phase(EdgeKind::M0Rise, i, dots))
            {
                // The mode-0 STAT rise carries the second-half halt law
                // — the same shape as the line-start OAM pulses — but
                // its dot moves with SCX/sprites/window, so the half is
                // decided here against the CPU's M-cycle: a rise in the
                // second half (PPU dots 3-4 within the cycle; the last
                // dot in double speed) is readable at once and fully
                // visible to the running CPU's interrupt sample, yet
                // missed by the halt-exit sampler for one M-cycle
                // (SameBoy GB_cpu_run samples the halt exit mid-cycle).
                // mooneye hblank_ly_scx_timing-GS and the gbmicrotest
                // int_hblank_halt_scx0-7 grid pin all eight SCX phases
                // between them.
                self.if_late |= IF_STAT_BIT;
            }
            if self.ppu.take_m0_access_flip() {
                // The OAM/VRAM accessibility unblock trails the IRQ rise by
                // one half-dot (gambatte m0Time = xpos lcd_hres+7 vs the IRQ
                // at +6). A CPU OAM read samples at the cc+2 MID phase — two
                // dots before this M-cycle's end-sampled view — so when the
                // unblock lands in the cycle's second half it still reads
                // mode 3 ($FF). The IRQ, mode-bit flip and every other
                // access keep the end view; only the OAM read consults this
                // (gambatte oam_access/postread_*). The edge is stamped with
                // its dot-END commit eighth ([`event_phase`]); the read decides
                // blocking against the single CPU-access observer phase
                // [`ACCESS_PHASE`] ([`stamp_blocks`]). Sub-dot event-phase
                // model, increment 1.
                self.m0_access_edge = Some(event_phase(EdgeKind::M0Access, i, dots));
            }
            if self.ppu.take_pal_access_flip() {
                // The CGB palette-RAM unblock commits at the M-cycle end
                // ([`event_phase`] gives `PalAccess` the whole-M-cycle block):
                // the FF69/FF6B read stays $FF for the entire straddle M-cycle,
                // not just its second half (gambatte cgbpal_m3end). INC-G3 task 5.
                self.pal_access_edge = Some(event_phase(EdgeKind::PalAccess, i, dots));
            }
            if self.ppu.take_m0_stat_flip() {
                // A sprite-line m3→m0 flip holds the double-speed FF41 mode bits
                // at the pre-flip mode 3 for the WHOLE straddle M-cycle
                // (`event_phase(StatMode)=END_PHASE`, INC-G3 task 6): INC-DS-1's
                // dot-END half-split caught only the +43 rows whose flip lands in
                // the M-cycle's second half; the whole-M-cycle block adds the +84
                // residual `m3stat_ds_1` rows whose flip lands in the first half
                // (gambatte sprites). Net-positive A/B trade (full-gbtr +84/−3,
                // net floor −84): the only regressions are the 3
                // `late_sizechange_sp00/01/39_ds_1` (a net-neutral in-cluster
                // swap — their `_ds_2` siblings are in the lift; whole-M-cycle
                // forces both same-line size-change reads to mode 3, the `_2`
                // want it and the `_1` do not, and no `event_phase` offset
                // separates two reads in one M-cycle). The sprite-line gate stays
                // (dropping it floors 5
                // bare-line reads at a different chain offset:
                // dma gdma/hdma_cycles_scx5_ds_2, lcd_offset m0stat_count). The
                // edge stamps the whole-M-cycle END phase ([`event_phase`]); the
                // FF41 read blocks against the single CPU-access observer phase
                // [`ACCESS_PHASE`] ([`stamp_blocks`]).
                self.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, i, dots));
            }
            // Dot-exact mode-0 entry: each visible line's hblank start
            // requests one HBlank DMA block, serviced at the head of the
            // CPU's next bus operation (gambatte video.cpp: memevent_hdma
            // fires at predictedNextM0Time). The flag is suppressed while
            // the core clock is gated (video.h EventTimes::flagHdmaReq:
            // `if (!intreq_.halted())`); the level detector keeps
            // tracking so a wake never sees a stale edge.
            let hb = self.ppu.hdma_trigger_level();
            if hb
                && !self.hdma_prev_hblank
                && self.hdma_mode == HdmaMode::ArmedLcdOn
                && !self.cpu_halted
            {
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
            self.hdma_prev_hblank = hb;
        }
        let div = self.timer.div_counter();
        self.apu.tick(div, self.double_speed);
        self.intf |= self.serial.tick(div) & IF_MASK & !tick_squash;
        self.intf |= self.joypad.take_irq() & IF_MASK;
        // RTC wall time is dot time (2 dots per M-cycle in double speed).
        self.cart.tick_rtc(dots as u32);
    }

    // ---- OAM DMA engine ------------------------------------------------

    /// Gate (true) or ungate (false) the OAM DMA controller's clock.
    ///
    /// The OAM DMA controller is clocked by the CPU core clock, which HALT
    /// (and STOP) switches off while the PPU keeps running on its own clock.
    /// A transfer in progress therefore does not proceed while the CPU is
    /// halted: bytes already copied stay, the byte in flight never commits,
    /// and the rest of OAM keeps its old contents — the PPU renders from
    /// that mixture for as long as the CPU sleeps. Hardware-verified by
    /// mooneye madness/mgb_oam_dma_halt_sprites.s ("OAM DMA is in the middle
    /// of OAM access (but not proceeding with it!)"); its observed sprite
    /// data pins the freeze mid-byte, with the overwritten OAM byte intact.
    ///
    /// Called by the CPU wiring on halt/stop entry and exit (via
    /// [`Bus::set_halted`]); the halted CPU performs no bus accesses on
    /// hardware, so the CPU-visible bus state during the freeze is
    /// unobservable and no bus conflict is modelled.
    ///
    /// While a transfer sits frozen mid-byte, the PPU is handed the frozen
    /// access (OAM index about to be replaced + in-flight source byte): the
    /// DMA controller is "in the middle of OAM access (but not proceeding
    /// with it!)" and the MGB PPU's OAM scan sees glitched data derived
    /// from exactly these bytes (madness/mgb_oam_dma_halt_sprites.s; see
    /// `Ppu::set_oam_dma_freeze`). A freeze during the setup delay has no
    /// OAM access in flight and hands over nothing.
    pub fn set_cpu_halted(&mut self, halted: bool) {
        if self.cpu_halted == halted {
            return;
        }
        if halted {
            // gambatte Memory::halt: a flagged-but-unserviced block
            // request is deferred (hdma_requested) and re-flagged at
            // wake — HBlank DMA never proceeds while the core clock is
            // gated; otherwise remember whether the hblank window was
            // already active so the same hblank cannot retrigger at wake.
            self.halt_hdma = if self.vram_dma_req.take().is_some() {
                HaltHdmaState::Requested
            } else if self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period() {
                HaltHdmaState::High
            } else {
                HaltHdmaState::Low
            };
        }
        self.engage_halt_gate(halted);
        if !halted {
            // The halt-mode wake restarts the OAM DMA controller's clock
            // one M-cycle ahead of the CPU pipeline: a single catch-up
            // cycle runs at the wake itself, before the CPU's first
            // post-wake M-cycle (SameBoy sm83_cpu.c `GB_cpu_run` halt
            // exit: `gb->dma_cycles = 4; GB_dma_run(gb)` on both the
            // IME=0 resume and the dispatch path, while `GB_dma_run`
            // returns early whenever `gb->halted`; hardware-pinned by
            // gambatte oamdma/oamdmasrc80_halt_*_read8000 out81 and
            // dma/hdma_transition_oamdma_2 out67, which read the
            // in-flight source index after a wake). The speed-switch
            // pause's gate release deliberately does NOT take this path:
            // oamdma/oamdmasrcC0_speedchange_readC000 out11 pins the
            // un-caught-up resume there (SameBoy's
            // speed_switch_halt_countdown expiry likewise skips the
            // catch-up). The conflict state left behind is unobservable —
            // every CPU bus access ticks the machine, refreshing it,
            // before the access.
            self.oam_dma_tick();
            // The catch-up byte commits at the wake itself (SameBoy's
            // GB_dma_run writes within the call); no PPU dots run before
            // the next machine cycle's head, so this is indistinguishable
            // from the regular deferred commit to the scan.
            self.oam_dma_commit_pending();
            self.vram_dma_unhalt();
        }
    }

    /// The raw core-clock gate: freezes the OAM DMA controller and hands
    /// the frozen access to the PPU (see [`Self::set_cpu_halted`] for the
    /// HBlank-DMA bookkeeping layered on top; `Interconnect::stop` drives
    /// this directly because the speed-switch pause sequences the HDMA
    /// state itself).
    fn engage_halt_gate(&mut self, halted: bool) {
        self.cpu_halted = halted;
        let freeze = if halted {
            self.dma_run
                .as_ref()
                .map(|run| (run.idx, self.oam_dma_source_read(run.src, run.idx)))
        } else {
            None
        };
        self.ppu.set_oam_dma_freeze(freeze);
    }

    /// Commit the previous M-cycle's OAM DMA byte to OAM. gambatte
    /// timestamps each copy at the *end* of its M-cycle (memory.cpp
    /// `updateOamDma`: `lastOamDmaUpdate_ += 4` before the
    /// `ioamhram_[oamDmaPos_]` store), so the PPU dots of the copying
    /// cycle still see the old byte — the mode-2 scan latch racing the
    /// transfer's first byte depends on it (late_sp01x/02x `_1`: the
    /// slot's Y is rewritten by byte 0 in the very cycle the scan latches
    /// it, and hardware still selects the old sprite). Runs at the head
    /// of every controller advance, before this cycle's copy is staged.
    fn oam_dma_commit_pending(&mut self) {
        if let Some((idx, byte)) = self.dma_pending_oam.take() {
            self.ppu.oam_dma_write(idx, byte);
        }
    }

    fn oam_dma_tick(&mut self) {
        self.dma_conflict = None;
        self.oam_dma_commit_pending();
        // The PPU's OAM view for this M-cycle's dots: disconnected while
        // the controller owns OAM, so the mode-2 scan latches $FF
        // (gambatte memory.cpp startOamDma/endOamDma switch the
        // OamReader's source to rdisabledRam). Both edges trail the
        // controller state by one M-cycle: gambatte timestamps
        // startOamDma at the byte-0 copy step — the end of our promoting
        // cycle, whose dots therefore still latch real OAM — and
        // endOamDma one position step *past* byte 159, so the dots of
        // the cycle after the last copy are still disconnected (the
        // `prev ||` term). The level deliberately keeps refreshing
        // through the halted/bus-stolen early returns below: a frozen
        // transfer owns OAM for the whole freeze
        // (oamdma_late_halt_stat/late_speedchange_stat).
        let owned = self.dma_run.is_some();
        self.ppu
            .set_oam_dma_active(owned || self.dma_oam_owned_prev);
        self.dma_oam_owned_prev = owned;
        // HALT/STOP gate the CPU core clock that drives this controller:
        // neither the setup delay nor the transfer advances while the CPU
        // is halted (see set_cpu_halted; madness/mgb_oam_dma_halt_sprites).
        // While a VRAM DMA owns the bus the controller cannot read its
        // source either: it advances via [`Self::oam_dma_bus_capture`]
        // from the steal loop instead of here.
        if self.cpu_halted || self.vram_dma_owns_bus {
            return;
        }
        self.oam_dma_promote_start();
        if let Some(run) = self.dma_run {
            let byte = self.oam_dma_source_read(run.src, run.idx);
            self.dma_pending_oam = Some((run.idx, byte));
            self.dma_conflict = Some(DmaConflict {
                kind: self.dma_src_kind(run.src),
                src_hi: (run.src >> 8) as u8,
                idx: run.idx,
                byte,
            });
            self.dma_run = (run.idx < 159).then_some(OamDmaRun {
                idx: run.idx + 1,
                ..run
            });
        }
    }

    /// Promote a pending start whose setup delay has elapsed. The old
    /// transfer (if any) keeps copying during the delay cycle
    /// (acceptance/oam_dma_restart) and is replaced exactly when the new
    /// one copies its first byte. Shared by the controller's own clock
    /// ([`Self::oam_dma_tick`]) and the VRAM-DMA steal advances
    /// ([`Self::oam_dma_bus_capture`]): gambatte's `oamDmaPos_` counts
    /// toward `oamDmaStartPos_` identically on both paths.
    fn oam_dma_promote_start(&mut self) {
        match &mut self.dma_start {
            Some(s) if s.delay == 0 => {
                let src = s.src;
                self.dma_start = None;
                self.dma_run = Some(OamDmaRun { src, idx: 0 });
            }
            Some(s) => s.delay -= 1,
            None => {}
        }
    }

    /// One OAM DMA controller advance during a VRAM-DMA bus steal: the
    /// controller cannot perform its own source read — the VRAM DMA owns
    /// the bus — so it latches the stolen transfer's bus traffic instead,
    /// writing `data` to the OAM cell addressed by the *bus address* low
    /// byte (`src & 0xFF`, NOT the controller's own position), or to the
    /// CGB-C extra OAM RAM with the usual bits-3/4 alias for low bytes ≥
    /// $A0. The position still advances, so the skipped source bytes are
    /// never copied (gambatte-core memory.cpp `dma()`:
    /// `ioamhram_[src & 0xFF] = data` / `ioamhram_[p & 0xE7] = data`
    /// per `oamDmaPos_` advance, gated on `!halted()`; hardware-pinned by
    /// gambatte dma/hdma_transition_oamdma_1/_2 and
    /// oamdma/oamdmasrcC000_hdmasrc0000).
    ///
    /// Called once per stolen M-cycle with that cycle's *last* byte: at
    /// normal speed gambatte's 4-cc advance grid (`cc - 3 >
    /// lOamDmaUpdate`, both M-cycle-aligned) lands every advance on the
    /// second of the two bytes copied per M-cycle; in double speed each
    /// M-cycle copies one byte and every byte advances.
    fn oam_dma_bus_capture(&mut self, src: u16, data: u8) {
        // The speed-switch pause can service a block while the core
        // clock is gated: the OAM DMA controller is frozen with the CPU
        // and does not advance (gambatte dma(): `&& !halted()`).
        if self.cpu_halted {
            return;
        }
        self.oam_dma_promote_start();
        if let Some(run) = self.dma_run {
            let p = src & 0xFF;
            if p < 0xA0 {
                self.ppu.oam_dma_write(p as u8, data);
            } else if self.model == Model::Cgb {
                // AGB skips the extra-RAM write (gambatte `!agbFlag_`).
                self.extra_oam[Self::extra_oam_index(src)] = data;
            }
            self.dma_run = (run.idx < 159).then_some(OamDmaRun {
                idx: run.idx + 1,
                ..run
            });
        }
    }

    /// Source class of a transfer from `src` (gambatte-core memory.cpp
    /// `oamDmaInitSetup`; see [`DmaSrcKind`]).
    fn dma_src_kind(&self, src: u16) -> DmaSrcKind {
        match src >> 8 {
            0x00..=0x7F => DmaSrcKind::Rom,
            0x80..=0x9F => DmaSrcKind::Vram,
            0xA0..=0xBF => DmaSrcKind::Sram,
            0xC0..=0xDF => DmaSrcKind::Wram,
            _ if self.model.is_cgb() => DmaSrcKind::Invalid,
            _ => DmaSrcKind::Wram,
        }
    }

    /// What the OAM DMA engine reads for byte `idx` of a transfer from
    /// `src`. Mode-based PPU blocking does not apply; ROM/SRAM/VRAM
    /// banking is live (gambatte memory.cpp `oamDmaSrcPtr`).
    fn oam_dma_source_read(&self, src: u16, idx: u8) -> u8 {
        let addr = src + u16::from(idx);
        match self.dma_src_kind(src) {
            DmaSrcKind::Rom => self.cart.read_rom(addr),
            DmaSrcKind::Vram => self.ppu.vram_read_raw(addr),
            DmaSrcKind::Sram => self.cart.read_ram(addr),
            // DMG $E0-$FF: incomplete address decoding re-reads WRAM
            // (acceptance/oam_dma/sources-GS: $FE/$FF read $DE00/$DF00).
            DmaSrcKind::Wram => {
                let addr = if addr >= 0xE000 { addr - 0x2000 } else { addr };
                self.wram[self.wram_index(addr)]
            }
            DmaSrcKind::Invalid => 0xFF,
        }
    }

    /// Whether a CPU access to `addr` collides with the running transfer's
    /// bus. One 16-bit mask per source class, bit n = 4 KiB page n
    /// conflicts (transcribed from gambatte-core memptrs.cpp
    /// `OamDmaConflictMap`; the FE/FF page never conflicts):
    ///
    /// * ROM/SRAM sources drive the external bus; on CGB the WRAM pages
    ///   C-F conflict *too*, with accesses redirected to WRAM (see
    ///   [`Self::dma_redirect_wram_index`]).
    /// * VRAM sources collide only with the VRAM pages 8-9.
    /// * WRAM sources collide with everything but VRAM on DMG (WRAM sits
    ///   on the external bus there) but only with pages C-F on CGB (its
    ///   own bus).
    fn in_dma_conflict_area(&self, kind: DmaSrcKind, addr: u16) -> bool {
        let pages: u16 = match kind {
            DmaSrcKind::Rom | DmaSrcKind::Sram | DmaSrcKind::Invalid => 0xFCFF,
            DmaSrcKind::Vram => 0x0300,
            DmaSrcKind::Wram if self.model.is_cgb() => 0xF000,
            DmaSrcKind::Wram => 0xFCFF,
        };
        addr < 0xFE00 && pages >> (addr >> 12) & 1 != 0
    }

    /// CGB conflict redirect for WRAM-region accesses during a non-WRAM
    /// transfer: the cell actually accessed is WRAM page 0 or the banked
    /// page — chosen by FF46 bit 4, not by the address — at offset
    /// `addr & 0xFFF` (gambatte memory.cpp:
    /// `cart_.wramdata(ioamhram_[0x146] >> 4 & 1)[p & 0xFFF]`; pinned by
    /// oamdma_srcXXXX_busypopDFFF/busypushC001+ cgb04c rows).
    fn dma_redirect_wram_index(&self, c: &DmaConflict, addr: u16) -> usize {
        if c.src_hi & 0x10 != 0 {
            self.wram_index(0xD000 | (addr & 0x0FFF))
        } else {
            usize::from(addr & 0x0FFF)
        }
    }

    // ---- CGB VRAM DMA ---------------------------------------------------

    /// Service any flagged VRAM-DMA request at the *head* of a CPU bus
    /// operation, before that operation's machine tick: the bus steal sits
    /// between the M-cycle whose dots flagged the request and the CPU's
    /// next M-cycle. M-cycle (not instruction) granularity is what the
    /// gambatte hdma_late_destl/_length/_wrambank and hdma_start `_1`/`_2`
    /// adjacent-cycle pairs resolve, and they also split by access type:
    /// a *write* whose own cycle contains the trigger commits before the
    /// steal (hdma_late_destl_1: the racing FF54 write wins), while a
    /// *read* in the trigger cycle yields — the DMA owns the bus before
    /// the read samples (hdma_start_2: the fetch's data byte is the
    /// post-block value), hence the second `service_vram_dma` call after
    /// the tick in `Bus::read`/`read_inc`. A request flagged during the
    /// STOP opcode fetch is deliberately still pending when `Bus::stop`
    /// runs (no bus operation in between) — the
    /// hdma_transition_speedchange matrix needs exactly that request.
    fn service_vram_dma(&mut self) {
        while self.vram_dma_req.is_some() && !self.vram_dma_stall {
            self.run_vram_dma();
        }
    }

    /// Service one flagged VRAM-DMA request, stalling the CPU while the
    /// rest of the machine keeps running. Pace: 2 bytes per stolen M-cycle
    /// at normal speed, 1 in double speed (gambatte memory.cpp `dma()`:
    /// `cc += 2 + 2 * doubleSpeed` per byte), plus one teardown M-cycle
    /// per service (`cc += 4`) — skipped when the block was deferred by a
    /// halt wake (`Memory::event` intevent_dma: `cc -= 4` for an
    /// unhalt-requested block).
    fn run_vram_dma(&mut self) {
        let Some(req) = self.vram_dma_req.take() else {
            return;
        };
        self.vram_dma_stall = true;
        let mut remaining = (usize::from(self.hdma5 & 0x7F) + 1) * 0x10;
        let mut length = if req == VramDmaReq::Gdma {
            remaining
        } else {
            0x10
        };
        // The full 16-bit destination counter terminates the transfer at
        // the 0x10000 crossing, latching FF55 bit 7 (gambatte dma():
        // `if (dmaDest + length >= 0x10000) { length = 0x10000 - dmaDest;
        // ioamhram_[0x155] |= 0x80; }`; hardware capture dma/dma_dst_wrap_2
        // — the transfer does NOT wrap back into VRAM).
        let to_wrap = 0x1_0000 - usize::from(self.hdma_dst);
        if length >= to_wrap {
            length = to_wrap;
            self.hdma5 |= 0x80;
        }
        remaining -= length;
        // A GDMA with the display off always retires its whole length
        // (gambatte dma(): `if (!(lcdc & en) && gdmaReqFlagged) dmaLength
        // = 0`), so a 0x10000-truncated copy still reads back $FF.
        if req == VramDmaReq::Gdma && !self.ppu.lcd_enabled() {
            remaining = 0;
        }
        let per_m = if self.double_speed { 1 } else { 2 };
        while length > 0 {
            // The byte-copy M-cycles own the bus: the OAM DMA controller's
            // own clocking is suppressed (the teardown cycle below is a
            // plain machine cycle again — gambatte restores
            // lastOamDmaUpdate_ before its `cc += 4`).
            self.vram_dma_owns_bus = true;
            self.tick_machine();
            self.vram_dma_owns_bus = false;
            for i in 0..per_m.min(length) {
                let src = self.hdma_src;
                let byte = self.vram_dma_source_read(src);
                self.ppu
                    .vram_write_raw(0x8000 | (self.hdma_dst & 0x1FFF), byte);
                self.hdma_src = src.wrapping_add(1);
                self.hdma_dst = self.hdma_dst.wrapping_add(1);
                length -= 1;
                if i + 1 == per_m {
                    // A concurrent OAM DMA advances once per stolen
                    // M-cycle, latching this cycle's last bus byte.
                    self.oam_dma_bus_capture(src, byte);
                }
            }
        }
        if req != VramDmaReq::HblankUnhalt {
            self.tick_machine(); // teardown (gambatte dma(): cc += 4)
        }
        if self.cpu_halted {
            // A block serviced while the core clock is gated — only the
            // speed-switch pause can do this (see `Interconnect::stop`) —
            // aborts the HBlank transfer: FF55 keeps its length bits and
            // gains bit 7 (gambatte dma(): `ioamhram_[0x155] = halted() ?
            // ioamhram_[0x155] | 0x80 : …`; pinned by
            // hdma_transition_speedchange_hdmalen*_hdma5).
            self.hdma5 |= 0x80;
        } else {
            self.hdma5 = ((remaining / 0x10) as u8).wrapping_sub(1) | (self.hdma5 & 0x80);
        }
        if self.hdma5 & 0x80 != 0 {
            // Completion/termination/abort disarms the HBlank engine
            // (gambatte dma(): `if ((FF55 & 0x80) && hdmaIsEnabled())
            // lcd_.disableHdma(cc)`).
            self.hdma_mode = HdmaMode::Disabled;
        }
        self.vram_dma_stall = false;
    }

    /// Wake-side HBlank-DMA re-evaluation, shared by halt/stop wake and
    /// the end of the speed-switch pause (gambatte Memory::event,
    /// `intevent_unhalt` and the halted `intevent_interrupts` path): a
    /// deferred request re-flags (without the teardown cycle), and an
    /// armed transfer whose halt began outside the hblank window fires if
    /// the wake lands inside one.
    fn vram_dma_unhalt(&mut self) {
        match self.halt_hdma {
            HaltHdmaState::Requested => self.vram_dma_req = Some(VramDmaReq::HblankUnhalt),
            HaltHdmaState::Low
                if self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period() =>
            {
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
            _ => {}
        }
        self.halt_hdma = HaltHdmaState::Low;
    }

    /// VRAM DMA source read. VRAM itself and the 0xE000+ region are not
    /// valid sources (Pan Docs); they read as 0xFF here (SameBoy
    /// Core/memory.c GB_hdma_run drives the bus only for ROM/SRAM/WRAM
    /// sources and leaves the idle data-bus byte for everything else —
    /// `gdma_invalid_sources_fill_destination_with_ff`; gambatte reads its
    /// decaying cart-bus latch, unmodelled here).
    fn vram_dma_source_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xDFFF => self.wram[self.wram_index(addr)],
            _ => 0xFF,
        }
    }

    /// FF55 write (gambatte memory.cpp nontrivial_ff_write case 0x55; the
    /// `hdma_mode` branches mirror `lcd_.hdmaIsEnabled()`).
    fn hdma5_write(&mut self, value: u8) {
        // The write always replaces the live length bits — cancelling
        // latches the *written* length with bit 7 set, not the old
        // remaining count (gambatte: `ioamhram_[0x155] = data & 0x7F`
        // before the cancel branch; SameBoy memory.c GB_IO_HDMA5 likewise
        // sets hdma_steps_left first).
        self.hdma5 = value & 0x7F;
        if self.hdma_mode != HdmaMode::Disabled {
            if value & 0x80 == 0 {
                self.hdma5 |= 0x80;
                self.hdma_mode = HdmaMode::Disabled;
            }
            // Bit 7 set while armed: only the length bits change.
        } else if value & 0x80 != 0 {
            // gambatte video.cpp enableHdma: with the LCD off one block
            // copies immediately (flagHdmaReq, SameBoy: `(STAT & 3) == 0
            // && display_state != 7 → hdma_on = true`); with the LCD on a
            // block fires at once only inside the hblank window.
            if self.ppu.lcd_enabled() {
                self.hdma_mode = HdmaMode::ArmedLcdOn;
                if self.ppu.hdma_period() {
                    self.vram_dma_req = Some(VramDmaReq::Hblank);
                }
            } else {
                self.hdma_mode = HdmaMode::ArmedLcdOff;
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
        } else {
            // General-purpose DMA: requested now, serviced at the next
            // instruction boundary (gambatte flagGdmaReq). Note a stale
            // active-looking FF55 (HBlank arming killed by an LCD
            // disable) reaches this branch too, exactly like upstream.
            self.vram_dma_req = Some(VramDmaReq::Gdma);
        }
    }

    // ---- memory routing -------------------------------------------------

    fn wram_index(&self, addr: u16) -> usize {
        let offset = usize::from(addr & 0x1FFF);
        if offset < 0x1000 {
            offset
        } else {
            let bank = if self.model.is_cgb() {
                usize::from(self.svbk & 7).max(1)
            } else {
                1
            };
            bank * 0x1000 + (offset - 0x1000)
        }
    }

    /// DMG OAM corruption bug (Pan Docs "OAM Corruption Bug"): a CPU
    /// access cycle with a $FE00-$FEFF value on the address bus while the
    /// PPU's mode-2 scan is on a corruptible row mangles that row.
    /// `addr` is the bus value: the access address for reads/writes, the
    /// 16-bit inc/dec unit's register value for internal cycles
    /// ([`Bus::tick_addr`]). The whole page triggers, including the
    /// FEA0-FEFF prohibited area (SameBoy keys on `addr < 0xFF00`; blargg
    /// oam_bug/8-instr_effect pops from $FEF0). Suppressed
    ///
    /// * on CGB/AGB — the bug is DMG-family-only,
    /// * while the core clock is gated off — the halted CPU performs no
    ///   bus accesses on hardware, keeping the discarded halt prefetch
    ///   (see `cpu::execute::step`) phantom-free, and
    /// * while the OAM DMA engine owns OAM — the interplay is untested on
    ///   hardware (SameBoy carries the same Todo), so the conservative
    ///   gate wins; `dma_conflict` is `Some` exactly in the M-cycles a
    ///   DMA byte copies.
    fn maybe_oam_bug(&mut self, addr: u16, kind: OamBugKind) {
        if !(0xFE00..=0xFEFF).contains(&addr)
            || self.model.is_cgb()
            || self.cpu_halted
            || self.dma_conflict.is_some()
        {
            return;
        }
        self.ppu.oam_bug(kind);
    }

    /// Cell of [`Self::extra_oam`] a FEA0-FEFF access decodes to: bits 3-4
    /// of the low address byte are ignored (gambatte-core memory.cpp masks
    /// the OAM-relative offset with $E7), leaving 24 cells — offsets
    /// $A0-$A7/$C0-$C7/$E0-$E7 — each mirrored 4 times.
    fn extra_oam_index(addr: u16) -> usize {
        let masked = usize::from(addr & 0xE7);
        (masked >> 5) * 8 + (masked & 7) - 40
    }

    /// FEA0-FEFF "prohibited" reads (Pan Docs "FEA0-FEFF range").
    ///
    /// * DMG family: $00 while OAM is idle, $FF while the PPU has OAM
    ///   locked (the mode-2 corruption itself lives in
    ///   [`Self::maybe_oam_bug`]).
    /// * CPU CGB C ([`Model::Cgb`], ARCHITECTURE §CGB revision policy):
    ///   extra OAM RAM behind the same lockout as OAM proper
    ///   ([`Self::extra_oam`]; gambatte oamdma busypushFEA1/FF01 rows).
    /// * AGB (and CGB revision E): the high nibble of the low address byte
    ///   twice.
    fn prohibited_read(&self, addr: u16) -> u8 {
        match self.model {
            Model::Cgb => {
                // The CGB FEA0-FEFF extra OAM RAM mirrors OAM read
                // blocking, including the cc+2 MID-phase second-half
                // unblock view (sub-dot event-phase model).
                if self.ppu.oam_read_blocked() || stamp_blocks(self.m0_access_edge, ACCESS_PHASE) {
                    0xFF
                } else {
                    self.extra_oam[Self::extra_oam_index(addr)]
                }
            }
            Model::Agb => {
                let lo = addr as u8;
                (lo & 0xF0) | (lo >> 4)
            }
            _ if self.ppu.mode_bits() >= 2 => 0xFF,
            _ => 0x00,
        }
    }

    /// Write counterpart of [`Self::prohibited_read`]: only the CGB-C
    /// extra OAM RAM is writable, under the same gating as OAM proper
    /// (dropped while the PPU scans or a DMA byte is in flight; the
    /// in-flight gate is gambatte memory.cpp's `oamDmaPos_ < oam_size`).
    fn prohibited_write(&mut self, addr: u16, value: u8) {
        if self.model == Model::Cgb && self.dma_conflict.is_none() && !self.ppu.oam_write_blocked()
        {
            self.extra_oam[Self::extra_oam_index(addr)] = value;
        }
    }

    fn read_no_tick(&mut self, addr: u16) -> u8 {
        if let Some(c) = self.dma_conflict {
            // OAM (and the prohibited area behind it) reads $FF while a DMA
            // byte is in flight (gbctr OAM DMA).
            if (0xFE00..=0xFEFF).contains(&addr) {
                return 0xFF;
            }
            // Reads on conflicting pages see the DMA engine's bus, except
            // the CGB WRAM-region redirect (gambatte-core memory.cpp
            // nontrivial_read's OAM-DMA conflict block).
            if self.in_dma_conflict_area(c.kind, addr) {
                if self.model.is_cgb() && c.kind != DmaSrcKind::Wram && addr >= 0xC000 {
                    return self.wram[self.dma_redirect_wram_index(&c, addr)];
                }
                if self.model.is_cgb() && c.kind == DmaSrcKind::Vram {
                    // CGB quirk: the conflicted read also clears the
                    // in-flight OAM byte (gambatte memory.cpp:
                    // `ioamhram_[oamDmaPos_] = 0` after a vram-source
                    // conflict read) — replacing the cycle's pending
                    // byte, like a derailed write.
                    self.dma_pending_oam = Some((c.idx, 0));
                }
                return c.byte;
            }
        }
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            // cc+2 MID-phase VRAM read: same mode-3→mode-0 unblock edge as
            // OAM below — a second-half unblock is not yet visible here
            // (sub-dot event-phase model, increment 2). Suppressed while an
            // HDMA is armed: the HDMA service seam writes VRAM at the same
            // mode-0 entry and its read-back interaction (gambatte
            // dma/hdma_start_*) is the HDMA-seam increment's job.
            0x8000..=0x9FFF
                if stamp_blocks(self.m0_access_edge, ACCESS_PHASE)
                    && self.hdma_mode == HdmaMode::Disabled =>
            {
                0xFF
            }
            0x8000..=0x9FFF => self.ppu.read(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
            0xFE00..=0xFE9F => {
                // cc+2 MID-phase OAM read: a mode-3→mode-0 unblock landing
                // in this M-cycle's second half is not yet visible here
                // (sub-dot event-phase model, increment 1).
                if stamp_blocks(self.m0_access_edge, ACCESS_PHASE) {
                    0xFF
                } else {
                    self.ppu.read(addr)
                }
            }
            0xFEA0..=0xFEFF => self.prohibited_read(addr),
            0xFF00..=0xFF7F => self.io_read(addr),
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)],
            0xFFFF => self.ie,
        }
    }

    fn write_no_tick(&mut self, addr: u16, value: u8) {
        // A CPU write on a conflicting page derails: the addressed memory
        // (including MBC registers) never sees it. On DMG the byte lands
        // in the in-flight OAM slot — wire-ANDed with the DMA byte for
        // WRAM sources, as-is otherwise; CGB additionally zeroes the data
        // for VRAM sources and redirects WRAM-region writes to WRAM
        // instead of OAM (gambatte-core memory.cpp nontrivial_write's
        // OAM-DMA conflict block; pinned by the gambatte
        // oamdma_srcXXXX_busypush/busypop matrix).
        if let Some(c) = self.dma_conflict {
            if self.in_dma_conflict_area(c.kind, addr) {
                // The derailed byte rides the transfer's own OAM write
                // slot, so it replaces the cycle's pending byte and lands
                // at the same end-of-cycle commit point
                // ([`Self::oam_dma_commit_pending`]).
                if self.model.is_cgb() {
                    if addr < 0xC000 {
                        let byte = if c.kind == DmaSrcKind::Vram { 0 } else { value };
                        self.dma_pending_oam = Some((c.idx, byte));
                    } else if c.kind != DmaSrcKind::Wram {
                        let i = self.dma_redirect_wram_index(&c, addr);
                        self.wram[i] = value;
                    }
                    // WRAM source + WRAM region: the write is swallowed.
                } else {
                    let byte = if c.kind == DmaSrcKind::Wram {
                        c.byte & value
                    } else {
                        value
                    };
                    self.dma_pending_oam = Some((c.idx, byte));
                }
                return;
            }
        }
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, value),
            // cc+2 MID-phase VRAM write: a mode-3→mode-0 unblock landing in
            // this M-cycle's second half is not yet visible here, so the
            // write is still locked out (dropped) — same edge/sub-dot
            // phase as the OAM/VRAM read (sub-dot event-phase model).
            0x8000..=0x9FFF if stamp_blocks(self.m0_access_edge, ACCESS_PHASE) => {}
            0x8000..=0x9FFF => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xA000..=0xBFFF => self.cart.write_ram(addr, value),
            0xC000..=0xFDFF => {
                let i = self.wram_index(addr);
                self.wram[i] = value;
            }
            0xFE00..=0xFE9F => {
                // CPU OAM writes are dropped while DMA owns OAM, and while
                // the cc+2 MID view still reads mode 3 (sub-dot phase).
                if self.dma_conflict.is_none() && !stamp_blocks(self.m0_access_edge, ACCESS_PHASE) {
                    self.intf |= self.ppu.write(addr, value) & IF_MASK;
                }
            }
            0xFEA0..=0xFEFF => self.prohibited_write(addr, value),
            0xFF00..=0xFF7F => self.io_write(addr, value),
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)] = value,
            0xFFFF => self.ie = value,
        }
    }

    fn io_read(&self, addr: u16) -> u8 {
        match addr {
            0xFF00 => self.joypad.read(),
            0xFF01 | 0xFF02 => self.serial.read(addr),
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF0F => 0xE0 | self.intf,
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF46 => self.dma_reg,
            // The STAT mode bits read at the cc+2 MID phase: in double speed
            // a read whose M-cycle straddles a sprite-line mode-3→mode-0 flip
            // still reads mode 3 for the WHOLE straddle M-cycle
            // (`event_phase(StatMode)=END_PHASE`, INC-G3 task 6 — not only a
            // 2nd-half flip), where the whole-dot end view has already flipped
            // to mode 0 (gambatte sprites m3stat_ds). Only the low mode bits
            // move; the enable bits and the live LYC compare keep the end view.
            // The `double_speed` gate is load-bearing, not belt-and-braces:
            // single-speed FF41 m3stat reads share this one stamp but want the
            // end view (cross-oracle — see
            // `stat_mode_override_requires_double_speed`). The LCD-on guard is
            // belt-and-braces: the flag is only set by a live flip (LCD on) and
            // reset every tick, so an LCD-off read can never carry it, but the
            // guard keeps the override from ever forcing mode 3 over the LCD-off
            // mode 0. Sub-dot event-phase model, INC-DS-1 + INC-G3 task 6.
            0xFF41
                if stamp_blocks(self.stat_mode_edge, ACCESS_PHASE)
                    && self.double_speed
                    && self.ppu.lcd_enabled() =>
            {
                self.ppu.read(0xFF41) | 0x03
            }
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B => self.ppu.read(addr),
            0xFF4D if self.cgb_mode => {
                0x7E | (u8::from(self.double_speed) << 7) | u8::from(self.key1_armed)
            }
            // VBK reads $FE|bank on CGB even in DMG mode (boot_hwio-C).
            0xFF4F => self.ppu.read(addr),
            // FF55 reads the live register verbatim: remaining blocks - 1
            // while a HBlank transfer runs, bit 7 set when none is
            // registered (gambatte ioamhram_[0x155]).
            0xFF55 if self.cgb_mode => self.hdma5,
            // RP: bits 2-5 unimplemented (1), bit 1 = received signal,
            // active low — no peer, so never receiving.
            0xFF56 if self.cgb_mode => 0x3C | (self.rp & 0xC1) | 0x02,
            // BCPS/OCPS stay readable in DMG-compat mode (boot_hwio-C reads
            // the boot leftovers $C8/$D0); the data ports do not.
            0xFF68 | 0xFF6A => self.ppu.read(addr),
            // cc+2 MID-phase CGB palette read: the pipe-end unblock commits at
            // the M-cycle end (whole-M-cycle block, see `pal_access_edge` /
            // [`event_phase`]), so the read stays $FF for the entire straddle
            // M-cycle and becomes readable only next M-cycle (sub-dot
            // event-phase model, INC-G3 task 5).
            0xFF69 | 0xFF6B
                if self.cgb_mode && stamp_blocks(self.pal_access_edge, ACCESS_PHASE) =>
            {
                0xFF
            }
            0xFF69 | 0xFF6B if self.cgb_mode => self.ppu.read(addr),
            0xFF6C => self.ppu.read(addr),
            0xFF70 if self.cgb_mode => 0xF8 | self.svbk,
            0xFF72 if self.model.is_cgb() => self.ff72,
            0xFF73 if self.model.is_cgb() => self.ff73,
            0xFF74 if self.cgb_mode => self.ff74,
            0xFF75 if self.model.is_cgb() => 0x8F | (self.ff75 & 0x70),
            // FF76/FF77 (PCM12/PCM34): read-only per-channel 4-bit digital
            // outputs (Pan Docs "PCM amplitude readouts").
            0xFF76 if self.model.is_cgb() => self.apu.pcm12(),
            0xFF77 if self.model.is_cgb() => self.apu.pcm34(),
            // FF50 (boot ROM disable) and everything unmapped: $FF.
            _ => 0xFF,
        }
    }

    fn io_write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF00 => self.joypad.write(value),
            0xFF01 | 0xFF02 => self.serial.write(addr, value),
            // A timer write never requests IF directly: a write-induced TIMA
            // overflow raises it only at the reload, from `Timer::tick`.
            0xFF04..=0xFF07 => {
                if addr == 0xFF04 {
                    // The DIV-reset falling edge must reach the serial
                    // clock within this cycle: the once-per-M-cycle
                    // sampled tick would miss it for the CGB fast clock,
                    // whose DIV bit is high again by the next sample
                    // (`Serial::div_write`; gambatte serial/
                    // start83_late_div_write_*).
                    let iff = self.serial.div_write(self.timer.div_counter());
                    self.intf |= iff & IF_MASK;
                    // Same for the frame sequencer: the reset's DIV-APU
                    // falling edge lands in this cycle (`Apu::div_write`).
                    self.apu.div_write(self.double_speed);
                }
                self.timer.write(addr, value)
            }
            0xFF0F => self.intf = value & IF_MASK,
            0xFF10..=0xFF3F => self.apu.write(addr, value),
            0xFF46 => {
                self.dma_reg = value;
                let src = u16::from(value) << 8;
                // A rewrite mid-flight retargets the running transfer
                // immediately: the handover copies before the new
                // transfer's byte 0 read the NEW source at the old
                // indices, and conflict like it (gambatte-core memory.cpp
                // FF46 handler updates ioamhram_[0x146] and re-runs
                // oamDmaInitSetup before the next copy; pinned by gambatte
                // oamdma_src8000_srcchange0000_busyread0000_1/2 — mooneye
                // oam_dma_restart restarts with the same page and cannot
                // discriminate).
                if let Some(run) = &mut self.dma_run {
                    run.src = src;
                }
                self.dma_start = Some(OamDmaStart { src, delay: 1 });
            }
            // PPU register writes can raise the STAT line in this very
            // cycle (stat_lyc_onoff round 4): `Ppu::write` returns the IF
            // bits the write raised, OR-ed in immediately.
            0xFF40 => {
                let was_on = self.ppu.lcd_enabled();
                self.intf |= self.ppu.write(addr, value) & IF_MASK;
                let now_on = self.ppu.lcd_enabled();
                if was_on && !now_on && self.hdma_mode == HdmaMode::ArmedLcdOn {
                    // Display disabled while HBlank DMA is armed: the
                    // arming dies with the LCD — FF55 keeps reading
                    // "active" but no further block ever copies until
                    // FF55 is rewritten (gambatte video.cpp lcdcChange:
                    // the disable branch sets every memevent, including
                    // memevent_hdma, to disabled_time).
                    self.hdma_mode = HdmaMode::Disabled;
                } else if !was_on && now_on && self.hdma_mode == HdmaMode::ArmedLcdOff {
                    // HBlank DMA armed while the LCD was off resumes at
                    // the new frame's mode-0 entries (gambatte
                    // lcdcChange enable branch: `if (hdmaIsEnabled())
                    // setm<memevent_hdma>(predictedNextM0Time())`).
                    self.hdma_mode = HdmaMode::ArmedLcdOn;
                }
            }
            0xFF41..=0xFF45 | 0xFF47..=0xFF4B => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF4D if self.cgb_mode => self.key1_armed = value & 1 != 0,
            0xFF4F if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            // FF51-FF54 are the *live* DMA address counters, not start
            // latches: `hdma_src`/`hdma_dst` advance as blocks copy, and a
            // register write merges into the current counter value
            // (gambatte memory.cpp cases 0x51-0x54; SameBoy GB_IO_HDMA1-4
            // agree). The high-byte writes keep the counter's full low
            // byte and apply no destination mask — the dest counter is a
            // full 16-bit register, masked only at the VRAM write
            // (`hdma_partial_src_rewrite_blends_live_counter`,
            // `gdma_continues_from_incremented_addresses`,
            // `gdma_terminates_at_dest_0x10000_crossing`).
            0xFF51 if self.cgb_mode => {
                self.hdma_src = (self.hdma_src & 0x00FF) | (u16::from(value) << 8)
            }
            0xFF52 if self.cgb_mode => {
                self.hdma_src = (self.hdma_src & 0xFF00) | u16::from(value & 0xF0)
            }
            0xFF53 if self.cgb_mode => {
                self.hdma_dst = (self.hdma_dst & 0x00FF) | (u16::from(value) << 8)
            }
            0xFF54 if self.cgb_mode => {
                self.hdma_dst = (self.hdma_dst & 0xFF00) | u16::from(value & 0xF0)
            }
            0xFF55 if self.cgb_mode => self.hdma5_write(value),
            0xFF56 if self.cgb_mode => self.rp = value & 0xC1,
            0xFF68 | 0xFF6A => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF69 | 0xFF6B if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            // OPRI is set up by the boot ROM and locked outside CGB mode.
            0xFF6C if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF70 if self.cgb_mode => self.svbk = value & 7,
            0xFF72 if self.model.is_cgb() => self.ff72 = value,
            0xFF73 if self.model.is_cgb() => self.ff73 = value,
            0xFF74 if self.cgb_mode => self.ff74 = value,
            0xFF75 if self.model.is_cgb() => self.ff75 = value & 0x70,
            // FF50 boot-disable: we start post-boot; writes are ignored.
            _ => {}
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
        self.read_no_tick(addr)
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
