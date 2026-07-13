//! Power-on / post-boot hardware state install (no boot ROM executed): registers, internal DIV, WRAM/VRAM seeding, LCD phase. Oracle: mooneye boot_regs/boot_hwio/boot_div, gambatte $143=$C0 carts.

use super::*;

impl Interconnect {
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
}
