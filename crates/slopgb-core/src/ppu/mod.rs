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
//! | 4            | STAT mode reads 2; OAM writes blocked; LYC compare valid (line 0's OAM pulse sits here, with its own dispatch-late/m1-blocked rules — see `refresh_stat`) |
//! | 80           | VRAM reads blocked; OAM scan complete |
//! | 84           | STAT mode reads 3; VRAM writes blocked |
//! | V0           | mode 0: STAT reads 0, mode-0 IRQ source asserts, OAM+VRAM unblock, OAM blocking level drops. V0 = 256 + SCX%8 + sprite/window penalties (`hblank_ly_scx_timing-GS`; `intr_2_mode0_timing`/`intr_2_oam_ok_timing`: 252 dots after the mode-2 IRQ becomes CPU-visible) |
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
//! # Known approximation: instantaneous OAM scan
//!
//! Sprite selection for a line happens in one step at dot 80 rather than
//! spread across dots 0-80 as on hardware (one OAM entry per 2 dots).
//! Combined with [`Ppu::oam_dma_write`] bypassing mode-based access
//! blocking, an OAM DMA byte landing mid-mode-2 can select sprites
//! differently than hardware, which would already have scanned past the
//! entry the byte lands in. No mooneye test pins this. The DMG OAM
//! corruption bug ([`Ppu::oam_bug`]) interacts with the same
//! approximation: corruption mutates OAM *before* the instantaneous
//! dot-80 scan, so rows the hardware scan had already consumed are
//! re-read post-corruption. Fine for blargg's oam_bug suite (it checks
//! the memory effect with the LCD subsequently disabled); the rendered
//! frame of the corrupted line itself is unpinned by any test ROM.

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
    /// (madness/mgb_oam_dma_halt_sprites.s — see `oam_scan` in render.rs).
    dma_freeze: Option<(u8, u8)>,

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
    /// `refresh_stat`). Drained by the interconnect via
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
    /// The rise currently being emitted by `refresh_stat` is a second-half
    /// commit (see `stat_halt_late`).
    stat_halt_late_pending: bool,
    /// The externally visible mode-0 flip (STAT mode bits, OAM/VRAM
    /// unblock): mode 3 finished on the current line.
    line_render_done: bool,
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
            enabled: false,
            line: 0,
            dot: 0,
            glitch_line: false,
            frame_skip: false,
            cmp: false,
            stat_line: false,
            pending_if: 0,
            stat_late: false,
            stat_halt_late: false,
            stat_halt_late_pending: false,
            line_render_done: true,
            render_finished: true,
            hdma_lead: false,
            wy_latch: false,
            wy2: 0,
            wy2_delay: 0,
            staged_ds: false,
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
        if self.wy2_delay > 0 {
            self.wy2_delay -= 1;
            if self.wy2_delay == 0 {
                self.wy2 = self.wy;
            }
        }
        if !self.enabled {
            return std::mem::take(&mut self.pending_if);
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
        self.refresh_stat(true);
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
        // Frame-sticky WY condition (gambatte weMaster): sampled at
        // discrete dots, not compared continuously — see `wy_latch`.
        // gambatte's line-cycle anchors translate to our dot convention
        // with a +1 shift on DMG (m3StartLineCycle is 83+cgb against our
        // model-independent mode-3 start at dot 84).
        let win_en = self.eff.lcdc & LCDC_WIN_ENABLE != 0;
        let late = u16::from(!self.model.is_cgb());
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
                    80 => self.oam_scan(),
                    84 => self.render_init(),
                    d => {
                        if self.render.active && d > 84 {
                            self.render_step();
                        }
                    }
                }
            }
        }
        if self.line == 153 && self.dot == 4 {
            // Line 153 quirk: LY reads 0 from dot 4 (TCAGBD §8.9).
            self.ly = 0;
        }
        if self.line == 144 && self.dot == 4 {
            // VBlank interrupt: 4 dots after LY becomes 144, together with
            // the visible mode 1 (TCAGBD; `vblank_stat_intr-GS`).
            self.pending_if |= IF_VBLANK;
        }
    }

    /// LY value the LYC comparator sees, or None while the delayed-LY value
    /// is invalid (comparison flag forced to 0). See module docs.
    fn compare_ly(&self) -> Option<u8> {
        if self.glitch_line {
            // LCD enable: the comparison runs immediately with LY=0
            // (`stat_lyc_onoff` rounds 1-4).
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
            0
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
    /// for the current M-cycle (see `refresh_stat`).
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

    /// Level of the shared STAT interrupt line for the given enable bits.
    fn stat_line_level(&self, en: u8) -> bool {
        let mut high = en & STAT_SRC_LYC != 0 && self.cmp;
        if !self.enabled {
            // With the LCD off only the (frozen) LYC source persists
            // (`stat_lyc_onoff` round 2: no edge across off/on with cmp=1).
            return high;
        }
        let vm = self.vis_mode();
        // HBlank source: follows the visible mode-0 window (including line
        // starts and 144:0-3) so consecutive sources overlap and block each
        // other (`stat_irq_blocking`). The glitched post-enable prefix is
        // not a real hblank.
        high |= en & STAT_SRC_HBLANK != 0
            && vm == 0
            && !(self.glitch_line && self.dot < GLITCH_MODE3_START);
        high |=
            en & STAT_SRC_VBLANK != 0 && (self.line >= 145 || (self.line == 144 && self.dot >= 4));
        if en & STAT_SRC_OAM != 0 {
            // The OAM *blocking level* spans the whole scan+render of every
            // visible line, dots 0..flip (gambatte mstat_irq.h doM0Event:
            // the m2 enable blocks the m0 IRQ — m2int_m0irq_*_out0; the
            // level also blocks the LYC dot-4 edge, lycm2int). The IRQ
            // itself is an *event* at the line-start dots — see
            // `refresh_stat` (SameBoy display.c mode_for_interrupt pulse).
            // Line 0's level starts at dot 4 with the LY/LYC validity.
            let oam_window = self.line <= 143
                && !self.glitch_line
                && !self.line_render_done
                && (self.line != 0 || self.dot >= 4);
            let cgb = self.model.is_cgb();
            // OAM pulse at vblank entry: one M-cycle before the vblank IF
            // on *both* families (wilbertpol intr_2_timing rounds 5-7 pin
            // MGB and CGB alike; gbmicrotest line_144_oam_int_b/c/d pin
            // DMG — `vblank_stat_intr-GS` sees it together with the
            // vblank IF through the DMG halt-late commit, see
            // `refresh_stat`).
            let pulse144 = self.line == 144 && self.dot == 0;
            // DMG: the OAM source also pulses on every later vblank line
            // (`intr_1_2_timing-GS`: mode1→mode2 IRQ distance is 464 dots —
            // one line + 8 dots).
            let vblank_pulse = !cgb && (145..=153).contains(&self.line) && self.dot == 12;
            high |= oam_window || pulse144 || vblank_pulse;
        }
        high
    }

    /// Recompute the comparison flag and STAT line; emit IF bit 1 on a
    /// rising edge of the shared line. `from_tick` distinguishes the
    /// dot-clock path from register-write paths: the line-0 OAM-rise
    /// special cases below are properties of the PPU's own mode-2 event,
    /// not of CPU writes.
    fn refresh_stat(&mut self, from_tick: bool) {
        if self.enabled {
            self.cmp = self.compare_ly() == Some(self.lyc);
        }
        let level = self.stat_line_level(self.stat_en);
        if level && !self.stat_line {
            // The OAM source's IRQ is an *event* at the line-start dots
            // (SameBoy display.c raises the OAM STAT interrupt via a
            // one-shot mode_for_interrupt; gambatte mstat_irq.h doM2Event),
            // but the source still behaves as a level edge for rises
            // landing inside the readable mode-2 window — gambatte's
            // m2enable/late_enable_* rows fire the IRQ for an enable
            // written mid-scan. Only a rise whose sole contributor is the
            // *mode-3 extension* of the OAM blocking level (dots 84..end,
            // which exists to block mode-0/LYC edges — m2int_m0irq,
            // lycm2int) emits nothing: there is no mode-2 condition left
            // to fire (gbmicrotest oam_int_if_level_d).
            let sans_oam = self.stat_line_level(self.stat_en & !STAT_SRC_OAM);
            let cgb = self.model.is_cgb();
            let oam_event_dot = from_tick
                && (((1..=143).contains(&self.line) && !self.glitch_line && self.dot == 0)
                    || (self.line == 0 && !self.glitch_line && self.dot == 4)
                    || (self.line == 144 && self.dot == 0)
                    || (!cgb && (145..=153).contains(&self.line) && self.dot == 12));
            let in_scan = self.line <= 143 && !self.glitch_line && self.dot < 84;
            if !sans_oam && !oam_event_dot && !in_scan {
                self.stat_line = level;
                self.stat_halt_late_pending = false;
                return;
            }
            // The dot-0 pulses (lines 1-143 and the 144 entry) sit in the
            // second half of their M-cycle: the halt-exit sampler *and*
            // the running CPU's same-cycle interrupt sample both miss
            // them for one M-cycle while the IF bit reads back at once —
            // the same silicon as the line-0 dot-4 rise below and the
            // timer's `if_late` (SameBoy raises the pulse 1 T-cycle
            // before the STAT change; the mealybug m3_* photographs pin
            // the dispatch anchor — their per-line handlers land their
            // mid-line writes one M-cycle after the commit cycle).
            // Exception: the CGB 144-entry pulse is visible to both
            // samplers in its own cycle (misc/ppu/vblank_stat_intr-C
            // measures it one cycle apart from the DMG family).
            let dot0_pulse = from_tick && self.dot == 0 && !sans_oam && !(cgb && self.line == 144);
            if dot0_pulse {
                self.stat_halt_late_pending = true;
            }
            // Line 0's OAM rise (dot 4) has event semantics pinned by
            // gambatte's hardware suite and the mealybug photographs:
            //
            // * with the mode-1 (vblank) source enable bit also set the
            //   IRQ is blocked entirely (gambatte mstat_irq.h doM2Event:
            //   `blockedByM1Irq = ly == 0 && (statReg_ &
            //   lcdstat_m1irqen)`; lcdirq_precedence/m2irq_ly00_lcdstat30
            //   expects no IRQ) — the line level still rises, so nothing
            //   re-edges later;
            // * otherwise the IF bit is readable immediately (gambatte
            //   lyc153int_m2irq reads it in the same M-cycle) but misses
            //   the CPU's interrupt sample for one extra M-cycle: on
            //   every other line the rise comes a T-cycle before the
            //   visible mode-2 flip (SameBoy display.c: "The OAM STAT
            //   interrupt occurs 1 T-cycle before STAT actually changes,
            //   except on line 0"), so only line 0's sits in the second
            //   half of the M-cycle for the dispatch sample. mealybug's
            //   handlers compensate ("line 0 timing is different by 4
            //   cycles", m3_bgp_change.asm) and their references pin the
            //   late dispatch.
            let line0_oam_rise = from_tick
                && self.line == 0
                && !self.glitch_line
                && self.dot == 4
                && self.stat_en & STAT_SRC_OAM != 0;
            if !line0_oam_rise {
                self.pending_if |= IF_STAT;
                if std::mem::take(&mut self.stat_halt_late_pending) {
                    self.stat_halt_late = true;
                }
                if dot0_pulse {
                    // Second-half commit: also dispatch-late (see above).
                    self.stat_late = true;
                }
            } else if self.stat_en & STAT_SRC_VBLANK == 0 {
                self.pending_if |= IF_STAT;
                self.stat_late = true;
            }
        }
        self.stat_halt_late_pending = false;
        self.stat_line = level;
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
        // Writes pass during dots 0-3 and 80-83 (`lcdon_write_timing-GS`).
        (4..80).contains(&self.dot) || (self.dot >= 84 && !self.line_render_done)
    }

    fn vram_read_blocked(&self) -> bool {
        if !self.enabled || self.line > 143 || self.line_render_done {
            return false;
        }
        if self.glitch_line {
            self.dot >= GLITCH_MODE3_START
        } else {
            self.dot >= 80
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
                // DMG STAT write bug: the write behaves as if 0xFF were
                // written first for one cycle, enabling every source
                // momentarily (Pan Docs "STAT bug"; CGB is unaffected).
                // The glitch raises IF from the mode-0/mode-1 levels and
                // the LYC match only — never from the m2 condition, which
                // is an event, not a level (gbmicrotest
                // stat_write_glitch_l0/l1/l154 comment tables: E2 in the
                // hblank/vblank/LYC positions, E0 in the mode-2 ones).
                // The *line level* still includes the OAM contribution.
                if !self.model.is_cgb() && self.enabled {
                    let level = self.stat_line_level(STAT_SRC_ALL);
                    let level_irq = self.stat_line_level(STAT_SRC_ALL & !STAT_SRC_OAM);
                    if level_irq && !self.stat_line {
                        self.pending_if |= IF_STAT;
                    }
                    self.stat_line = level;
                }
                self.stat_en = value & STAT_SRC_ALL;
                self.refresh_stat(false);
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
                self.lyc = value;
                // The comparison retriggers immediately on LYC writes while
                // the comparison clock runs (`stat_lyc_onoff`).
                self.refresh_stat(false);
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
            self.stat_halt_late_pending = false;
            self.hdma_lead = false;
            self.render.active = false;
            self.render.win_active = false;
            self.win_start_pending = false;
            let white = self.white();
            self.front.fill(white);
            self.refresh_stat(false);
        } else if !was_on && now_on {
            // LCD on: glitched first line (`lcdon_timing-GS`); the LYC
            // comparison restarts against LY=0 immediately and can raise
            // the STAT line in this very cycle (`stat_lyc_onoff` round 4).
            self.enabled = true;
            self.line = 0;
            self.dot = 0;
            self.ly = 0;
            self.glitch_line = true;
            // Hardware keeps the panel blank for the whole first frame
            // after enabling (see `frame_skip`).
            self.frame_skip = true;
            self.line_render_done = false;
            self.render_finished = false;
            self.stat_halt_late_pending = false;
            self.hdma_lead = false;
            self.render.active = false;
            self.wy_latch = false;
            self.win_line = 0xFF;
            self.win_start_pending = false;
            self.refresh_stat(false);
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
    /// `oam_scan` in render.rs).
    pub fn set_oam_dma_freeze(&mut self, freeze: Option<(u8, u8)>) {
        self.dma_freeze = freeze;
    }

    /// Test hook for the interconnect wiring tests.
    #[cfg(test)]
    pub(crate) fn oam_dma_freeze(&self) -> Option<(u8, u8)> {
        self.dma_freeze
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
    // `refresh_stat` comment for the sources): the IF bit is readable
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
            // Normal line: the pulse commits at dot 0 — a second-half
            // commit, so it misses the dispatch sample of its own cycle
            // too (the mealybug m3_* photo handlers pin the anchor).
            run_to(&mut p, 0, 451);
            p.take_stat_late();
            assert_eq!(p.tick() & IF_STAT, IF_STAT, "{model:?} line 1");
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
            assert_eq!(p.tick() & 2, 2, "{model:?}: pulse at (1,0)");
            assert!(
                p.take_stat_halt_late(),
                "{model:?}: dot-0 pulse is halt-late"
            );
        }
    }

    #[test]
    fn mode0_irq_at_256_plus_scx_fine() {
        for scx in [0u8, 1, 4, 5, 7, 8, 13] {
            let mut p = dmg();
            p.write(0xFF41, 0x08);
            p.write(0xFF43, scx);
            p.write(0xFF40, 0x81);
            run_to(&mut p, 1, 4); // line start: hblank source dropped
            let v0 = 256 + u16::from(scx & 7);
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
    fn oam_level_blocks_mode0_edges_entirely() {
        // With both the OAM and hblank sources enabled the line never
        // falls on visible lines: the OAM blocking level spans dots
        // 0..mode-3 end and hands over to the mode-0 level without a gap,
        // which hands over to the next line's dots 0-3 and OAM level
        // (gambatte m2int_m0irq_*_out0: the m2 enable blocks the m0 IRQ;
        // `stat_irq_blocking` semantics).
        let mut p = dmg();
        p.write(0xFF41, 0x28); // hblank + OAM sources
        p.write(0xFF40, 0x81);
        // One edge when the glitch line's hblank rises at 250...
        let ifs = run_to(&mut p, 0, 250);
        assert_eq!(ifs & 2, 2, "glitch-line hblank edge");
        // ...and none after: the line stays high for the whole frame.
        let ifs = run_to(&mut p, 143, 455);
        assert_eq!(ifs & 2, 0, "no further edge on any visible line");
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
        assert_eq!(p.tick() & 2, 2, "CGB OAM pulse at 144:0");
        assert!(!p.take_stat_halt_late(), "CGB 144:0 pulse is not halt-late");
        assert!(
            !p.take_stat_late(),
            "CGB 144:0 pulse dispatches in its own cycle"
        );
        tick_n(&mut p, 3);
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
}
