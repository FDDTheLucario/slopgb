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
pub(crate) mod cycle_clock;
pub(crate) mod interconnect;
pub(crate) mod joypad;
pub(crate) mod mode_timeline;
pub(crate) mod model;
pub(crate) mod ppu;
pub(crate) mod serial;
pub(crate) mod stat_update;
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
        Self::new_inner(model, rom, false)
    }

    /// Like [`Self::new`], but enables the Stage-B Tier-2 reclock *before* the
    /// post-boot state is applied, so the deferred-frame DIV phase
    /// re-calibration (`interconnect/boot.rs`, the `boot_div`/`boot_sclk` fix)
    /// lands at hand-off. This mirrors the production path once the port flips
    /// the construction default; the runtime [`Self::set_tier2_reclock`] toggle
    /// cannot reproduce it because boot has already run by the time it is
    /// called. Off-path (and `set_tier2_reclock`-only) construction is
    /// unchanged.
    #[doc(hidden)]
    pub fn new_with_reclock(model: Model, rom: Vec<u8>) -> Result<Self, CartridgeError> {
        Self::new_inner(model, rom, true)
    }

    fn new_inner(model: Model, rom: Vec<u8>, tier2: bool) -> Result<Self, CartridgeError> {
        let cart = cartridge::Cartridge::from_bytes(rom)?;
        let mut bus = interconnect::Interconnect::new(model, cart);
        let mut cpu = cpu::Cpu::new(model);
        if tier2 {
            bus.set_tier2_reclock(true);
        }
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

    /// True once the CPU has executed an undefined opcode (0xD3, 0xDB,
    /// 0xDD, 0xE3, 0xE4, 0xEB, 0xEC, 0xED, 0xF4, 0xFC, 0xFD) and
    /// hard-locked — wilbertpol's mooneye fork ends its tests with 0xED.
    #[doc(hidden)]
    pub fn debug_undefined_hit(&self) -> bool {
        self.cpu.debug_undefined_hit()
    }

    /// Port validation hook — enable the SameBoy cycle-exact flag-on path
    /// (leading-edge cc+0 reads + the `StatUpdate` engine + the `vis_early`
    /// back-date + the A6 halt-late masks). Off in production until the staged
    /// port flips the default (`docs/sameboy-port/PORT-PLAN.md`); the gbtr S0
    /// kernel-pair acceptance spec drives it on to measure the convergence.
    #[doc(hidden)]
    pub fn set_leading_edge_reads(&mut self, on: bool) {
        self.bus.set_leading_edge_reads(on);
    }

    /// Port validation hook — enable the Stage-B Tier-2 dispatch reclock
    /// (deferred-commit machine advance + the −2 interrupt-dispatch retime).
    /// Implies [`Self::set_leading_edge_reads`]. Off in production; the
    /// make-or-break thesis measurement drives it on (`PORT-PLAN.md` Tier 2).
    #[doc(hidden)]
    pub fn set_tier2_reclock(&mut self, on: bool) {
        self.bus.set_tier2_reclock(on);
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
}
