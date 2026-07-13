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

// Behavior-preserving submodules (each a second `impl Ppu` block, except
// `oam_bug` which holds free fns). The struct, its fields, the `PipeRegs`/
// `StagedWrite` helper structs, and the consts stay here.
mod access;
mod blocking;
mod engine;
mod line_setup;
mod lyc;
mod oam_bug;
#[path = "stat_irq/reclock.rs"]
mod reclock;
mod regs;
#[path = "regs/stage.rs"]
mod regs_stage;
mod render;
mod sgb;
mod stat_irq;
#[path = "stat_irq/ff0f.rs"]
mod stat_irq_ff0f;
#[path = "stat_irq/read_laws.rs"]
mod stat_irq_read_laws;
#[path = "stat_irq/read_laws_exit.rs"]
mod stat_irq_read_laws_exit;
mod state;

use crate::SCREEN_PIXELS;
use crate::model::Model;

use render::Render;
use sgb::SgbView;

// `OamBugKind` is referenced crate-wide as `crate::ppu::OamBugKind`; the
// OAM-bug pattern fns are called bare from `blocking.rs` via its `use super::*`.
pub(crate) use oam_bug::OamBugKind;
use oam_bug::{oam_bug_read_increase_pattern, oam_bug_read_pattern, oam_bug_write_pattern};

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

/// Pending [`Ppu::eng_stat`] transition staged by a CGB single-speed FF41
/// write on the tier2/LE engine path (was a bare `(u8, u8, bool, u8, u8)`).
/// The tick-by-tick two-phase-commit schedule this drives lives on the
/// [`Ppu::eng_stat_pending`] field doc.
#[derive(Clone, Copy)]
pub(crate) struct EngStatPending {
    /// Phase-1 view: mode bits are NEW, bit6 (the LYC enable) still OLD.
    pub phase1: u8,
    /// The final committed FF41 STAT-source value.
    pub fin: u8,
    /// The pre-write STAT line was HIGH (gates whether the delayed final rise
    /// may fire).
    pub pre_high: bool,
    /// `mode_for_interrupt` sampled at the T0+1T instant — what `fin` is
    /// evaluated against.
    pub mfi_t0: u8,
    /// Engine ticks elapsed since the deferred commit dot (the stage counter).
    pub k: u8,
}

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
#[derive(Clone)]
struct PipeRegs {
    lcdc: u8,
    /// The BG fetcher's LCDC addressing view (map/data select).
    /// Tracks `lcdc` in lockstep in production (byte-identical), but under the
    /// tier2 render reclock it lags the eager control commit by the +4
    /// render-frame offset (see `render_lcdc_pending`), so a mid-mode-3 LCDC
    /// bit3/bit4 write (bgtilemap/bgtiledata) reaches the fetch grid at the
    /// production/SameBoy dot instead of the leading edge. The window bit5
    /// (abort/reenable/enable) side-effects + the FF41 read laws keep the eager
    /// `lcdc` — their tier2 pins are calibrated to the cc+0 control commit.
    render_lcdc: u8,
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
#[derive(Clone)]
struct StagedWrite {
    addr: u16,
    value: u8,
    /// Dots until the new value drives the pipeline's register view.
    dots_left: u8,
}

#[derive(Clone)]
pub struct Ppu {
    model: Model,
    frame_count: u64,

    /// The CPU has written an LCD register (FF40-FF4B) since power-on — the boot
    /// hand-off PPU frame is no longer pristine. Gates the DMG boot-frame read
    /// law ([`Self::boot_read`]): the `poweron_*` ROMs read the untouched boot
    /// frame (pure NOP sled, no PPU write), while every other early reader
    /// configures the PPU first — `lcdon_to_*`/`oam_read`/`sprite`/`win` toggle
    /// the LCD (FF40), the gambatte kernel/halt STAT-ISR tests arm a mode
    /// interrupt (FF41) — and reads its own frame at cc+0. Set on the tier2 CPU
    /// write path only (`interconnect/cycle.rs`), so the boot ROM's own register
    /// install does not trip it; never read in production → byte-identical OFF.
    lcd_regs_written: bool,

    // Registers.
    lcdc: u8,
    /// STAT bits 3-6 (interrupt source enables).
    stat_en: u8,
    /// The engine's FF41 enable view (SameBoy reads the two-phase
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
    eng_stat_pending: Option<EngStatPending>,
    /// HALFDOT (#11dw): a DMG FF41 engine-view (`eng_stat`) write scheduled to
    /// commit at its true WriteCpu sub-dot position, `(value, half_dots_left)`,
    /// counted down by the odd-half engine ([`Ppu::stat_update_half`]) so the
    /// disable/enable lands at the coincident LYC re-latch / mode-0 flip half-
    /// dot rather than the whole-dot cc+4 commit. `None` (and unread) except
    /// under `eager_value` → production/tier2 byte-identical.
    eng_stat_half: Option<(u8, u8)>,
    /// Previous engine tick's `mode_for_interrupt` (m0-flip detection for
    /// the fast-forward above). Tier-2/LE only.
    eng_mfi_prev: u8,
    /// The DS analogue of the m0-flip fast-forward dip: the (line,
    /// dot) of the last DS FF41 commit that DROPPED the LYC enable. At DS
    /// the engine view is immediate (no stage), so a bit6-drop landing on
    /// the dot before the mode-3→0 flip collapses the hardware's
    /// drop-then-rise into one seamless tick; the flip tick consumes this
    /// to force the sub-dot dip (`m0enable/lycdisable_ff41_ds_1` want 2).
    ff41_ds_drop: Option<(u8, u16)>,
    /// FF0F group-B write-race squash: dots remaining in which a STAT
    /// engine rise is CONSUMED by a just-committed bit1-clearing FF0F write
    /// (SameBoy `GB_CONFLICT_WRITE_CPU`: the IF write lands leading-edge +1 T
    /// and beats a co/prior-instant rise; a consumed rise does not
    /// level-re-raise — strict edge). Armed to 2 at the deferred FF0F write,
    /// decremented per engine dot, one-shot on consumption. Tier-2 only.
    stat_if_squash: u8,
    /// The deferred-path dispatch-ack squash, the PPU-side
    /// replacement for the interconnect's whole-dot bit-0/1 `ack_squash_dots`
    /// (zeroed under tier2). A rise of the acked bit landing within the
    /// per-SOURCE window after the ack is merged into the dispatch (SameBoy:
    /// the rise fp at/before the SBACK fp was already in the sampled IF);
    /// past it, it survives and re-sets IF (the six `late_*_retrigger` rows).
    /// Windows in dots (SS, DS): mode-0 (0, 1) ·
    /// mode-2 pulse (0, 0) · LYC / mode-1 / vblank-IF (2, 0). Armed to 2 at
    /// `ack`, decremented per dot, consumed as `ctr >= 3 − W`. `mask` names
    /// the acked bit. Tier-2 only (never armed flag-off).
    ack_squash_ppu_mask: u8,
    ack_squash_ppu: u8,
    /// The line-0 dot-4 OAM pulse's read-view age: armed (1)
    /// when that pulse's engine rise fires, decremented per dot. A deferred
    /// FF0F read landing on the SAME dot masks the just-folded bit from its
    /// VERDICT (CPU-read-first at the shared instant — measured on SameBoy:
    /// `SBREAD ff0f` at the rise fp reads clear; `lyc153int_m2irq_1` reads
    /// line-0 dot 4 co-instant with the pulse and wants 0, its `_2` sibling
    /// reads 4 dots later and sees it). Verdict-only; `intf` keeps the bit.
    ly0_pulse_age: u8,
    /// The SHIFTED-frame (post-STOP,
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
    /// of the pixel-pipe reclock. Advanced by
    /// [`Self::tick_half`] on the tier2 deferred path only; production
    /// ([`Self::tick`], the whole-dot advance) never touches it and leaves it 0
    /// so the flag-off path is byte-identical. `dhalf == 0` after a
    /// dot-completing half-dot (the whole-dot work ran), `1` mid-dot (a DS read
    /// landing on an odd CPU-T resolves here — the sub-dot read position
    /// samples).
    dhalf: u8,
    /// The persistent LCD phase residual, in 8 MHz half-dots: the
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
    /// Shadow of SameBoy's `double_speed_alignment` (mod 8): the LCD
    /// age in 8 MHz half-dots since the last enable (+2 per dot while the LCD
    /// is on, reset at enable), with a −4 correction per STOP pause (the
    /// measured SameBoy-vs-slopgb pause-length + freeze-withholding delta,
    /// calibrated on the offset1 enter→leave segment: SameBoy Δdsa 24 vs
    /// slopgb Δage 28 mod 8). The DS→SS leave shift depends on this
    /// alignment (`2 + (sb_dsa8 & 4)`: dsa7=0 rows need +2, the dsa7=4
    /// offset3 rows need +6 — build-measured). Increment is unconditional
    /// (mod-8 counter, unobservable flag-off); consumed tier2-only.
    sb_dsa8: u8,
    /// Total machine STOPADV advance applied this ROM, in DOTS
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
    /// The source of the STAT IRQ that set the
    /// currently-pending STAT bit was the mode-2 OAM line-start rise (`mfi==2`),
    /// not a mode-0/LYC rise. A sticky level updated on every STAT 0→1 edge in
    /// [`Self::stat_update_halt_masks`]; the interconnect's `dispatch_retime`
    /// reads it to apply the per-ISR deferred-read carry (the OAM-ISR handler's
    /// reads land 1 M-cycle = 2 dots DS later than the mode-0-ISR's, decoupled
    /// from the IF-delivery ack). Reached only under `tier2_reclock` (via
    /// `dispatch_retime`); production (tier2 off) never reaches it.
    stat_rise_oam: bool,
    /// The currently-pending STAT IRQ was the mode-0 HBlank rise
    /// (`mfi==0`). The mode-0 ISR read lands +2 dots early (vs the mode-2 +4),
    /// so its carry is half. Mutually exclusive with [`Self::stat_rise_oam`]
    /// (one source per 0→1 edge); both false for a pure-LYC rise.
    stat_rise_m0: bool,
    /// Set by the interconnect's
    /// `dispatch_retime` when it carried a STAT-ISR read (`carry_read`), so the
    /// FIRST FF41 mode read of the handler — now landed at SameBoy's absolute
    /// cfl — resolves its verdict against SameBoy's bare exit `SBex` instead of
    /// slopgb's native mode (a full 3↔0 override, both directions). Cleared by
    /// the interconnect after that FF41 read (one-shot). This SCOPING is the
    /// global-consistency fix: the blanket `M2HOLD` exit law fired for
    /// non-carried polled/other-ISR reads too (dropping 50 SameBoy-passes whose
    /// native frame was already correct); gating the SBex override on
    /// `read_carried` confines it to exactly the reads the carry moved to
    /// SameBoy's frame. Reached only under `tier2_reclock` (via
    /// `dispatch_retime`); production (tier2 off) never reaches it.
    read_carried: bool,
    /// Set by the interconnect's eager CGB halt wake (`halt_wake_mid_impl`) when
    /// the halt exits on the mode-0 STAT rise: the halt-woken IME=1 dispatch's
    /// first FF41 read is a re-fetch M-cycle that SameBoy resolves at the next
    /// line's OAM (mode 2), which the eager read reaches only once its +8hd cc+4
    /// debt has crossed the line boundary (`read_pos_hd >= LINE_DOTS*2`). Stays
    /// set across the sub-boundary polls, fires+clears on the boundary-crossing
    /// FF41 read (`vis_mode_read`), backstop-cleared at the next halt entry.
    /// Only the sub-M-cycle wake peek separates the want-0 siblings off this read
    /// position, so the flag has no collateral (#11cz was −9 without the peek).
    /// Armed only on the eager clock (`eager_value`) → byte-identical OFF.
    halt_refetch: bool,
    /// The externally visible mode-0 flip (STAT mode bits, OAM/VRAM
    /// unblock): rises with `m0_src` ahead of the pipe end (see
    /// `m0_flip_events` in render.rs), and can drop back mid-line when
    /// a late write arms a new stall (`m0_unflip`).
    line_render_done: bool,
    /// The dot `line_render_done` fired on this line (0 =
    /// not fired yet / dropped by `m0_unflip`). The half-dot bare-exit law
    /// (`vis_mode_read`) anchors the CPU-visible mode-3→0 exit to the RENDER's
    /// actual flip (`exit_hd = 2*flip_dot + 2`), so a mid-line SCX write moves
    /// the exit exactly as the fine-scroll hunt resolved it (late_scx4 /
    /// scx_m3_extend — a live-`scx` closed form mis-frames the missed-hunt
    /// leg). Recorded on both fire paths (projection + the `advance_lx` pipe-
    /// end fallback); reset at line start, `m0_unflip`, and LCD transitions.
    /// Only read under `tier2_reclock` → production byte-identical.
    flip_dot: u16,
    /// The CPU-visible STAT mode→0 boundary back-dated to
    /// SameBoy's cycle-exact frame, **decoupled from the IRQ-dispatch flip**
    /// (`line_render_done`/`m0_src`). On the `leading_edge_reads` flag-on path
    /// this rises 3 dots *before* `line_render_done` on bare single-speed lines,
    /// so `vis_mode` reads 0 at SameBoy's `ModeTimeline::visible_mode0_dot`
    /// (our-line dot 251 = 254 − 3) while the dispatch stays at our dot 254 —
    /// the instrumented separator of the kernel pair (`m2int_m3stat_1` read at
    /// our dot 248 stays mode 3, `m0int_m3stat_2` at dot 252 reads mode 0).
    /// Gated to
    /// **bare single-speed** lines (`r.fetched == 0 && !win_active && !glitch &&
    /// !ds`), the regime the +3 back-date was measured on; the sprite/window
    /// (+2 DMG) and double-speed (+4) back-dates are derived-but-unmeasured and
    /// stay on `line_render_done` for now. **Always `false` on the flag-off
    /// (production) path** (the set is gated on `leading_edge_reads`), so
    /// `vis_mode` reads `line_render_done` exactly — byte-identical in
    /// production. The OAM/VRAM accessibility unblock (`blocking.rs`) keeps the
    /// `line_render_done` dot for now (the visible-vs-accessibility 3-dot window
    /// is a later back-dating). Reset at line start + on `m0_unflip`.
    vis_early: bool,
    /// The window vis-HOLD: the dot until which the
    /// CPU-visible STAT mode stays 3 on a `win_active` line, EVEN AFTER
    /// `line_render_done`/`vis_early`. The symmetric inverse of `vis_early`
    /// (which only ANTICIPATES the visible flip earlier): SameBoy extends a
    /// TRIGGERING window's mode-3 to ≈ `263 + SCX&7` (the measured window-length
    /// law), past the counter-pinned
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
    /// it is the visible-mode half of the parallel window-length model
    /// (which must also replicate the WY-latch trigger to drive it).
    /// **Always 0 on
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
    /// The **interrupt-facing** mode, decoupled from the
    /// CPU-visible `vis_mode` (SameBoy `mode_for_interrupt`, `gb.h:612`).
    /// On a visible line it diverges from the visible mode in two one-dot
    /// windows: the OAM (mode-2) IRQ mode goes to 2
    /// **one dot before** the visible byte does (lines 1-143, `display.c:1787`
    /// vs `:1792`), and the mode-0 IRQ mode goes to 0 **one dot after** the
    /// visible byte does (`display.c:2108` vs `:2091`). That 2-dot relative
    /// swing is what separates the `m2int`/`m0int` kernel pair. Consumed by the
    /// [`StatUpdate`](crate::stat_update) engine on the flag-on path
    /// (`stat_update_tick`); on the flag-off (production) path it is maintained
    /// every dot but read only by the decoupling test. Mirrors `vis_mode` on
    /// VBlank / glitch lines (anchor swing not modelled there yet).
    mode_for_interrupt: u8,
    /// One-dot-delayed mirror of `line_render_done`, the substrate for the
    /// mode-0 lag above: `line_render_done` rises on the visible 3→0 flip
    /// dot, so its previous-dot value is still false there and true one dot
    /// later — exactly the dot the IRQ-facing mode transitions to 0.
    mfi_m0_prev: bool,
    /// Flag-on path only: SameBoy's `GB_STAT_update`
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
    /// selecting the [`StatUpdate`](crate::stat_update) engine over
    /// `stat_events_tick`. Off in production until the atomic flip;
    /// forwarded by [`Interconnect::set_leading_edge_reads`].
    leading_edge_reads: bool,
    /// PPU-side copy of the interconnect's `tier2_reclock` flag (the −2
    /// dispatch reclock). Gates the mode-0 IRQ dispatch move
    /// (254→252) so the leading-edge-only specs (which set
    /// `leading_edge_reads` but NOT this) keep the validated baseline frame.
    /// Forwarded by [`Interconnect::set_tier2_reclock`].
    tier2_reclock: bool,
    /// PPU-side copy of the interconnect's `eager_value` flag (the eager clock
    /// plus tier2 read/render laws as cc+0 value peeks, dispatch staying cc+4).
    /// Implies `leading_edge_reads` but NOT `tier2_reclock`. Off in production.
    /// Forwarded by [`Interconnect::set_eager_value`].
    eager_value: bool,
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
    /// dot-loop index (the eighth-grid MID-vs-End comparison; routes only
    /// the OAM-read arm).
    /// `Some(lead_eighths)` when the flip lands this dot, carrying its sub-dot
    /// offset for [`event_phase`](crate::interconnect) (`Some(0)` =
    /// the net-zero dot-END commit), `None` otherwise.
    m0_access_flip: Option<i8>,
    /// The CGB palette-RAM unblock fired on the current dot
    /// (`render_finished` set true at the pipe end, one dot after the
    /// HDMA trigger `hdma_lead`). Like `m0_access_flip` but anchored at the
    /// palette/render-end edge: a CPU FF69/FF6B read samples at the cc+2
    /// MID phase, so it still reads $FF when the unblock lands in the
    /// M-cycle's second half. Drained via [`Self::take_pal_access_flip`]
    /// (sub-dot event-phase model; routes only the CGB palette read).
    /// `Some(lead_eighths)` when the flip lands this dot (the per-SCX
    /// palette-unblock sub-dot offset is carried here; `Some(0)` = net-zero
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
    /// through other dispatch chains, are the parked multi-chain
    /// problem.
    /// `Some(lead_eighths)` when the flip lands this dot (the flip's sub-dot
    /// offset is carried here; `Some(0)` = net-zero whole-M-cycle commit),
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
    /// Eager line-153 discriminated STAT-delivery retime (`ly_lyc_153_write`):
    /// the dot of the last FF45 write committed on line 153 (CGB), or
    /// `u16::MAX` for "no write this line" — reset at [`Self::start_line`]. It
    /// distinguishes a FRESH write landing near the dots-6-7 coincidence window
    /// (the enable side-effect zone / the late-disable early delivery) from a
    /// steady-state LYC=153 that must fire normally. Eager-only.
    l153_lyc_write_dot: u16,
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
    /// The dot the render pipe ended this line (pixel 160
    /// shipped, `render_finished` rise; 0 = not yet this line). The CGB
    /// palette-RAM read/write unblock trails it by 1 dot at single speed and
    /// is coincident in double speed on the deferred (cc+0) frame — the
    /// `cgbpal_m3end` constraint table.
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
    /// **Shadow WY-trigger (tier2 + CGB only; byte-identical OFF —
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
    /// The POST-SWITCH bare-exit law latches (tier2-only writers;
    /// byte-identical OFF). The speedchange m3stat 4-variable exit table
    /// collapses to per-class rp-frame exits
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
    /// keystone) of the most recent LCD-on leave; 2 when never left.
    stop_leave_k: u8,
    /// The most recent LCD enable happened in double speed (the lcdoff
    /// dances re-enable in DS; the −4 rp class term — the DS enable
    /// re-anchors the PPU frame where a run-through LCD keeps the SS
    /// boot phase).
    lcd_enable_in_ds: bool,
    /// The IMMEDIATE-WY twin of [`Self::wy_trig_sb`]. SameBoy's
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
    /// The BOUNDARY-WY cross-line trigger: a WY write
    /// committing in a line's tail (dot >= 452) or head (dot < 4) whose
    /// value matches the CURRENT (old) line latches SameBoy's
    /// `wy_triggered` (its scheduled `wy_check` still compares the old
    /// `current_line`), while slopgb's render (`wy_latch`) and the
    /// wy2-lagged shadow both miss it — every later line renders bare
    /// where SameBoy draws the window. Frame-sticky like `wy_triggered`;
    /// reset at the frame top. Tier2 + CGB only (byte-identical OFF).
    wy_xline_trig: bool,
    /// The last CPU VRAM write ATTEMPT's (line, dot), for the
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
    /// A mid-mode-3 LCDC write's deferred render-view commit
    /// `(value, dots_left)`: the tier2 render reclock lags the BG fetcher's
    /// `eff.render_lcdc` behind the eager control commit by the render frame
    /// (`RENDER_LCDC_DELAY` dots). `None` outside a pending defer; production
    /// never schedules one (byte-identical).
    render_lcdc_pending: Option<(u8, u8)>,

    render: Render,

    front: Box<[u32; SCREEN_PIXELS]>,
    back: Box<[u32; SCREEN_PIXELS]>,
    dmg_palette: [u32; 4],

    /// Super Game Boy presentation state (palettes / attribute map / window
    /// mask), `Some` only on `Model::Sgb`/`Sgb2`. Drives the DMG-output
    /// colorization in [`Self::dmg_shade`] and the MASK_EN frame handling in
    /// [`Self::start_line`]. `None` on every other model, so their output is
    /// byte-identical to the pre-SGB core (see `docs/hardware-state/sgb.md`).
    sgb: Option<SgbView>,
}

/// The BG-fetcher LCDC render-view defer, in PPU dots: the eager
/// control commit lands at the write's leading edge (cc+0), the render fetch
/// grid is calibrated +4 late, so the addressing view re-commits this many
/// `Ppu::tick`s later.
const RENDER_LCDC_DELAY: u8 = 3;

fn pixel_buffer(fill: u32) -> Box<[u32; SCREEN_PIXELS]> {
    vec![fill; SCREEN_PIXELS]
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
