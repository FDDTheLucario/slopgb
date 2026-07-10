//! Read-only / debug accessor surface: model + peripheral getters, the
//! CGB-mode + double-speed views, cartridge/serial handles, the debug
//! write, and the side-effect-free `peek_no_io`. No timing; construction
//! (`new`) stays in the parent. Oracle: full mooneye + gbtr matrix.

use super::*;

impl Interconnect {
    /// True when the machine runs in native CGB mode (CGB/AGB hardware with
    /// a CGB-flagged cart, as opposed to DMG compatibility mode).
    pub(crate) fn cgb_mode(&self) -> bool {
        self.cgb_mode
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

    /// Read-only APU view (debugger wave/channel panels). Side-effect-free.
    pub fn apu(&self) -> &Apu {
        &self.apu
    }

    pub fn joypad_mut(&mut self) -> &mut Joypad {
        &mut self.joypad
    }

    /// Read-only joypad view (debugger button state). Side-effect-free.
    pub fn joypad(&self) -> &Joypad {
        &self.joypad
    }

    /// CGB double-speed mode (KEY1 bit 7) — the debugger's `spd` view.
    pub(crate) fn double_speed(&self) -> bool {
        self.double_speed
    }

    /// Enter (`true`) / leave (`false`) native CGB mode. Used by the opt-in
    /// boot-ROM path (`attach_boot_rom`) to run the CGB boot ROM in true
    /// power-on CGB mode and by the FF4C DMG-compat lock at hand-off; mirrors
    /// the DMG-compat routing `Interconnect::new` precomputes. Never reached on
    /// a `new` (no-boot) path, so it is golden-safe.
    pub(crate) fn set_cgb_mode(&mut self, on: bool) {
        self.cgb_mode = on;
        self.ppu.set_dmg_compat(self.model.is_cgb() && !on);
        self.serial.set_cgb(on);
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

    /// Debugger memory write: store `value` at `addr` with no M-cycle timing
    /// (the symmetric counterpart of [`Self::peek_no_io`] / the debug read path).
    /// Used by [`crate::GameBoy::debug_call`] to push a return address — a
    /// live-debugger-only `&mut` path, never on a golden/test run.
    pub fn debug_write(&mut self, addr: u16, value: u8) {
        self.write_no_tick(addr, value);
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
    ///
    /// For live IO-register values (FF00-FF7F resolved from peripheral state)
    /// use [`Self::debug_read`], which delegates the non-IO ranges back here.
    pub(crate) fn peek_no_io(&self, addr: u16) -> u8 {
        match addr {
            // Show the mapped boot ROM in the debugger views too (inert / cart
            // when no boot ROM is active — golden-safe).
            0x0000..=0x7FFF => self
                .boot_rom_byte(addr)
                .unwrap_or_else(|| self.cart.read_rom(addr)),
            0x8000..=0x9FFF => self.ppu.vram_read_raw(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
            0xFE00..=0xFE9F => self.ppu.oam_read_raw(addr),
            0xFEA0..=0xFF7F => 0xFF,
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)],
            0xFFFF => self.ie,
        }
    }
}
