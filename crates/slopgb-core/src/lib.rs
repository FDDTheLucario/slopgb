//! slopgb-core: cycle-accurate Game Boy (DMG) and Game Boy Color (CGB) emulator core.
//!
//! Zero dependencies, `forbid(unsafe_code)`, fully deterministic. The frontend
//! drives emulation via [`GameBoy::run_frame`] (or [`GameBoy::step`] for
//! instruction granularity), reads pixels from [`GameBoy::frame`], and drains
//! audio samples from [`GameBoy::drain_audio`].
//!
//! # Timing model
//!
//! The CPU is the clock master. Every CPU M-cycle advances the rest of the
//! machine (PPU, timer, DMA, APU, serial) by one M-cycle *before* the memory
//! access of that cycle is performed. See `docs/ARCHITECTURE.md` for the full
//! contract.

pub(crate) mod apu;
pub(crate) mod cartridge;
pub(crate) mod cpu;
pub mod debug;
pub(crate) mod interconnect;
pub(crate) mod joypad;
pub(crate) mod model;
pub(crate) mod ppu;
pub(crate) mod serial;
pub(crate) mod timer;

pub use apu::DEFAULT_SAMPLE_RATE;
pub use cartridge::CartridgeError;
pub use cpu::Registers;
pub use joypad::Button;
pub use model::Model;

// Escape hatch for the crate's integration tests, which drive the CPU and
// interconnect directly (OAM DMA freeze/timing tests). Not public API.
#[doc(hidden)]
pub use cartridge::Cartridge;
#[doc(hidden)]
pub use cpu::{Bus, Cpu};
#[doc(hidden)]
pub use interconnect::Interconnect;

/// Screen width in pixels.
pub const SCREEN_W: usize = 160;
/// Screen height in pixels.
pub const SCREEN_H: usize = 144;
/// Pixels per frame.
pub const SCREEN_PIXELS: usize = SCREEN_W * SCREEN_H;
/// T-cycles (dots) per frame with the LCD on.
pub const CYCLES_PER_FRAME: u32 = 70224;
/// Master clock in Hz (T-cycles / dots per second, normal speed).
pub const CLOCK_HZ: u32 = 4_194_304;

/// A complete emulated Game Boy.
pub struct GameBoy {
    cpu: cpu::Cpu,
    bus: interconnect::Interconnect,
}

impl GameBoy {
    /// Build a machine for `model` with the given cartridge ROM image.
    ///
    /// No boot ROM is executed: CPU registers, hardware registers and timers
    /// are initialised to the exact post-boot state of `model`.
    pub fn new(model: Model, rom: Vec<u8>) -> Result<Self, CartridgeError> {
        let cart = cartridge::Cartridge::from_bytes(rom)?;
        let mut bus = interconnect::Interconnect::new(model, cart);
        let mut cpu = cpu::Cpu::new(model);
        bus.apply_post_boot_state();
        if bus.cgb_mode() {
            // CGB-flagged cart: the CGB/AGB boot ROM hands off DE=$FF56
            // HL=$000D instead of the DMG-cart values in the per-model
            // table (Pan Docs "CPU registers", Power-Up Sequence). A/F/B/C
            // are cart-independent. Pure register-file override with no
            // timing side effects; it only needs to land before the first
            // `step`.
            cpu.regs_mut().set_de(0xFF56);
            cpu.regs_mut().set_hl(0x000D);
        }
        Ok(Self { cpu, bus })
    }

    /// Pick the best model for a ROM from its CGB-support header flag
    /// (CGB if the ROM supports or requires it, otherwise DMG). Uses the
    /// same bit-7 predicate as the interconnect's CGB-mode gate
    /// (`cartridge::cgb_flag`), matching what the CGB boot ROM checks.
    pub fn auto_model(rom: &[u8]) -> Model {
        match rom.get(0x143) {
            Some(&flag) if cartridge::cgb_flag(flag) => Model::Cgb,
            _ => Model::Dmg,
        }
    }

    /// Execute one CPU instruction (or one halted/stopped M-cycle).
    pub fn step(&mut self) {
        self.cpu.step(&mut self.bus);
    }

    /// Run until the next frame is complete (vblank reached), or — with the
    /// LCD off — until an equivalent number of cycles has elapsed.
    pub fn run_frame(&mut self) {
        let target = self.bus.frame_count().wrapping_add(1);
        let deadline = self.bus.cycles().wrapping_add(u64::from(CYCLES_PER_FRAME));
        while self.bus.frame_count() != target && self.bus.cycles() < deadline {
            self.step();
        }
    }

    /// XRGB8888 pixels of the most recently completed frame, row-major.
    pub fn frame(&self) -> &[u32; SCREEN_PIXELS] {
        self.bus.ppu().frame()
    }

    /// Count of completed frames since power-on.
    pub fn frame_count(&self) -> u64 {
        self.bus.frame_count()
    }

    /// Total elapsed T-cycles since power-on.
    pub fn cycles(&self) -> u64 {
        self.bus.cycles()
    }

    /// Press a joypad button (held until [`Self::release`]).
    pub fn press(&mut self, b: Button) {
        self.bus.joypad_mut().press(b);
    }

    /// Release a previously pressed joypad button.
    pub fn release(&mut self, b: Button) {
        self.bus.joypad_mut().release(b);
    }

    /// Move all pending stereo samples (interleaved L/R, `CLOCK_HZ / 64`-ish
    /// native rate decided by the APU) into `out`.
    pub fn drain_audio(&mut self, out: &mut Vec<(f32, f32)>) {
        self.bus.apu_mut().drain_samples(out);
    }

    /// Set the audio output sample rate in Hz (default
    /// [`DEFAULT_SAMPLE_RATE`]).
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.bus.apu_mut().set_sample_rate(hz);
    }

    /// Map the four DMG shades to XRGB8888 colors (ignored on CGB models).
    pub fn set_dmg_palette(&mut self, palette: [u32; 4]) {
        self.bus.ppu_mut().set_dmg_palette(palette);
    }

    /// Battery-backed cartridge RAM (plus RTC state for MBC3), if any.
    pub fn save_data(&self) -> Option<Vec<u8>> {
        self.bus.cartridge().save_data()
    }

    /// Restore battery-backed RAM previously obtained from [`Self::save_data`].
    /// Returns false if the image was rejected (wrong size / no battery).
    pub fn load_save_data(&mut self, data: &[u8]) -> bool {
        self.bus.cartridge_mut().load_save_data(data)
    }

    /// True once the CPU has executed `LD B,B` (opcode 0x40) — the mooneye
    /// test suite's "test finished" software breakpoint.
    pub fn debug_breakpoint_hit(&self) -> bool {
        self.cpu.debug_breakpoint_hit()
    }

    /// CPU register snapshot, for test harnesses.
    pub fn cpu_regs(&self) -> Registers {
        self.cpu.regs()
    }

    /// The hardware model this machine was built as.
    pub fn model(&self) -> Model {
        self.bus.model()
    }

    // ---- test-harness escape hatches (not public API) ----

    /// Drain the bytes "printed" over the link port: every completed
    /// internal-clock serial transfer (SB <- byte, then $81 to SC — the
    /// blargg test-ROM protocol) appends the byte that was shifted out.
    /// The undrained buffer is capped at 64 KiB.
    #[doc(hidden)]
    pub fn take_serial_output(&mut self) -> Vec<u8> {
        self.bus.take_serial_output()
    }

    /// Side-effect-free memory peek: no M-cycle passes and nothing is
    /// mutated (`&self`). Follows live ROM/VRAM/cart-RAM/WRAM banking and
    /// intentionally ignores PPU VRAM/OAM access blocking; IO registers
    /// (FF00-FF7F) are not peekable and read $FF (see
    /// `Interconnect::peek`).
    #[doc(hidden)]
    pub fn peek(&self, addr: u16) -> u8 {
        self.bus.peek(addr)
    }

    /// Read for the bgb-style debugger views: like [`Self::peek`] but resolves
    /// the IO registers (FF00-FF7F) to their live hardware values instead of
    /// the `$FF` `peek` returns. Side-effect-free; the value is what the CPU
    /// would read at this instant. Use this for the memory dump and I/O map.
    #[must_use]
    pub fn debug_read(&self, addr: u16) -> u8 {
        self.bus.debug_read(addr)
    }

    /// The top `n` 16-bit words of the stack as `(address, word)` pairs,
    /// descending from `SP` like bgb's stack pane: `(SP, [SP]), (SP-2, [SP-2]),
    /// …`. Words are little-endian; addresses wrap at `0x0000`.
    #[must_use]
    pub fn stack(&self, n: usize) -> Vec<(u16, u16)> {
        let sp = self.cpu_regs().sp;
        (0..n)
            .map(|i| {
                let addr = sp.wrapping_sub((2 * i) as u16);
                let lo = self.bus.debug_read(addr);
                let hi = self.bus.debug_read(addr.wrapping_add(1));
                (addr, u16::from(lo) | (u16::from(hi) << 8))
            })
            .collect()
    }

    /// Whole 16 KiB VRAM for the debug VRAM viewer: CGB bank 0 is `[..0x2000]`,
    /// bank 1 is `[0x2000..]` (DMG fills only bank 0). Decode tiles/maps with
    /// [`debug::tile_pixels`]. Side-effect-free.
    #[must_use]
    pub fn vram(&self) -> &[u8; 0x4000] {
        self.bus.ppu().debug_vram()
    }

    /// Raw 160-byte OAM (40 sprites × 4 bytes). Decode with
    /// [`debug::oam_sprites`].
    #[must_use]
    pub fn oam(&self) -> &[u8; 0xA0] {
        self.bus.ppu().debug_oam()
    }

    /// Raw CGB palette RAM `(BG, OBJ)`, 64 bytes each (8 palettes × 4 colors ×
    /// 2 bytes, little-endian 15-bit BGR555). DMG palettes are BGP/OBP/OBP1,
    /// readable via [`Self::debug_read`] at FF47/FF48/FF49.
    #[must_use]
    pub fn cgb_palette_ram(&self) -> (&[u8; 64], &[u8; 64]) {
        self.bus.ppu().debug_palette_ram()
    }

    /// Run instructions until `PC` matches one of `breakpoints` (returns that
    /// address) or `max_instructions` have executed (returns `None`) — the
    /// debugger's "run" / "run to cursor". The check is *after* each step, so a
    /// breakpoint on the current `PC` doesn't stop instantly; "run" always
    /// advances off the current line and a loop back to a breakpoint still
    /// stops. This drives emulation forward; only call it for a debugger run,
    /// never on a golden/test path.
    pub fn run_until_breakpoint(
        &mut self,
        breakpoints: &[u16],
        max_instructions: u64,
    ) -> Option<u16> {
        for _ in 0..max_instructions {
            self.step();
            let pc = self.cpu_regs().pc;
            if breakpoints.contains(&pc) {
                return Some(pc);
            }
        }
        None
    }

    /// True once the CPU has executed an undefined opcode (0xD3, 0xDB,
    /// 0xDD, 0xE3, 0xE4, 0xEB, 0xEC, 0xED, 0xF4, 0xFC, 0xFD) and
    /// hard-locked — wilbertpol's mooneye fork ends its tests with 0xED.
    #[doc(hidden)]
    pub fn debug_undefined_hit(&self) -> bool {
        self.cpu.debug_undefined_hit()
    }

    /// Drain the raw audio tap: one stereo sample per dot, taken straight
    /// off the APU channel mixer *before* the box-average resampler and the
    /// high-pass "output capacitor" stage (`Apu::output_cycle`). The
    /// gambatte test harness compares this stream for its `_outaudio`
    /// sample-equality verdicts, which [`Self::drain_audio`]'s filtered
    /// output would distort (a decaying high-pass tail reads as "sound", a
    /// flattened distinct input as "silence"). Capped at two frames of
    /// backlog — drain right before the frame under test.
    #[doc(hidden)]
    pub fn drain_audio_raw(&mut self, out: &mut Vec<(f32, f32)>) {
        self.bus.apu_mut().drain_raw_samples(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with_cgb_flag(flag: u8) -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x143] = flag;
        rom
    }

    /// Pan Docs "CPU registers" (Power-Up Sequence): on CGB/AGB hardware
    /// the boot ROM hands a CGB-flagged cart off with DE=$FF56 HL=$000D;
    /// a DMG cart gets DE=$0008 HL=$007C (mooneye misc/boot_regs-cgb/-A —
    /// every mooneye ROM is DMG-flagged). A/F/B/C are cart-independent:
    /// AGB's extra `inc b` gives B=$01/F=$00 for both cart kinds.
    #[test]
    fn cgb_flagged_cart_boot_regs() {
        for (model, af, bc) in [(Model::Cgb, 0x1180, 0x0000), (Model::Agb, 0x1100, 0x0100)] {
            let gb = GameBoy::new(model, rom_with_cgb_flag(0x80)).unwrap();
            let r = gb.cpu_regs();
            assert_eq!(r.af(), af, "{model:?} CGB cart AF");
            assert_eq!(r.bc(), bc, "{model:?} CGB cart BC");
            assert_eq!(r.de(), 0xFF56, "{model:?} CGB cart DE");
            assert_eq!(r.hl(), 0x000D, "{model:?} CGB cart HL");

            let gb = GameBoy::new(model, rom_with_cgb_flag(0x00)).unwrap();
            let r = gb.cpu_regs();
            assert_eq!(r.af(), af, "{model:?} DMG cart AF");
            assert_eq!(r.bc(), bc, "{model:?} DMG cart BC");
            assert_eq!(r.de(), 0x0008, "{model:?} DMG cart DE");
            assert_eq!(r.hl(), 0x007C, "{model:?} DMG cart HL");
        }
    }

    #[test]
    fn debug_read_resolves_io_but_peek_does_not() {
        let gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
        // peek keeps IO out of band ($FF); debug_read returns the live value.
        // Post-boot LY is a valid scanline (0..=153), so it can't be the $FF
        // peek hands back — proving debug_read took the io_read path.
        assert_eq!(gb.peek(0xFF44), 0xFF, "peek must not read IO");
        assert!(
            gb.debug_read(0xFF44) <= 153,
            "debug_read should give live LY"
        );
        // Outside IO, debug_read is identical to peek (and to ROM contents).
        assert_eq!(gb.debug_read(0x0143), gb.peek(0x0143));
        assert_eq!(gb.debug_read(0x0143), 0x00); // the CGB flag we wrote
        for addr in [0x0000u16, 0x4000, 0xC000, 0xFF80, 0xFFFF] {
            assert_eq!(gb.debug_read(addr), gb.peek(addr), "non-IO {addr:#06x}");
        }
    }

    #[test]
    fn stack_descends_from_sp_little_endian() {
        let gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
        let sp = gb.cpu_regs().sp;
        let s = gb.stack(3);
        assert_eq!(s.len(), 3);
        // Addresses descend by two from SP (bgb's stack pane order).
        assert_eq!(s[0].0, sp);
        assert_eq!(s[1].0, sp.wrapping_sub(2));
        assert_eq!(s[2].0, sp.wrapping_sub(4));
        // Each word is the little-endian pair at its address.
        for &(addr, word) in &s {
            let want = u16::from(gb.debug_read(addr))
                | (u16::from(gb.debug_read(addr.wrapping_add(1))) << 8);
            assert_eq!(word, want, "word @ {addr:#06x}");
        }
    }

    /// A ROM whose entry (`0x100`) is `nop; jp 0x150` and `0x150..` is nops,
    /// so PC walks 0x100 -> 0x101 -> 0x150 -> 0x151 -> 0x152 … deterministically.
    fn linear_code_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x100] = 0x00; // nop
        rom[0x101..0x104].copy_from_slice(&[0xC3, 0x50, 0x01]); // jp 0150
        // 0x150.. already 0x00 (nop) from the zero-fill.
        rom
    }

    #[test]
    fn run_until_breakpoint_stops_at_the_address() {
        let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        assert_eq!(gb.cpu_regs().pc, 0x100);
        // 0x100 nop -> 0x101 jp -> 0x150 nop -> 0x151. bp at 0x151.
        assert_eq!(gb.run_until_breakpoint(&[0x151], 100), Some(0x151));
        assert_eq!(gb.cpu_regs().pc, 0x151);
    }

    #[test]
    fn run_until_breakpoint_respects_the_step_limit() {
        let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        // No reachable breakpoint -> runs the cap, returns None.
        assert_eq!(gb.run_until_breakpoint(&[0xBEEF], 5), None);
        assert_eq!(gb.run_until_breakpoint(&[], 3), None);
    }

    #[test]
    fn run_until_breakpoint_advances_off_the_current_pc() {
        let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        // A breakpoint on the *current* PC must not stop instantly — one step
        // moves to 0x101, which isn't the (already-left) 0x100.
        assert_eq!(gb.run_until_breakpoint(&[0x100], 1), None);
        assert_eq!(gb.cpu_regs().pc, 0x101);
    }
}
