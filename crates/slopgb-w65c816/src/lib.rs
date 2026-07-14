//! Clean-room WDC 65C816 CPU core — the SNES-side CPU used by the Super Game
//! Boy. Bus-generic (tested against TomHarte vectors with a flat bus; hosted as
//! a slopgb coprocessor plugin with a comm-port bus). Built only from the WDC
//! datasheet, Eyes & Lichty, and test vectors/ROMs — never an emulator's source.

mod addressing;
mod cpu;
mod dispatch;
mod ops_ctrl;
mod ops_load;
mod ops_logic;
mod ops_stack;
mod regs;

pub use cpu::Cpu;
pub use regs::{Regs, flag};

/// The 24-bit bus a 65C816 talks to. A flat RAM backs the vector tests; the
/// coprocessor plugin backs it with guest RAM + host comm ports.
pub trait Bus {
    /// Read one byte at a 24-bit address (`bank << 16 | addr`).
    fn read(&mut self, addr: u32) -> u8;
    /// Write one byte at a 24-bit address.
    fn write(&mut self, addr: u32, value: u8);
}
