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

// Behavior-preserving submodules (each a second `impl Ppu` block). The struct,
// its fields, the enums, the consts, and the core driver (new/tick/step_dot/
// start_line) stay here.
mod blocking;
mod line_setup;
mod lyc;
#[path = "stat_irq/read_laws.rs"]
mod stat_irq_read_laws;
#[path = "stat_irq/reclock.rs"]
mod reclock;
mod regs;
mod render;
mod stat_irq;

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
    /// #11bg — the S5 engine's FF41 enable view (SameBoy reads the two-phase
    /// `io_registers[GB_IO_STAT]` itself in `GB_STAT_update`). Mirrors
    /// `stat_en` everywhere except the staged window after a CGB FF41 write
    /// on the tier2/LE path: SameBoy's `GB_CONFLICT_STAT_CGB[_DOUBLE]`
    /// (sm83_cpu.c:168-188) commits the write in TWO phases — at T0 every bit
    /// lands EXCEPT the LYC enable (bit 6, single speed) / the HBlank enable
    /// (bit 3, double speed), which hold their OLD value one more T-cycle.
    /// The failing lycEnable want-pairs straddle exactly that lag (a disable
    /// whose phase-1 window covers the `ly_for_comparison` latch dot still
    /// fires the LYC edge — `ff41_disable_2` dual-traced: SBWRITE val=40
    /// phase-1 then the ly6 STAT_IRQ then val=00). Production/flag-off never
    /// reads this (only `stat_update_tick` consumes it) → byte-identical OFF.
    eng_stat: u8,
    /// Pending [`Self::eng_stat`] transition from a CGB single-speed FF41
    /// write: `(phase1, final, pre_write_line_high, mfi_at_t0,
    /// ticks_since_write)`. Schedule (engine ticks after the deferred commit
    /// dot D; SameBoy T0 ≈ D+2, dual-traced):
    /// * tick D+1: the OLD view;
    /// * tick D+2 (T0): `phase1` = mode bits NEW, **bit6 OLD**
    ///   (`GB_CONFLICT_STAT_CGB` holds the LYC enable one T past the mode
    ///   bits). A rise here is a mode-source enable reaching its effective
    ///   instant and fires; a fall is forced silently. The tick's
    ///   `mode_for_interrupt` is saved as `mfi_at_t0`;
    /// * tick D+3: externals still edge against phase-1 (`ff41_disable_2`'s
    ///   ly6 dot-4 LYC latch rise against the armed old bit6);
    /// * tick D+4: the final value, EVALUATED against `mfi_at_t0` (the
    ///   T0+1T-instant mode — the sub-dot dip `lyc1_m2irq_late_lycdisable_1`
    ///   pins: the line falls before the next line's OAM carryover, so the
    ///   ly2 mode-2 rise re-fires). A fall forces the line low silently; a
    ///   rise (the bit6-late enable) fires iff the line was LOW at the write
    ///   (the m1→LYC handoff `lyc153_late_enable_m1disable_3` stays silent —
    ///   hardware is hazard-free where SameBoy's intersection phase dips,
    ///   E0-vs-E2 measured), delivered through the CGB `lyc_if_delay` (the
    ///   FF41 twin of the FF45-write delay — `lyc_ff41_trigger_delay`).
    /// * At the line's mode-3→0 flip a stage past T0 FAST-FORWARDS to final
    ///   (the flip sits later than T0+1T in SameBoy's frame), with a forced
    ///   dip when the final value cannot hold the line
    ///   (`m0enable/lycdisable_ff41_scx*`: the dying LYC hold and the mode-0
    ///   rise are separated on hardware, collapsed by slopgb's early flip).
    eng_stat_pending: Option<(u8, u8, bool, u8, u8)>,
    /// Previous engine tick's `mode_for_interrupt` (m0-flip detection for
    /// the fast-forward above). Tier-2/LE only.
    eng_mfi_prev: u8,
    /// #11bg — the DS analogue of the m0-flip fast-forward dip: the (line,
    /// dot) of the last DS FF41 commit that DROPPED the LYC enable. At DS
    /// the engine view is immediate (no stage), so a bit6-drop landing on
    /// the dot before the mode-3→0 flip collapses the hardware's
    /// drop-then-rise into one seamless tick; the flip tick consumes this
    /// to force the sub-dot dip (`m0enable/lycdisable_ff41_ds_1` want 2).
    ff41_ds_drop: Option<(u8, u16)>,
    /// #11bh — FF0F group-B write-race squash: dots remaining in which a STAT
    /// engine rise is CONSUMED by a just-committed bit1-clearing FF0F write
    /// (SameBoy `GB_CONFLICT_WRITE_CPU`: the IF write lands leading-edge +1 T
    /// and beats a co/prior-instant rise; a consumed rise does not
    /// level-re-raise — strict edge). Armed to 2 at the deferred FF0F write,
    /// decremented per engine dot, one-shot on consumption. Tier-2 only.
    stat_if_squash: u8,
    /// #11bh group C — the deferred-path dispatch-ack squash, the PPU-side
    /// replacement for the interconnect's whole-dot bit-0/1 `ack_squash_dots`
    /// (zeroed under tier2). A rise of the acked bit landing within the
    /// per-SOURCE window after the ack is merged into the dispatch (SameBoy:
    /// the rise fp at/before the SBACK fp was already in the sampled IF);
    /// past it, it survives and re-sets IF (the six `late_*_retrigger` rows).
    /// Windows in dots (SS, DS), dual-traced 2026-07-03: mode-0 (0, 1) ·
    /// mode-2 pulse (0, 0) · LYC / mode-1 / vblank-IF (2, 0). Armed to 2 at
    /// `ack`, decremented per dot, consumed as `ctr >= 3 − W`. `mask` names
    /// the acked bit. Tier-2 only (never armed flag-off).
    ack_squash_ppu_mask: u8,
    ack_squash_ppu: u8,
    /// #11bh group E — the line-0 dot-4 OAM pulse's read-view age: armed (1)
    /// when that pulse's engine rise fires, decremented per dot. A deferred
    /// FF0F read landing on the SAME dot masks the just-folded bit from its
    /// VERDICT (CPU-read-first at the shared instant — measured on SameBoy:
    /// `SBREAD ff0f` at the rise fp reads clear; `lyc153int_m2irq_1` reads
    /// line-0 dot 4 co-instant with the pulse and wants 0, its `_2` sibling
    /// reads 4 dots later and sees it). Verdict-only; `intf` keeps the bit.
    ly0_pulse_age: u8,
    /// #11bh item-7 count-row slice — the SHIFTED-frame (post-STOP,
    /// `lcd_shift_dots != 0`) mode-0 rise's read-view age + dot: a deferred
    /// FF0F poll landing on the rise's own dot reads the bit CLEAR
    /// (CPU-first at the shared instant; the lcd_offset count rows'
    /// first-poll law — `offset1_lyc99int_m0irq_count_scx2_ds_1` polls
    /// dot 257 co-instant with the rise, wants E0; the error is ONE-SIDED,
    /// the `_2` siblings read 2 dots later and keep seeing it).
    /// Verdict-only.
    m0sh_age: u8,
    m0sh_dot: u16,
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
    /// Half-dot phase within the current dot (0 or 1), the 8 MHz sub-dot grain
    /// of the pixel-pipe reclock (HALFDOT-BUILD-PLAN.md Part A). Advanced by
    /// [`Self::tick_half`] on the tier2 deferred path only; production
    /// ([`Self::tick`], the whole-dot advance) never touches it and leaves it 0
    /// so the flag-off path is byte-identical. `dhalf == 0` after a
    /// dot-completing half-dot (the whole-dot work ran), `1` mid-dot (a DS read
    /// landing on an odd CPU-T resolves here — the sub-dot read position Part B
    /// samples). See [`Self::sub_dot`].
    dhalf: u8,
    /// #11bd — the persistent LCD phase residual, in 8 MHz half-dots: the
    /// accumulated difference between SameBoy's CPU-grid shift at each STOP
    /// speed-switch LEAVE (+2 hd per leave, measured by the lcd_offset
    /// enable-phase dual-traces) and the machine advance the STOPADV default
    /// applied (w=4). Carried across LCD disable/enable (the phase is a
    /// CPU-grid-vs-PPU displacement, not a frame property — the lcd_offset
    /// ROMs re-enable the LCD after their excursions and the offset
    /// persists). Consumed by the tier2 offset-sensitive laws
    /// (accessibility windows / write-triggers / the P1 comparison); always 0
    /// flag-off (only written from the tier2 STOP path) so production is
    /// byte-identical.
    lcd_phase_hd: i16,
    /// #11bd — shadow of SameBoy's `double_speed_alignment` (mod 8): the LCD
    /// age in 8 MHz half-dots since the last enable (+2 per dot while the LCD
    /// is on, reset at enable), with a −4 correction per STOP pause (the
    /// measured SameBoy-vs-slopgb pause-length + freeze-withholding delta,
    /// calibrated on the offset1 enter→leave segment: SameBoy Δdsa 24 vs
    /// slopgb Δage 28 mod 8). The DS→SS leave shift depends on this
    /// alignment (`2 + (sb_dsa8 & 4)`: dsa7=0 rows need +2, the dsa7=4
    /// offset3 rows need +6 — build-measured). Increment is unconditional
    /// (mod-8 counter, unobservable flag-off); consumed tier2-only.
    sb_dsa8: u8,
    /// #11bd — total machine STOPADV advance applied this ROM, in DOTS
    /// (Σ k/2 over the DS→SS leaves). The frame-anchored write/read-instant
    /// law windows (line-start tails, line-153 wrap, LY holds) were
    /// calibrated on unshifted ROMs; a post-leave CPU access lands
    /// `lcd_shift_dots` deeper in the frame (the advance moved the enable
    /// anchor and the LY-polls re-sync), so those laws classify against
    /// [`Self::law_pos`]. 0 for never-switched ROMs and flag-off.
    lcd_shift_dots: u16,
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
    /// #11aq (C2 read-position carry): the source of the STAT IRQ that set the
    /// currently-pending STAT bit was the mode-2 OAM line-start rise (`mfi==2`),
    /// not a mode-0/LYC rise. A sticky level updated on every STAT 0→1 edge in
    /// [`Self::stat_update_halt_masks`]; the interconnect's `dispatch_retime`
    /// reads it to apply the per-ISR deferred-read carry (the OAM-ISR handler's
    /// reads land 1 M-cycle = 2 dots DS later than the mode-0-ISR's, decoupled
    /// from the IF-delivery ack — `cpu-timing-map.md §7.1`). Inert unless
    /// `SLOPGB_M2CARRY`; production never reaches `stat_update_halt_masks`.
    stat_rise_oam: bool,
    /// #11aq: the currently-pending STAT IRQ was the mode-0 HBlank rise
    /// (`mfi==0`). The mode-0 ISR read lands +2 dots early (vs the mode-2 +4),
    /// so its carry is half. Mutually exclusive with [`Self::stat_rise_oam`]
    /// (one source per 0→1 edge); both false for a pure-LYC rise.
    stat_rise_m0: bool,
    /// #11ar (C2 SCOPED carried-read exit override): set by the interconnect's
    /// `dispatch_retime` when it carried a STAT-ISR read (`carry_read`), so the
    /// FIRST FF41 mode read of the handler — now landed at SameBoy's absolute
    /// cfl — resolves its verdict against SameBoy's bare exit `SBex` instead of
    /// slopgb's native mode (a full 3↔0 override, both directions). Cleared by
    /// the interconnect after that FF41 read (one-shot). This SCOPING is the
    /// #11aq global-consistency fix: the blanket `M2HOLD` exit law fired for
    /// non-carried polled/other-ISR reads too (dropping 50 SameBoy-passes whose
    /// native frame was already correct); gating the SBex override on
    /// `read_carried` confines it to exactly the reads the carry moved to
    /// SameBoy's frame. Inert unless `SLOPGB_M2CARRY`.
    read_carried: bool,
    /// The externally visible mode-0 flip (STAT mode bits, OAM/VRAM
    /// unblock): rises with `m0_src` ahead of the pipe end (see
    /// `m0_flip_events` in render.rs), and can drop back mid-line when
    /// a late write arms a new stall (`m0_unflip`).
    line_render_done: bool,
    /// PORT 1 (#11bc) — the dot `line_render_done` fired on this line (0 =
    /// not fired yet / dropped by `m0_unflip`). The half-dot bare-exit law
    /// (`vis_mode_read`) anchors the CPU-visible mode-3→0 exit to the RENDER's
    /// actual flip (`exit_hd = 2*flip_dot + 2`), so a mid-line SCX write moves
    /// the exit exactly as the fine-scroll hunt resolved it (late_scx4 /
    /// scx_m3_extend — a live-`scx` closed form mis-frames the missed-hunt
    /// leg). Recorded on both fire paths (projection + the `advance_lx` pipe-
    /// end fallback); reset at line start, `m0_unflip`, and LCD transitions.
    /// Only read under `tier2_reclock` → production byte-identical.
    flip_dot: u16,
    /// Port Stage S2c — the CPU-visible STAT mode→0 boundary back-dated to
    /// SameBoy's cycle-exact frame, **decoupled from the IRQ-dispatch flip**
    /// (`line_render_done`/`m0_src`). On the `leading_edge_reads` flag-on path
    /// this rises 3 dots *before* `line_render_done` on bare single-speed lines,
    /// so `vis_mode` reads 0 at SameBoy's `ModeTimeline::visible_mode0_dot`
    /// (our-line dot 251 = 254 − 3) while the dispatch stays at our dot 254 —
    /// the instrumented separator of the kernel pair (`m2int_m3stat_1` read at
    /// our dot 248 stays mode 3, `m0int_m3stat_2` at dot 252 reads mode 0;
    /// `ppu-subdot-ladder.md` "A5 INSTRUMENTED + KERNEL SEPARATED"). Gated to
    /// **bare single-speed** lines (`r.fetched == 0 && !win_active && !glitch &&
    /// !ds`), the regime the +3 back-date was measured on; the sprite/window
    /// (+2 DMG) and double-speed (+4) back-dates are derived-but-unmeasured and
    /// stay on `line_render_done` for now. **Always `false` on the flag-off
    /// (production) path** (the set is gated on `leading_edge_reads`), so
    /// `vis_mode` reads `line_render_done` exactly — byte-identical in
    /// production. The OAM/VRAM accessibility unblock (`blocking.rs`) keeps the
    /// `line_render_done` dot for now (the visible-vs-accessibility 3-dot window
    /// is the S4 back-dating). Reset at line start + on `m0_unflip`.
    vis_early: bool,
    /// Port Stage C/S5 mech-1 — the window vis-HOLD: the dot until which the
    /// CPU-visible STAT mode stays 3 on a `win_active` line, EVEN AFTER
    /// `line_render_done`/`vis_early`. The symmetric inverse of `vis_early`
    /// (which only ANTICIPATES the visible flip earlier): SameBoy extends a
    /// TRIGGERING window's mode-3 to ≈ `263 + SCX&7` (the measured window-length
    /// law, `window-groundtruth-2026-06-24.md`), past the counter-pinned
    /// dispatch dot, while slopgb's window flip is flat at ~261. Set in
    /// `m0_flip_events` when the flip fires on a `win_active` line under
    /// `tier2_reclock` (0 = no hold); consumed only by `vis_mode` — the IRQ
    /// dispatch (`line_render_done`) is NOT moved.
    ///
    /// **Validated foundation, currently INERT** (like `cycle_clock`/
    /// `mode_timeline`): the rows it targets are blocked on a SEPARATE missing
    /// piece — the want=3 window rows render BARE on the measurement frame
    /// (`wy_ok=false`, a render-level WY-latch trigger gap, `win_active=false`
    /// so the hold cannot reach them) and the win-active fails read BEFORE the
    /// dispatch (want=0, need the opposite direction). Measured 0/233 alone;
    /// it is the visible-mode half of the C2 parallel window-length model
    /// (which must also replicate the WY-latch trigger to drive it). See
    /// `measurements/vis-hold-target-exhaustion-2026-06-26.md`. **Always 0 on
    /// the flag-off path** (never set when `tier2_reclock` is false) →
    /// byte-identical in production. Reset at line start + on `m0_unflip`, like
    /// `vis_early`.
    vis_hold_until: u16,
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
    /// Port Stage S2b: the **interrupt-facing** mode, decoupled from the
    /// CPU-visible `vis_mode` (SameBoy `mode_for_interrupt`, `gb.h:612`).
    /// On a visible line it diverges from the visible mode in two one-dot
    /// windows (`ppu-timing-map.md` §2): the OAM (mode-2) IRQ mode goes to 2
    /// **one dot before** the visible byte does (lines 1-143, `display.c:1787`
    /// vs `:1792`), and the mode-0 IRQ mode goes to 0 **one dot after** the
    /// visible byte does (`display.c:2108` vs `:2091`). That 2-dot relative
    /// swing is what separates the `m2int`/`m0int` kernel pair. Consumed by the
    /// S5 [`StatUpdate`](crate::stat_update) engine on the flag-on path
    /// (`stat_update_tick`); on the flag-off (production) path it is maintained
    /// every dot but read only by the S2b decoupling test. Mirrors `vis_mode` on
    /// VBlank / glitch lines (anchor swing not modelled there until S5).
    mode_for_interrupt: u8,
    /// One-dot-delayed mirror of `line_render_done`, the substrate for the
    /// mode-0 lag above: `line_render_done` rises on the visible 3→0 flip
    /// dot, so its previous-dot value is still false there and true one dot
    /// later — exactly the dot the IRQ-facing mode transitions to 0.
    mfi_m0_prev: bool,
    /// Port Stage S5 (flag-on path only): SameBoy's `GB_STAT_update`
    /// rising-edge STAT interrupt line ([`StatUpdate`](crate::stat_update)),
    /// driven each dot from `mode_for_interrupt` | the LYC latch and replacing
    /// `stat_events_tick` when `leading_edge_reads` is on. Inert (never read)
    /// while the flag is off, so it changes nothing in production.
    stat_update: crate::stat_update::StatUpdate,
    /// SameBoy `lyc_interrupt_line` (`display.c:534`): the LYC==LY STAT source
    /// as a *latch* — re-evaluated to `ly_for_comparison == LYC` whenever
    /// `ly_for_comparison` is a real line, and HELD across the `-1` "no line"
    /// gaps (so a match survives the line-boundary dot). The LYC input
    /// `stat_update` consumes on the flag-on path.
    lyc_interrupt_line: bool,
    /// PPU-side copy of the interconnect's `leading_edge_reads` master flag,
    /// selecting the S5 [`StatUpdate`](crate::stat_update) engine over
    /// `stat_events_tick`. Off in production until the atomic flip (S2+S3);
    /// forwarded by [`Interconnect::set_leading_edge_reads`].
    leading_edge_reads: bool,
    /// PPU-side copy of the interconnect's `tier2_reclock` flag (port Stage B,
    /// the −2 dispatch reclock). Gates the B3 mode-0 IRQ dispatch move
    /// (254→252) so the leading-edge-only S0 specs (which set
    /// `leading_edge_reads` but NOT this) keep the validated Tier-1 frame.
    /// Forwarded by [`Interconnect::set_tier2_reclock`].
    tier2_reclock: bool,
    /// The STAT IF bit handed out by the last tick came from the mode-0
    /// source rise. The interconnect drains this and applies the
    /// half-cycle halt law: a rise landing in the second half of the
    /// CPU's M-cycle is readable at once — and fully visible to the
    /// running CPU's interrupt sample — but missed by the halt-exit
    /// sampler for one M-cycle (the same shape as the line-start OAM
    /// pulses' `stat_halt_late`). mooneye hblank_ly_scx_timing-GS and
    /// gbmicrotest int_hblank_halt_scx0-7 pin all eight SCX phases.
    m0_rise: bool,
    /// The mode-3→mode-0 OAM/VRAM *accessibility* unblock fired on the
    /// current dot (`line_render_done` set true by `m0_flip_events`).
    /// On hardware this access edge trails the mode-0 IRQ rise by one
    /// half-dot (gambatte `m0Time`/accessibility at xpos lcd_hres+7 vs the
    /// IRQ at lcd_hres+6); a CPU accessibility read samples at the cc+2
    /// MID phase, two dots before this whole-M-cycle's end-sampled view,
    /// so it still reads mode 3 when the unblock lands in the M-cycle's
    /// second half. The interconnect drains this via
    /// [`Self::take_m0_access_flip`] and half-classifies it against the
    /// dot-loop index (the eighth-grid MID-vs-End comparison; increment 1
    /// of the sub-dot event-phase model — routes only the OAM-read arm).
    /// `Some(lead_eighths)` when the flip lands this dot, carrying its sub-dot
    /// offset for [`event_phase`](crate::interconnect) (reclock S1; `Some(0)` =
    /// the net-zero dot-END commit), `None` otherwise.
    m0_access_flip: Option<i8>,
    /// The CGB palette-RAM unblock fired on the current dot
    /// (`render_finished` set true at the pipe end, one dot after the
    /// HDMA trigger `hdma_lead`). Like `m0_access_flip` but anchored at the
    /// palette/render-end edge: a CPU FF69/FF6B read samples at the cc+2
    /// MID phase, so it still reads $FF when the unblock lands in the
    /// M-cycle's second half. Drained via [`Self::take_pal_access_flip`]
    /// (sub-dot event-phase model; routes only the CGB palette read).
    /// `Some(lead_eighths)` when the flip lands this dot (reclock S1/S2 carry
    /// the per-SCX palette-unblock sub-dot offset here; `Some(0)` = net-zero
    /// whole-M-cycle commit), `None` otherwise.
    pal_access_flip: Option<i8>,
    /// The mode-3→mode-0 *STAT mode-bit* flip fired on the current dot
    /// (`line_render_done` set true by `m0_flip_events`). Gated to
    /// *sprite-extended* lines (`r.fetched != 0`) — the complement of
    /// `m0_access_flip`'s bare-line gate: it routes the FF41 mode-bit read,
    /// which the OAM/VRAM-read gate does not cover, on exactly the lines the
    /// `m3stat_ds` cluster exercises. Bare-line DS reads reach FF41 through
    /// the DMA-cycle / lcd-offset chains at a different sub-cycle offset, so a
    /// bare-line override regresses them (the parked multi-chain problem). In
    /// CGB double speed the visible flip lands at a sub-dot (cc) phase the
    /// whole-dot grid cannot place, so a CPU STAT read whose M-cycle straddles
    /// the flip still reads mode 3 (gambatte's `m3stat_ds_1` rows). The
    /// interconnect drains this via [`Self::take_m0_stat_flip`] and
    /// half-classifies it against the dot-loop index (`2*(i+1) > dots`): a
    /// second-half flip holds the FF41 read at mode 3. The override is gated
    /// to double speed; the single-speed read, and DS reads reaching FF41
    /// through other dispatch chains, are the parked multi-chain INC3
    /// problem. Sub-dot event-phase model, increment INC-DS-1.
    /// `Some(lead_eighths)` when the flip lands this dot (reclock S1/S3 carry the
    /// flip's sub-dot offset here; `Some(0)` = net-zero whole-M-cycle commit),
    /// `None` otherwise.
    m0_stat_flip: Option<i8>,
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
    /// Part C/D (tier2) — the dot the render pipe ended this line (pixel 160
    /// shipped, `render_finished` rise; 0 = not yet this line). The CGB
    /// palette-RAM read/write unblock trails it by 1 dot at single speed and
    /// is coincident in double speed on the deferred (cc+0) frame — the
    /// `cgbpal_m3end` constraint table (`c-palette-rederive-2026-07-02.md`).
    /// Consumed only by the tier2 arm of [`Self::pal_ram_blocked`]; reset per
    /// line, so production (which never reads it) is byte-identical.
    pal_open_dot: u16,

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
    /// **C2 #11af shadow WY-trigger (tier2 + CGB only; byte-identical OFF —
    /// these fields are never updated nor read on the production path).**
    /// SameBoy latches `wy_triggered` from a *continuous* `WY == LY` compare
    /// during the visible frame (`display.c` `wy_check`), whereas slopgb's
    /// production `wy_latch` samples only at the three gambatte weMaster dots
    /// (line 0 dot 2, dots 450/454) — so a mid-line late-WY write that SameBoy
    /// catches is missed by slopgb's discrete sampler. This sticky latch
    /// re-derives SameBoy's decision for the FF41-read window-length law
    /// ([`Self::vis_mode_read`]) without touching `line_render_done` / the
    /// render. Reset at line 0; set the first dot `win_en && wy2 == ly`.
    wy_trig_sb: bool,
    /// The (line, dot) the shadow latch was set — the window extends mode 3 on
    /// a line iff the latch was set on an earlier line OR on this line at/before
    /// the WX-activation dot ([`Render::wx_match_dot`]). See `wy_trig_sb`.
    wy_trig_sb_line: u8,
    wy_trig_sb_dot: u16,
    /// #11bi — the POST-SWITCH bare-exit law latches (tier2-only writers;
    /// byte-identical OFF). The speedchange m3stat 4-variable exit table
    /// (`measurements/speedchange-postswitch-exit-2026-07-03.md`, all 120
    /// legs dual-traced) collapses to per-class rp-frame exits
    /// `E = C + 2*(SCX&7)` consumed by [`Self::vis_exit_hd`]:
    ///   SS post-leave: `C = 504 + leave_k − 4*[lcd_enable_in_ds]`
    ///   DS post-enter: `C = 502 + leave_k` (leave_k = 2 when never left)
    /// scoped to dances whose FIRST LCD-on switching STOP sits MID-FRAME
    /// (line < 144): the whole tier2 DS/SS suite is calibrated on the
    /// VBlank/boot-prologue frame (kernel `_ds`, lcd_offset offset1-3,
    /// gdma_cycles all anchor at ly144 — measured), which already absorbs
    /// the switch error; only the mid-frame-anchored speedchange dances
    /// (v1/2/3/4/5 ly44 + m2int lcdoff variants, first STOP at ly68/ly133)
    /// expose the true post-switch exit. `stop_anchor_midframe` is the
    /// first-LCD-on-STOP-since-enable position latch, taken at the STOP
    /// DECISION instant (the lcdoff dances anchor at their STOP#2 decision,
    /// ly0 dot12 — the DS re-enable reset the line counter); an LCD enable
    /// re-anchors the frame and clears it (SameBoy `double_speed_alignment
    /// = 0` at enable — the e-law: the DS enable quantizes the phase).
    stop_anchor_set: bool,
    stop_anchor_midframe: bool,
    /// A DS→SS STOP leave completed with the LCD enabled (the SameBoy
    /// freeze path the exit table measures). Cleared at LCD off/on.
    stop_leave_lcd_on: bool,
    /// The leave advance k (half-dots, 2 or 6 = the `sb_dsa8`-branched
    /// #11bd keystone) of the most recent LCD-on leave; 2 when never left.
    stop_leave_k: u8,
    /// The most recent LCD enable happened in double speed (the lcdoff
    /// dances re-enable in DS; the −4 rp class term — the DS enable
    /// re-anchors the PPU frame where a run-through LCD keeps the SS
    /// boot phase).
    lcd_enable_in_ds: bool,
    /// C2 #11aw — the IMMEDIATE-WY twin of [`Self::wy_trig_sb`]. SameBoy's
    /// `wy_check` compares LY against the immediate WY register
    /// (`io_registers[GB_IO_WY]`), NOT the 6-dot-lagged `wy2` copy slopgb's
    /// render (and `wy_trig_sb`) use. A late WY→(non-LY) write (`late_wy_1toFF`)
    /// UN-triggers SameBoy's window (raw WY != LY at the line-start compare)
    /// while slopgb — comparing the still-lagged `wy2` — triggers it and renders
    /// the window (`win_active`). This sticky latch (set the first dot
    /// `win_en && self.wy == ly`, immediate WY) re-derives SameBoy's trigger;
    /// when slopgb's render triggered (`win_active`) but this did NOT
    /// (`!wy_trig_sb_raw`), the line is SameBoy-bare and the FF41 read law
    /// ([`Self::vis_mode_read`]) forces mode 0. Reset at line 0. tier2 + CGB.
    wy_trig_sb_raw: bool,
    /// #11bd item 4 — the BOUNDARY-WY cross-line trigger: a WY write
    /// committing in a line's tail (dot >= 452) or head (dot < 4) whose
    /// value matches the CURRENT (old) line latches SameBoy's
    /// `wy_triggered` (its scheduled `wy_check` still compares the old
    /// `current_line`), while slopgb's render (`wy_latch`) and the
    /// wy2-lagged shadow both miss it — every later line renders bare
    /// where SameBoy draws the window. Frame-sticky like `wy_triggered`;
    /// reset at the frame top. Tier2 + CGB only (byte-identical OFF).
    wy_xline_trig: bool,
    /// #11bd item 5 — the last CPU VRAM write ATTEMPT's (line, dot), for the
    /// DS line-end VRAM read release: a readback following a same-line write
    /// within ~2 DS M-cycles keeps the SS view (SameBoy spreads the write's
    /// M-cycle cost across the readback — `vramw_m3end_ds_2` wants BLOCKED
    /// at the dot where the write-free `prewrite_ds` readback is open).
    vram_wr_line: u8,
    vram_wr_dot: u16,
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

/// S5 read-dot tracer gate: true iff `SLOPGB_S5DBG` is set in the environment.
/// Cached once (the dispatch trace runs every dot, so a per-tick `getenv` would
/// dominate the probe run-time). Byte-identical when unset — the tracer is a
/// session-local measurement aid for the atomic read-frame reclock; see
/// `docs/sameboy-port/tools/stat-irq-trace.md`.
pub(crate) fn s5dbg_on() -> bool {
    use std::sync::OnceLock;
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::env::var_os("SLOPGB_S5DBG").is_some())
}

/// TEMP (#11an+) per-bus-op ISR T-sequence trace gate (`SLOPGB_ISRTRACE`):
/// logs every deferred read/write/internal access's (addr, ly, dot, clk, pend)
/// so the handler advance can be lined up against SameBoy's `SB2` per-access
/// trace. Byte-identical when unset; never read in production.
pub(crate) fn isrtrace_on() -> bool {
    use std::sync::OnceLock;
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::env::var_os("SLOPGB_ISRTRACE").is_some())
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
            eng_stat: 0,
            eng_stat_pending: None,
            eng_mfi_prev: 0,
            ff41_ds_drop: None,
            stat_if_squash: 0,
            ack_squash_ppu_mask: 0,
            ack_squash_ppu: 0,
            ly0_pulse_age: 0,
            m0sh_age: 0,
            m0sh_dot: 0,
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
            dhalf: 0,
            lcd_phase_hd: 0,
            sb_dsa8: 0,
            lcd_shift_dots: 0,
            glitch_line: false,
            frame_skip: false,
            cmp: false,
            stat_line: false,
            pending_if: 0,
            stat_late: false,
            m0_src: false,
            m0_rise_dot: false,
            mode_for_interrupt: 0,
            mfi_m0_prev: false,
            stat_update: crate::stat_update::StatUpdate::new(),
            lyc_interrupt_line: false,
            leading_edge_reads: false,
            tier2_reclock: false,
            m0_rise: false,
            m0_access_flip: None,
            pal_access_flip: None,
            m0_stat_flip: None,
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
            stat_rise_oam: false,
            stat_rise_m0: false,
            read_carried: false,
            line_render_done: true,
            flip_dot: 0,
            vis_early: false,
            vis_hold_until: 0,
            render_finished: true,
            hdma_lead: false,
            pal_open_dot: 0,
            wy_latch: false,
            wy2: 0,
            wy2_delay: 0,
            wy_trig_sb: false,
            wy_trig_sb_line: 0,
            wy_trig_sb_dot: 0,
            wy_trig_sb_raw: false,
            stop_anchor_set: false,
            stop_anchor_midframe: false,
            stop_leave_lcd_on: false,
            stop_leave_k: 2,
            lcd_enable_in_ds: false,
            wy_xline_trig: false,
            vram_wr_line: 0xFF,
            vram_wr_dot: 0,
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
            // S5 flag-on engine: with the LCD off `GB_STAT_update` returns
            // early (`display.c:525`) and the interrupt line is held low, so a
            // re-enable edge-detects from a clean low. Inert flag-off (the
            // fields are unread), so this changes nothing in production.
            self.stat_update = crate::stat_update::StatUpdate::new();
            self.lyc_interrupt_line = false;
            // #11bg — a staged FF41 engine view must not survive an LCD-off
            // gap and apply at a stale tick after re-enable.
            self.eng_stat = self.stat_en;
            self.eng_stat_pending = None;
            self.ff41_ds_drop = None;
            self.stat_if_squash = 0;
            self.ack_squash_ppu = 0;
            self.ack_squash_ppu_mask = 0;
            return std::mem::take(&mut self.pending_if);
        }
        if self.lyc_if_delay > 0 {
            self.lyc_if_delay -= 1;
            if self.lyc_if_delay == 0 {
                // CGB-deferred FF45-write STAT IRQ (see `lyc_if_delay`).
                self.pending_if |= IF_STAT;
            }
        }
        // #11bd: the SameBoy `double_speed_alignment` shadow (see `sb_dsa8`).
        self.sb_dsa8 = (self.sb_dsa8 + 2) & 7;
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
        // S2b: maintain the decoupled interrupt-facing mode (inert — not yet
        // consulted; the STAT engine swap that reads it is S5). Runs after
        // step_dot so it sees this dot's `line_render_done` flip.
        self.update_mode_for_interrupt();
        if self.leading_edge_reads {
            // S5 flag-on path: the SameBoy `GB_STAT_update` rising-edge engine
            // off the decoupled `mode_for_interrupt` + the LYC latch.
            self.stat_update_tick();
        } else {
            // Production path: the gambatte-derived per-source event engine.
            self.stat_events_tick();
        }
        // #11bh group C — age the dispatch-ack squash window (armed only on
        // the tier2 path; a saturating decrement of an always-0 counter is
        // byte-identical flag-off).
        self.ack_squash_ppu = self.ack_squash_ppu.saturating_sub(1);
        self.ly0_pulse_age = self.ly0_pulse_age.saturating_sub(1);
        self.m0sh_age = self.m0sh_age.saturating_sub(1);
        std::mem::take(&mut self.pending_if)
    }

    /// Advance one 8 MHz HALF-dot (HALFDOT-BUILD-PLAN.md Part A). The pixel-pipe
    /// reclock's grain: two half-dots per whole dot (single speed = 2 half-dots
    /// per CPU-T; double speed = 1). The first half of a dot (`dhalf 0→1`) does
    /// no structural work and the second (`dhalf 1→0`) runs the whole-dot
    /// [`Self::tick`] body, so a run of aligned half-dots is byte-identical to
    /// the whole-dot advance (Stage 1); the seam is that later stages move a
    /// mode-3-exit / read boundary onto the odd half-dot. Called only on the
    /// tier2 deferred path ([`Interconnect::advance_machine_t`]); production
    /// never calls it, so the flag-off path is untouched. Returns the IF bits
    /// produced (0 on the non-completing half).
    pub(crate) fn tick_half(&mut self) -> u8 {
        if self.dhalf == 0 {
            self.dhalf = 1;
            return 0;
        }
        self.dhalf = 0;
        self.tick()
    }

    /// Whether the half-dot just advanced by [`Self::tick_half`] completed a
    /// whole dot (the whole-dot body ran). The caller folds the PPU's IF /
    /// accessibility edges only on a completing half.
    pub(crate) fn dot_completed(&self) -> bool {
        self.dhalf == 0
    }

    /// The current half-dot phase (0 = at a dot boundary, 1 = mid-dot). Part B
    /// samples this to resolve a deferred read to the exact 8 MHz half-dot the
    /// CPU-T lands on (a DS read on an odd CPU-T sits mid-dot). Always 0 on the
    /// whole-dot production path. Consumed by [`Self::read_pos_hd`] (Part B,
    /// HALFDOT-BUILD-PLAN.md §3B) and the `read_deferred` dual-trace.
    pub(crate) fn sub_dot(&self) -> u8 {
        self.dhalf
    }

    /// Part B (HALFDOT-BUILD-PLAN.md §3B) — the deferred read's EXACT half-dot
    /// position within the current line: `2*dot + dhalf` on the 8 MHz grid.
    /// [`Interconnect::read_deferred`] advances the machine T-granularly to the
    /// read's leading edge (the `GB_display_sync` analogue), so at the sample
    /// instant this IS the read's true half-dot — a DS read landing on an odd
    /// CPU-T resolves mid-dot (`dhalf == 1`), which the whole-dot `self.dot`
    /// alone cannot represent (the "+3 not +4" DS ISR read offset). Every
    /// half-dot read-position law compares against this ONE value; the per-ISR
    /// sub-M-cycle carry is [`Self::isr_read_carry_hd`], kept separate so
    /// polled reads stay uncarried. Production reads never resolve mid-dot
    /// (`dhalf` stays 0 flag-off) and no flag-off law consumes this.
    pub(crate) fn read_pos_hd(&self) -> i32 {
        2 * i32::from(self.dot) + i32::from(self.dhalf)
    }

    /// Part B — the per-ISR deferred-read sub-M-cycle carry (8 MHz half-dots),
    /// applied ON TOP of [`Self::read_pos_hd`] by the laws that model a
    /// STAT-ISR handler's first FF41 read. The #11aq/#11ar measured offsets:
    /// a carried (`read_carried`) mode-2 OAM-ISR read sits +4 hd late of the
    /// polled frame at single speed, a mode-0 HBlank-ISR read +2 hd; in double
    /// speed only the mode-0-ISR read differs (−4 hd — the #11ar full-carry
    /// law's `off = m0 ? 2 : 4` rewritten on the half-dot grid, exit-folded).
    /// 0 for polled/uncarried reads. Byte-identical OFF (`read_carried` is
    /// only armed on the tier2 dispatch path).
    pub(super) fn isr_read_carry_hd(&self) -> i32 {
        if !self.read_carried {
            return 0;
        }
        if self.ds {
            if self.stat_rise_m0 { -4 } else { 0 }
        } else if self.stat_rise_oam {
            4
        } else if self.stat_rise_m0 {
            2
        } else {
            0
        }
    }

    /// #11bd — accumulate the LCD phase residual at a STOP speed-switch leave
    /// (see [`Self::lcd_phase_hd`]). Tier2 STOP path only.
    pub(crate) fn add_lcd_phase(&mut self, hd: i16) {
        self.lcd_phase_hd += hd;
    }

    /// #11bd — the SameBoy `double_speed_alignment` shadow, mod 8 (see
    /// [`Self::sb_dsa8`]). Read by the STOP leave shift; the −4-per-pause
    /// correction is applied by [`Self::dsa_pause_correction`].
    pub(crate) fn sb_dsa(&self) -> u8 {
        self.sb_dsa8
    }

    /// #11bd — apply the per-STOP-pause alignment correction (−4 mod 8, the
    /// measured SameBoy-vs-slopgb pause delta). Tier2 STOP path only.
    pub(crate) fn dsa_pause_correction(&mut self) {
        self.sb_dsa8 = (self.sb_dsa8 + 4) & 7;
    }

    /// #11bd — record a machine STOPADV advance (see [`Self::lcd_shift_dots`]).
    pub(crate) fn add_lcd_shift(&mut self, dots: u16) {
        self.lcd_shift_dots += dots;
    }

    /// #11bi — latch the post-switch exit-table anchor at a switching STOP
    /// (see [`Self::stop_anchor_midframe`]). Called at the STOP decision
    /// point, tier2 only; the FIRST LCD-on switching STOP since the last
    /// LCD enable pins the dance's calibration class.
    pub(crate) fn note_switch_stop(&mut self) {
        if self.enabled && !self.stop_anchor_set {
            self.stop_anchor_set = true;
            self.stop_anchor_midframe = self.line < 144;
        }
    }

    /// #11bi — record a DS→SS STOP leave (see [`Self::stop_leave_lcd_on`]);
    /// `k` = the applied leave advance in half-dots. Tier2 only.
    pub(crate) fn note_switch_leave(&mut self, k: u8) {
        if self.enabled {
            self.stop_leave_lcd_on = true;
            self.stop_leave_k = k;
        }
    }

    /// #11bd — the current access position mapped back onto the un-shifted
    /// calibrated frame (see [`Self::lcd_shift_dots`]): subtract the machine
    /// advance, wrapping across the line boundary. Identity when no advance
    /// was applied (never-switched ROMs, production).
    pub(super) fn law_pos(&self) -> (u8, u16) {
        let s = self.lcd_shift_dots;
        if s == 0 {
            return (self.line, self.dot);
        }
        if self.dot >= s {
            (self.line, self.dot - s)
        } else {
            let prev = if self.line == 0 { 153 } else { self.line - 1 };
            (prev, LINE_DOTS - (s - self.dot))
        }
    }

    /// Forward the interconnect's `leading_edge_reads` master flag to the PPU,
    /// selecting the S5 [`StatUpdate`](crate::stat_update) engine. Off in
    /// production until the atomic flip (which flips the default in `new`, not
    /// via this hook); driven by [`Interconnect::set_leading_edge_reads`] (the
    /// S5 unit tests + the S0 kernel-pair acceptance spec).
    pub(crate) fn set_leading_edge_reads(&mut self, on: bool) {
        self.leading_edge_reads = on;
    }

    /// Forward the interconnect's `tier2_reclock` flag (port Stage B). Gates
    /// the B3 mode-0 IRQ dispatch move; implies `leading_edge_reads`.
    pub(crate) fn set_tier2_reclock(&mut self, on: bool) {
        self.tier2_reclock = on;
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
        // C2 #11af shadow WY-trigger (tier2-only; byte-identical OFF).
        // SameBoy's `wy_triggered` is a continuous `WY == LY` latch, sticky for
        // the frame; reset it at the frame top (line 0 dot 0) and set it the
        // first dot the compare holds on any visible line. See `wy_trig_sb`.
        // #11bj: recording widened to DMG (was CGB-only) for the DMG window
        // law port — the DMG arms in `read_laws.rs` read the same latches.
        if self.tier2_reclock {
            if self.line == 0 && self.dot == 0 {
                self.wy_trig_sb = false;
                self.wy_trig_sb_raw = false;
                self.wy_xline_trig = false;
            }
            // #11aw — the raw-WY sticky latch (immediate `self.wy`, SameBoy's
            // `wy_check` input), the un-trigger discriminator. Gated `dot >= 4`
            // (the mode-2 OAM-scan compare window SameBoy runs `wy_check` in):
            // a line-start (dot 0) WY write commits AFTER slopgb's dot-0 PPU tick
            // (the tick runs before `write_no_tick`), so a dot-0 compare would
            // read the OLD WY and mis-latch; `dot >= 4` samples the settled WY,
            // matching SameBoy's post-write compare (`late_wy_1toFF_1` WY→FF at
            // dot 0 → WY=FF by dot 4 → never latches → SameBoy-bare).
            if self.line < 144
                && self.dot >= 4
                && !self.wy_trig_sb_raw
                && win_en
                && self.wy == self.ly
            {
                self.wy_trig_sb_raw = true;
            }
            if self.line < 144 && !self.wy_trig_sb && win_en && self.wy2 == self.ly {
                self.wy_trig_sb = true;
                self.wy_trig_sb_line = self.ly;
                self.wy_trig_sb_dot = self.dot;
                if crate::ppu::s5dbg_on() {
                    eprintln!(
                        "SLOPGB wytrigset ly={} dot={} wy2={}",
                        self.ly, self.dot, self.wy2
                    );
                }
            }
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
            // TEMP measurement scaffold (#11an genuine-length vs read-frame
            // split): trace the EFFECTIVE CPU-visible mode-3→0 EXIT dot — the
            // dot `vis_mode_read()` (what the FF41 register read returns,
            // incl. the window law / vis_hold / m0_unflip re-projection)
            // actually flips 3→0. This is the slopgb ground-truth exit to
            // line up against SameBoy SBMODE, robust to `vis_early` resets the
            // `visflip` tracer can't see. `SLOPGB_S5DBG`, byte-identical unset.
            if crate::ppu::s5dbg_on() {
                use std::cell::Cell;
                thread_local!(static PREV: Cell<u8> = const { Cell::new(255) });
                let vm = self.vis_mode_read();
                PREV.with(|p| {
                    if p.get() == 3 && vm == 0 {
                        eprintln!("SLOPGB visexit ly={} dot={}", self.line, self.dot);
                    }
                    p.set(vm);
                });
            }
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
            // #11bh group C — a vblank-vector ack 1-2 dots earlier (SS)
            // merges this raise into the dispatch it interrupted
            // (`lycint_vblankirq_late_retrigger_2` want 0: ack 144:2, raise
            // 144:4 consumed; the `_ds_1` ack at 144:3 DELIVERS — DS window
            // 0). Never armed flag-off → production byte-identical.
            let w = if self.ack_squash_ppu_mask & IF_VBLANK != 0 && !self.ds {
                2
            } else {
                0
            };
            if w > 0 && self.ack_squash_ppu >= 3 - w {
                self.ack_squash_ppu = 0;
            } else {
                self.pending_if |= IF_VBLANK;
            }
        }
    }

    // --- CPU access blocking (boundaries from lcdon_timing-GS /
    // --- lcdon_write_timing-GS; see module docs) ---

    // --- DMG OAM corruption bug (Pan Docs "OAM Corruption Bug") ---

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

    /// #11bd — [`Self::hdma_period`] classified on the un-shifted frame for
    /// CPU-instant consults (FF55 arming, halt-entry snapshot, wake re-eval,
    /// STOP window): a shifted entry near the line end mis-reads the 3-dot
    /// margin (`hdma_late_m0halt_lcdoffset3_1` enters halt at dot 455 where
    /// the un-shifted frame is dot 452 — still inside). Cross-line law
    /// positions keep the conservative false. Identity when unshifted; the
    /// per-dot machine edge detector keeps the real [`Self::hdma_period`].
    pub(crate) fn hdma_period_law(&self) -> bool {
        if self.lcd_shift_dots == 0 {
            return self.hdma_period();
        }
        let len = if self.glitch_line {
            GLITCH_LINE_DOTS
        } else {
            LINE_DOTS
        };
        let (ll, ld) = self.law_pos();
        self.hdma_trigger_level() && ll == self.line && ld + 3 < len
    }

    /// LCDC bit 7 as committed (architectural view).
    pub(crate) fn lcd_enabled(&self) -> bool {
        self.enabled
    }

    /// S5 read-dot tracer position: the PPU's current `(line, dot)` scan
    /// position. Pure accessor (no behaviour), used by the `SLOPGB_S5DBG`
    /// FF41-read trace in [`crate::interconnect::Interconnect::read_deferred`]
    /// to line slopgb's read dot up against SameBoy's `cycles_for_line`.
    pub(crate) fn scan_pos(&self) -> (u8, u16) {
        (self.line, self.dot)
    }

    /// Whether the PPU is on the LCD-enable glitch line (452 dots, dot-82
    /// pipe). The tier2 SCX write-strobe deferral (#11bb) keeps the
    /// production staging there — the glitch line's render geometry carries
    /// its own C1.2-calibrated offsets (`GLITCH_MODE3_START` 78 entry, +2
    /// `early_lead`), and the +4 render-frame lag mis-frames its fine-scroll
    /// hunt (`enable_display/ly0_late_scx7_m3stat_*`, measured).
    pub(crate) fn glitch_active(&self) -> bool {
        self.glitch_line
    }

    /// TEMP (#11an) read-state probe for the genuine-length/read-frame split:
    /// `(win_active, vis_early, line_render_done, vis_hold_until, raw vis_mode,
    /// n_sprites)` at the deferred FF41 read. Lets the `SLOPGB ff41` trace show
    /// WHY the read sees its mode (window still active → extended mode 3 vs a
    /// bare re-projection). Pure accessor; revert with the tracer.
    pub(crate) fn dbg_read_state(&self) -> (bool, bool, bool, u16, u8, u8) {
        (
            self.render.win_active,
            self.vis_early,
            self.line_render_done,
            self.vis_hold_until,
            self.vis_mode(),
            self.render.n_sprites,
        )
    }

    /// #11bj measurement accessor — window-abort state at the deferred FF41
    /// read: `(win_aborted, win_predraw_abort, win_predraw_abort_dot,
    /// wx_match_dot)`. Lets the `SLOPGB ff41` trace show which abort class a
    /// disabled-window line took (pre-draw bare vs in-draw kept-cost). Pure
    /// accessor; reverts with the tracer.
    pub(crate) fn dbg_abort_state(&self) -> (bool, bool, u16, u16, u8, u8) {
        (
            self.render.win_aborted,
            self.render.win_predraw_abort,
            self.render.win_predraw_abort_dot,
            self.render.wx_match_dot,
            self.eff.scx & 7,
            self.render.wx_match_scx,
        )
    }

    /// #11bj measurement accessor — window enable/re-enable/WX-write commit
    /// dots at the deferred FF41 read: `(win_enable_dot, win_reenable_dot,
    /// wx_write_dot)`. For the late-enable / re-enable / late-WX law fits.
    pub(crate) fn dbg_win_dots(&self) -> (u16, u16, u16) {
        (
            self.render.win_enable_dot,
            self.render.win_reenable_dot,
            self.render.wx_write_dot,
        )
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
#[path = "mod_tests.rs"]
mod tests;
