//! I/O registers `$F0-$FF`: control/test, DSP address/data, the four comm
//! ports, the aux registers, and the timer target/counter registers.
//!
//! # Comm-port latch model
//!
//! Each of the four ports `$F4-$F7` is **two** independent latches:
//! - [`Spc700::port_in`]  — written by the SNES ([`Spc700::snes_write_port`]),
//!   read by the APU (`MOV A, $F4`).
//! - [`Spc700::port_out`] — written by the APU (`MOV $F4, A`),
//!   read by the SNES ([`Spc700::snes_read_port`]).
//!
//! So an APU write to `$F4` is never visible to a later APU read of `$F4`; the
//! two sides communicate only through the paired opposite latch. (fullsnes,
//! "SNES APU I/O Ports".)
//!
//! # DSP seam (Phase 3)
//!
//! `$F2` selects a DSP register, `$F3` reads/writes it. Attach the S-DSP with
//! [`Spc700::attach_dsp`]; the raw `$F2` address is forwarded to [`Dsp`]. With
//! no DSP attached a 128-byte shadow answers so the SPC700 is self-consistent
//! standalone.

use super::*;

/// The S-DSP seam Phase 3 plugs into. The SPC700 forwards `$F2`/`$F3` accesses
/// here (the raw `$F2` address is passed through, so the DSP sees the read-only
/// mirror bit) and ticks the DSP from each instruction's cycle count.
pub trait Dsp {
    /// Read the DSP register selected by the raw `$F2` value.
    fn read(&mut self, addr: u8) -> u8;
    /// Write the DSP register selected by the raw `$F2` value.
    fn write(&mut self, addr: u8, val: u8);
    /// Advance the DSP by `cycles` SPC700 (1.024 MHz) cycles. Default: no-op.
    fn tick(&mut self, _cycles: u32) {}
}

impl Spc700 {
    // -- Phase 3 seam ------------------------------------------------------

    /// Attach the S-DSP (Phase 3). Replaces any previously attached DSP.
    pub fn attach_dsp(&mut self, dsp: Box<dyn Dsp>) {
        self.dsp = Some(dsp);
    }

    /// Detach and return the S-DSP, if any.
    pub fn detach_dsp(&mut self) -> Option<Box<dyn Dsp>> {
        self.dsp.take()
    }

    /// SNES side writes comm port `n` (0-3); the APU reads it at `$F4+n`.
    pub fn snes_write_port(&mut self, n: usize, v: u8) {
        self.port_in[n & 3] = v;
    }

    /// SNES side reads comm port `n` (0-3); it holds what the APU wrote to `$F4+n`.
    pub fn snes_read_port(&self, n: usize) -> u8 {
        self.port_out[n & 3]
    }

    // -- DSP register access ----------------------------------------------

    fn dsp_read(&mut self) -> u8 {
        match &mut self.dsp {
            Some(d) => d.read(self.dsp_addr),
            None => self.dsp_shadow[(self.dsp_addr & 0x7F) as usize],
        }
    }

    fn dsp_write(&mut self, v: u8) {
        match &mut self.dsp {
            Some(d) => d.write(self.dsp_addr, v),
            // The real S-DSP ignores writes when the address' bit 7 is set
            // (registers `$80-$FF` are the read-only mirror).
            None => {
                if self.dsp_addr < 0x80 {
                    self.dsp_shadow[self.dsp_addr as usize] = v;
                }
            }
        }
    }

    /// Advance any attached DSP by `cyc` cycles (called from `step`).
    pub(super) fn tick_dsp(&mut self, cyc: u32) {
        if let Some(d) = &mut self.dsp {
            d.tick(cyc);
        }
    }

    // -- $F0-$FF decode ----------------------------------------------------

    /// Snapshot the `$F0-$FF` I/O register latches for an `.spc` dump. Unlike
    /// [`Spc700::io_read`] this is non-destructive (reading timer counters does
    /// not clear them) and returns the *retained* state a player must restore:
    /// control (`$F1`, timer enables + IPL) and the timer targets (`$FA-$FC`),
    /// without which the SPC's timers stay stopped and a timer-paced driver
    /// (N-SPC) never advances. Timer output counters (`$FD-$FF`) read back 0;
    /// the player re-derives them from the running stage.
    pub fn io_snapshot(&self) -> [u8; 16] {
        let mut r = [0u8; 16];
        r[0x0] = self.test;
        r[0x1] = self.control;
        r[0x2] = self.dsp_addr;
        r[0x4..0x8].copy_from_slice(&self.port_in); // reads of $F4-$F7 return these
        r[0x8] = self.aux[0];
        r[0x9] = self.aux[1];
        r[0xA] = self.timer[0].target;
        r[0xB] = self.timer[1].target;
        r[0xC] = self.timer[2].target;
        r
    }

    /// Read an I/O register (`reg` in `$F0..=$FF`).
    pub(super) fn io_read(&mut self, reg: u8) -> u8 {
        match reg {
            0xF0 => self.test,    // TEST — write-mostly; stored value on read.
            0xF1 => self.control, // CONTROL — likewise (driver never reads it).
            0xF2 => self.dsp_addr,
            0xF3 => self.dsp_read(),
            0xF4..=0xF7 => self.port_in[(reg - 0xF4) as usize],
            0xF8 => self.aux[0],
            0xF9 => self.aux[1],
            0xFA..=0xFC => 0, // timer targets are write-only.
            0xFD => self.timer[0].read_out(),
            0xFE => self.timer[1].read_out(),
            _ => self.timer[2].read_out(), // 0xFF
        }
    }

    /// Write an I/O register (`reg` in `$F0..=$FF`).
    pub(super) fn io_write(&mut self, reg: u8, v: u8) {
        match reg {
            0xF0 => self.test = v,
            0xF1 => self.write_control(v),
            0xF2 => self.dsp_addr = v,
            0xF3 => self.dsp_write(v),
            0xF4..=0xF7 => self.port_out[(reg - 0xF4) as usize] = v,
            0xF8 => self.aux[0] = v,
            0xF9 => self.aux[1] = v,
            0xFA => self.timer[0].target = v,
            0xFB => self.timer[1].target = v,
            0xFC => self.timer[2].target = v,
            _ => {} // 0xFD-0xFF counters are read-only; writes ignored.
        }
    }

    /// `$F1` CONTROL: timer enables (bits 0-2), port-clear strobes (bits 4-5),
    /// IPL-ROM enable (bit 7). A 0→1 enable edge resets that timer's stage +
    /// output counter; the strobes clear the paired input latches.
    fn write_control(&mut self, v: u8) {
        for i in 0..3 {
            let now = v & (1 << i) != 0;
            let was = self.control & (1 << i) != 0;
            self.timer[i].enabled = now;
            if now && !was {
                self.timer[i].reset_counters();
            }
        }
        if v & 0x10 != 0 {
            self.port_in[0] = 0;
            self.port_in[1] = 0;
        }
        if v & 0x20 != 0 {
            self.port_in[2] = 0;
            self.port_in[3] = 0;
        }
        // Keep only the retained bits (enables + IPL); strobes read back as 0.
        self.control = v & 0b1000_0111;
    }
}
