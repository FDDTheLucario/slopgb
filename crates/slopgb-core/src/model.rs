//! Hardware model selection and per-model post-boot state.

/// Which physical Game Boy we are emulating.
///
/// Models differ in initial (post-boot-ROM) CPU/hardware-register state, in
/// a handful of timing details, and (CGB/AGB) in the whole color subsystem.
/// Mooneye test ROM filename suffixes map onto these:
/// `-dmg0`→[`Model::Dmg0`], `-dmgABC`/`-dmgABCmgb`→[`Model::Dmg`],
/// `-mgb`→[`Model::Mgb`], `-S`/`-sgb`→[`Model::Sgb`], `-sgb2`→[`Model::Sgb2`],
/// `-GS`→DMG+SGB, `-C`/`-cgb`/`-cgbABCDE`→[`Model::Cgb`], `-A`→[`Model::Agb`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Model {
    /// Original DMG, early revision 0 boot ROM.
    Dmg0,
    /// DMG revisions A/B/C — "the" Game Boy.
    Dmg,
    /// Game Boy Pocket.
    Mgb,
    /// Super Game Boy.
    Sgb,
    /// Super Game Boy 2.
    Sgb2,
    /// Game Boy Color. Canonical revision: CPU CGB C (CGB-CPU-04) — the
    /// revision the bulk of the reference corpus (gambatte `cgb04c`,
    /// mealybug `_cgb_c`) was captured on; see docs/ARCHITECTURE.md
    /// §CGB revision policy. Revision-agnostic suffixes (`-cgbABCDE`)
    /// also map here.
    Cgb,
    /// Game Boy Advance running in CGB mode.
    Agb,
}

impl Model {
    /// True for models with the color PPU and CGB-only hardware
    /// (VRAM/WRAM banking, palettes, HDMA, double speed).
    pub fn is_cgb(self) -> bool {
        matches!(self, Model::Cgb | Model::Agb)
    }

    /// True for the Super Game Boy models — the ones whose PPU carries the SGB
    /// command/colorization view and that accept a SNES-side coprocessor.
    pub fn is_sgb(self) -> bool {
        matches!(self, Model::Sgb | Model::Sgb2)
    }
}

/// Exact machine state at the moment the boot ROM hands control to the
/// cartridge (PC=0x100). One table entry per [`Model`].
///
/// `div_counter` is the internal 16-bit DIV counter value, which encodes how
/// long the boot ROM ran — several mooneye `boot_div*` tests measure it.
#[derive(Debug, Clone, Copy)]
pub struct PostBootState {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub div_counter: u16,
    /// Initial values for FF00..=FF7F and FFFF where they differ from the
    /// peripheral reset defaults: (address, value) pairs applied in order.
    pub hwio: &'static [(u16, u8)],
    /// PPU position at hand-off, in dots from the start of a steady frame
    /// (line = dots / 456, dot-in-line = dots % 456). The boot ROM enabled
    /// the LCD long before PC=0x100, so the machine starts mid-frame; the
    /// interconnect warms the PPU up to exactly this position.
    pub lcd_phase_dots: u32,
}

/// Wave RAM as left by a DMG/MGB/SGB boot (hardware leaves it random; this
/// is one frequently observed DMG power-up pattern, used for determinism —
/// mooneye `boot_hwio-*` masks wave RAM out).
const WAVE_RAM_DMG: [u8; 16] = [
    0x84, 0x40, 0x43, 0xAA, 0x2D, 0x78, 0x92, 0x3C, 0x60, 0x59, 0x59, 0xB0, 0x34, 0xB8, 0x2E, 0xDA,
];

/// CGB boot ROM initialises wave RAM to a 00/FF pattern (Pan Docs,
/// "Power-up sequence").
const WAVE_RAM_CGB: [u8; 16] = [
    0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF,
];

/// Builds the shared part of every hwio table. The APU writes replay the
/// boot ROM's audio setup in its write order (NR52 power first), so the
/// APU's internal state (channel 1 beep on the models that beep, frame
/// sequencer reset) comes out right rather than being forced.
macro_rules! hwio_table {
    (p1: $p1:expr, beep: $b13:expr, $b14:expr, wave: $w:expr, obp: $obp:expr, dma: $dma:expr) => {
        &[
            (0xFF00, $p1),
            (0xFF01, 0x00),
            (0xFF02, 0x00),
            (0xFF05, 0x00),
            (0xFF06, 0x00),
            (0xFF07, 0x00),
            // IF: the boot ROM ran with the LCD on and no handlers, so a
            // vblank request is pending (boot_hwio-* expect $E1).
            (0xFF0F, 0xE1),
            (0xFF26, 0x80),
            (0xFF11, 0x80),
            (0xFF12, 0xF3),
            (0xFF25, 0xF3),
            (0xFF24, 0x77),
            (0xFF13, $b13),
            (0xFF14, $b14),
            (0xFF30, $w[0]),
            (0xFF31, $w[1]),
            (0xFF32, $w[2]),
            (0xFF33, $w[3]),
            (0xFF34, $w[4]),
            (0xFF35, $w[5]),
            (0xFF36, $w[6]),
            (0xFF37, $w[7]),
            (0xFF38, $w[8]),
            (0xFF39, $w[9]),
            (0xFF3A, $w[10]),
            (0xFF3B, $w[11]),
            (0xFF3C, $w[12]),
            (0xFF3D, $w[13]),
            (0xFF3E, $w[14]),
            (0xFF3F, $w[15]),
            (0xFF42, 0x00),
            (0xFF43, 0x00),
            (0xFF45, 0x00),
            (0xFF47, 0xFC),
            (0xFF48, $obp),
            (0xFF49, $obp),
            (0xFF4A, 0x00),
            (0xFF4B, 0x00),
            // FF46 is the OAM DMA register *file value*, not a write (a
            // write would start a transfer); the interconnect installs it
            // directly.
            (0xFF46, $dma),
        ]
    };
}

/// DMG-family boot: P1 both columns selected ($CF), channel-1 beep left
/// playing (NR52 reads $F1), DMA register $FF (boot_hwio-dmgABCmgb).
const HWIO_DMG: &[(u16, u8)] =
    hwio_table!(p1: 0x00, beep: 0x83, 0x87, wave: WAVE_RAM_DMG, obp: 0xFF, dma: 0xFF);

/// SGB boot: P1 deselected ($FF), frequency written to NR13/NR14 but never
/// triggered (NR52 reads $F0 — boot_hwio-S).
const HWIO_SGB: &[(u16, u8)] =
    hwio_table!(p1: 0x30, beep: 0xC1, 0x07, wave: WAVE_RAM_DMG, obp: 0xFF, dma: 0xFF);

/// CGB/AGB boot with a DMG cart: P1 deselected ($FF), beep on (NR52 $F1),
/// OBP0/OBP1 = $00, DMA register $00, 00/FF wave pattern (misc/boot_hwio-C).
const HWIO_CGB: &[(u16, u8)] =
    hwio_table!(p1: 0x30, beep: 0x83, 0x87, wave: WAVE_RAM_CGB, obp: 0x00, dma: 0x00);

impl Model {
    /// Post-boot state table for this model.
    ///
    /// CPU registers come from the mooneye `boot_regs-*` assertions.
    ///
    /// `div_counter` is pinned by the `boot_div*` sources: with the
    /// tick-then-access bus contract the first DIV read of e.g.
    /// `boot_div-dmgABCmgb` lands on M-cycle 14 after hand-off and must
    /// observe $AC "immediately after an increment", which together with the
    /// later phase-shifted reads confines the counter at hand-off to a
    /// 4-T-cycle window; the value ≡ 0 (mod 4) is chosen because the CPU
    /// M-cycle grid and the DIV counter share the same crystal from reset.
    /// For SGB/SGB2 this is a *base* value: the real SGB boot duration
    /// depends on the cartridge header bits sent to the SNES
    /// (`boot_div-S` vs `boot_div2-S`); the interconnect adds 4 T-cycles per
    /// zero bit of the transferred packets on top of this base.
    ///
    /// `lcd_phase_dots` is pinned (DMG ABC/MGB) by the gbmicrotest
    /// poweron_stat/ly/oam/vram comment tables (captured on a DMG-CPU-08):
    /// the boot ROM hands off exactly 60 dots before the start of line 0,
    /// i.e. 70224 - 60 = 70164 — inside the coarser window the LY/STAT
    /// bytes of mooneye `boot_hwio-*` allow (DMG ABC reads STAT=$80 at dot
    /// 4556 and LY=$0A at dot 4760 after hand-off → [70028,70224)∪[0,8)).
    /// DMG0 reads STAT=$83/LY=$01 (→ window [66208,66376), midpoint 66292;
    /// no finer oracle exists for that revision).
    /// SGB masks the LCD registers out and reuses the DMG value. CGB/AGB
    /// hands off inside vblank (LY in the $90-$94 range per the hardware
    /// reports in gbdev/pandocs#426). The table stores the **DMG-cart**
    /// hand-off (the boot ROM's DMG-compat path runs 0x7D8 T-cycles
    /// longer than the CGB-cart path — see
    /// `Interconnect::apply_post_boot_state`, which subtracts that cut
    /// from both `div_counter` and `lcd_phase_dots` in CGB mode):
    /// `div_counter` is pinned by mooneye misc/boot_div-cgbABCDE/-A
    /// (mooneye ROMs are DMG carts), and the CGB-cart LCD dot by
    /// gambatte's hardware-calibrated init state (initstate.cpp
    /// `videoCycles = cgb ? 144*456 + 164 + agb*4 : 153*456 + 396` — the
    /// DMG value equals the gbmicrotest-pinned 70164 exactly, anchoring
    /// the unit conversion), which the gambatte display_startstate
    /// cgb04c rows measure ($143=$C0 carts): line 144 ($90) dot 164, AGB
    /// 4 dots later. DMG-cart hand-off is therefore 144*456+164+0x7D8 =
    /// line 148 ($94) dot 348 — inside the pandocs#426 window.
    pub fn post_boot_state(self) -> PostBootState {
        // Common fields; per-model values below.
        let base = PostBootState {
            a: 0,
            f: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            sp: 0xFFFE,
            pc: 0x0100,
            div_counter: 0,
            hwio: HWIO_DMG,
            lcd_phase_dots: 70164,
        };
        match self {
            // boot_regs-dmg0, boot_div-dmg0, boot_hwio-dmg0.
            Model::Dmg0 => PostBootState {
                a: 0x01,
                f: 0x00,
                b: 0xFF,
                c: 0x13,
                d: 0x00,
                e: 0xC1,
                h: 0x84,
                l: 0x03,
                div_counter: 0x182C,
                lcd_phase_dots: 66292,
                ..base
            },
            // boot_regs-dmgABC, boot_div-dmgABCmgb, boot_hwio-dmgABCmgb.
            Model::Dmg => PostBootState {
                a: 0x01,
                f: 0xB0,
                b: 0x00,
                c: 0x13,
                d: 0x00,
                e: 0xD8,
                h: 0x01,
                l: 0x4D,
                div_counter: 0xABC8,
                ..base
            },
            // boot_regs-mgb; same boot ROM timing as DMG ABC
            // (boot_div-dmgABCmgb and boot_hwio-dmgABCmgb pass on MGB).
            Model::Mgb => PostBootState {
                a: 0xFF,
                f: 0xB0,
                b: 0x00,
                c: 0x13,
                d: 0x00,
                e: 0xD8,
                h: 0x01,
                l: 0x4D,
                div_counter: 0xABC8,
                ..base
            },
            // boot_regs-sgb, boot_div-S/boot_div2-S (header-dependent
            // base — see the method docs), boot_hwio-S.
            Model::Sgb => PostBootState {
                a: 0x01,
                f: 0x00,
                b: 0x00,
                c: 0x14,
                d: 0x00,
                e: 0x00,
                h: 0xC0,
                l: 0x60,
                div_counter: 0xD170,
                hwio: HWIO_SGB,
                ..base
            },
            // boot_regs-sgb2; same boot timing as SGB.
            Model::Sgb2 => PostBootState {
                a: 0xFF,
                f: 0x00,
                b: 0x00,
                c: 0x14,
                d: 0x00,
                e: 0x00,
                h: 0xC0,
                l: 0x60,
                div_counter: 0xD170,
                hwio: HWIO_SGB,
                ..base
            },
            // misc/boot_regs-cgb, misc/boot_div-cgbABCDE, misc/boot_hwio-C.
            // These are the DMG-cart-on-CGB values (every mooneye ROM is a
            // DMG-mode cart); for CGB-flagged carts `GameBoy::new`
            // overrides DE=$FF56 HL=$000D after the post-boot warmup (Pan
            // Docs "CPU registers" — the cart kind is not known here).
            Model::Cgb => PostBootState {
                a: 0x11,
                f: 0x80,
                b: 0x00,
                c: 0x00,
                d: 0x00,
                e: 0x08,
                h: 0x00,
                l: 0x7C,
                div_counter: 0x2674,
                hwio: HWIO_CGB,
                lcd_phase_dots: 67836,
                ..base
            },
            // misc/boot_regs-A, misc/boot_div-A.
            Model::Agb => PostBootState {
                a: 0x11,
                f: 0x00,
                b: 0x01,
                c: 0x00,
                d: 0x00,
                e: 0x08,
                h: 0x00,
                l: 0x7C,
                div_counter: 0x2678,
                hwio: HWIO_CGB,
                lcd_phase_dots: 67840,
                ..base
            },
        }
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
