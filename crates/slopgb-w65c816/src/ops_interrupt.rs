//! Software interrupts (`BRK`/`COP`), the interrupt return (`RTI`), and the
//! `WAI`/`STP` wait/stop ops. A software interrupt pushes the return state,
//! masks IRQs, clears decimal, zeroes the program bank and vectors through a
//! fixed address; in native mode the program bank is pushed too and the vectors
//! differ. Per the WDC W65C816S datasheet and vectors.

use super::*;

/// Interrupt vector addresses (bank 0): (emulation, native).
const BRK_VECTOR: (u32, u32) = (0xFFFE, 0xFFE6);
const COP_VECTOR: (u32, u32) = (0xFFF4, 0xFFE4);
const NMI_VECTOR: (u32, u32) = (0xFFFA, 0xFFEA);

impl Cpu {
    /// `BRK`: software interrupt through the IRQ/BRK vector.
    pub(crate) fn brk(&mut self, bus: &mut impl Bus) {
        self.software_interrupt(bus, BRK_VECTOR);
    }

    /// `COP`: software interrupt through the coprocessor vector.
    pub(crate) fn cop(&mut self, bus: &mut impl Bus) {
        self.software_interrupt(bus, COP_VECTOR);
    }

    fn software_interrupt(&mut self, bus: &mut impl Bus, vector: (u32, u32)) {
        // Both are two-byte instructions: the signature byte is read and skipped.
        self.fetch8(bus);
        if !self.regs.e {
            let pbr = self.regs.pbr;
            self.push8(bus, pbr);
        }
        let pc = self.regs.pc;
        self.push8(bus, (pc >> 8) as u8);
        self.push8(bus, pc as u8);
        let p = self.regs.p;
        self.push8(bus, p);
        self.set_flag(flag::I, true);
        self.set_flag(flag::D, false);
        self.regs.pbr = 0;
        let addr = if self.regs.e { vector.0 } else { vector.1 };
        let lo = self.read8(bus, addr) as u16;
        let hi = self.read8(bus, addr.wrapping_add(1)) as u16;
        self.regs.pc = lo | (hi << 8);
    }

    /// Hardware NMI: push the return state and vector through `$FFFA`
    /// (emulation) / `$FFEA` (native), per the WDC W65C816S datasheet. Unlike
    /// `BRK` there is no signature byte — the pushed PC is the next
    /// un-executed instruction — and the emulation-mode pushed P has bit 4
    /// clear (the hardware-interrupt signature). Wakes a `WAI`-ing CPU.
    /// The caller invokes this between instructions (a real /NMI is sampled
    /// at instruction boundaries); [`Cpu::nmi`] wraps it with the per-call
    /// cycle accounting.
    pub(crate) fn nmi_sequence(&mut self, bus: &mut impl Bus) {
        self.waiting = false;
        self.io();
        self.io();
        if !self.regs.e {
            let pbr = self.regs.pbr;
            self.push8(bus, pbr);
        }
        let pc = self.regs.pc;
        self.push8(bus, (pc >> 8) as u8);
        self.push8(bus, pc as u8);
        let p = if self.regs.e {
            self.regs.p & !flag::X
        } else {
            self.regs.p
        };
        self.push8(bus, p);
        self.set_flag(flag::I, true);
        self.set_flag(flag::D, false);
        self.regs.pbr = 0;
        let addr = if self.regs.e {
            NMI_VECTOR.0
        } else {
            NMI_VECTOR.1
        };
        let lo = self.read8(bus, addr) as u16;
        let hi = self.read8(bus, addr.wrapping_add(1)) as u16;
        self.regs.pc = lo | (hi << 8);
    }

    /// `RTI`: pull P then PC (and, in native mode, the program bank). PC is used
    /// as pulled (no increment, unlike `RTS`).
    pub(crate) fn rti(&mut self, bus: &mut impl Bus) {
        self.io();
        self.io();
        let mut p = self.pull8(bus);
        let lo = self.pull8(bus) as u16;
        let hi = self.pull8(bus) as u16;
        self.regs.pc = lo | (hi << 8);
        if self.regs.e {
            // Emulation forces the M/X bits (they read back as set).
            p |= flag::M | flag::X;
        } else {
            self.regs.pbr = self.pull8(bus);
        }
        self.regs.p = p;
        // Selecting 8-bit index (native) drops the index high bytes.
        if !self.regs.e && self.regs.p & flag::X != 0 {
            self.regs.x &= 0x00FF;
            self.regs.y &= 0x00FF;
        }
    }

    /// `WAI`: halt until an interrupt. The vectors show two internal cycles plus
    /// the wait cycle; the host resumes the CPU by clearing `waiting`.
    pub(crate) fn wai(&mut self) {
        self.io();
        self.io();
        self.io();
        self.waiting = true;
    }

    /// `STP`: stop the clock until RESET. Same cycle shape as `WAI`.
    pub(crate) fn stp(&mut self) {
        self.io();
        self.io();
        self.io();
        self.stopped = true;
    }
}
