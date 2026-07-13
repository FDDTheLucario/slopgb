//! MBC3 real-time clock behavior (deterministic tick from emulated cycles).

use super::*;

impl Rtc {
    pub(super) fn new() -> Self {
        Rtc {
            regs: [0; 5],
            latched: [0; 5],
            subsec: 0,
            // Power-on value chosen so a lone 0x01 write does not latch.
            latch_prev: 0xFF,
        }
    }

    pub(super) fn write_latch(&mut self, value: u8) {
        // Pan Docs: writing 0x00 then 0x01 latches the current time.
        if self.latch_prev == 0x00 && value == 0x01 {
            self.latched = self.regs;
        }
        self.latch_prev = value;
    }

    pub(super) fn write_reg(&mut self, index: usize, value: u8) {
        self.regs[index] = value & RTC_MASKS[index];
        if index == 0 {
            // Writing the seconds register resets the internal sub-second
            // divider (rtc3test "sub-second writes" on hardware).
            self.subsec = 0;
        }
    }

    fn halted(&self) -> bool {
        self.regs[RTC_DH] & RTC_HALT != 0
    }

    pub(super) fn tick_cycles(&mut self, t_cycles: u32) {
        if self.halted() {
            return;
        }
        let total = u64::from(self.subsec) + u64::from(t_cycles);
        self.subsec = (total % u64::from(CYCLES_PER_SECOND)) as u32;
        for _ in 0..total / u64::from(CYCLES_PER_SECOND) {
            self.tick_second();
        }
    }

    fn tick_second(&mut self) {
        let [s, m, h, dl, dh] = &mut self.regs;
        // Each counter wraps at its bit width; the carry into the next
        // counter fires only when the nominal limit (60/60/24) is hit.
        *s = (*s + 1) & 0x3F;
        if *s != 60 {
            return;
        }
        *s = 0;
        *m = (*m + 1) & 0x3F;
        if *m != 60 {
            return;
        }
        *m = 0;
        *h = (*h + 1) & 0x1F;
        if *h != 24 {
            return;
        }
        *h = 0;
        let day = ((u16::from(*dh & 0x01) << 8) | u16::from(*dl)) + 1;
        *dl = day as u8;
        *dh = (*dh & !0x01) | ((day >> 8) as u8 & 0x01);
        if day == 512 {
            // 9-bit day counter overflowed: sticky carry flag.
            *dh |= RTC_CARRY;
        }
    }
}
