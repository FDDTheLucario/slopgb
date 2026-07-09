//! Miscellaneous single-byte ops: `NOP`, the flag/control instructions, and the
//! `SLEEP`/`STOP` halts.
//!
//! `CLRV` clears **both** V and H (the only op that clears H standalone).
//! `EI`/`DI` set/clear the I flag; the SPC700 has no interrupt sources wired in
//! the SNES APU, so I is otherwise inert. `SLEEP` and `STOP` both halt the
//! oscillator — modelled identically here as a `stopped` state cleared only by
//! reset (their 7-cycle count, from the hardware trace, lives in the [`CYCLES`]
//! table). (fullsnes, "SPC700 Opcodes".)

use super::*;

impl Spc700 {
    pub(super) fn op_misc(&mut self, op: u8) {
        match op {
            0x00 => {}                        // NOP
            0x60 => self.psw.c = false,       // CLRC
            0x80 => self.psw.c = true,        // SETC
            0xED => self.psw.c = !self.psw.c, // NOTC
            0x20 => self.psw.p = false,       // CLRP
            0x40 => self.psw.p = true,        // SETP
            0xE0 => {
                // CLRV: clears V and H together.
                self.psw.v = false;
                self.psw.h = false;
            }
            0xA0 => self.psw.i = true,          // EI
            0xC0 => self.psw.i = false,         // DI
            0xEF | 0xFF => self.stopped = true, // SLEEP / STOP
            _ => unreachable!("op_misc dispatched a non-misc opcode: {op:#04X}"),
        }
    }
}
