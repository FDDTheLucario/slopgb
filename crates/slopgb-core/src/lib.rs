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
        let cpu = cpu::Cpu::new(model);
        bus.apply_post_boot_state();
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
}
