//! Serial port (FF01 SB, FF02 SC). Timer/serial work package.
//!
//! The internal serial clock is derived from DIV counter bits, so transfer
//! timing depends on DIV phase (mooneye: `serial/boot_sclk_align-dmgABCmgb`).
//! No link cable peer: incoming bits read as 1.
//!
//! Internal clock (Pan Docs "Serial Data Transfer"): 8192 Hz = a shift on
//! every falling edge of DIV counter bit 8. With the CGB fast-clock bit
//! (SC bit 1) set: 262144 Hz = falling edge of DIV counter bit 3. In CGB
//! double speed the DIV counter itself runs twice as fast (the caller passes
//! the post-increment counter each M-cycle), which yields the documented
//! 16384 Hz / 524288 Hz rates.

pub struct Serial {
    cgb: bool,
    sb: u8,
    sc: u8,
    /// Bits left to shift in the active transfer (0 = idle).
    bits_left: u8,
    /// DIV counter as of the previous `tick`, for edge detection.
    prev_div: u16,
}

impl Serial {
    pub fn new(cgb: bool) -> Self {
        Self {
            cgb,
            sb: 0,
            sc: 0,
            bits_left: 0,
            prev_div: 0,
        }
    }

    /// DIV counter bit whose falling edge drives the shift clock.
    fn clock_mask(&self) -> u16 {
        if self.cgb && self.sc & 0x02 != 0 {
            1 << 3 // 262144 Hz (524288 Hz in double speed)
        } else {
            1 << 8 // 8192 Hz (16384 Hz in double speed)
        }
    }

    /// Advance one M-cycle. `div` is the timer's internal 16-bit DIV counter
    /// *after* this cycle's increment. Returns IF bits (bit 3 = serial).
    pub fn tick(&mut self, div: u16) -> u8 {
        let mask = self.clock_mask();
        let falling = self.prev_div & mask != 0 && div & mask == 0;
        self.prev_div = div;
        // Shifts happen only with an active transfer using the internal
        // clock (SC bit 7 and bit 0 both set). With an external clock and no
        // peer, the transfer never advances.
        if !falling || self.bits_left == 0 || self.sc & 0x81 != 0x81 {
            return 0;
        }
        // MSB out first; no peer, so 1 bits come in.
        self.sb = (self.sb << 1) | 1;
        self.bits_left -= 1;
        if self.bits_left == 0 {
            self.sc &= 0x7F; // transfer-in-progress flag clears itself
            return 0x08;
        }
        0
    }

    /// Read FF01/FF02.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF01 => self.sb,
            // SC unused bits read 1: DMG implements bits 7 and 0 only,
            // CGB adds bit 1 (fast clock).
            0xFF02 => {
                if self.cgb {
                    0x7C | self.sc
                } else {
                    0x7E | self.sc
                }
            }
            _ => 0xFF,
        }
    }

    /// Write FF01/FF02.
    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF01 => self.sb = value,
            0xFF02 => {
                let mask = if self.cgb { 0x83 } else { 0x81 };
                self.sc = value & mask;
                // Setting bit 7 (re)starts a transfer; clearing it aborts.
                // The shift clock itself stays aligned to the DIV counter
                // (mooneye boot_sclk_align: edges align to the counter reset
                // time, not to the SC write).
                self.bits_left = if self.sc & 0x80 != 0 { 8 } else { 0 };
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Advance one M-cycle: bump the external div by 4 and tick.
    fn step(s: &mut Serial, div: &mut u16) -> u8 {
        *div = div.wrapping_add(4);
        s.tick(*div)
    }

    /// Run until tick returns IF bits; returns the div value at completion.
    fn run_until_irq(s: &mut Serial, div: &mut u16, max_mcycles: u32) -> Option<u16> {
        for _ in 0..max_mcycles {
            if step(s, div) != 0 {
                return Some(*div);
            }
        }
        None
    }

    #[test]
    fn sb_readback() {
        let mut s = Serial::new(false);
        s.write(0xFF01, 0x5A);
        assert_eq!(s.read(0xFF01), 0x5A);
    }

    #[test]
    fn sc_unused_bits_read_one_dmg() {
        let mut s = Serial::new(false);
        s.write(0xFF02, 0x00);
        assert_eq!(s.read(0xFF02), 0x7E);
        s.write(0xFF02, 0x01);
        assert_eq!(s.read(0xFF02), 0x7F);
        s.write(0xFF02, 0x80);
        assert_eq!(s.read(0xFF02), 0xFE);
    }

    #[test]
    fn sc_unused_bits_read_one_cgb() {
        let mut s = Serial::new(true);
        s.write(0xFF02, 0x00);
        assert_eq!(s.read(0xFF02), 0x7C);
        s.write(0xFF02, 0x02);
        assert_eq!(s.read(0xFF02), 0x7E);
        s.write(0xFF02, 0x83);
        assert_eq!(s.read(0xFF02), 0xFF);
    }

    /// A transfer started at div = 0 shifts on every falling edge of bit 8
    /// (every 512 T-cycles) and completes with IF bit 3 on the 8th edge.
    #[test]
    fn transfer_completes_on_eighth_bit8_falling_edge() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(8 * 512));
        assert_eq!(s.read(0xFF01), 0xFF); // 1s shifted in (no peer)
        assert_eq!(s.read(0xFF02), 0x7F); // bit 7 cleared
    }

    /// Shifts move the MSB out first and pull 1s in.
    #[test]
    fn shifts_msb_first_with_incoming_ones() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0b1010_0000);
        s.write(0xFF02, 0x81);
        while div < 512 {
            assert_eq!(step(&mut s, &mut div), 0);
        }
        assert_eq!(s.read(0xFF01), 0b0100_0001);
        while div < 1024 {
            step(&mut s, &mut div);
        }
        assert_eq!(s.read(0xFF01), 0b1000_0011);
    }

    /// mooneye boot_sclk_align: the shift clock is a divider of the main
    /// clock, so edges align to the DIV counter phase, not to the SC write.
    #[test]
    fn transfer_alignment_depends_on_div_phase() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        while div < 600 {
            step(&mut s, &mut div);
        }
        s.write(0xFF02, 0x81);
        // First edge at 1024, completion at 1024 + 7 * 512.
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(1024 + 7 * 512));
    }

    /// An edge in the M-cycle *before* SC is written does not count: the
    /// write happens after that cycle's tick.
    #[test]
    fn edge_before_sc_write_does_not_shift() {
        let mut s = Serial::new(false);
        let mut div = 508u16;
        s.tick(div); // seed prev_div with bit 8 high... (508: bit 8 = 1)
        div = 512;
        s.tick(div); // falling edge, but no transfer is active yet
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(1024 + 7 * 512));
    }

    /// External clock (SC bit 0 = 0) with no peer: nothing ever happens and
    /// the transfer flag stays set.
    #[test]
    fn external_clock_never_completes() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x42);
        s.write(0xFF02, 0x80);
        assert_eq!(run_until_irq(&mut s, &mut div, 20_000), None);
        assert_eq!(s.read(0xFF01), 0x42);
        assert_eq!(s.read(0xFF02), 0xFE); // bit 7 still set
    }

    /// Clock edges with no transfer in progress do nothing.
    #[test]
    fn idle_edges_do_nothing() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x42);
        for _ in 0..2000 {
            assert_eq!(step(&mut s, &mut div), 0);
        }
        assert_eq!(s.read(0xFF01), 0x42);
    }

    /// CGB fast clock (SC bit 1): falling edge of DIV bit 3, i.e. a shift
    /// every 16 T-cycles, full transfer in 128.
    #[test]
    fn cgb_fast_clock_uses_bit3() {
        let mut s = Serial::new(true);
        let mut div = 0u16;
        s.write(0xFF02, 0x83);
        assert_eq!(run_until_irq(&mut s, &mut div, 100), Some(8 * 16));
        assert_eq!(s.read(0xFF01), 0xFF);
    }

    /// DMG has no fast-clock bit; writing it is ignored.
    #[test]
    fn dmg_ignores_fast_clock_bit() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF02, 0x83);
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(8 * 512));
    }

    /// A DIV counter reset (DIV write) that drops the clock bit from 1 to 0
    /// is a falling edge and clocks the shifter, like the timer.
    #[test]
    fn div_reset_jump_can_clock_shifter() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        while div < 300 {
            step(&mut s, &mut div); // div = 300: bit 8 high
        }
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        s.tick(4); // DIV was written: counter jumped to 0, then +4
        assert_eq!(s.read(0xFF01), 0x01); // one bit shifted
    }

    /// Clearing SC bit 7 aborts an in-flight transfer.
    #[test]
    fn sc_write_aborts_transfer() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 1024 {
            step(&mut s, &mut div); // two bits shifted
        }
        assert_eq!(s.read(0xFF01), 0x03);
        s.write(0xFF02, 0x01);
        assert_eq!(run_until_irq(&mut s, &mut div, 20_000), None);
        assert_eq!(s.read(0xFF01), 0x03); // partial data kept
    }
}
