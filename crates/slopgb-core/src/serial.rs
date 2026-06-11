//! Serial port (FF01 SB, FF02 SC). Timer/serial work package.
//!
//! The internal serial clock is derived from DIV counter bits, so transfer
//! timing depends on DIV phase (mooneye: `serial/boot_sclk_align-dmgABCmgb`).
//! No link cable peer: incoming bits read as 1.

pub struct Serial {
    // Serial work package owns state (SB, SC, bit counter, clock edge state).
}

impl Serial {
    pub fn new(cgb: bool) -> Self {
        let _ = cgb;
        Self {}
    }

    /// Advance one M-cycle. `div` is the timer's internal 16-bit DIV counter
    /// *after* this cycle's increment. Returns IF bits (bit 3 = serial).
    pub fn tick(&mut self, div: u16) -> u8 {
        let _ = div;
        todo!("serial work package")
    }

    /// Read FF01/FF02.
    pub fn read(&self, addr: u16) -> u8 {
        let _ = addr;
        todo!("serial work package")
    }

    /// Write FF01/FF02.
    pub fn write(&mut self, addr: u16, value: u8) {
        let _ = (addr, value);
        todo!("serial work package")
    }
}
