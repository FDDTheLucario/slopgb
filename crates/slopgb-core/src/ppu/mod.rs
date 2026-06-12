//! Dot-accurate PPU with pixel FIFO. PPU work package.
//!
//! Stepped one dot (T-cycle) at a time by the interconnect. Mode timing must
//! be exact: variable-length mode 3 (SCX fine scroll, window, sprite fetch
//! stalls), STAT interrupt line blocking, LY=153→0 early wrap, LCD-enable
//! first-frame quirks (mooneye `acceptance/ppu/*`, `lcdon_*`).
//!
//! Renders DMG (4-shade via BGP/OBP through a configurable RGB palette) and
//! CGB (BG/OBJ palette RAM, VRAM bank 1 attributes, master priority via OPRI).
//!
//! # Scanline timeline (mooneye sources + the gbmicrotest/wilbertpol grids)
//!
//! All positions are dots within a 456-dot line, with dot 0 = the dot where
//! LY changes (the convention `lcdon_timing-GS` measurements decode to).
//! "state(T)" below means the state a CPU read observes after T dots have
//! been ticked.
//!
//! | dot          | event |
//! |--------------|-------|
//! | 0            | LY := line; OAM reads blocked; LYC compare invalid (flag 0); STAT mode reads 0; **OAM (mode-2) IRQ pulse** on lines 1-143 — readable in the same M-cycle but a second-half commit: the halt-exit sampler *and* the running CPU's same-cycle interrupt sample miss it for one M-cycle (SameBoy display.c raises the OAM STAT interrupt 1 T-cycle before STAT changes; the mealybug per-line handlers and wilbertpol intr_2_timing pin the views). The OAM *blocking level* rises here and holds through mode 3, blocking mode-0/LYC edges under it (gambatte m2int_m0irq/lycm2int) |
//! | 4            | STAT mode reads 2; OAM writes blocked; LYC compare valid (line 0's OAM pulse sits here, with its own dispatch-late/m1-blocked rules — see `stat_events_tick`) |
//! | 80           | VRAM reads blocked (the serial scan's last entry latch sits at dot 81 — see §Dot-serial OAM scan) |
//! | 84           | STAT mode reads 3; VRAM writes blocked |
//! | P − 2        | mode 0: STAT reads 0, mode-0 IRQ source asserts, OAM+VRAM unblock, OAM blocking level drops — two dots before the pipe end P = 256 + SCX%8 + sprite/window penalties (three on sprite-laden DMG lines, whose first OBJ fetch costs 6 dots: the flip stays on its mooneye dot while the pixels shift — see `obj_fetch_base`); `m0_flip_events` in render.rs: the gbmicrotest hblank_int/int_hblank grids pin the IRQ dot, mooneye intr_2_mode0_timing/_sprites and the gbmicrotest ppu_sprite0/win*_b grids the flip — both at 254 + SCX%8 on a bare line. The pipe-end anchors (HBlank-DMA trigger, CGB palette-RAM blocking) stay at P |
//!
//! VBlank: line 144 dots 0-3 still read STAT mode 0 (the mode-0 IRQ source
//! stays asserted, keeping the STAT line gapless for `stat_irq_blocking`);
//! mode 1 and the VBlank IF bit assert at 144:4. The OAM IRQ source pulses
//! at 144:0 on *both* families — one M-cycle before the vblank IF
//! (wilbertpol intr_2_timing rounds 5-7 pin MGB and CGB alike; gbmicrotest
//! line_144_oam_int_b/c/d pin DMG). The DMG commit is halt-late, which is
//! how `vblank_stat_intr-GS` observes the pulse together with the vblank
//! IF, while the CGB one is halt-visible in its own cycle
//! (`misc/ppu/vblank_stat_intr-C`). On DMG the OAM source pulses again at
//! dot 12 of every later vblank line (`intr_1_2_timing-GS` measures
//! mode1→mode2 IRQ distance = 464 dots, i.e. one line + 8 dots).
//!
//! Line 153: LY reads 153 during dots 0-3 only, then 0; the LYC compare sees
//! 153 during dots 4-7, is invalid during 8-11, and sees 0 from dot 12
//! (TCAGBD §8.9).
//!
//! LCD enable starts a glitched line 0 (`lcdon_timing-GS`): 452 dots long,
//! no OAM scan (STAT reads mode 0, OAM/VRAM accessible), mode 3 (and all
//! read+write blocking) during dots 78..250+SCX%8, then a real hblank.
//!
//! # CGB-C deltas (the per-model timeline axis)
//!
//! The table above is the DMG grid; `Model::Cgb`/`Agb` differ in
//! CPU-visible windows only (each is cited at its implementation site):
//!
//! * **Readable LYC flag** ([`Ppu::compare_ly`]): no forced-invalid gaps —
//!   the flag holds the previous line's compare through dots 0-3 and
//!   switches at dot 4; line 153 holds 153 through dot 11 and switches to
//!   0 at dot 12 (wilbertpol ly_lyc-C/144-C/153-C rounds 7-8). The
//!   IRQ-side comparison ([`Ppu::compare_ly_irq`] vs the delayed
//!   [`Ppu::lyc_event`] copy) keeps DMG-shaped windows, event-clocked.
//! * **FF45 writes** ([`Ppu::write_lyc_cgb`]): gambatte lycRegChange —
//!   writes within 4 dots of a line's event can't reach it, boundary
//!   writes compare against the upcoming line, and a raised IF lands one
//!   M-cycle after the write at single speed.
//! * **STAT mode** ([`Ppu::vis_mode`]): line 0 dots 0-3 read mode 1 (the
//!   vblank persists; no mode-0 gap — wilbertpol ly00_mode1_2-C), and the
//!   vblank STAT-source level extends with it.
//! * **VRAM read blocking** starts at dot 83 (gambatte vramReadable
//!   `76 + 3*cgb`; age vram-read).
//! * **OAM writes** are blocked during dots 0-3 of lines whose predecessor
//!   was visible, and the DMG dots-80-83 writable gap does not exist
//!   (gambatte oamWritable; age oam-write).
//! * **LY=153** loads two dots early at single speed — readable from
//!   (152,454) — and wraps to 0 at the DMG dot 4; double speed loads on
//!   time and wraps at dot 6 (wilbertpol ly_new_frame-C; age ly ds rows;
//!   SameBoy display.c).
//! * **FF41 writes** never fire from the OAM blocking level except in the
//!   last M-cycle before a visible line's pulse, and an m1 enable written
//!   into mode 1's final M-cycle raises nothing (gambatte
//!   statChangeTriggersStatIrqCgb; wilbertpol stat_write_if-C).
//! * Boot hand-off sits at frame dot 144·456+164 (AGB +4) — gambatte
//!   initstate videoCycles, display_startstate (`model.rs`).
//!
//! # Dot-serial OAM scan
//!
//! Sprite selection is spread across mode 2 — one OAM entry latched and
//! evaluated per 2 dots (gbctr "OAM scan"; gambatte sprite_mapper.cpp
//! `OamReader` latches (y,x) per entry at the same rate; SameBoy
//! display.c's mode-2 loop), entry i on dot 2i+3 on every model (see
//! `scan_latch_dot` in render.rs for the anchoring; the last entry lands
//! on dot 81, before mode 3 consumes the result at dot 84). Consequences
//! the test corpus pins:
//!
//! * An OAM mutation landing mid-scan reaches only entries the scan has
//!   not yet consumed — a DMA byte (committed at its copy cycle's end,
//!   `Interconnect::oam_dma_commit_pending`) or OAM-bug corruption row
//!   ([`Ppu::oam_bug`], which by construction hits the row *at* the scan
//!   position) never re-selects an already-latched entry. blargg's
//!   oam_bug suite keeps checking the memory effect only; the corrupted
//!   line's own selection is unpinned.
//! * While the OAM DMA controller owns OAM — running or frozen by
//!   HALT/STOP — the scan's reads are disconnected and latch $FF, a
//!   disabled sprite ([`Ppu::oam_dma_active`]; gambatte memory.cpp
//!   startOamDma/endOamDma switch its OamReader source to rdisabledRam).
//!   The gambatte oamdma/late_sp00/01/02/39{x,y} `_1`/`_2` pairs pin both
//!   window edges against individual slots' latch dots at M-cycle
//!   granularity, oamdma_late_halt_stat the freeze persistence, and the
//!   strikethrough.gb reference the per-slot vanishing (its residual
//!   7-pixel glitch-sprite cell is undocumented DMA-driver residue —
//!   see the baseline note in tests/gbtr/smallsuites.rs). The known
//!   approximation left: gambatte resolves the `_ds` races at half-dot
//!   (cc) granularity — our whole-dot lattice keeps the single-speed
//!   calibration, leaving the ds `out3` rows on the documented-swap
//!   list together with the frozen ds mode-0 flip lead they also race.
//! * The per-entry LCDC.2 sample (8×16 selection) happens at each
//!   entry's latch dot (gambatte OamReader lsbuf_), which the gambatte
//!   sprites/late_sizechange* families race per slot.
//! * On MGB with the transfer frozen mid-byte by the core-clock gate,
//!   every entry reads as the documented glitch sprite instead
//!   (madness/mgb_oam_dma_halt_sprites.s — `mgb_dma_freeze_glitch_entry`
//!   in render.rs); the other models' frozen-DMA glitches are
//!   unreferenced and keep the plain $FF disconnect, which the
//!   dmg08-verified oamdma_late_halt_stat rows confirm for selection.

mod render;

use crate::SCREEN_PIXELS;
use crate::model::Model;

use render::Render;

/// Dots per normal scanline.
const LINE_DOTS: u16 = 456;
/// The glitched first line after LCD enable is 4 dots short: LY=1 appears at
/// dot 452 in `lcdon_timing-GS` (state(448) reads LY=0, state(452) reads 1).
const GLITCH_LINE_DOTS: u16 = 452;
/// Mode 3 / blocking start on the glitched LCD-enable line.
const GLITCH_MODE3_START: u16 = 78;

// --- LCDC (FF40) bit assignments (Pan Docs "LCD Control") ---

/// LCDC bit 7: LCD & PPU enable.
const LCDC_ENABLE: u8 = 0x80;
/// LCDC bit 6: window tile map area (0 = 9800, 1 = 9C00).
const LCDC_WIN_MAP: u8 = 0x40;
/// LCDC bit 5: window enable.
const LCDC_WIN_ENABLE: u8 = 0x20;
/// LCDC bit 4: BG/window tile data area (1 = unsigned 8000 addressing).
const LCDC_TILE_DATA: u8 = 0x10;
/// LCDC bit 3: BG tile map area (0 = 9800, 1 = 9C00).
const LCDC_BG_MAP: u8 = 0x08;
/// LCDC bit 2: OBJ size (0 = 8x8, 1 = 8x16).
const LCDC_OBJ_SIZE: u8 = 0x04;
/// LCDC bit 1: OBJ enable.
const LCDC_OBJ_ENABLE: u8 = 0x02;
/// LCDC bit 0: BG/window enable (DMG and DMG-compat mode) / BG master
/// priority (native CGB).
const LCDC_BG_ENABLE: u8 = 0x01;

// --- STAT (FF41) interrupt source enables (Pan Docs "LCD Status") ---

/// STAT bit 6: LYC=LY interrupt source enable.
const STAT_SRC_LYC: u8 = 0x40;
/// STAT bit 5: mode-2 (OAM) interrupt source enable.
const STAT_SRC_OAM: u8 = 0x20;
/// STAT bit 4: mode-1 (VBlank) interrupt source enable.
const STAT_SRC_VBLANK: u8 = 0x10;
/// STAT bit 3: mode-0 (HBlank) interrupt source enable.
const STAT_SRC_HBLANK: u8 = 0x08;
/// All four interrupt source enables: the writable FF41 bits.
const STAT_SRC_ALL: u8 = STAT_SRC_LYC | STAT_SRC_OAM | STAT_SRC_VBLANK | STAT_SRC_HBLANK;

// --- IF (FF0F) bits the PPU can raise (Pan Docs "Interrupts") ---

/// IF bit 0: VBlank interrupt.
const IF_VBLANK: u8 = 0x01;
/// IF bit 1: STAT interrupt.
const IF_STAT: u8 = 0x02;

/// The pixel pipeline's live view of the rendering registers.
///
/// Identical to the architectural registers except inside a write M-cycle:
/// the CPU drives the data bus during the second half of the cycle (gbctr
/// "Memory access timing" — the store lands around T3, not after T4), so
/// the dot-clocked pipeline observes a rendering-register write ~2 dots
/// (1 in double speed) before the tick-then-access commit point. The
/// STAT/LYC/IRQ machinery and CPU reads deliberately keep using the
/// architectural registers — every mooneye anchor was calibrated there,
/// and nothing mooneye can observe resolves below 4-dot granularity.
/// See [`Ppu::stage_write`].
struct PipeRegs {
    lcdc: u8,
    scy: u8,
    scx: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    wy: u8,
    wx: u8,
}

/// An IO write in flight on the bus: staged by the interconnect before the
/// write M-cycle ticks, expiring into [`PipeRegs`] mid-cycle (see
/// [`Ppu::stage_write`]).
struct StagedWrite {
    addr: u16,
    value: u8,
    /// Dots until the new value drives the pipeline's register view.
    dots_left: u8,
}

pub struct Ppu {
    model: Model,
    frame_count: u64,

    // Registers.
    lcdc: u8,
    /// STAT bits 3-6 (interrupt source enables).
    stat_en: u8,
    scy: u8,
    scx: u8,
    /// LY as read through FF44 (153-quirk aware).
    ly: u8,
    lyc: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    wy: u8,
    wx: u8,
    /// CGB VRAM bank select (bit 0).
    vbk: u8,
    /// CGB object priority mode (FF6C bit 0: 1 = DMG-style X priority).
    opri: u8,
    /// Integration addition: CGB hardware running a DMG cart ("DMG
    /// compatibility mode"). Rendering remaps pixels through BGP/OBP0/OBP1
    /// into the boot-installed compat palettes, and OAM flag bit 4 selects
    /// the object palette (Pan Docs "DMG compatibility mode").
    dmg_compat: bool,
    bcps: u8,
    ocps: u8,
    bg_pal_ram: [u8; 64],
    obj_pal_ram: [u8; 64],

    vram: Box<[u8; 0x4000]>,
    oam: [u8; 0xA0],
    /// OAM DMA transfer frozen mid-byte by the HALT/STOP core clock gate,
    /// as (OAM index about to be replaced, in-flight source byte). Set by
    /// the interconnect; while set, the MGB OAM scan sees glitched data
    /// (madness/mgb_oam_dma_halt_sprites.s — see
    /// `mgb_dma_freeze_glitch_entry` in render.rs).
    dma_freeze: Option<(u8, u8)>,
    /// The OAM DMA controller owns OAM for the current M-cycle's dots: the
    /// PPU's OAM view is disconnected and the mode-2 scan latches $FF — a
    /// disabled sprite — instead of real entries (gambatte memory.cpp
    /// startOamDma/endOamDma switch the OamReader's source to
    /// rdisabledRam, all $FF, for exactly the copying window; the level
    /// persists across HALT/STOP freezes, which is what
    /// oamdma_late_halt_stat/late_speedchange_stat measure). Maintained
    /// per M-cycle by `Interconnect::oam_dma_tick`.
    oam_dma_active: bool,

    // Timing state.
    enabled: bool,
    /// Internal line counter 0..=153 (the visible LY differs on line 153).
    line: u8,
    /// Dot within the line; the value is the "current time" T so that after
    /// D calls to [`Self::tick`] the observable state is state(D).
    dot: u16,
    /// First line after LCD enable (no OAM scan, shifted mode 3, 452 dots).
    glitch_line: bool,
    /// The frame currently being rendered is the first one after an LCD
    /// enable: hardware does not display it — the panel stays blank/white
    /// for one frame (Pan Docs "LCDC.7"; SameBoy display.c
    /// `GB_FRAMESKIP_LCD_TURNED_ON`). Cleared at the vblank that would have
    /// presented it.
    frame_skip: bool,
    /// LY=LYC comparison flag (STAT bit 2). Frozen while the LCD is off
    /// (`stat_lyc_onoff`).
    cmp: bool,
    /// Current level of the shared STAT interrupt line (IRQ on rising edge:
    /// `stat_irq_blocking`).
    stat_line: bool,
    /// IF bits produced but not yet handed to the interconnect.
    pending_if: u8,
    /// The STAT IF bit just produced came from the line-0 OAM rise, which
    /// sits in the second half of its M-cycle: readable immediately, but
    /// it misses the CPU's interrupt sample for that one cycle (see
    /// `stat_events_tick`). Drained by the interconnect via
    /// [`Self::take_stat_late`].
    stat_late: bool,
    /// The STAT IF bit just produced was a second-half commit (a line-start
    /// OAM pulse, or a mode-0 rise landing on a dot ≡ 3,0 mod 4): readable
    /// immediately, but the halt-exit sampler misses it for one M-cycle —
    /// the same shape as the timer's `if_late` mask (SameBoy `GB_cpu_run`
    /// halt path; the gbmicrotest int_oam_*/int_hblank_halt_scx* grids and
    /// the wilbertpol intr_2_timing halt roundings pin the law). Drained by
    /// the interconnect via [`Self::take_stat_halt_late`].
    stat_halt_late: bool,
    /// The externally visible mode-0 flip (STAT mode bits, OAM/VRAM
    /// unblock): rises with `m0_src` ahead of the pipe end (see
    /// `m0_flip_events` in render.rs), and can drop back mid-line when
    /// a late write arms a new stall (`m0_unflip`).
    line_render_done: bool,
    /// The mode-0 STAT IRQ source level: rises on the visible flip's
    /// dot — 2 dots before the pipe end on a bare line, 1 in double
    /// speed and on window-stalled lines, 0 on DMG window-aborted lines
    /// (see `m0_flip_events` in render.rs) — taking over the OAM
    /// blocking level gaplessly, and drops at dot 4 of the next line
    /// when the mode-2 window becomes visible.
    m0_src: bool,
    /// `m0_src` rose on the current dot: the rise emitted by
    /// `stat_events_tick` this tick is the mode-0 event and carries the
    /// half-cycle halt law (see [`Self::take_m0_rise`]).
    m0_rise_dot: bool,
    /// The STAT IF bit handed out by the last tick came from the mode-0
    /// source rise. The interconnect drains this and applies the
    /// half-cycle halt law: a rise landing in the second half of the
    /// CPU's M-cycle is readable at once — and fully visible to the
    /// running CPU's interrupt sample — but missed by the halt-exit
    /// sampler for one M-cycle (the same shape as the line-start OAM
    /// pulses' `stat_halt_late`). mooneye hblank_ly_scx_timing-GS and
    /// gbmicrotest int_hblank_halt_scx0-7 pin all eight SCX phases.
    m0_rise: bool,
    /// Dots until a CGB-deferred FF45-write STAT IRQ is emitted (0 =
    /// none). On CGB at single speed an FF45 write whose comparison
    /// raises the STAT line produces its IF bit one M-cycle after the
    /// write instead of inside the write cycle (gambatte lycRegChange:
    /// `cgb && !ds` schedules `memevent_oneshot_statirq` at cc+5; the
    /// lyc_ff45_trigger_delay dmg08_out0/cgb04c_out2 split and the
    /// wilbertpol ly_lyc_*write-C rounds pin the cycle).
    lyc_if_delay: u8,
    /// CGB: the LYC value the line-start IRQ event samples — a delayed
    /// copy of FF45 (gambatte LycIrq::regChange keeps `lycReg_` when the
    /// write lands within ~4 dots of the scheduled event). An FF45 write
    /// committing during dots 0-3 of a line (or 8-11 of line 153 for the
    /// LYC=0 event at dot 12) reaches the comparator only after that
    /// line's event has fired: wilbertpol ly_lyc_write-C round 2 (a
    /// match-killing write at the line-start cycle still IRQs) and round
    /// 4 (a match-making write there does not). Mirrors `lyc` exactly on
    /// DMG and outside those windows.
    lyc_event: u8,
    /// The IRQ-side LY=LYC comparison — like `cmp` but evaluated against
    /// [`Self::lyc_event`]. Drives the STAT line's LYC source; FF41
    /// reads keep showing the live `cmp`.
    cmp_irq: bool,
    /// Delayed FF41 copy consulted by the m0/m1/m2 event predicates
    /// (gambatte mstat_irq.h `MStatIrqEvent::statReg_`). On DMG it
    /// mirrors `stat_en`; on CGB a write lands here 6 dots after its
    /// architectural commit, so an event in the write's following
    /// M-cycle still sees the old enables (`statRegChange`'s
    /// `cc + 2*cgb < nextEventTime` guard — the copy otherwise refreshes
    /// at each event, which the fixed 6-dot catch-up subsumes).
    stat_ev: u8,
    stat_ev_staged: Option<(u8, u8)>,
    /// Delayed FF45 copy consulted by the m0/m2 event predicates
    /// (mstat_irq.h `MStatIrqEvent::lycReg_`): CGB FF45 writes land 8
    /// dots late (`lycRegChange`'s `cc + 5*cgb + 1 - ds < nextEventTime`
    /// — one M-cycle wider than the FF41 guard; the m0 event's fresh
    /// view widens it by one more), DMG writes immediately (`cc + 1 <`
    /// only suppresses sub-M-cycle parities).
    lyc_ev_m: u8,
    lyc_ev_m_staged: Option<(u8, u8)>,
    /// Delayed FF41 copy consulted by the LYC event predicate (gambatte
    /// lyc_irq.cpp `LycIrq::statReg_`): CGB writes land 6 dots late
    /// (`regChange`'s `time_ - cc > 2`), DMG immediately.
    stat_lyc_ev: u8,
    stat_lyc_ev_staged: Option<(u8, u8)>,
    /// Mode 3 actually finished (pixel 160 shipped, dot D). This is the
    /// anchor the HBlank-DMA machinery and CGB palette-RAM blocking were
    /// calibrated against (gambatte dma/hdma_start `_1`/`_2` pairs); the
    /// visible flip above leads it by 3 dots and must not retime them.
    render_finished: bool,
    /// Pixel 159 shipped: the HBlank DMA trigger leads the mode-3 end by
    /// one dot (gambatte-core next_m0_time.cpp anchors `memevent_hdma` at
    /// xpos `lcd_hres + 7`, one xpos before the 168 that ends mode 3 —
    /// the dma/hdma_start and hdma_late_* `_1`/`_2` adjacent-cycle pairs
    /// pin the lead). See [`Self::hdma_trigger_level`].
    hdma_lead: bool,

    // Window state.
    /// The frame-sticky WY condition (gambatte ppu.cpp weMaster). NOT a
    /// continuous comparison: hardware samples `win_en && WY == LY` at
    /// three discrete points — assigned at line 0 dot 2, OR-ed at dots
    /// 450/454 (+1 on DMG) of every visible line against the current and
    /// the upcoming LY (gambatte weMasterCheck{Ly0,PriorToLyInc,
    /// AfterLyInc}LineCycle; the gambatte window/arg/late_wy_* family
    /// pins the sample points). The trigger additionally accepts a *live*
    /// `wy2 == LY` match (see [`Self::wy2`]).
    wy_latch: bool,
    /// Delayed copy of WY used by the live window-trigger comparison
    /// (gambatte video.cpp wyChange: wy2 lags the write — modelled as
    /// the architectural commit plus 2 dots on DMG, 6 on CGB, 5 in
    /// double speed, via `wy2_delay`; immediate with the LCD off).
    wy2: u8,
    /// Dots until `wy2` catches up with the architectural WY (CGB only).
    wy2_delay: u8,
    /// The most recent staged rendering write was double-speed (1-dot)
    /// staging — used to pick the wy2 catch-up delay.
    staged_ds: bool,
    /// CGB double speed is engaged (set by the interconnect at the STOP
    /// speed switch): the CPU's bus-access offset halves, so the mode-0
    /// flip/IRQ lead over the pipe end is 1 dot instead of 2 (see
    /// `m0_flip_events`; the gambatte *_ds STAT rows pin the halved
    /// lead the same way the write strobe's 1-dot staging is pinned).
    ds: bool,
    /// Window internal line counter (gambatte winYPos): initialized to
    /// 0xFF at frame start and incremented at each window *activation*
    /// (gambatte ppu.cpp plotPixel/M3Start::f0), so a same-line retrigger
    /// draws the next row (mattcurrie comprehensive-ppu-doc §WIN_EN).
    win_line: u8,
    /// DMG: a WX=166 match leaves its window-start request unconsumed past
    /// the last pipeline dot (gambatte handleWinDrawStartReq only honors
    /// requests at xpos >= 167 on CGB); the request survives to the next
    /// line's mode-3 start, which begins with the window already drawing
    /// (gambatte M3Start::f0).
    win_start_pending: bool,

    /// Pipeline-view rendering registers (see [`PipeRegs`]).
    eff: PipeRegs,
    /// Rendering-register write in flight on the bus (see
    /// [`Self::stage_write`]).
    staged: Option<StagedWrite>,

    render: Render,

    front: Box<[u32; SCREEN_PIXELS]>,
    back: Box<[u32; SCREEN_PIXELS]>,
    dmg_palette: [u32; 4],
}

fn pixel_buffer(fill: u32) -> Box<[u32; SCREEN_PIXELS]> {
    vec![fill; SCREEN_PIXELS]
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

/// How a CPU access with a $FE00-$FEFF value on the address bus collides
/// with the OAM scan on DMG-family models (Pan Docs "OAM Corruption Bug").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OamBugKind {
    /// A memory write, or the internal M-cycle of a 16-bit
    /// increment/decrement-unit operation (INC rr/DEC rr, the PUSH/CALL/
    /// RST pre-push cycle via SP, LD SP,HL via HL) — no memory access
    /// needed, the value on the address bus suffices.
    Write,
    /// A plain memory read.
    Read,
    /// A memory read performed in the same M-cycle as a 16-bit
    /// increment/decrement of the address register: POP/RET via SP,
    /// LD A,(HL+)/(HL-) via HL.
    ReadIncrease,
}

// The corruption patterns operate on 8-byte OAM rows; `row` is the byte
// base of the row the scan is on (8..=0x98 — the callers guarantee the
// preceding row exists). All bit operations are byte-wise, exactly as in
// SameBoy v0.12.1 Core/memory.c (GB_trigger_oam_bug{,_read,_read_increase}),
// the implementation Pan Docs' "OAM Corruption Bug" chapter documents.

/// "Write corruption": the row's first word becomes
/// `((a ^ c) & (b ^ c)) ^ c` with a = that word, b = the preceding row's
/// first word, c = the preceding row's third word; the rest of the row is
/// copied from the preceding row.
fn oam_bug_write_pattern(oam: &mut [u8; 0xA0], row: usize) {
    for i in 0..2 {
        let (a, b, c) = (oam[row + i], oam[row - 8 + i], oam[row - 4 + i]);
        oam[row + i] = ((a ^ c) & (b ^ c)) ^ c;
    }
    for i in 2..8 {
        oam[row + i] = oam[row - 8 + i];
    }
}

/// "Read corruption": like the write pattern but the glitched first word
/// is `b | (a & c)` and lands in *both* the current and the preceding row.
fn oam_bug_read_pattern(oam: &mut [u8; 0xA0], row: usize) {
    for i in 0..2 {
        let (a, b, c) = (oam[row + i], oam[row - 8 + i], oam[row - 4 + i]);
        let glitched = b | (a & c);
        oam[row - 8 + i] = glitched;
        oam[row + i] = glitched;
    }
    for i in 2..8 {
        oam[row + i] = oam[row - 8 + i];
    }
}

/// "Read corruption during a 16-bit increase" (rows 4..=18 only — the
/// caller guards): the *preceding* row's first word becomes
/// `(b & (a | c | d)) | (a & c & d)` with a = the first word two rows
/// back, b = the preceding row's first word, c = the current row's first
/// word, d = the preceding row's third word; then the whole preceding row
/// (glitched word included) is copied to both the current row and two
/// rows back.
fn oam_bug_read_increase_pattern(oam: &mut [u8; 0xA0], row: usize) {
    for i in 0..2 {
        let (a, b, c, d) = (
            oam[row - 0x10 + i],
            oam[row - 8 + i],
            oam[row + i],
            oam[row - 4 + i],
        );
        oam[row - 8 + i] = (b & (a | c | d)) | (a & c & d);
    }
    for i in 0..8 {
        let byte = oam[row - 8 + i];
        oam[row - 0x10 + i] = byte;
        oam[row + i] = byte;
    }
}

impl Ppu {
    pub fn new(model: Model) -> Self {
        Self {
            model,
            frame_count: 0,
            lcdc: 0,
            stat_en: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0,
            obp0: 0,
            obp1: 0,
            wy: 0,
            wx: 0,
            vbk: 0,
            opri: 0,
            dmg_compat: false,
            bcps: 0,
            ocps: 0,
            bg_pal_ram: [0xFF; 64],
            obj_pal_ram: [0xFF; 64],
            vram: vec![0u8; 0x4000]
                .into_boxed_slice()
                .try_into()
                .unwrap_or_else(|_| unreachable!()),
            oam: [0; 0xA0],
            dma_freeze: None,
            oam_dma_active: false,
            enabled: false,
            line: 0,
            dot: 0,
            glitch_line: false,
            frame_skip: false,
            cmp: false,
            stat_line: false,
            pending_if: 0,
            stat_late: false,
            m0_src: false,
            m0_rise_dot: false,
            m0_rise: false,
            lyc_if_delay: 0,
            lyc_event: 0,
            cmp_irq: false,
            stat_ev: 0,
            stat_ev_staged: None,
            lyc_ev_m: 0,
            lyc_ev_m_staged: None,
            stat_lyc_ev: 0,
            stat_lyc_ev_staged: None,
            stat_halt_late: false,
            line_render_done: true,
            render_finished: true,
            hdma_lead: false,
            wy_latch: false,
            wy2: 0,
            wy2_delay: 0,
            staged_ds: false,
            ds: false,
            win_line: 0xFF,
            win_start_pending: false,
            eff: PipeRegs {
                lcdc: 0,
                scy: 0,
                scx: 0,
                bgp: 0,
                obp0: 0,
                obp1: 0,
                wy: 0,
                wx: 0,
            },
            staged: None,
            render: Render::new(),
            front: pixel_buffer(0xFF_FFFF),
            back: pixel_buffer(0xFF_FFFF),
            dmg_palette: [0xFF_FFFF, 0xAA_AAAA, 0x55_5555, 0x00_0000],
        }
    }

    /// Stage a rendering-register write `dots` PPU dots before its
    /// architectural commit. The interconnect calls this *before* ticking
    /// the write M-cycle and commits via [`Self::write`] afterwards, so
    /// the pixel pipeline sees the new value land mid-cycle exactly as the
    /// bus drives it on hardware (gbctr "Memory access timing"), while
    /// everything the tick-then-access contract calibrates (STAT, IRQ,
    /// access blocking, LCDC.7 enable/disable) keeps the architectural
    /// commit point. `dots` is 2 at normal speed, 1 in double speed (the
    /// second half of the M-cycle either way).
    ///
    /// Non-rendering addresses are ignored; rendering registers are FF40
    /// (pipeline bits only — bit 7 acts at the commit), FF42/FF43 and
    /// FF47-FF4B.
    pub(crate) fn stage_write(&mut self, addr: u16, value: u8, dots: u8) {
        if !matches!(addr, 0xFF40 | 0xFF42 | 0xFF43 | 0xFF47..=0xFF4B) {
            return;
        }
        // WX reaches the pixel pipeline one dot later than the palette
        // class — at the architectural tick's strobe point rather than
        // mid-cycle. Pinned by the mealybug m3_wx_4/5/6_change triple:
        // their shared WX=LY rewrite lands exactly between the WX=5 and
        // WX=6 prefill comparator dots (the WX=5 line still triggers,
        // the WX=6 line does not), which only the +1 commit satisfies
        // (gambatte wxChange likewise updates wx one cycle later than
        // the dmg palette path).
        let dots = if addr == 0xFF4B { dots + 1 } else { dots };
        // Speed hint for the FF4A wy2 scheduling below (1-dot staging
        // only happens in double speed).
        self.staged_ds = dots <= 1;
        // One bus op per M-cycle: a previous stage has always expired or
        // been architecturally committed by now; flush defensively if not.
        if let Some(s) = self.staged.take() {
            self.commit_eff(s.addr, s.value);
        }
        self.staged = Some(StagedWrite {
            addr,
            value,
            dots_left: dots,
        });
    }

    /// Fold an expired staged write into the pipeline-view registers.
    fn commit_eff(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF40 => {
                let old = self.eff.lcdc;
                self.eff.lcdc = value;
                // LCDC.5 cleared while the window machine is drawing:
                // the window aborts at the pipeline view's commit point
                // (gambatte ppu.cpp setLcdc clears win_draw_started
                // immediately; the tile data already latched still ships
                // — see `window_abort`).
                if old & LCDC_WIN_ENABLE != 0 && value & LCDC_WIN_ENABLE == 0 && self.render.active
                {
                    self.window_abort();
                }
            }
            0xFF42 => self.eff.scy = value,
            0xFF43 => self.eff.scx = value,
            0xFF47 => self.eff.bgp = value,
            0xFF48 => self.eff.obp0 = value,
            0xFF49 => self.eff.obp1 = value,
            0xFF4A => self.eff.wy = value,
            0xFF4B => self.eff.wx = value,
            _ => {}
        }
    }

    /// Advance the in-flight write strobe by one dot. The dot on which
    /// `dots_left` hits 0 is the transition dot: on pre-CGB models the DMG
    /// palette registers read old OR new for that single dot (mealybug
    /// README, m3_bgp_change: "BGP takes the value old OR new for one
    /// cycle"; the CGB-C reference shows a clean switch); from the next
    /// dot on, the new value drives the pipeline view.
    fn strobe_tick(&mut self) {
        let Some(s) = &mut self.staged else { return };
        if s.dots_left > 0 {
            s.dots_left -= 1;
            if s.dots_left == 0 && !self.model.is_cgb() {
                match s.addr {
                    0xFF47 => self.eff.bgp |= s.value,
                    0xFF48 => self.eff.obp0 |= s.value,
                    0xFF49 => self.eff.obp1 |= s.value,
                    _ => {}
                }
            }
        } else {
            let (addr, value) = (s.addr, s.value);
            self.staged = None;
            self.commit_eff(addr, value);
        }
    }

    /// Advance one dot. Returns IF bits to request
    /// (bit 0 = vblank, bit 1 = STAT), 0 if none.
    pub fn tick(&mut self) -> u8 {
        self.strobe_tick();
        // Delayed event-register copies catch up (see `stat_ev`); applied
        // before this dot's events so a value staged K dots ago becomes
        // visible to events from dot W+K on.
        for (staged, cur) in [
            (&mut self.stat_ev_staged, &mut self.stat_ev),
            (&mut self.lyc_ev_m_staged, &mut self.lyc_ev_m),
            (&mut self.stat_lyc_ev_staged, &mut self.stat_lyc_ev),
        ] {
            if let Some((value, dots)) = staged {
                *dots -= 1;
                if *dots == 0 {
                    *cur = *value;
                    *staged = None;
                }
            }
        }
        if self.wy2_delay > 0 {
            self.wy2_delay -= 1;
            if self.wy2_delay == 0 {
                self.wy2 = self.wy;
            }
        }
        if !self.enabled {
            return std::mem::take(&mut self.pending_if);
        }
        if self.lyc_if_delay > 0 {
            self.lyc_if_delay -= 1;
            if self.lyc_if_delay == 0 {
                // CGB-deferred FF45-write STAT IRQ (see `lyc_if_delay`).
                self.pending_if |= IF_STAT;
            }
        }
        self.dot += 1;
        let len = if self.glitch_line {
            GLITCH_LINE_DOTS
        } else {
            LINE_DOTS
        };
        if self.dot == len {
            self.dot = 0;
            self.glitch_line = false;
            // The window line counter advances at window *activation*
            // (see `win_line`), not at line end.
            self.render.win_active = false;
            self.line = if self.line == 153 { 0 } else { self.line + 1 };
            self.start_line();
        }
        self.step_dot();
        self.stat_events_tick();
        std::mem::take(&mut self.pending_if)
    }

    fn start_line(&mut self) {
        match self.line {
            0 => {
                self.ly = 0;
                // The WY latch is *assigned* at line 0 dot 2 (see
                // `step_dot`) — that sample is the frame reset.
                // gambatte M2_Ly0::f0: winYPos = 0xFF — the first
                // activation of the frame increments it to row 0.
                self.win_line = 0xFF;
                self.line_render_done = false;
                self.render_finished = false;
                self.hdma_lead = false;
                self.render.active = false;
            }
            1..=143 => {
                self.ly = self.line;
                self.line_render_done = false;
                self.render_finished = false;
                self.hdma_lead = false;
                self.render.active = false;
            }
            144 => {
                self.ly = 144;
                self.frame_count += 1;
                if self.frame_skip {
                    // The first frame after an LCD enable is not displayed
                    // (Pan Docs "LCDC.7"; SameBoy display.c
                    // `GB_FRAMESKIP_LCD_TURNED_ON`): drop the rendered
                    // frame and present blank/white instead.
                    self.frame_skip = false;
                    let white = self.white();
                    self.front.fill(white);
                } else {
                    std::mem::swap(&mut self.front, &mut self.back);
                }
            }
            _ => self.ly = self.line,
        }
    }

    fn step_dot(&mut self) {
        // CGB: the line-start LYC event's delayed FF45 copy catches up
        // outside the 4-dot lead-in of each event — dot 4, and 153:12
        // for the LYC=0 event (see `lyc_event`; gambatte
        // LycIrq::regChange's `time_ - cc` windows).
        if self.model.is_cgb() {
            let protected =
                (1..=4).contains(&self.dot) || (self.line == 153 && (9..=12).contains(&self.dot));
            if !protected {
                self.lyc_event = self.lyc;
            }
        }
        // Frame-sticky WY condition (gambatte weMaster): sampled at
        // discrete dots, not compared continuously — see `wy_latch`.
        // gambatte's line-cycle anchors translate to our dot convention
        // with a +1 shift on DMG (m3StartLineCycle is 83+cgb against our
        // model-independent mode-3 start at dot 84).
        let win_en = self.eff.lcdc & LCDC_WIN_ENABLE != 0;
        let late = u16::from(!self.model.is_cgb());
        if self.dot == 4 {
            // The mode-0 IRQ source level (raised by the previous line's
            // `m0_flip_events`) drops when the mode-2 window becomes
            // visible.
            self.m0_src = false;
        }
        if self.line == 0 && self.dot == 2 {
            // Line 0: assignment, not OR — this is the frame reset
            // (gambatte M2_Ly0::f0).
            self.wy_latch = win_en && self.eff.wy == 0;
        } else if self.line < 143 && !self.glitch_line {
            if self.dot == 450 + late {
                self.wy_latch |= win_en && self.ly == self.eff.wy;
            } else if self.dot == 454 + late {
                // Just before the LY increment the comparison already
                // sees the upcoming line (gambatte M2_LyNon0::f1).
                self.wy_latch |= win_en && self.ly + 1 == self.eff.wy;
            }
        }
        if self.line <= 143 {
            if self.glitch_line {
                if self.dot == GLITCH_MODE3_START {
                    self.render_init();
                } else if self.render.active {
                    self.render_step();
                }
            } else {
                match self.dot {
                    // Serial OAM scan: one entry latched + evaluated per
                    // 2 dots (see `scan_latch_dot` in render.rs); the last
                    // entry is consumed before mode 3 starts at dot 84.
                    d if d < 84 => self.oam_scan_step(),
                    84 => self.render_init(),
                    d => {
                        if self.render.active && d > 84 {
                            self.render_step();
                        }
                    }
                }
            }
            // Visible mode-0 flip + IRQ-source rise (after the dot's
            // render step so the projection sees this dot's state).
            self.m0_flip_events();
        }
        if self.model.is_cgb() && !self.ds && self.line == 152 && self.dot == 454 {
            // CGB-C single speed loads LY=153 two dots before line 153
            // starts: the readable window is dots -2..3 around the
            // boundary, which is how wilbertpol ly_new_frame-C's
            // frame-anchored reads (the boot grid sits 2 dots off the
            // M-cycle lattice, see Model::post_boot_state) catch 153 on
            // two consecutive M-cycles while age ly-dmgC-cgbBC's
            // enable-anchored ladder sees it exactly once.
            self.ly = 153;
        }
        if self.line == 153 {
            // Line 153 quirk: LY reads 0 from dot 4 (TCAGBD §8.9). In
            // CGB double speed the wrap comes 2 dots later — age
            // ly-dmgC-cgbBC's ds ladder reads 153 at three consecutive
            // 2-dot-spaced points; SameBoy display.c holds LY=153 for
            // the longer sleep when `cgb_double_speed`.
            let wrap = if self.model.is_cgb() && self.ds { 6 } else { 4 };
            if self.dot == wrap {
                self.ly = 0;
            }
        }
        if self.line == 144 && self.dot == 4 {
            // VBlank interrupt: 4 dots after LY becomes 144, together with
            // the visible mode 1 (TCAGBD; `vblank_stat_intr-GS`).
            self.pending_if |= IF_VBLANK;
        }
    }

    /// LY value the LYC comparator's *readable flag* sees, or None while
    /// the delayed-LY value is invalid (flag forced to 0). See module
    /// docs. The IRQ-side comparison uses [`Self::compare_ly_irq`].
    fn compare_ly(&self) -> Option<u8> {
        if self.glitch_line {
            // LCD enable: the comparison runs immediately with LY=0
            // (`stat_lyc_onoff` rounds 1-4).
            return Some(0);
        }
        if self.model.is_cgb() {
            // CGB-C: no forced-invalid gaps in the *readable* flag — it
            // holds the previous line's value through dots 0-3 and
            // switches at dot 4; line 153 holds 153 through dot 11
            // (twice the DMG window) before switching to 0 at dot 12
            // (wilbertpol ly_lyc-C/ly_lyc_144-C/ly_lyc_153-C rounds 7-8
            // pin the holds; ly_lyc_0-C's expectations equal the -GS
            // build's, so the 0-compare start stays at 153:12).
            return Some(match (self.line, self.dot) {
                (0, _) => 0,
                (153, 0..=3) => 152,
                (153, 4..=11) => 153,
                (153, _) => 0,
                (line, 0..=3) => line - 1,
                (line, _) => line,
            });
        }
        self.compare_ly_irq()
    }

    /// LY value the IRQ-side comparator sees (and the DMG readable
    /// flag). Unlike the CGB readable flag it drops at line starts —
    /// gambatte's lyc and m1 IRQs are separate events, and the m1 event
    /// at 144:4 fires even when LYC matched line 143 (lycint143_m1irq
    /// expects both IRQs; a held level would swallow the m1 edge).
    fn compare_ly_irq(&self) -> Option<u8> {
        if self.glitch_line {
            return Some(0);
        }
        match self.line {
            0 => Some(0),
            153 => match self.dot {
                0..=3 => None,
                4..=7 => Some(153),
                8..=11 => None,
                _ => Some(0),
            },
            _ => {
                if self.dot < 4 {
                    None
                } else {
                    Some(self.line)
                }
            }
        }
    }

    /// STAT mode bits as read through FF41. This is *not* the rendering
    /// state machine: mode reads 0 during the first 4 dots of every line
    /// (and during 144:0-3), and mode 3 appears 4 dots after VRAM read
    /// locking (`lcdon_timing-GS` tables).
    fn vis_mode(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        if self.line >= 144 {
            if self.line == 144 && self.dot < 4 {
                0
            } else {
                1
            }
        } else if self.glitch_line {
            if self.dot < GLITCH_MODE3_START || self.line_render_done {
                0
            } else {
                3
            }
        } else if self.dot < 4 {
            // CGB line 0: the vblank's mode 1 persists through dots 0-3
            // — there is no mode-0 gap before the OAM scan (wilbertpol
            // ly00_mode1_2-C vs ly00_mode1_0-GS; SameBoy display.c only
            // clears the mode bits at the line-0 LY-write dot on DMG;
            // gambatte getStat's mode-1 window runs to 3 cycles before
            // line 0's mode 2).
            u8::from(self.model.is_cgb() && self.line == 0)
        } else if self.dot < 84 {
            2
        } else if !self.line_render_done {
            3
        } else {
            0
        }
    }

    /// STAT mode bits (FF41 bits 0-1) as currently visible to the CPU, for
    /// the interconnect (FEA0-FEFF prohibited-area reads key on OAM locking).
    pub(crate) fn mode_bits(&self) -> u8 {
        self.vis_mode()
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] came
    /// from the line-0 OAM rise and must miss the CPU's interrupt sample
    /// for the current M-cycle (see `stat_events_tick`).
    pub(crate) fn take_stat_late(&mut self) -> bool {
        std::mem::take(&mut self.stat_late)
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] was a
    /// second-half commit that the halt-exit sampler must miss for one
    /// M-cycle (see the `stat_halt_late` field docs). Drained by the
    /// interconnect into its `if_late` halt-wake mask.
    pub(crate) fn take_stat_halt_late(&mut self) -> bool {
        std::mem::take(&mut self.stat_halt_late)
    }

    /// Whether the STAT IF bit handed out by the last [`Self::tick`] came
    /// from the mode-0 source rise (`m0_flip_events`). The interconnect
    /// drains this and, when the rise landed in the second half of the
    /// CPU's M-cycle, masks it from the halt-exit sampler for one
    /// M-cycle (see the `m0_rise` field docs).
    pub(crate) fn take_m0_rise(&mut self) -> bool {
        std::mem::take(&mut self.m0_rise)
    }

    /// Level of the shared STAT interrupt line for the given enable bits.
    /// The LYC source uses the IRQ-side comparison (`cmp_irq` — the
    /// delayed `lyc_event` copy on CGB); FF41 reads show the live `cmp`.
    fn stat_line_level(&self, en: u8) -> bool {
        let mut high = en & STAT_SRC_LYC != 0 && self.cmp_irq;
        if !self.enabled {
            // With the LCD off only the (frozen) LYC source persists
            // (`stat_lyc_onoff` round 2: no edge across off/on with cmp=1).
            return high;
        }
        let vm = self.vis_mode();
        // HBlank source: rises at the mode-0 IRQ event (`m0_src`, one
        // dot *before* the visible flip — gambatte memevent_m0irq one
        // xpos ahead of its m0 anchor) and holds through the hblank and
        // the next line's dots 0-3 (and 144:0-3) so consecutive sources
        // overlap and block each other (`stat_irq_blocking`). The
        // glitched post-enable prefix is not a real hblank.
        high |= en & STAT_SRC_HBLANK != 0
            && ((self.line <= 143 && self.m0_src) || (vm == 0 && self.dot < 4))
            && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
        // Vblank source. On CGB the level extends through line 0 dots
        // 0-3 together with the persisting visible mode 1 (gambatte
        // getStat's mode-1 window + the lycEnable lyc0_m1disable
        // cgb04c_outE0 rows: edges under it stay blocked).
        high |= en & STAT_SRC_VBLANK != 0
            && (self.line >= 145
                || (self.line == 144 && self.dot >= 4)
                || (self.model.is_cgb() && self.line == 0 && !self.glitch_line && self.dot < 4));
        if en & STAT_SRC_OAM != 0 {
            // The OAM *blocking level* spans the whole scan+render of every
            // visible line, dots 0..the mode-0 source rise — one dot past
            // the visible flip, so the hblank source takes over without a
            // gap (gambatte mstat_irq.h doM0Event: the m2 enable blocks
            // the m0 IRQ — m2int_m0irq_*_out0; the level also blocks the
            // LYC dot-4 edge, lycm2int). The IRQ itself is an *event* at
            // the line-start dots — see `stat_events_tick` (SameBoy display.c
            // mode_for_interrupt pulse). Line 0's level starts at dot 4
            // with the LY/LYC validity.
            let oam_window = self.line <= 143
                && !self.glitch_line
                && if self.dot < 4 {
                    // Line-start dots 0-3: the previous line's `m0_src`
                    // is still set; the level is high here exactly as
                    // before (line 0's starts at dot 4).
                    self.line != 0
                } else {
                    !self.m0_src
                };
            let cgb = self.model.is_cgb();
            // OAM pulse at vblank entry: one M-cycle before the vblank IF
            // on *both* families (wilbertpol intr_2_timing rounds 5-7 pin
            // MGB and CGB alike; gbmicrotest line_144_oam_int_b/c/d pin
            // DMG — `vblank_stat_intr-GS` sees it together with the
            // vblank IF through the DMG halt-late commit, see
            // `stat_events_tick`).
            let pulse144 = self.line == 144 && self.dot == 0;
            // DMG: the OAM source also pulses on every later vblank line
            // (`intr_1_2_timing-GS`: mode1→mode2 IRQ distance is 464 dots —
            // one line + 8 dots).
            let vblank_pulse = !cgb && (145..=153).contains(&self.line) && self.dot == 12;
            high |= oam_window || pulse144 || vblank_pulse;
        }
        high
    }

    /// Recompute the readable comparison flag (`cmp`), the IRQ-side
    /// comparison (`cmp_irq`) and the legacy line level (`stat_line` —
    /// kept for the LCD-off edge path and the CGB FF45 trigger's level
    /// check; IF emission no longer hangs on it).
    fn refresh_cmp(&mut self, from_tick: bool) {
        if self.enabled {
            self.cmp = self.compare_ly() == Some(self.lyc);
            if !self.model.is_cgb() {
                self.cmp_irq = self.cmp;
            } else if !from_tick
                || self.glitch_line
                || self.dot == 0
                || self.dot == 4
                || (self.line == 153 && (self.dot == 8 || self.dot == 12))
            {
                // The IRQ-side comparison is event-clocked on CGB: it
                // re-evaluates at the window-boundary dots and on
                // register writes, against the delayed `lyc_event` copy
                // — a copy that caught up *between* events changes the
                // line level only at the next event (no IRQ for an FF45
                // write that lands inside its line's protected window:
                // wilbertpol ly_lyc_write-C round 4).
                self.cmp_irq = self.compare_ly_irq() == Some(self.lyc_event);
            }
        }
        self.stat_line = self.stat_line_level(self.stat_en);
    }

    /// Register-write edge for the LCD-off state and LCDC transitions:
    /// with the LCD off only the frozen LYC source contributes
    /// (`stat_lyc_onoff`), and the enable transition can raise the line
    /// in its own cycle (round 4).
    fn legacy_level_edge(&mut self) {
        let was = self.stat_line;
        self.refresh_cmp(false);
        if self.stat_line && !was {
            self.pending_if |= IF_STAT;
        }
    }

    /// The LY value gambatte's `getLycCmpLy` compares STAT-write and
    /// FF45-write triggers against: the *held* compare — the previous
    /// line's value persists through the line-start dots (their compare
    /// switches 2 cc before the LY increment, which sits near our dot 4
    /// — see the FF45 trigger tables), and line 153 holds 153 through
    /// dot 11. Identical on both models (it is the CGB readable-flag
    /// table; the DMG readable flag differs only by its forced-invalid
    /// gaps, which the *trigger* comparison does not have).
    fn lyc_cmp_held(&self) -> u8 {
        if self.glitch_line {
            return 0;
        }
        match (self.line, self.dot) {
            (0, _) => 0,
            (153, 0..=3) => 152,
            (153, 4..=11) => 153,
            (153, _) => 0,
            (line, 0..=3) => line - 1,
            (line, _) => line,
        }
    }

    /// The trigger-side LYC level: the held compare matches the live
    /// FF45 value and the source is enabled (gambatte `lycperiod`).
    fn lyc_period(&self) -> bool {
        self.lyc == self.lyc_cmp_held() && self.lyc < 154
    }

    /// Per-source STAT IRQ events, fired from the dot clock (gambatte
    /// mstat_irq.h `MStatIrqEvent` + lyc_irq.cpp `LycIrq`, ported
    /// function by function). There is no wired-OR STAT line on the IRQ
    /// side: each source is an *event* whose rise is allowed or
    /// suppressed by a predicate over the *other* sources' enables —
    /// sampled through delayed register copies — and the delayed LYC
    /// value. Truth table (live = `stat_en` at the event tick; ev/evl =
    /// the delayed [`Self::stat_ev`]/[`Self::stat_lyc_ev`] FF41 copies;
    /// lycm/lyce = the delayed [`Self::lyc_ev_m`]/[`Self::lyc_event`]
    /// FF45 copies):
    ///
    /// | event (line, dot) | exists iff | fires iff (provenance) |
    /// |---|---|---|
    /// | m2 pulse (N∈1-144, 0) | live m2en ∧ ¬live m0en | ¬(ev lycen ∧ lycm = N−1) — `doM2Event` blockedByLycIrq compares the *previous* line (its compare is still held at the pulse dot); the ¬m0en exists-gate is `mode2IrqSchedule` routing every per-line event to the line-0 slot while m0en is set |
    /// | m2 pulse (0, 4) | live m2en | ¬(ev m1en) ∧ ¬(ev lycen ∧ lycm = 0) — `doM2Event` blockedByM1Irq + blockedByLycIrq |
    /// | m2 pulse, DMG only (N∈145-153, 12) | live m2en | ¬(live m1en) ∧ ¬(live lycen ∧ cmp_irq) — no gambatte equivalent (mooneye `intr_1_2_timing-GS`); keeps the pre-port level blocking |
    /// | m0 rise (`m0_flip_events`) | (live ∨ ev) m0en | ¬(ev lycen ∧ lycm = N) — `doM0Event`: *not* blocked by m2en (lcdirq_precedence/m0irq_ly44_lcdstat28) |
    /// | m1 (144, 4) | live m1en | ¬(ev ∧ (m2en ∨ m0en)) — `doM1Event` |
    /// | LYC (N∈1-153, 4), lyce = N | (live ∨ evl) lycen | N ∈ 1-144: ¬(evl m2en); else ¬(evl m1en) — `LycIrq::doEvent` + `lycIrqBlockedByM2OrM1StatIrq` (keyed on the LYC *value*, so LYC=144 is m2-blocked and never m1-blocked) |
    /// | LYC=0 (153, 12), lyce = 0 | (live ∨ evl) lycen | ¬(evl m1en) |
    ///
    /// Emission masks: the (N,0) pulses are second-half commits
    /// (`stat_late` + `stat_halt_late`; the CGB 144 entry is exempt —
    /// misc/ppu/vblank_stat_intr-C), the (0,4) pulse is dispatch-late
    /// (`stat_late`; SameBoy "except on line 0", mealybug's "line 0
    /// timing is different by 4 cycles" handlers), the m0 rise carries
    /// the half-cycle halt law (`m0_rise`); LYC and m1 events commit
    /// plain.
    fn stat_events_tick(&mut self) {
        self.refresh_cmp(true);
        let cgb = self.model.is_cgb();
        let live = self.stat_en;
        let ev = self.stat_ev;
        let evl = self.stat_lyc_ev;
        let mut fired = 0u8;

        // m2 line-start pulse (a CGB STAT write committing in this same
        // M-cycle still reaches the pulse — handled retroactively in the
        // FF41 write path, see `m2_pulse_fires`).
        if !self.glitch_line
            && self.dot == 0
            && (1..=144).contains(&self.line)
            && self.m2_pulse_fires(live)
        {
            fired |= IF_STAT;
            if !(cgb && self.line == 144) {
                self.stat_late = true;
                self.stat_halt_late = true;
            }
        }
        if !self.glitch_line && self.line == 0 && self.dot == 4 {
            // m2 line-0 pulse (the one slot that survives the m0en
            // schedule routing).
            if live & STAT_SRC_OAM != 0
                && ev & STAT_SRC_VBLANK == 0
                && !(ev & STAT_SRC_LYC != 0 && self.lyc_ev_m == 0)
            {
                fired |= IF_STAT;
                self.stat_late = true;
            }
        }
        if !cgb && (145..=153).contains(&self.line) && self.dot == 12 {
            // DMG vblank-line OAM pulses.
            if live & STAT_SRC_OAM != 0
                && live & STAT_SRC_VBLANK == 0
                && !(live & STAT_SRC_LYC != 0 && self.cmp_irq)
            {
                fired |= IF_STAT;
            }
        }
        if self.line == 144 && self.dot == 4 {
            // m1 event, one M-cycle after the 144:0 pulse, together with
            // the vblank IF.
            if live & STAT_SRC_VBLANK != 0 && ev & (STAT_SRC_OAM | STAT_SRC_HBLANK) == 0 {
                fired |= IF_STAT;
            }
        }
        if std::mem::take(&mut self.m0_rise_dot) {
            // m0 event on the visible flip's dot (incl. un-flip refires).
            // The m0 event's delayed view is one M-cycle *fresher* than
            // the m1/m2 events': the mstat_irq guards are uniform in
            // gambatte cc, but the m0 event dot carries a smaller
            // calibration skew on our grid, so a write in the preceding
            // M-cycle already lands (m0enable disable_1 out0 vs
            // disable_2 out2 pin the 2-dot cell) — take pending staged
            // values that are within their last 3 dots.
            let ev_m0 = self.stat_ev_fresh();
            let lyc_m0 = self.lyc_ev_m_fresh();
            if (live | ev_m0) & STAT_SRC_HBLANK != 0
                && !(ev_m0 & STAT_SRC_LYC != 0 && lyc_m0 == self.line)
            {
                fired |= IF_STAT;
                self.m0_rise = true;
            }
        }
        // LYC events: once per frame at the (delayed) LYC value's line.
        let lyc_val = if self.glitch_line {
            None
        } else if self.line >= 1 && self.dot == 4 && self.lyc_event == self.line {
            Some(self.line)
        } else if self.line == 153 && self.dot == 12 && self.lyc_event == 0 {
            Some(0)
        } else {
            None
        };
        if let Some(value) = lyc_val {
            let blocker = if (1..=144).contains(&value) {
                STAT_SRC_OAM
            } else {
                STAT_SRC_VBLANK
            };
            // The enable side ORs the live registers with the delayed
            // copy (gambatte's `(statReg_ | statRegSrc_) & lycirqen`):
            // both a just-enabled and a just-disabled source fire.
            if (live | evl) & STAT_SRC_LYC != 0 && evl & blocker == 0 {
                fired |= IF_STAT;
            }
        }
        self.pending_if |= fired;
    }

    // --- CPU access blocking (boundaries from lcdon_timing-GS /
    // --- lcdon_write_timing-GS; see module docs) ---

    pub(crate) fn oam_read_blocked(&self) -> bool {
        self.enabled
            && self.line <= 143
            && !self.line_render_done
            && (!self.glitch_line || self.dot >= GLITCH_MODE3_START)
    }

    pub(crate) fn oam_write_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 {
            return false;
        }
        if self.glitch_line {
            return self.dot >= GLITCH_MODE3_START && !self.line_render_done;
        }
        if self.model.is_cgb() {
            // CGB: line-start dots 0-3 block writes too, unless the
            // previous line was a vblank line (line 0 here — gambatte
            // oamWritable's `lineCycles + 3 + cgb >= 456` arm falls back
            // to `ly >= 143`, and lyCounter still reads 153 there), and
            // the DMG dots-80-83 writable gap does not exist (the
            // `lineCycles == 76 && !cgb` escape; SameBoy raises
            // oam_write_blocked at CGB line starts; age oam-write-cgbBCE).
            return if self.dot < 4 {
                self.line != 0
            } else {
                self.dot < 84 || !self.line_render_done
            };
        }
        // Writes pass during dots 0-3 and 80-83 (`lcdon_write_timing-GS`).
        (4..80).contains(&self.dot) || (self.dot >= 84 && !self.line_render_done)
    }

    fn vram_read_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.line_render_done {
            return false;
        }
        // CGB read locking starts 3 dots later than DMG — a read at
        // state(80) still returns data (gambatte vramReadable
        // `lineCycles + ds < 76 + 3*cgb`; SameBoy keeps vram_read_blocked
        // false through the OAM scan on CGB; age vram-read-cgbBCE).
        let late = if self.model.is_cgb() { 3 } else { 0 };
        if self.glitch_line {
            self.dot >= GLITCH_MODE3_START + late
        } else {
            self.dot >= 80 + late
        }
    }

    fn vram_write_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.line_render_done {
            return false;
        }
        if self.glitch_line {
            self.dot >= GLITCH_MODE3_START
        } else {
            // Write locking begins 4 dots after read locking
            // (`lcdon_write_timing-GS`: a write at line dot 80 still lands).
            self.dot >= 84
        }
    }

    /// Palette RAM (BCPD/OCPD) is inaccessible while the PPU is reading
    /// palettes, i.e. during mode 3 (Pan Docs). Anchored at the *render*
    /// end (dot D), not the visible mode-0 read flip 3 dots earlier — the
    /// gambatte cgbpal_m3 write-window calibration sits on this anchor.
    fn pal_ram_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.render_finished {
            return false;
        }
        self.dot
            >= if self.glitch_line {
                GLITCH_MODE3_START
            } else {
                84
            }
    }

    // --- DMG OAM corruption bug (Pan Docs "OAM Corruption Bug") ---

    /// Byte base (8..=0x98) of the OAM row the mode-2 scan makes
    /// vulnerable to the DMG OAM corruption bug for an access observing
    /// the current state, or `None` outside the scan.
    ///
    /// Anchoring (the one free parameter, calibrated against blargg's
    /// oam_bug ROMs, which are the only hardware oracle in the corpus):
    /// under tick-then-access an access at state(T) covers dots T-4..T.
    /// 4-scanline_timing pins the first corrupting INC DE of a visible
    /// line to the cycle covering dots 0-3 and the last to 72-75, with
    /// 76-79 already clean; 5-timing_bug confirms dots 0-3 on lines 0, 1
    /// and 143; 6-timing_no_bug brackets every visible line and hammers
    /// vblank. That is 19 corruptible M-cycles for the 19 corruptible
    /// rows 1..=19, so the access at state(T) corrupts row T/4, base
    /// (T/4)*8, for T in 4..80. The row-per-cycle mapping is pinned by
    /// 8-instr_effect's OAM-dump CRCs and by 7-timing_effect's expected
    /// CRC $7D792E7C, which is reproduced exactly by simulating the
    /// ROM's checksummed output for this mapping (the shipped single
    /// itself self-destructs — see the baseline note in
    /// tests/gbtr/blargg.rs). No scan runs on vblank lines or the
    /// 452-dot LCD-enable glitch line (lcdon_timing-GS), and rows 0xA0
    /// bytes apart never reach row 0 (Pan Docs: the first row is never
    /// the corrupted row; SameBoy guards `accessed_oam_row >= 8`).
    pub(crate) fn oam_bug_row(&self) -> Option<u8> {
        if !self.enabled || self.line > 143 || self.glitch_line || !(4..80).contains(&self.dot) {
            return None;
        }
        Some((self.dot / 4 * 8) as u8)
    }

    /// Apply the DMG OAM corruption bug for an access of the given kind
    /// happening this M-cycle. The interconnect gates on model family,
    /// address range, halt state and OAM DMA; everything PPU-positional
    /// is decided here via [`Self::oam_bug_row`].
    pub(crate) fn oam_bug(&mut self, kind: OamBugKind) {
        let Some(row) = self.oam_bug_row() else {
            return;
        };
        let row = usize::from(row);
        match kind {
            OamBugKind::Write => oam_bug_write_pattern(&mut self.oam, row),
            OamBugKind::Read => oam_bug_read_pattern(&mut self.oam, row),
            OamBugKind::ReadIncrease => {
                // The special pattern only fires for rows 4..=18 (SameBoy
                // v0.12.1 guards 0x20 <= row < 0x98); the plain read
                // corruption of the read itself applies regardless — a
                // no-op when the special pattern's row copies ran.
                if (0x20..0x98).contains(&row) {
                    oam_bug_read_increase_pattern(&mut self.oam, row);
                }
                oam_bug_read_pattern(&mut self.oam, row);
            }
        }
    }

    /// Read VRAM (0x8000-0x9FFF, current bank), OAM (0xFE00-0xFE9F), or a
    /// PPU register (FF40-FF4B, FF4F, FF68-FF6B). Mode-based access blocking
    /// applies to VRAM/OAM.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                if self.vram_read_blocked() {
                    0xFF
                } else {
                    self.vram[self.vram_index(addr)]
                }
            }
            0xFE00..=0xFE9F => {
                if self.oam_read_blocked() {
                    0xFF
                } else {
                    self.oam[usize::from(addr - 0xFE00)]
                }
            }
            0xFF40 => self.lcdc,
            0xFF41 => 0x80 | self.stat_en | (u8::from(self.cmp) << 2) | self.vis_mode(),
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F if self.model.is_cgb() => 0xFE | self.vbk,
            0xFF68 if self.model.is_cgb() => 0x40 | self.bcps,
            0xFF69 if self.model.is_cgb() => {
                if self.pal_ram_blocked() {
                    0xFF
                } else {
                    self.bg_pal_ram[usize::from(self.bcps & 0x3F)]
                }
            }
            0xFF6A if self.model.is_cgb() => 0x40 | self.ocps,
            0xFF6B if self.model.is_cgb() => {
                if self.pal_ram_blocked() {
                    0xFF
                } else {
                    self.obj_pal_ram[usize::from(self.ocps & 0x3F)]
                }
            }
            0xFF6C if self.model.is_cgb() => 0xFE | self.opri,
            _ => 0xFF,
        }
    }

    /// Write counterpart of [`Self::read`]. Returns IF bits raised by the
    /// write itself (same encoding as [`Self::tick`]): STAT/LYC/LCDC writes
    /// can raise the STAT line in the very M-cycle of the write —
    /// `stat_lyc_onoff` round 4 needs that interrupt to dispatch before the
    /// next instruction — so the caller must OR the returned bits into IF
    /// immediately, like a `tick` result.
    pub fn write(&mut self, addr: u16, value: u8) -> u8 {
        // Architectural commit point: converge the pipeline view with the
        // registers (the staged copy of this same write may already have
        // expired into it — see `stage_write`; writes that never went
        // through the staging path land in both views here).
        if self.staged.as_ref().is_some_and(|s| s.addr == addr) {
            self.staged = None;
        }
        self.commit_eff(addr, value);
        match addr {
            0x8000..=0x9FFF => {
                if !self.vram_write_blocked() {
                    self.vram[self.vram_index(addr)] = value;
                }
            }
            0xFE00..=0xFE9F => {
                if !self.oam_write_blocked() {
                    self.oam[usize::from(addr - 0xFE00)] = value;
                }
            }
            0xFF40 => self.write_lcdc(value),
            0xFF41 => {
                let old = self.stat_en;
                let data = value & STAT_SRC_ALL;
                if self.enabled {
                    let fire = if self.model.is_cgb() {
                        // Retroactive pulse reach: the CGB line-start m2
                        // pulse sits a sub-cycle after our dot-0 tick, so
                        // a write committing in that same M-cycle still
                        // decides it (the un-fire direction is
                        // unrepresentable — m2enable disable_1 stays a
                        // documented swap).
                        let retro = self.dot == 0
                            && !self.glitch_line
                            && (1..=143).contains(&self.line)
                            && old & STAT_SRC_HBLANK == 0
                            && !self.m2_pulse_fires(old)
                            && self.m2_pulse_fires(data);
                        retro || self.stat_write_trigger_cgb(old, data)
                    } else {
                        // The glitch trigger, plus the DMG pulse reach:
                        // an m2 enable committing at the pulse's M-cycle
                        // or the one after re-decides a pulse that did
                        // not exist under the old enables (old m2en off),
                        // blocked by the held LYC match — through the
                        // *new* lyc enable at dot 0, either enable at
                        // dot 4 (the m2enable late_enable /
                        // late_enable_after_lycint(_disable) dmg08 cell
                        // grids pin all eleven cells).
                        let retro = (self.dot == 0 || self.dot == 4)
                            && !self.glitch_line
                            && (1..=144).contains(&self.line)
                            && old & (STAT_SRC_OAM | STAT_SRC_HBLANK) == 0
                            && data & STAT_SRC_OAM != 0
                            && data & STAT_SRC_HBLANK == 0
                            && {
                                let lycen = if self.dot == 0 { data } else { data | old };
                                !(lycen & STAT_SRC_LYC != 0 && self.lyc_ev_m == self.line - 1)
                            };
                        retro || self.stat_write_trigger_dmg(old)
                    };
                    if fire {
                        self.pending_if |= IF_STAT;
                    }
                    self.stat_en = data;
                    self.stage_stat_copies();
                    self.refresh_cmp(false);
                } else {
                    self.stat_en = data;
                    self.flush_stat_copies();
                    self.legacy_level_edge();
                }
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only.
            0xFF4A => {
                self.wy = value;
                // The live window-trigger comparison uses a delayed WY
                // copy — see `wy2`.
                if self.enabled {
                    // CGB: ~6 dots after the architectural commit (5 in
                    // double speed); DMG: 2 (gambatte wyChange: wy2 at
                    // cc+6-ds on CGB with the LCD on, cc+2 otherwise,
                    // one cycle later than the wx commit; calibrated
                    // against the gambatte window/arg/late_wy_* rounds).
                    self.wy2_delay = if !self.model.is_cgb() {
                        2
                    } else if self.staged_ds {
                        5
                    } else {
                        6
                    };
                } else {
                    self.wy2 = value;
                }
            }
            0xFF45 => {
                let old = self.lyc;
                self.lyc = value;
                // The comparison retriggers immediately on LYC writes while
                // the comparison clock runs (`stat_lyc_onoff`).
                if self.enabled && old != value {
                    if self.model.is_cgb() {
                        self.write_lyc_cgb(old, value);
                    } else {
                        self.write_lyc_dmg(old, value);
                    }
                } else {
                    self.lyc_event = value;
                    self.lyc_ev_m = value;
                    self.legacy_level_edge();
                }
            }
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4B => self.wx = value,
            0xFF4F if self.model.is_cgb() => self.vbk = value & 1,
            0xFF68 if self.model.is_cgb() => self.bcps = value & 0xBF,
            0xFF69 if self.model.is_cgb() => {
                if !self.pal_ram_blocked() {
                    self.bg_pal_ram[usize::from(self.bcps & 0x3F)] = value;
                }
                // Auto-increment happens even when the write is blocked
                // (Pan Docs, "LCD Color Palettes (CGB only)").
                if self.bcps & 0x80 != 0 {
                    self.bcps = 0x80 | (self.bcps.wrapping_add(1) & 0x3F);
                }
            }
            0xFF6A if self.model.is_cgb() => self.ocps = value & 0xBF,
            0xFF6B if self.model.is_cgb() => {
                if !self.pal_ram_blocked() {
                    self.obj_pal_ram[usize::from(self.ocps & 0x3F)] = value;
                }
                if self.ocps & 0x80 != 0 {
                    self.ocps = 0x80 | (self.ocps.wrapping_add(1) & 0x3F);
                }
            }
            0xFF6C if self.model.is_cgb() => self.opri = value & 1,
            _ => {}
        }
        std::mem::take(&mut self.pending_if)
    }

    /// CGB FF45 write path (LCD on): the IRQ decision follows gambatte's
    /// `lycRegChangeTriggersStatIrq` — writes committing near a line
    /// boundary compare against the *upcoming* line's value, with the
    /// simultaneous-increment exception — and a raised IF lands one
    /// M-cycle after the write at single speed (`lyc_if_delay`). The
    /// line-start event comparator keeps a delayed copy (`lyc_event`)
    /// that writes inside the event's 4-dot lead-in cannot reach.
    /// Pinned by wilbertpol ly_lyc_write-C / ly_lyc_0_write-C /
    /// ly_lyc_153_write-C and the gambatte lycEnable family.
    fn write_lyc_cgb(&mut self, old: u8, value: u8) {
        // Event-comparator copy (gambatte LycIrq::regChange windows):
        // protected at the event's lead-in M-cycle, and — CGB only — for
        // a boundary write in the previous line's last M-cycle whose new
        // value targets the imminent upcoming-line event (`time_ - cc >
        // 6 + 4*ds` reaches one M-cycle further back than the DMG `> 4`;
        // lycEnable/lyc153_late_ff45_enable_2 cgb04c_outE0 pins the
        // cell — the matching write at (152,452) misses the (153,4)
        // event while its DMG sibling fires).
        let upcoming = if self.line == 152 { 153 } else { self.line + 1 };
        let protected = !self.glitch_line
            && (self.dot < 4
                || (self.line == 153 && (8..12).contains(&self.dot))
                || (self.line <= 152 && self.dot >= 452 && value == upcoming));
        if !protected {
            self.lyc_event = value;
        }
        // The m0/m2 events' delayed FF45 copy (mstat_irq.h lycRegChange
        // `cc + 5*cgb + 1 - ds < nextM0/M2IrqTime`): wider than the FF41
        // window by one M-cycle — staged 8 dots (m0enable/
        // lycdisable_ff45_2/_3 keep the old value at their line's m0
        // event through the fresh view's `d <= 1`, while
        // lyc1_m2irq_late_lyc255_1's write 8 dots before the pulse
        // lands).
        self.lyc_ev_m_staged = Some((value, if self.ds { 2 } else { 8 }));
        // Trigger target: the compare value gambatte's predicate uses,
        // translated to commit-dot coordinates (gambatte cc = commit
        // state minus 4; tail window = returned timeToNextLy <= 6).
        // `None` = the simultaneous-increment exception (old value
        // matched the held compare inside the tail: "lyc flag never
        // goes low -> no trigger").
        let target = if self.glitch_line {
            Some(0)
        } else if self.line == 153 {
            match self.dot {
                // Line-152 tail: the upcoming line is 153.
                0..=3 => Some(153),
                // incLy(153) = 0, with the exception while ret > 2.
                4..=7 if old == 153 => None,
                _ => Some(0),
            }
        } else {
            match self.dot {
                // Tail of the previous line / last M-cycle of this one:
                // the upcoming line's number.
                0..=3 => Some(self.line),
                452..=455 if old == self.line => None,
                452..=455 => Some(if self.line == 152 { 153 } else { self.line + 1 }),
                _ => Some(self.line),
            }
        };
        // The trigger is an event, not a line edge: it fires even while
        // another source holds the line high, blocked only by gambatte's
        // lycRegChangeStatTriggerBlockedByM0OrM1Irq — a pending mode-0
        // IRQ for a now-matching value on visible lines, the m1 enable
        // on vblank lines (except the very end of line 153) — and by an
        // already-matching lyc level (the old value's match means the
        // target comparison fails, handled by `target` above; an
        // unchanged-source rise needs `stat_line` low).
        let blocked = if self.line <= 143 && !self.glitch_line {
            // Blocked only once this line's mode-0 IRQ has passed (the
            // write sits in the hblank): gambatte checks the next m0irq
            // event lying beyond the line end. Writes earlier in the
            // line fire (lycwirq_trigger_m0_early_ly44 rows).
            self.stat_en & STAT_SRC_HBLANK != 0 && self.m0_src && value == self.line
        } else {
            self.stat_en & STAT_SRC_VBLANK != 0 && !(self.line == 153 && self.dot >= 452)
        };
        let lyc_level_high = self.stat_line && self.stat_line_level(STAT_SRC_LYC & self.stat_en);
        let fire = self.stat_en & STAT_SRC_LYC != 0
            && target == Some(value)
            && !blocked
            && !lyc_level_high;
        // Converge the readable flag and the line level (no edge — the
        // trigger decision above is the only write-path IF source).
        self.refresh_cmp(false);
        if fire {
            if self.ds {
                self.pending_if |= IF_STAT;
            } else {
                self.lyc_if_delay = 4;
            }
        }
    }

    /// DMG FF45 write path (LCD on): gambatte `lycRegChangeTriggersStatIrq`
    /// plus `LycIrq::regChange`'s DMG copy rule. The dot tables translate
    /// gambatte's `getLycCmpLy` to our grid (the gambatte-side LY
    /// increment sits near our dot 6, so writes committing at dots 0-3
    /// still see the previous line, and a dot-4 commit sees the compare
    /// already switched to the new line). Calibrated against the
    /// lycEnable lyc153_late_ff45_enable / lycwirq_trigger_ly00_stat50 /
    /// lycwirq_trigger_m0_late ladders.
    fn write_lyc_dmg(&mut self, old: u8, value: u8) {
        // Delayed event copy (`time_ - cc > 4 || timeSrc != time_`): only
        // a write committing at the line-start M-cycle of its own (new)
        // target event misses that event; everything else lands.
        let protected = !self.glitch_line
            && ((self.dot == 0 && self.line >= 1 && value == self.line)
                || (self.line == 153 && self.dot == 8 && value == 0));
        if !protected {
            self.lyc_event = value;
        }
        // The m0/m2 events' copy updates immediately on DMG
        // (mstat_irq.h lycRegChange `cc + 1 < nextEventTime`).
        self.lyc_ev_m = value;
        // Write trigger: compare target per getLycCmpLy. `None` = the
        // simultaneous-increment exception ("lyc flag never goes low ->
        // no trigger": the old value still matches the held compare in
        // the tail cell, so the flag never drops).
        let prev = if self.line == 0 { 153 } else { self.line - 1 };
        let target = if self.glitch_line {
            Some(0)
        } else {
            match self.dot {
                0..=3 if prev == 153 => Some(0),
                0..=3 if old == prev => None,
                0..=3 => Some(self.line),
                4..=7 if prev == 153 => Some(0),
                4..=7 => Some(self.line),
                8..=11 if self.line == 153 && old == 153 => None,
                _ if self.line == 153 => Some(0),
                _ => Some(self.line),
            }
        };
        // lycRegChangeStatTriggerBlockedByM0OrM1Irq on the same grid:
        // visible lines block a now-matching value once the line's m0
        // event has passed; vblank lines block under the m1 enable,
        // except the compare-wrap cell at (0,4) (`ly == 153 &&
        // timeToNextLy <= 2`; lycwirq_trigger_ly00_stat50_3 fires there
        // while _1/_2 stay blocked).
        let their_line = if self.dot < 8 { prev } else { self.line };
        let blocked = if self.glitch_line {
            false
        } else if their_line <= 143 {
            self.stat_en & STAT_SRC_HBLANK != 0
                && (self.m0_src || self.dot < 8)
                && value == their_line
        } else {
            self.stat_en & STAT_SRC_VBLANK != 0 && !(self.line == 0 && self.dot == 4)
        };
        if self.stat_en & STAT_SRC_LYC != 0 && target == Some(value) && !blocked {
            self.pending_if |= IF_STAT;
        }
        self.refresh_cmp(false);
    }

    /// gambatte `statChangeTriggersStatIrqDmg`: the DMG STAT-write glitch
    /// — the write momentarily enables every source (Pan Docs "STAT
    /// bug"), raising IF from the hblank/vblank levels and the held LYC
    /// match (never from the mode-2 condition), suppressed per source
    /// when the corresponding *old* enable already held the line high.
    /// Independent of the written value. gbmicrotest
    /// stat_write_glitch_l0/l1/l143/l154 pin the position grid.
    fn stat_write_trigger_dmg(&self, old: u8) -> bool {
        let lyc_high = self.lyc_period();
        // Visible-line region (gambatte ly < 144: our dots 0-3 still
        // belong to the previous line on their grid, so line 144's first
        // M-cycle is still "line 143, hblank").
        if self.line <= 143 || (self.line == 144 && self.dot < 4) {
            // This line's mode-0 time passed = a real hblank (the
            // LCD-enable glitch prefix is not one).
            let hblank = (self.m0_src || self.dot < 4)
                && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
            if hblank {
                old & STAT_SRC_HBLANK == 0 && !(lyc_high && old & STAT_SRC_LYC != 0)
            } else {
                // Mode 2/3: only the LYC condition fires the glitch.
                lyc_high && old & STAT_SRC_LYC == 0
            }
        } else {
            old & STAT_SRC_VBLANK == 0 && !(lyc_high && old & STAT_SRC_LYC != 0)
        }
    }

    /// gambatte `statChangeTriggersStatIrqCgb` (+ the M2/M0LycOrM1
    /// helpers): CGB STAT writes raise IF only for newly-enabled
    /// sources —
    /// * lyc: enabling while the held compare matches fires anywhere
    ///   (an old lyc enable suppresses everything);
    /// * m0: enabling during mode 2/3 of a visible line fires at the
    ///   write; in the hblank it raises nothing;
    /// * m1: enabling during vblank fires, except in mode 1's last
    ///   M-cycle (line 0 dots 0-3, where only the lyc condition can
    ///   fire — the old `m1_tail_veto`);
    /// * m2: only in the last M-cycle before a visible line's pulse
    ///   (`statChangeTriggersM2IrqCgb`; the m2enable late_enable
    ///   ladders pin the window).
    fn stat_write_trigger_cgb(&self, old: u8, data: u8) -> bool {
        if data & !old & STAT_SRC_ALL == 0 {
            return false;
        }
        // The CGB write's compare view: the trigger-side compare has
        // already switched to the new line at our dot 0 (gambatte's CGB
        // write cc sits later against getLycCmpLy's −2 switch:
        // miscmstatirq m1statwirq_trigger_ly94 round 2 fires its m1
        // enable at the line boundary because the LYC=148 period has
        // ended, while lycEnable lyc_ff41_enable_3's same-cell enable
        // still matches its own line and fires).
        let cmp_cgb = if self.glitch_line {
            0
        } else {
            match (self.line, self.dot) {
                (0, _) => 0,
                (153, 0..=7) => 153,
                (153, _) => 0,
                (line, _) => line,
            }
        };
        let lyc_high = self.lyc == cmp_cgb;
        if lyc_high && old & STAT_SRC_LYC != 0 {
            return false;
        }
        let lyc_fire = lyc_high && data & STAT_SRC_LYC != 0;
        // m2 sub-trigger window (kept from the pre-port calibration;
        // gambatte's ly==143 and ly==153 branches are empty at single
        // speed, so the (144,0) and (0,0) cells never fire it).
        let m2 = old & STAT_SRC_OAM == 0
            && data & (STAT_SRC_OAM | STAT_SRC_HBLANK) == STAT_SRC_OAM
            && (1..=143).contains(&self.line)
            && self.dot < 2 + 2 * u16::from(self.ds);
        // gambatte's ly-region split on our grid: dots 0-3 still belong
        // to the previous line, so (0, 0-3) is mode 1's tail and
        // (144, 0-3) line 143's hblank (an m0 enable written there still
        // fires: m1/ly143_late_m0enable_ds_1 cgb04c_out3).
        let vis = (self.line <= 143 && !(self.line == 0 && self.dot < 4))
            || (self.line == 144 && self.dot < 4);
        let main = if vis {
            // A scheduled mode-0 event still ahead within this line
            // (gambatte `eventTimes_(memevent_m0irq) <
            // lyCounter.time()`): the write trigger defers to it. The
            // m0irq event is (re)scheduled with the *new* enables before
            // the trigger check, so a fresh m0 enable during mode 2/3
            // stays silent (its event fires instead: m0enable
            // late_enable_1), while the same enable in the hblank — the
            // prediction then points at the next line, beyond the LY
            // increment — raises IF at the write (m1/m1irq_m0enable_1).
            let crossed = self.m0_src && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
            let m0_pending = !crossed && (old | data) & STAT_SRC_HBLANK != 0;
            // Line-boundary tail (`timeToNextLy <= 4 + 4*ds`).
            let tail = self.dot < 4;
            if m0_pending || tail {
                lyc_fire
            } else if old & STAT_SRC_HBLANK != 0 {
                false
            } else {
                data & STAT_SRC_HBLANK != 0 || lyc_fire
            }
        } else {
            // Vblank region. Mode 1's last M-cycle (line 0 dots 0-3)
            // doesn't fire a written m1 enable, and an old m1 enable
            // still suppresses a written lyc condition there (gambatte's
            // `old & m1irqen` arm; miscmstatirq
            // lycstatwirq_trigger_ly00_10_50_1 reads E0).
            let m1_tail = self.line == 0 && self.dot < 4;
            if old & STAT_SRC_VBLANK != 0 {
                false
            } else {
                (data & STAT_SRC_VBLANK != 0 && !m1_tail) || lyc_fire
            }
        };
        main || m2
    }

    /// Stage the delayed event-register FF41 copies after a write
    /// (gambatte statRegChange guards): CGB copies land 6 dots after the
    /// architectural commit — an event in the following M-cycle still
    /// sees the old enables — DMG copies update immediately.
    fn stage_stat_copies(&mut self) {
        if self.model.is_cgb() {
            // The guard windows are in machine cycles (`cc + 2*cgb <
            // nextEventTime`), so the dot spans halve in double speed.
            let k = if self.ds { 2 } else { 6 };
            self.stat_ev_staged = Some((self.stat_en, k));
            self.stat_lyc_ev_staged = Some((self.stat_en, k));
        } else {
            self.flush_stat_copies();
        }
    }

    /// The m0 event's (and the CGB line-start pulses') *fresher* view of
    /// the delayed copies: those events carry a smaller calibration skew
    /// on our dot grid, so a staged write within its last few dots
    /// already counts for them (m0enable disable_1/2 and
    /// lyc1_m2irq_late_lycdisable_1 pin the cells).
    fn stat_ev_fresh(&self) -> u8 {
        match self.stat_ev_staged {
            Some((v, d)) if d <= 3 => v,
            _ => self.stat_ev,
        }
    }

    fn lyc_ev_m_fresh(&self) -> u8 {
        match self.lyc_ev_m_staged {
            Some((v, d)) if d <= 1 => v,
            _ => self.lyc_ev_m,
        }
    }

    /// Predicate of the line-start m2 pulse (lines 1-144 dot 0) for the
    /// given live enables: exists iff m2 enabled and m0 not (gambatte
    /// mode2IrqSchedule routes every per-line event to the line-0 slot
    /// while m0en is set), blocked by the previous line's still-held LYC
    /// compare through the delayed copies (doM2Event blockedByLycIrq).
    /// Also consulted retroactively by the CGB FF41 write path: a write
    /// committing at the pulse's own M-cycle reaches it on CGB
    /// (m2enable lyc1_late_m2enable_lycdisable_1 cgb04c_out2 vs the same
    /// row's dmg08_out0).
    fn m2_pulse_fires(&self, en: u8) -> bool {
        let (evp, lycp) = if self.model.is_cgb() {
            (self.stat_ev_fresh(), self.lyc_ev_m_fresh())
        } else {
            (self.stat_ev, self.lyc_ev_m)
        };
        en & STAT_SRC_OAM != 0
            && en & STAT_SRC_HBLANK == 0
            && !(evp & STAT_SRC_LYC != 0 && lycp == self.line - 1)
    }

    /// Synchronise every delayed event copy with the live registers
    /// (LCD transitions: gambatte lcdReset / LycIrq::lcdReset).
    fn flush_stat_copies(&mut self) {
        self.stat_ev = self.stat_en;
        self.stat_ev_staged = None;
        self.stat_lyc_ev = self.stat_en;
        self.stat_lyc_ev_staged = None;
        self.lyc_ev_m = self.lyc;
        self.lyc_ev_m_staged = None;
    }

    fn write_lcdc(&mut self, value: u8) {
        let was_on = self.lcdc & LCDC_ENABLE != 0;
        self.lcdc = value;
        let now_on = value & LCDC_ENABLE != 0;
        if was_on && !now_on {
            // LCD off: LY=0, mode 0, instantly; the comparison clock stops
            // with the flag frozen (`stat_lyc_onoff`); the displayed frame
            // goes white.
            self.enabled = false;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            self.glitch_line = false;
            // Invariant hygiene: frame_skip only matters while enabled and
            // every enable re-arms it; don't leave it stale across off.
            self.frame_skip = false;
            self.line_render_done = true;
            self.render_finished = true;
            self.m0_src = false;
            self.m0_rise_dot = false;
            self.hdma_lead = false;
            // An in-flight CGB FF45-write IRQ dies with the LCD
            // (gambatte: disabling cancels every scheduled memevent).
            self.lyc_if_delay = 0;
            self.flush_stat_copies();
            self.render.active = false;
            self.render.win_active = false;
            self.win_start_pending = false;
            let white = self.white();
            self.front.fill(white);
            self.legacy_level_edge();
        } else if !was_on && now_on {
            // LCD on: glitched first line (`lcdon_timing-GS`); the LYC
            // comparison restarts against LY=0 immediately and can raise
            // the STAT line in this very cycle (`stat_lyc_onoff` round 4).
            self.enabled = true;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            // The event comparator's delayed FF45 copy restarts in sync
            // (gambatte lycIrq.lcdReset).
            self.lyc_event = self.lyc;
            self.glitch_line = true;
            // Hardware keeps the panel blank for the whole first frame
            // after enabling (see `frame_skip`).
            self.frame_skip = true;
            self.line_render_done = false;
            self.render_finished = false;
            self.m0_src = false;
            self.m0_rise_dot = false;
            self.hdma_lead = false;
            self.flush_stat_copies();
            self.render.active = false;
            self.wy_latch = false;
            self.win_line = 0xFF;
            self.win_start_pending = false;
            self.legacy_level_edge();
        }
    }

    fn white(&self) -> u32 {
        if self.model.is_cgb() {
            0xFF_FFFF
        } else {
            self.dmg_palette[0]
        }
    }

    fn vram_index(&self, addr: u16) -> usize {
        usize::from(self.vbk) * 0x2000 + usize::from(addr & 0x1FFF)
    }

    /// OAM write from the DMA engine: ignores mode-based blocking.
    pub fn oam_dma_write(&mut self, index: u8, value: u8) {
        if usize::from(index) < self.oam.len() {
            self.oam[usize::from(index)] = value;
        }
    }

    /// Interconnect wiring: an OAM DMA transfer is frozen mid-byte because
    /// HALT/STOP gated the core clock the DMA controller runs on
    /// (`Some((oam_index, in_flight_source_byte))`), or the freeze ended /
    /// no transfer was in flight (`None`). While frozen, the MGB PPU's OAM
    /// scan sees glitched data derived from the frozen access instead of
    /// real OAM entries (madness/mgb_oam_dma_halt_sprites.s; see
    /// `mgb_dma_freeze_glitch_entry` in render.rs).
    /// Interconnect wiring: CGB double speed engaged/left (see `ds`).
    pub(crate) fn set_double_speed(&mut self, ds: bool) {
        self.ds = ds;
    }

    pub fn set_oam_dma_freeze(&mut self, freeze: Option<(u8, u8)>) {
        self.dma_freeze = freeze;
    }

    /// Interconnect wiring: the OAM DMA controller owns (true) or released
    /// (false) OAM for the coming M-cycle's dots — see the
    /// [`Self::oam_dma_active`] field docs for the scan semantics and the
    /// gambatte derivation of the level's edges.
    pub(crate) fn set_oam_dma_active(&mut self, active: bool) {
        self.oam_dma_active = active;
    }

    /// Test hook for the interconnect wiring tests.
    #[cfg(test)]
    pub(crate) fn oam_dma_freeze(&self) -> Option<(u8, u8)> {
        self.dma_freeze
    }

    /// Test hook for the interconnect wiring tests: the scan's OAM view is
    /// disconnected for the current M-cycle's dots.
    #[cfg(test)]
    pub(crate) fn oam_dma_scan_disconnected(&self) -> bool {
        self.oam_dma_active
    }

    /// Test hook: raw (BG, OBJ) palette RAM. FF69/FF6B reads are gated on
    /// CGB mode by the interconnect and on mode 3 here, so the post-boot
    /// palette tests need an ungated view.
    #[cfg(test)]
    pub(crate) fn palette_ram(&self) -> (&[u8; 64], &[u8; 64]) {
        (&self.bg_pal_ram, &self.obj_pal_ram)
    }

    /// VRAM read for CGB HDMA (no mode blocking — the engine is responsible
    /// for scheduling). Doubles as the active-bank view for the
    /// interconnect's side-effect-free `peek`.
    pub fn vram_read_raw(&self, addr: u16) -> u8 {
        self.vram[self.vram_index(addr)]
    }

    /// OAM read ignoring mode-based and DMA blocking, for the
    /// interconnect's side-effect-free `peek`.
    pub(crate) fn oam_read_raw(&self, addr: u16) -> u8 {
        self.oam[usize::from(addr - 0xFE00)]
    }

    /// VRAM write for CGB HDMA.
    pub fn vram_write_raw(&mut self, addr: u16, value: u8) {
        let i = self.vram_index(addr);
        self.vram[i] = value;
    }

    /// True while the PPU is in a real hblank (mode 3 finished on a visible
    /// line); the visible STAT mode-0 window at line starts is excluded.
    /// The HBlank DMA engine edge-detects [`Self::hdma_trigger_level`]
    /// (this level led by one dot) instead.
    pub fn hblank_active(&self) -> bool {
        self.enabled && self.line <= 143 && self.render_finished
    }

    /// The HBlank DMA trigger level: the real hblank of a visible line,
    /// led by one dot (see [`Self::hdma_lead`]). The interconnect's
    /// per-dot edge detector flags one block request per rising edge.
    /// Anchored at the render end (dot D−1 via the lead), independent of
    /// the visible mode-0 read flip at D−3 (gambatte-core derives
    /// `predictedNextM0Time` from the pixel pipe, and the dma/hdma_start
    /// `_1`/`_2` pairs pin it there).
    pub(crate) fn hdma_trigger_level(&self) -> bool {
        self.enabled && self.line <= 143 && (self.render_finished || self.hdma_lead)
    }

    /// The HBlank DMA trigger window: inside a visible line's hblank (as
    /// [`Self::hdma_trigger_level`] sees it), ending 3 dots before the
    /// line ends (gambatte-core video.cpp `isHdmaPeriod`:
    /// `ly < 144 && cc + 3 + 3 * ds < lyCounter.time() && cc >= m0Time` —
    /// the cc margin is 3 dots at either speed, and the m0 time derives
    /// from the same led `predictedNextM0Time` anchor). The interconnect
    /// consults this when HBlank DMA is enabled mid-window and when a
    /// halt/stop wake re-evaluates a pending block.
    pub(crate) fn hdma_period(&self) -> bool {
        let len = if self.glitch_line {
            GLITCH_LINE_DOTS
        } else {
            LINE_DOTS
        };
        self.hdma_trigger_level() && self.dot + 3 < len
    }

    /// LCDC bit 7 as committed (architectural view).
    pub(crate) fn lcd_enabled(&self) -> bool {
        self.enabled
    }

    /// XRGB8888 pixels of the most recently *completed* frame.
    pub fn frame(&self) -> &[u32; SCREEN_PIXELS] {
        &self.front
    }

    /// Completed frames since power-on. With the LCD off this stops
    /// advancing; `GameBoy::run_frame` falls back to a cycle deadline.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Map DMG shades 0..=3 to XRGB8888 (frontend palette option).
    pub fn set_dmg_palette(&mut self, palette: [u32; 4]) {
        self.dmg_palette = palette;
    }

    /// Integration addition: enable DMG compatibility rendering on a CGB
    /// model (CGB hardware running a non-CGB cart). Set once by the
    /// interconnect at power-on; no effect on DMG models.
    pub fn set_dmg_compat(&mut self, compat: bool) {
        self.dmg_compat = compat;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dmg() -> Ppu {
        Ppu::new(Model::Dmg)
    }

    fn cgb() -> Ppu {
        Ppu::new(Model::Cgb)
    }

    /// Tick `n` dots, OR-ing the returned IF bits.
    fn tick_n(p: &mut Ppu, n: u32) -> u8 {
        let mut ifs = 0;
        for _ in 0..n {
            ifs |= p.tick();
        }
        ifs
    }

    /// Tick until the PPU sits at (line, dot); returns OR of IF bits seen.
    fn run_to(p: &mut Ppu, line: u8, dot: u16) -> u8 {
        let mut ifs = 0;
        let mut guard = 0u32;
        while !(p.line == line && p.dot == dot) {
            ifs |= p.tick();
            guard += 1;
            assert!(guard < 200_000, "run_to({line},{dot}) never reached");
        }
        ifs
    }

    // --- lcdon_timing-GS: read state at 4*(c+2) dots after LCD enable ---

    const LCDON_CYCLES: [[u32; 8]; 3] = [
        [0, 17, 60, 110, 130, 174, 224, 244],
        [1, 18, 61, 111, 131, 175, 225, 245],
        [2, 19, 62, 112, 132, 176, 226, 246],
    ];

    fn lcdon_case(lyc: u8, pass: usize, col: usize) -> Ppu {
        let mut p = dmg();
        p.write(0xFF45, lyc);
        p.write(0xFF40, 0x81);
        tick_n(&mut p, 4 * (LCDON_CYCLES[pass][col] + 2));
        p
    }

    fn check_lcdon_table(lyc: u8, addr: u16, expect: &[[u8; 8]; 3]) {
        for pass in 0..3 {
            for col in 0..8 {
                let p = lcdon_case(lyc, pass, col);
                assert_eq!(
                    p.read(addr),
                    expect[pass][col],
                    "pass {pass} col {col} (cycle {})",
                    LCDON_CYCLES[pass][col]
                );
            }
        }
    }

    #[test]
    fn lcdon_ly_table() {
        check_lcdon_table(
            0,
            0xFF44,
            &[
                [0, 0, 0, 0, 1, 1, 1, 2],
                [0, 0, 0, 1, 1, 1, 2, 2],
                [0, 0, 0, 1, 1, 1, 2, 2],
            ],
        );
    }

    #[test]
    fn lcdon_stat_lyc0_table() {
        check_lcdon_table(
            0,
            0xFF41,
            &[
                [0x84, 0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82],
                [0x84, 0x87, 0x84, 0x80, 0x82, 0x80, 0x80, 0x82],
                [0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82, 0x83],
            ],
        );
    }

    #[test]
    fn lcdon_stat_lyc1_table() {
        check_lcdon_table(
            1,
            0xFF41,
            &[
                [0x80, 0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82],
                [0x80, 0x83, 0x80, 0x80, 0x86, 0x84, 0x80, 0x82],
                [0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82, 0x83],
            ],
        );
    }

    #[test]
    fn lcdon_oam_read_table() {
        check_lcdon_table(
            0,
            0xFE00,
            &[
                [0x00, 0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF],
                [0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF, 0xFF],
                [0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF, 0xFF],
            ],
        );
    }

    #[test]
    fn lcdon_vram_read_table() {
        check_lcdon_table(
            0,
            0x8000,
            &[
                [0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00],
                [0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF],
                [0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF],
            ],
        );
    }

    // --- Line-0 OAM STAT IRQ event semantics ---
    //
    // The line-0 mode-2 rise differs from every other line's (see the
    // `stat_events_tick` comment for the sources): the IF bit is readable
    // immediately (gambatte lyc153int_m2irq) but misses the CPU's
    // interrupt sample for one M-cycle (SameBoy raises the OAM IRQ "1
    // T-cycle before STAT actually changes, except on line 0"; mealybug
    // m3_bgp_change compensates "line 0 timing is different by 4
    // cycles"), and it is blocked entirely while the mode-1 source enable
    // is set (gambatte mstat_irq.h doM2Event `blockedByM1Irq`;
    // lcdirq_precedence/m2irq_ly00_lcdstat30).

    #[test]
    fn line0_oam_irq_is_readable_but_dispatch_late() {
        for model in [Model::Dmg, Model::Cgb] {
            let mut p = Ppu::new(model);
            p.write(0xFF41, 0x20); // OAM source only
            p.write(0xFF40, 0x81);
            // Normal line: the pulse commits at dot 0 (CGB: dot 1 — a
            // line-start write still reaches it, see `stat_events_tick`;
            // both land within the same M-cycle) — a second-half commit,
            // so it misses the dispatch sample of its own cycle too (the
            // mealybug m3_* photo handlers pin the anchor).
            run_to(&mut p, 0, 451);
            p.take_stat_late();
            let pulse = p.tick() | if model.is_cgb() { p.tick() } else { 0 };
            assert_eq!(pulse & IF_STAT, IF_STAT, "{model:?} line 1");
            assert!(
                p.take_stat_late(),
                "{model:?} line-1 pulse is dispatch-late"
            );
            // Line 0: the IF bit appears in the same M-cycle but is
            // flagged late for the dispatch sample.
            run_to(&mut p, 0, 0);
            p.take_stat_late();
            assert_eq!(tick_n(&mut p, 4) & IF_STAT, IF_STAT, "{model:?} line 0");
            assert!(p.take_stat_late(), "{model:?} line 0 rise is late");
        }
    }

    #[test]
    fn line0_oam_irq_blocked_by_vblank_enable() {
        // With the mode-1 source enable also set, the line-0 OAM rise
        // raises no IRQ at all; the line level still rises, so nothing
        // re-edges later in the OAM window.
        let mut p = dmg();
        p.write(0xFF41, 0x30); // OAM + VBLANK sources
        p.write(0xFF40, 0x81);
        run_to(&mut p, 150, 0);
        run_to(&mut p, 0, 0); // drain vblank-window IRQs
        assert_eq!(
            tick_n(&mut p, 84) & IF_STAT,
            0,
            "line 0 OAM rise is blocked while the vblank enable is set"
        );
        // The next line's pulse (at dot 0) is unaffected.
        let ifs = run_to(&mut p, 0, 455);
        assert_eq!(ifs & IF_STAT, 0, "nothing else fires during line 0");
        assert_eq!(p.tick() & IF_STAT, IF_STAT, "line-1 pulse at (1,0)");
    }

    // --- Per-source STAT IRQ event predicates (gambatte mstat_irq.h /
    // --- lyc_irq.cpp port) ---

    #[test]
    fn lyc_event_fires_despite_hblank_enable() {
        // gambatte lcdirq_precedence/lycirq_ly44_lcdstat48: with the
        // mode-0 source enabled alongside LYC, the LYC event of its line
        // still raises IF — the sources are independent events, not a
        // wired-OR level (LycIrq::doEvent is blocked by the m2 enable
        // only on visible lines, never by m0).
        let mut p = dmg();
        p.write(0xFF45, 68);
        p.write(0xFF41, 0x48); // LYC + mode-0 sources
        p.write(0xFF40, 0x81);
        run_to(&mut p, 67, 400); // past line 67's m0 event
        let ifs = run_to(&mut p, 68, 8);
        assert_eq!(ifs & IF_STAT, IF_STAT, "LYC event fires under m0 enable");
    }

    #[test]
    fn m1_event_blocked_by_oam_enable() {
        // gambatte mstat_irq.h doM1Event: the vblank STAT event at 144:4
        // is suppressed when the (delayed) m2 enable is set — the 144:0
        // OAM pulse is the only STAT IF of the vblank entry.
        let mut p = dmg();
        p.write(0xFF45, 200);
        p.write(0xFF41, 0x30); // OAM + VBLANK sources
        p.write(0xFF40, 0x81);
        run_to(&mut p, 143, 400);
        let ifs = run_to(&mut p, 144, 1);
        assert_eq!(ifs & IF_STAT, IF_STAT, "144:0 OAM pulse fires");
        let ifs = run_to(&mut p, 144, 8);
        assert_eq!(ifs & IF_STAT, 0, "m1 event blocked by the m2 enable");
        assert_eq!(ifs & IF_VBLANK, IF_VBLANK, "vblank IF unaffected");
    }

    #[test]
    fn cgb_stat_disable_in_event_leadin_still_fires() {
        // gambatte lycEnable/ff41_disable_2 (dmg08_out0_cgb04c_out2): a
        // STAT write committing in the last M-cycle before the LYC event
        // does not reach the event's delayed enable copy on CGB
        // (LycIrq::regChange `time_ - cc > 2`); on DMG it does.
        for (model, expect) in [(Model::Dmg, 0), (Model::Cgb, IF_STAT)] {
            let mut p = Ppu::new(model);
            p.write(0xFF45, 68);
            p.write(0xFF41, 0x48);
            p.write(0xFF40, 0x81);
            run_to(&mut p, 67, 400);
            run_to(&mut p, 68, 0);
            p.write(0xFF41, 0x00); // disable committing at (68,0)
            let ifs = run_to(&mut p, 68, 8);
            assert_eq!(ifs & IF_STAT, expect, "{model:?}");
        }
    }

    #[test]
    fn dmg_ff45_write_in_event_leadin_misses_event() {
        // gambatte lycEnable/lyc153_late_ff45_enable_3 (dmg08_outE0): an
        // FF45 write committing at the line-start M-cycle cannot reach
        // that line's LYC event on DMG either (LycIrq::regChange
        // `time_ - cc > 4 || timeSrc != time_`), and the write trigger
        // sees the old value still matching the held compare ("lyc flag
        // never goes low -> no trigger").
        let mut p = dmg();
        p.write(0xFF45, 152);
        p.write(0xFF41, 0x40);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 152, 300); // past the (152,4) event
        run_to(&mut p, 153, 0);
        p.write(0xFF45, 153); // commits at (153,0)
        let ifs = run_to(&mut p, 153, 8);
        assert_eq!(ifs & IF_STAT, 0, "protected write misses the 153 event");
    }

    // --- lcdon_write_timing-GS ---

    const WRITE_NOPS: [u32; 19] = [
        0, 17, 18, 60, 61, 110, 111, 112, 130, 131, 132, 174, 175, 224, 225, 226, 244, 245, 246,
    ];

    #[test]
    fn lcdon_oam_write_table() {
        let expect: [u8; 19] = [
            0x81, 0x81, 0x00, 0x00, 0x81, 0x81, 0x81, 0x00, 0x00, 0x81, 0x00, 0x00, 0x81, 0x81,
            0x81, 0x00, 0x00, 0x81, 0x00,
        ];
        for (i, &nops) in WRITE_NOPS.iter().enumerate() {
            let mut p = dmg();
            p.write(0xFF40, 0x81);
            tick_n(&mut p, 4 * (nops + 2));
            p.write(0xFE00, 0x81);
            assert_eq!(p.oam[0], expect[i], "nops {nops}");
        }
    }

    #[test]
    fn lcdon_vram_write_table() {
        let expect: [u8; 19] = [
            0x81, 0x81, 0x00, 0x00, 0x81, 0x81, 0x81, 0x81, 0x81, 0x81, 0x00, 0x00, 0x81, 0x81,
            0x81, 0x81, 0x81, 0x81, 0x00,
        ];
        for (i, &nops) in WRITE_NOPS.iter().enumerate() {
            let mut p = dmg();
            p.write(0xFF40, 0x81);
            tick_n(&mut p, 4 * (nops + 2));
            p.write(0x8000, 0x81);
            assert_eq!(p.vram[0], expect[i], "nops {nops}");
        }
    }

    // --- Line lengths and LY=153 quirk ---

    #[test]
    fn steady_line_boundaries() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        tick_n(&mut p, 451);
        assert_eq!(p.read(0xFF44), 0); // glitch line 0 is 452 dots
        p.tick();
        assert_eq!(p.read(0xFF44), 1);
        tick_n(&mut p, 455);
        assert_eq!(p.read(0xFF44), 1); // state(907)
        p.tick();
        assert_eq!(p.read(0xFF44), 2); // state(908)
    }

    #[test]
    fn ly153_reads_zero_from_dot_4() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 3);
        assert_eq!(p.read(0xFF44), 153);
        p.tick();
        assert_eq!(p.read(0xFF44), 0);
        run_to(&mut p, 0, 0);
        assert_eq!(p.read(0xFF44), 0);
    }

    #[test]
    fn ly153_lyc153_compare_window() {
        let mut p = dmg();
        p.write(0xFF45, 153);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 3);
        assert_eq!(p.read(0xFF41) & 4, 0); // compare invalid dots 0-3
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 4); // dots 4-7 compare vs 153
        tick_n(&mut p, 3);
        assert_eq!(p.read(0xFF41) & 4, 4);
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 0); // dots 8-11 invalid
        tick_n(&mut p, 4);
        assert_eq!(p.read(0xFF41) & 4, 0); // dot 12+: compare vs 0
    }

    #[test]
    fn ly153_lyc0_compare_from_dot_12() {
        let mut p = dmg();
        p.write(0xFF45, 0);
        p.write(0xFF41, 0x40);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 11);
        assert_eq!(p.read(0xFF41) & 4, 0);
        assert_eq!(p.tick(), 0x02, "LYC=0 IRQ fires at 153:12");
        assert_eq!(p.read(0xFF41) & 4, 4);
        // The compare stays set through line 0; no further edge.
        assert_eq!(run_to(&mut p, 1, 0) & 2, 0);
    }

    #[test]
    fn lyc_compare_invalid_first_4_dots_of_line() {
        let mut p = dmg();
        p.write(0xFF45, 2);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 2, 0);
        assert_eq!(p.read(0xFF41) & 4, 0);
        tick_n(&mut p, 3);
        assert_eq!(p.read(0xFF41) & 4, 0); // state(2,3)
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 4); // state(2,4)
    }

    // --- VBlank / frame ---

    #[test]
    fn vblank_if_at_144_dot4_and_frame_count_at_dot0() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        assert_eq!(p.frame_count(), 0);
        let ifs = run_to(&mut p, 144, 0);
        assert_eq!(ifs & 1, 0, "no vblank IF before 144:4");
        assert_eq!(p.frame_count(), 1);
        tick_n(&mut p, 3);
        assert_eq!(p.tick() & 1, 1, "vblank IF at state(144,4)");
        // Exactly one vblank IF per frame.
        let ifs = run_to(&mut p, 144, 3);
        assert_eq!(ifs & 1, 0);
        assert_eq!(p.tick() & 1, 1);
        assert_eq!(p.frame_count(), 2);
    }

    #[test]
    fn stat_mode_during_vblank() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 3);
        assert_eq!(p.read(0xFF41) & 3, 0, "144:0-3 still reads mode 0");
        p.tick();
        assert_eq!(p.read(0xFF41) & 3, 1);
        run_to(&mut p, 150, 100);
        assert_eq!(p.read(0xFF41) & 3, 1);
        // OAM and VRAM accessible during vblank (mem_oam).
        p.write(0xFE05, 0x5A);
        assert_eq!(p.read(0xFE05), 0x5A);
        p.write(0x9000, 0xA5);
        assert_eq!(p.read(0x9000), 0xA5);
    }

    // --- STAT interrupt sources ---

    #[test]
    fn oam_irq_pulses_at_line_start() {
        let mut p = dmg();
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        // No mode-2 source on the glitched line. On lines 1-143 the OAM
        // IRQ is an *event* committing at state(line,0) — the LY-increment
        // M-cycle, one M-cycle before the readable mode 2 (SameBoy
        // display.c: "The OAM STAT interrupt occurs 1 T-cycle before STAT
        // actually changes, except on line 0"; the gbmicrotest
        // oam_int_*/int_oam_* grids pin the cycle).
        let ifs = run_to(&mut p, 0, 451);
        assert_eq!(ifs & 2, 0, "no OAM source on the glitch line");
        assert_eq!(p.tick(), 0x02, "OAM IRQ pulse at state(1,0)");
        // The blocking level holds through scan+render: no second edge.
        assert_eq!(run_to(&mut p, 1, 300) & 2, 0);
        run_to(&mut p, 1, 455);
        assert_eq!(p.tick(), 0x02, "next pulse at state(2,0)");
    }

    #[test]
    fn line_start_oam_pulse_is_halt_late() {
        // The dot-0 commit sits in the second half of its M-cycle: the
        // halt-exit sampler misses it for one cycle on every model
        // (gbmicrotest int_oam_* halt rows; wilbertpol intr_2_timing halt
        // rounds land one M-cycle after the IF rows on MGB and CGB alike).
        for model in [Model::Dmg, Model::Cgb] {
            let mut p = Ppu::new(model);
            p.write(0xFF41, 0x20);
            p.write(0xFF40, 0x81);
            run_to(&mut p, 0, 451);
            p.take_stat_halt_late();
            let pulse = p.tick() | if model.is_cgb() { p.tick() } else { 0 };
            assert_eq!(pulse & 2, 2, "{model:?}: pulse at the (1,0) M-cycle");
            assert!(
                p.take_stat_halt_late(),
                "{model:?}: dot-0 pulse is halt-late"
            );
        }
    }

    #[test]
    fn mode0_irq_at_254_plus_scx_fine() {
        // The IRQ source rises with the visible flip, 2 dots before the
        // pipe end (see render.rs `m0_flip_events`).
        for scx in [0u8, 1, 4, 5, 7, 8, 13] {
            let mut p = dmg();
            p.write(0xFF41, 0x08);
            p.write(0xFF43, scx);
            p.write(0xFF40, 0x81);
            run_to(&mut p, 1, 4); // line start: hblank source dropped
            let v0 = 254 + u16::from(scx & 7);
            let ifs = run_to(&mut p, 1, v0 - 1);
            assert_eq!(ifs & 2, 0, "scx {scx}: no hblank IRQ before {v0}");
            assert_eq!(p.tick(), 0x02, "scx {scx}: hblank IRQ at {v0}");
        }
    }

    #[test]
    fn oam_level_blocks_lyc_edge_and_next_pulse() {
        let mut p = dmg();
        p.write(0xFF45, 2);
        p.write(0xFF41, 0x60); // LYC + OAM sources
        p.write(0xFF40, 0x81);
        run_to(&mut p, 1, 455); // drains line 1's own (1,0) pulse
        assert_eq!(p.tick() & 2, 2, "OAM pulse at (2,0)");
        // LYC=2 turns true at (2,4) under the OAM blocking level: no edge
        // (gambatte lycm2int shape). The LYC level then holds to the end
        // of line 2 and overlaps the (3,0) pulse, blocking it too.
        let ifs = run_to(&mut p, 3, 100);
        assert_eq!(ifs & 2, 0, "LYC edge and the (3,0) pulse both blocked");
    }

    #[test]
    fn oam_enable_does_not_block_mode0_events() {
        // With both the OAM and hblank sources enabled, every visible
        // line's mode-0 event still fires: gambatte mstat_irq.h
        // doM0Event is blocked only by a matching delayed LYC, never by
        // the m2 enable (lcdirq_precedence/m0irq_ly44_lcdstat28 expects
        // the m0 IRQ with lcdstat $28), while the per-line m2 pulses
        // vanish (mode2IrqSchedule routes them to the line-0 slot while
        // m0en is set) — so exactly one IF per line, from the m0 event.
        let mut p = dmg();
        p.write(0xFF45, 200);
        p.write(0xFF41, 0x28); // hblank + OAM sources
        p.write(0xFF40, 0x81);
        let ifs = run_to(&mut p, 0, 252);
        assert_eq!(ifs & 2, 2, "glitch-line hblank event");
        run_to(&mut p, 1, 4);
        for line in 1..=10u8 {
            let ifs = run_to(&mut p, line, 250);
            assert_eq!(ifs & 2, 0, "line {line}: no IF before the m0 event");
            let ifs = run_to(&mut p, line + 1, 4);
            assert_eq!(ifs & 2, 2, "line {line}: m0 event fires under m2en");
        }
    }

    #[test]
    fn oam_pulse_at_vblank_entry_dmg() {
        // 144-entry OAM pulse at 144:0, one M-cycle *before* the vblank IF
        // at 144:4, on the DMG family too (wilbertpol intr_2_timing rounds
        // 5-7; gbmicrotest line_144_oam_int_b/c/d). The DMG commit is
        // halt-late, which is what lets `vblank_stat_intr-GS` observe the
        // pulse and the vblank IF in the same halt-wake cycle.
        let mut p = dmg();
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 143, 455);
        p.take_stat_halt_late();
        p.take_stat_late();
        assert_eq!(p.tick(), 0x02, "OAM pulse at 144:0, before the vblank IF");
        assert!(p.take_stat_halt_late(), "DMG 144:0 pulse is halt-late");
        assert!(p.take_stat_late(), "DMG 144:0 pulse is dispatch-late too");
        tick_n(&mut p, 3);
        assert_eq!(p.tick() & 1, 1, "vblank IF at 144:4");
    }

    #[test]
    fn oam_pulse_at_vblank_entry_cgb_not_halt_late() {
        let mut p = cgb();
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        // Run past line 143's render (the OAM level falls at the visible
        // flip), then assert the vblank-entry pulse at 144:0. Unlike the
        // visible-line pulses, the CGB 144-entry commit is visible to the
        // halt-exit sampler in its own cycle (misc/ppu/vblank_stat_intr-C
        // measures it one cycle apart from the DMG family).
        run_to(&mut p, 143, 300);
        let ifs = run_to(&mut p, 143, 455);
        assert_eq!(ifs & 2, 0, "no OAM edge between the flip and 144:0");
        p.take_stat_halt_late();
        p.take_stat_late();
        assert_eq!(tick_n(&mut p, 2) & 2, 2, "CGB OAM pulse in the 144:0 cycle");
        assert!(!p.take_stat_halt_late(), "CGB 144:0 pulse is not halt-late");
        assert!(
            !p.take_stat_late(),
            "CGB 144:0 pulse dispatches in its own cycle"
        );
        tick_n(&mut p, 2);
        assert_eq!(p.tick() & 1, 1, "vblank IF 4 dots later");
    }

    #[test]
    fn vblank_line_oam_pulses_dot12_dmg_only() {
        let mut p = dmg();
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 145, 11);
        assert_eq!(p.tick() & 2, 2, "DMG: OAM pulse at 145:12");
        run_to(&mut p, 146, 11);
        assert_eq!(p.tick() & 2, 2, "DMG: OAM pulse at 146:12");

        let mut c = cgb();
        c.write(0xFF41, 0x20);
        c.write(0xFF40, 0x81);
        run_to(&mut c, 145, 0);
        let ifs = run_to(&mut c, 153, 450);
        assert_eq!(ifs & 2, 0, "CGB: no vblank-line OAM pulses");
    }

    #[test]
    fn vblank_source_continuous_through_vblank() {
        let mut p = dmg();
        p.write(0xFF41, 0x10);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 3);
        assert_eq!(p.tick() & 2, 2, "mode-1 source rises at 144:4");
        let ifs = run_to(&mut p, 153, 455);
        assert_eq!(ifs & 2, 0, "no further edge during vblank");
        // Next frame's vblank gives the next edge.
        let ifs = run_to(&mut p, 144, 4);
        assert_eq!(ifs & 2, 2);
    }

    // --- stat_lyc_onoff behaviours ---

    #[test]
    fn lyc_flag_frozen_while_lcd_off() {
        let mut p = dmg();
        p.write(0xFF41, 0x40);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 10);
        p.write(0xFF45, 0x90); // LY = LYC = 144
        p.tick();
        assert_eq!(p.read(0xFF41), 0xC5); // cmp set, mode 1 (vblank)
        p.write(0xFF40, 0x01); // LCD off
        assert_eq!(p.read(0xFF41), 0xC4, "flag retained");
        assert_eq!(p.write(0xFF45, 0x01), 0, "comparison clock stopped: no IRQ");
        assert_eq!(p.read(0xFF41), 0xC4, "comparison clock stopped");
        assert_eq!(p.write(0xFF40, 0x81), 0); // LCD on: LY=0 vs LYC=1
        assert_eq!(p.read(0xFF41), 0xC0);
    }

    #[test]
    fn lyc_no_edge_when_comparison_unchanged_across_off_on() {
        let mut p = dmg();
        p.write(0xFF41, 0x40);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 10);
        p.write(0xFF45, 0x90);
        p.tick();
        p.write(0xFF40, 0x01);
        p.write(0xFF45, 0x00); // will match LY=0 on enable
        assert_eq!(p.read(0xFF41), 0xC4);
        assert_eq!(p.write(0xFF40, 0x81), 0, "no edge: flag stayed set");
        assert_eq!(p.read(0xFF41), 0xC4);
    }

    #[test]
    fn lyc_irq_on_lcd_enable() {
        let mut p = dmg();
        p.write(0xFF41, 0x40);
        p.write(0xFF45, 0x00);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 10);
        p.write(0xFF40, 0x01); // off with cmp clear (LY=144 vs 0)
        assert_eq!(p.read(0xFF41), 0xC0);
        // On: LY=0 vs LYC=0 -> rising edge.
        assert_eq!(
            p.write(0xFF40, 0x81),
            0x02,
            "stat_lyc_onoff round 4: IRQ in the enabling write's cycle"
        );
        assert_eq!(p.read(0xFF41), 0xC4);
    }

    #[test]
    fn stat_write_bug_dmg_only() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 1, 300); // real hblank, no sources enabled
        assert_eq!(p.read(0xFF41) & 3, 0);
        assert_eq!(
            p.write(0xFF41, 0x00),
            0x02,
            "DMG STAT write momentarily enables every source"
        );

        let mut c = cgb();
        c.write(0xFF40, 0x81);
        run_to(&mut c, 1, 300);
        assert_eq!(c.write(0xFF41, 0x00), 0, "CGB lacks the STAT write bug");
    }

    #[test]
    fn stat_write_bug_never_fires_from_the_oam_source() {
        // The glitch write enables every source for one cycle, but the m2
        // source is an event, not a level: a write landing mid-scan or
        // mid-render raises nothing (gbmicrotest stat_write_glitch_l0/l1
        // comment tables show E2 only in the hblank/vblank/LYC-match
        // positions and E0 in the mode-2 ones).
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 1, 40); // mode 2 (OAM scan)
        assert_eq!(p.write(0xFF41, 0x00), 0, "no IRQ from the mode-2 position");
        run_to(&mut p, 1, 150); // mode 3 (OAM blocking level still high)
        assert_eq!(p.write(0xFF41, 0x00), 0, "no IRQ from the mode-3 position");
        // A vblank-position write still fires (E2 in the l154 table).
        run_to(&mut p, 145, 100);
        assert_eq!(p.write(0xFF41, 0x00), 0x02, "vblank level fires");
    }

    // --- LCD off ---

    #[test]
    fn lcd_off_state() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 50, 100);
        p.write(0xFF40, 0x01);
        assert_eq!(p.read(0xFF44), 0);
        assert_eq!(p.read(0xFF41) & 3, 0);
        assert!(p.frame().iter().all(|&px| px == 0xFF_FFFF));
        let fc = p.frame_count();
        tick_n(&mut p, 100_000);
        assert_eq!(p.frame_count(), fc, "frame counter frozen while off");
        assert_eq!(p.read(0xFF44), 0);
        // OAM/VRAM freely accessible.
        p.write(0xFE10, 0x12);
        assert_eq!(p.read(0xFE10), 0x12);
    }

    /// The first frame after the LCD is (re-)enabled is not displayed: the
    /// panel stays blank/white for one frame and real output resumes with
    /// the following frame (Pan Docs "LCDC.7" warning on mid-frame
    /// enabling; SameBoy display.c skips presenting that frame —
    /// `GB_FRAMESKIP_LCD_TURNED_ON`; little-things-gb/firstwhite verifies
    /// it on hardware).
    #[test]
    fn first_frame_after_lcd_enable_is_blank() {
        let mut p = dmg();
        p.write(0xFF47, 0xE4); // identity BGP
        // Tile 0 row 0 black; the map is all tile 0, so line 0 renders
        // black across.
        p.vram[0] = 0xFF;
        p.vram[1] = 0xFF;
        p.write(0xFF40, 0x91);
        run_to(&mut p, 144, 0); // first frame boundary after enable
        assert!(
            p.frame().iter().all(|&px| px == 0xFF_FFFF),
            "first frame after LCD enable must be presented blank"
        );
        run_to(&mut p, 0, 0);
        run_to(&mut p, 144, 0); // second frame boundary
        assert_eq!(p.frame()[0], 0x00_0000, "second frame shows content");
    }

    #[test]
    fn frame_count_steady_period() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 144, 0);
        assert_eq!(p.frame_count(), 1);
        tick_n(&mut p, 70_224);
        assert_eq!(p.frame_count(), 2, "70224 dots per steady frame");
    }

    // --- DMG OAM corruption bug (Pan Docs "OAM Corruption Bug") ---

    /// PPU on a steady visible line with every OAM byte distinct, so any
    /// corruption pattern is observable and attributable.
    fn oam_bug_ppu(line: u8, dot: u16) -> Ppu {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        run_to(&mut p, line, dot);
        for (i, byte) in p.oam.iter_mut().enumerate() {
            *byte = (i as u8) ^ 0xA5;
        }
        p
    }

    /// blargg oam_bug/4-scanline_timing + 5-timing_bug pin the corruptible
    /// window in M-cycle units: the access covering dots 0-3 of a visible
    /// line corrupts the first row and the one covering dots 72-75 the
    /// last, while 76-79 (and everything later) is clean. Under
    /// tick-then-access the accessing CPU observes state(T) with the cycle
    /// covering T-4..T, so rows 8..=0x98 map to T in 4..80.
    #[test]
    fn oam_bug_row_window_tracks_scan() {
        let mut p = dmg();
        assert_eq!(p.oam_bug_row(), None, "LCD off");
        p.write(0xFF40, 0x81);
        // Glitch line: no OAM scan (lcdon_timing-GS), never vulnerable.
        for _ in 0..GLITCH_LINE_DOTS {
            assert_eq!(p.oam_bug_row(), None, "glitch line dot {}", p.dot);
            p.tick();
        }
        // Steady visible line: rows step every 4 dots through 4..80.
        for line in [1u8, 2, 143] {
            run_to(&mut p, line, 0);
            for dot in 0..456u16 {
                let expect = if (4..80).contains(&dot) {
                    Some((dot / 4 * 8) as u8)
                } else {
                    None
                };
                assert_eq!(p.oam_bug_row(), expect, "line {line} dot {dot}");
                p.tick();
            }
        }
        // VBlank lines never scan.
        run_to(&mut p, 144, 0);
        for _ in 0..456 {
            assert_eq!(p.oam_bug_row(), None, "vblank dot {}", p.dot);
            p.tick();
        }
    }

    #[test]
    fn oam_bug_write_pattern_formula() {
        // Dot 16 -> row 0x20 (row 4).
        let mut p = oam_bug_ppu(1, 16);
        let before = p.oam;
        p.oam_bug(OamBugKind::Write);
        let row = 0x20;
        for i in 0..2 {
            let (a, b, c) = (before[row + i], before[row - 8 + i], before[row - 4 + i]);
            assert_eq!(p.oam[row + i], ((a ^ c) & (b ^ c)) ^ c, "glitched byte {i}");
        }
        for i in 2..8 {
            assert_eq!(p.oam[row + i], before[row - 8 + i], "copied byte {i}");
        }
        for (i, &byte) in p.oam.iter().enumerate() {
            if !(row..row + 8).contains(&i) {
                assert_eq!(byte, before[i], "byte {i} outside the row untouched");
            }
        }
    }

    #[test]
    fn oam_bug_write_pattern_first_row_references_row_zero() {
        // Dot 4 -> row 8: operands come from row 0, which stays intact.
        let mut p = oam_bug_ppu(1, 4);
        let before = p.oam;
        p.oam_bug(OamBugKind::Write);
        let (a, b, c) = (before[8], before[0], before[4]);
        assert_eq!(p.oam[8], ((a ^ c) & (b ^ c)) ^ c);
        assert_eq!(p.oam[..8], before[..8], "row 0 untouched");
    }

    #[test]
    fn oam_bug_read_pattern_formula() {
        let mut p = oam_bug_ppu(1, 16);
        let before = p.oam;
        p.oam_bug(OamBugKind::Read);
        let row = 0x20;
        for i in 0..2 {
            let (a, b, c) = (before[row + i], before[row - 8 + i], before[row - 4 + i]);
            let glitched = b | (a & c);
            assert_eq!(p.oam[row + i], glitched, "current row byte {i}");
            assert_eq!(p.oam[row - 8 + i], glitched, "preceding row byte {i}");
        }
        for i in 2..8 {
            assert_eq!(p.oam[row + i], before[row - 8 + i], "copied byte {i}");
            assert_eq!(p.oam[row - 8 + i], before[row - 8 + i], "prev tail intact");
        }
    }

    #[test]
    fn oam_bug_read_pattern_on_uniform_oam_is_invisible() {
        // blargg 3-non_causes tolerates read corruption only because
        // b | (a & c) is the identity on uniform data.
        let mut p = oam_bug_ppu(1, 16);
        p.oam = [0x5A; 0xA0];
        p.oam_bug(OamBugKind::Read);
        assert_eq!(p.oam, [0x5A; 0xA0]);
    }

    #[test]
    fn oam_bug_read_increase_pattern_at_row_4_and_up() {
        let mut p = oam_bug_ppu(1, 16);
        let before = p.oam;
        p.oam_bug(OamBugKind::ReadIncrease);
        let row = 0x20;
        // Glitched first word lands in the *preceding* row, then that row
        // (glitched word included) is copied to both the current row and
        // two rows back (SameBoy v0.12.1 GB_trigger_oam_bug_read_increase;
        // the trailing plain read corruption is a no-op after the copy).
        let mut expect_prev = [0u8; 8];
        expect_prev.copy_from_slice(&before[row - 8..row]);
        for i in 0..2 {
            let (a, b, c, d) = (
                before[row - 0x10 + i],
                before[row - 8 + i],
                before[row + i],
                before[row - 4 + i],
            );
            expect_prev[i] = (b & (a | c | d)) | (a & c & d);
        }
        for (i, &expect) in expect_prev.iter().enumerate() {
            assert_eq!(p.oam[row - 0x10 + i], expect, "two rows back {i}");
            assert_eq!(p.oam[row - 8 + i], expect, "preceding row {i}");
            assert_eq!(p.oam[row + i], expect, "current row {i}");
        }
        for (i, &byte) in p.oam.iter().enumerate() {
            if !(row - 0x10..row + 8).contains(&i) {
                assert_eq!(byte, before[i], "byte {i} outside the rows untouched");
            }
        }
    }

    #[test]
    fn oam_bug_read_increase_in_first_rows_is_plain_read() {
        // Rows 1..=3 (and the last row) skip the special pattern: SameBoy
        // v0.12.1 guards 0x20 <= row < 0x98. Dot 8 -> row 0x10.
        let mut p = oam_bug_ppu(1, 8);
        let mut reference = oam_bug_ppu(1, 8);
        p.oam_bug(OamBugKind::ReadIncrease);
        reference.oam_bug(OamBugKind::Read);
        assert_eq!(p.oam, reference.oam);

        // Dot 76 -> row 0x98 (the last row): also plain read only.
        let mut p = oam_bug_ppu(1, 76);
        let mut reference = oam_bug_ppu(1, 76);
        p.oam_bug(OamBugKind::ReadIncrease);
        reference.oam_bug(OamBugKind::Read);
        assert_eq!(p.oam, reference.oam);
    }

    #[test]
    fn oam_bug_outside_window_is_a_no_op() {
        for dot in [0u16, 80, 200, 300] {
            let mut p = oam_bug_ppu(1, dot);
            let before = p.oam;
            p.oam_bug(OamBugKind::Write);
            p.oam_bug(OamBugKind::Read);
            p.oam_bug(OamBugKind::ReadIncrease);
            assert_eq!(p.oam, before, "dot {dot}");
        }
    }

    // --- CGB-C LY/STAT line timeline (single speed) ---
    //
    // The CGB line grid differs from the DMG one in a handful of
    // CPU-visible windows; each test below cites the hardware oracle in
    // its comment. DMG behaviour must stay bit-identical (mooneye-frozen).

    /// CGB line 0 dots 0-3 read STAT mode 1 — the vblank persists into
    /// line 0; there is no mode-0 gap (wilbertpol ly00_mode1_2-C round 6
    /// vs ly00_mode1_0-GS; SameBoy display.c keeps STAT mode for line 0
    /// on CGB at the LY write dot; gambatte getStat's mode-1 window ends
    /// 3 cycles before line 0's mode 2).
    #[test]
    fn cgb_line0_reads_mode1_dots_0_3() {
        let mut p = cgb();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 400); // past the glitch frame
        run_to(&mut p, 0, 0);
        assert_eq!(p.read(0xFF41) & 3, 1, "CGB line 0 dot 0 reads mode 1");
        tick_n(&mut p, 3);
        assert_eq!(p.read(0xFF41) & 3, 1, "CGB line 0 dot 3 reads mode 1");
        p.tick();
        assert_eq!(p.read(0xFF41) & 3, 2, "mode 2 from dot 4");

        let mut d = dmg();
        d.write(0xFF40, 0x81);
        run_to(&mut d, 153, 400);
        run_to(&mut d, 0, 0);
        assert_eq!(d.read(0xFF41) & 3, 0, "DMG line 0 dot 0 reads mode 0");
    }

    /// CGB has no forced-invalid LYC gap at line starts: the comparator
    /// holds the previous line's value through dots 0-3 and switches at
    /// dot 4 (wilbertpol ly_lyc-C round 7: STAT reads $C4 — mode 0, flag
    /// still set for LYC = previous line — at the start of the next
    /// line; ly_lyc_144-C round 7 pins the same on the 144→145 edge).
    #[test]
    fn cgb_lyc_compare_holds_previous_line_through_dot_3() {
        let mut p = cgb();
        p.write(0xFF45, 2);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 3, 0);
        assert_eq!(p.read(0xFF41) & 4, 4, "CGB (3,0): flag holds line 2");
        tick_n(&mut p, 3);
        assert_eq!(p.read(0xFF41) & 4, 4, "CGB (3,3): flag holds line 2");
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 0, "CGB (3,4): compares line 3");

        let mut d = dmg();
        d.write(0xFF45, 2);
        d.write(0xFF40, 0x81);
        run_to(&mut d, 3, 0);
        assert_eq!(d.read(0xFF41) & 4, 0, "DMG (3,0): invalid window");
    }

    /// CGB line 153 LYC windows: the comparator sees 152 during dots
    /// 0-3, 153 during dots 4-11 (twice as long as DMG — wilbertpol
    /// ly_lyc_153-C rounds 7/8 read $C5 one M-cycle later than the -GS
    /// build), and 0 from dot 12 (same dot as DMG — ly_lyc_0-C ==
    /// ly_lyc_0-GS expectations).
    #[test]
    fn cgb_ly153_lyc_compare_windows() {
        let mut p = cgb();
        p.write(0xFF45, 153);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 0);
        assert_eq!(p.read(0xFF41) & 4, 0, "dots 0-3 hold the 152 compare");
        tick_n(&mut p, 4);
        assert_eq!(p.read(0xFF41) & 4, 4, "dot 4: 153 compare");
        tick_n(&mut p, 7);
        assert_eq!(p.read(0xFF41) & 4, 4, "dot 11: still 153");
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 0, "dot 12: 0 compare");

        // LYC=152 stays matched through 153's dots 0-3.
        let mut p = cgb();
        p.write(0xFF45, 152);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 3);
        assert_eq!(p.read(0xFF41) & 4, 4, "dots 0-3 hold the 152 compare");
        p.tick();
        assert_eq!(p.read(0xFF41) & 4, 0, "dot 4: 153 compare");
    }

    /// CGB-C: LY reads 153 from dot 454 of line 152 — two dots before
    /// the line starts — through dot 3, wrapping to 0 at dot 4 like DMG
    /// (wilbertpol ly_new_frame-C reads 153 on two consecutive
    /// frame-anchored M-cycles — the CGB boot grid sits 2 dots off the
    /// M lattice — while age ly-dmgC-cgbBC's enable-anchored ladder
    /// sees it exactly once, and three times at 2-dot spacing in double
    /// speed; only the early load satisfies all three).
    #[test]
    fn cgb_ly153_loads_two_dots_early() {
        let mut p = cgb();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 152, 453);
        assert_eq!(p.read(0xFF44), 152);
        p.tick();
        assert_eq!(p.read(0xFF44), 153, "LY=153 from (152,454)");
        run_to(&mut p, 153, 3);
        assert_eq!(p.read(0xFF44), 153);
        p.tick();
        assert_eq!(p.read(0xFF44), 0, "LY=0 from (153,4)");

        let mut d = dmg();
        d.write(0xFF40, 0x81);
        run_to(&mut d, 152, 454);
        assert_eq!(d.read(0xFF44), 152, "DMG keeps LY=152 to the line end");
    }

    /// CGB VRAM read blocking starts 3 dots later than DMG — a read at
    /// state(80) still returns data (gambatte vramReadable `lineCycles <
    /// 76 + 3*cgb`; SameBoy oam_search_index-37 `vram_read_blocked =
    /// !GB_is_cgb`; age vram-read-cgbBCE).
    #[test]
    fn cgb_vram_read_open_through_dot_82() {
        let mut p = cgb();
        p.write(0xFF40, 0x81);
        p.write(0x9000, 0x5A);
        run_to(&mut p, 1, 80);
        assert_eq!(p.read(0x9000), 0x5A, "CGB state(80) readable");
        tick_n(&mut p, 3);
        assert_eq!(p.read(0x9000), 0xFF, "CGB state(83) blocked");

        let mut d = dmg();
        d.write(0xFF40, 0x81);
        d.write(0x9000, 0x5A);
        run_to(&mut d, 1, 80);
        assert_eq!(d.read(0x9000), 0xFF, "DMG state(80) blocked");
    }

    /// CGB OAM write blocking: line-start dots 0-3 block writes on lines
    /// whose predecessor was a visible line, and the DMG dots-80-83
    /// writable gap does not exist (gambatte oamWritable: blocked from
    /// `lineCycles + 3 + cgb >= 456` with the `lineCycles == 76` escape
    /// DMG-only; SameBoy sets oam_write_blocked = GB_is_cgb at line
    /// start; age oam-write-cgbBCE).
    #[test]
    fn cgb_oam_write_blocked_at_line_start_and_scan_end() {
        let mut p = cgb();
        p.write(0xFF40, 0x81);
        run_to(&mut p, 2, 0);
        p.write(0xFE00, 0x12);
        assert_eq!(p.oam[0], 0, "CGB (2,0) write blocked");
        run_to(&mut p, 2, 80);
        p.write(0xFE00, 0x34);
        assert_eq!(p.oam[0], 0, "CGB (2,80) write blocked");
        // Line 0's dots 0-3 follow a vblank line: writable (gambatte
        // oamWritable's `ly >= 143` arm — lyCounter still reads 153).
        run_to(&mut p, 0, 0);
        p.write(0xFE00, 0x56);
        assert_eq!(p.oam[0], 0x56, "CGB (0,0) write lands");

        let mut d = dmg();
        d.write(0xFF40, 0x81);
        run_to(&mut d, 2, 0);
        d.write(0xFE00, 0x12);
        assert_eq!(d.oam[0], 0x12, "DMG (2,0) write lands");
        run_to(&mut d, 2, 80);
        d.write(0xFE00, 0x34);
        assert_eq!(d.oam[0], 0x34, "DMG (2,80) write lands");
    }

    /// CGB single speed: an FF45 write whose comparison raises the STAT
    /// line produces its IF bit one M-cycle after the write instead of
    /// inside the write cycle (gambatte lycRegChange schedules a oneshot
    /// at cc+5 for cgb && !ds; lyc_ff45_trigger_delay_2 carries the
    /// dmg08_out0/cgb04c_out2 split).
    #[test]
    fn cgb_lyc_write_irq_is_one_mcycle_late() {
        let mut p = cgb();
        p.write(0xFF41, 0x40);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 5, 200);
        assert_eq!(p.write(0xFF45, 5), 0, "CGB: no IF in the write cycle");
        assert_eq!(tick_n(&mut p, 4) & IF_STAT, IF_STAT, "IF one cycle later");

        let mut d = dmg();
        d.write(0xFF41, 0x40);
        d.write(0xFF40, 0x81);
        run_to(&mut d, 5, 200);
        assert_eq!(d.write(0xFF45, 5), IF_STAT, "DMG: IF in the write cycle");
    }

    /// CGB FF45 writes near a line boundary follow gambatte's
    /// lycRegChangeTriggersStatIrq: a write committing at the line-start
    /// M-cycle cannot stop that line's event (the delayed `lyc_event`
    /// copy), a now-matching value written there raises nothing, and a
    /// write in the previous line's last M-cycle compares against the
    /// upcoming line (wilbertpol ly_lyc_write-C rounds 1-4).
    #[test]
    fn cgb_lyc_write_line_boundary_windows() {
        // Round-2 shape: killing the match at (N,0) is too late — the
        // dot-4 event still fires from the old LYC.
        let mut p = cgb();
        p.write(0xFF41, 0x40);
        p.write(0xFF45, 2);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 2, 0);
        assert_eq!(p.write(0xFF45, 0xF0), 0);
        assert_eq!(tick_n(&mut p, 4) & IF_STAT, IF_STAT, "event from old LYC");

        // Round-4 shape: making a match at (N,0) raises nothing — the
        // event sampled the old value and the write-trigger compares
        // the upcoming line.
        let mut p = cgb();
        p.write(0xFF41, 0x40);
        p.write(0xFF45, 0xF0);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 3, 0);
        assert_eq!(p.write(0xFF45, 2), 0);
        assert_eq!(tick_n(&mut p, 12) & IF_STAT, 0, "no IRQ this line");

        // Round-1 shape: a kill one M-cycle earlier does reach the event.
        let mut p = cgb();
        p.write(0xFF41, 0x40);
        p.write(0xFF45, 2);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 1, 452);
        assert_eq!(p.write(0xFF45, 0xF0), 0);
        assert_eq!(tick_n(&mut p, 12) & IF_STAT, 0, "event disarmed");
    }

    /// The CGB vblank STAT-source level extends through line 0 dots 0-3
    /// together with the visible mode 1 (gambatte getStat + the
    /// lycEnable lyc0_m1disable cgb04c_outE0 rows: an LYC edge under it
    /// stays blocked).
    #[test]
    fn cgb_vblank_level_holds_through_line0_dots_0_3() {
        let mut p = cgb();
        p.write(0xFF41, 0x10);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 153, 400);
        run_to(&mut p, 0, 3);
        assert!(p.stat_line, "level still high at (0,3)");
        p.tick();
        assert!(!p.stat_line, "level drops at (0,4)");

        let mut d = dmg();
        d.write(0xFF41, 0x10);
        d.write(0xFF40, 0x81);
        run_to(&mut d, 153, 400);
        run_to(&mut d, 0, 0);
        assert!(!d.stat_line, "DMG level low from (0,0)");
    }
}
