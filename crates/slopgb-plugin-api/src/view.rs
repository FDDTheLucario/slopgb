//! [`GameBoyView`] — read-only handle onto the live machine, and the
//! [`Registers`] snapshot it returns.

use crate::abi::{self, Reg};

/// Read-only window onto the running Game Boy, handed to
/// [`Plugin::on_frame`](crate::Plugin::on_frame) each frame. Reads hit an owned
/// host snapshot: cheap, frame-consistent, never perturbs emulation.
pub struct GameBoyView {
    _private: (),
}

impl GameBoyView {
    /// Constructed only by the generated export shim.
    #[doc(hidden)]
    #[must_use]
    pub fn __new() -> Self {
        Self { _private: () }
    }

    /// One byte of the CPU address space (`$0000..=$FFFF`, bank 0), no I/O
    /// side effects.
    #[must_use]
    pub fn read(&self, addr: u16) -> u8 {
        (abi::host_read(i32::from(addr)) & 0xFF) as u8
    }

    /// One register or I/O value.
    #[must_use]
    pub fn reg(&self, reg: Reg) -> u16 {
        (abi::host_reg(reg.index()) & 0xFFFF) as u16
    }

    /// All exposed registers at once.
    #[must_use]
    pub fn registers(&self) -> Registers {
        Registers {
            af: self.reg(Reg::Af),
            bc: self.reg(Reg::Bc),
            de: self.reg(Reg::De),
            hl: self.reg(Reg::Hl),
            sp: self.reg(Reg::Sp),
            pc: self.reg(Reg::Pc),
            lcdc: self.reg(Reg::Lcdc) as u8,
            stat: self.reg(Reg::Stat) as u8,
            ly: self.reg(Reg::Ly) as u8,
        }
    }

    /// Append a UTF-8 line to the host plugin log.
    pub fn log(&self, line: &str) {
        abi::host_log(line.as_ptr() as i32, line.len() as i32);
    }
}

/// Frame-consistent CPU registers plus key LCD I/O bytes, from
/// [`GameBoyView::registers`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub sp: u16,
    pub pc: u16,
    /// `LCDC` `$FF40`.
    pub lcdc: u8,
    /// `STAT` `$FF41`.
    pub stat: u8,
    /// `LY` `$FF44`.
    pub ly: u8,
}
