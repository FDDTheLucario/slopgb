//! Boot-ROM mapping on [`Interconnect`] (opt-in, golden-safe): a boot ROM
//! attached by `GameBoy::new_with_boot` overlays the low cart region until it
//! writes FF50. Every method is inert when no boot ROM is attached
//! (`boot_active` false — the default + every golden/test path), so the
//! cart-ROM read is byte-identical there. The boot region read + FF50-disable
//! consumers live in `memory.rs`; the struct fields stay in the parent.

use super::*;

impl Interconnect {
    /// Attach a boot ROM and map it over the low cart region. The frontend
    /// `GameBoy::new_with_boot` path only — `new` never calls this.
    pub(crate) fn attach_boot_rom(&mut self, boot_rom: Vec<u8>) {
        self.boot_rom = Some(boot_rom);
        self.boot_active = true;
        // Real CGB/AGB hardware powers up in full CGB mode regardless of the
        // cart: the boot ROM writes the DMG-compat palettes + OPRI while still
        // in CGB mode and only locks DMG-compatibility (KEY0/FF4C) at the very
        // end. `new` precomputes the post-lock value, so for a DMG cart on CGB
        // those boot-ROM IO writes would be dropped (cgb_mode false). Enter true
        // power-on CGB mode here so they land; the FF4C handler re-locks
        // DMG-compat before hand-off. Only the boot path reaches this — `new` is
        // untouched, so it stays golden-safe.
        if self.model.is_cgb() {
            self.set_cgb_mode(true);
        }
    }

    /// Whether the boot ROM is currently mapped (debug/test view).
    pub(crate) fn boot_active(&self) -> bool {
        self.boot_active
    }

    /// The boot-ROM byte overlaying cart address `addr`, or `None` when no boot
    /// ROM is mapped there (`None` whenever `boot_active` is false, so the
    /// cart-ROM read is byte-identical on every golden path). The mapped region
    /// is selected by boot-ROM size: a 256-byte DMG/MGB/SGB boot ROM covers
    /// 0x0000-0x00FF; a 2304-byte CGB/AGB boot ROM covers 0x0000-0x00FF and
    /// 0x0200-0x08FF (the 0x0100-0x01FF cart-header window shows the cart).
    pub(crate) fn boot_rom_byte(&self, addr: u16) -> Option<u8> {
        if !self.boot_active {
            return None;
        }
        let rom = self.boot_rom.as_ref()?;
        let in_region = match rom.len() {
            0x100 => addr < 0x0100,
            0x900 => addr < 0x0100 || (0x0200..0x0900).contains(&addr),
            _ => false,
        };
        in_region.then(|| rom.get(addr as usize).copied()).flatten()
    }
}
