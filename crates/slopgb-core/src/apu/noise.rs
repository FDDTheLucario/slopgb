//! Noise channel (channel 4): 15-bit LFSR clocked from a divisor table.

use super::envelope::Envelope;
use super::length::LengthCounter;

pub(super) struct Noise {
    pub(super) enabled: bool,
    pub(super) dac: bool,
    pub(super) length: LengthCounter,
    pub(super) envelope: Envelope,
    /// NR43 bits 7-4.
    pub(super) clock_shift: u8,
    /// NR43 bit 3: 7-bit LFSR mode.
    pub(super) width7: bool,
    /// NR43 bits 2-0.
    pub(super) divisor_code: u8,
    pub(super) timer: u32,
    pub(super) lfsr: u16,
}

impl Noise {
    pub(super) fn new() -> Self {
        Self {
            enabled: false,
            dac: false,
            length: LengthCounter::new(64),
            envelope: Envelope::new(),
            clock_shift: 0,
            width7: false,
            divisor_code: 0,
            timer: 8,
            lfsr: 0x7FFF,
        }
    }

    /// Divisor table: code 0 -> 8, otherwise code * 16 (Pan Docs).
    fn divisor(&self) -> u32 {
        if self.divisor_code == 0 {
            8
        } else {
            u32::from(self.divisor_code) * 16
        }
    }

    /// LFSR clock period in T-cycles.
    pub(super) fn period(&self) -> u32 {
        self.divisor() << self.clock_shift
    }

    pub(super) fn write_nr43(&mut self, value: u8) {
        self.clock_shift = value >> 4;
        self.width7 = value & 0x08 != 0;
        self.divisor_code = value & 0x07;
    }

    pub(super) fn read_nr43(&self) -> u8 {
        (self.clock_shift << 4) | (u8::from(self.width7) << 3) | self.divisor_code
    }

    fn clock_lfsr(&mut self) {
        let xor = (self.lfsr ^ (self.lfsr >> 1)) & 1;
        self.lfsr = (self.lfsr >> 1) | (xor << 14);
        if self.width7 {
            // In 7-bit mode the feedback bit also lands in bit 6.
            self.lfsr = (self.lfsr & !(1 << 6)) | (xor << 6);
        }
    }

    /// Advance one T-cycle.
    pub(super) fn step(&mut self) {
        debug_assert!(
            self.timer > 0,
            "noise frequency timer invariant violated: must stay >= 1"
        );
        self.timer -= 1;
        if self.timer == 0 {
            self.timer = self.period();
            // Clock shifts 14 and 15 give the LFSR no clocks (gbdev wiki,
            // "Game Boy Sound Operation" — Noise channel).
            if self.clock_shift < 14 {
                self.clock_lfsr();
            }
        }
    }

    /// Current digital output, 0-15: bit 0 of the LFSR, inverted.
    pub(super) fn digital(&self) -> u8 {
        if self.enabled && self.lfsr & 1 == 0 {
            self.envelope.volume
        } else {
            0
        }
    }

    pub(super) fn trigger(&mut self) {
        self.enabled = self.dac;
        self.timer = self.period();
        self.envelope.trigger();
        self.lfsr = 0x7FFF;
    }

    pub(super) fn power_off(&mut self, clear_length_counter: bool) {
        self.enabled = false;
        self.dac = false;
        self.clock_shift = 0;
        self.width7 = false;
        self.divisor_code = 0;
        self.length.power_off(clear_length_counter);
        self.envelope.power_off();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nr43_round_trips() {
        let mut n = Noise::new();
        n.write_nr43(0xAD);
        assert_eq!(n.read_nr43(), 0xAD);
        assert_eq!(n.clock_shift, 0xA);
        assert!(n.width7);
        assert_eq!(n.divisor_code, 5);
    }

    #[test]
    fn divisor_table() {
        let mut n = Noise::new();
        for (code, want) in [(0u8, 8u32), (1, 16), (2, 32), (3, 48), (7, 112)] {
            n.divisor_code = code;
            n.clock_shift = 0;
            assert_eq!(n.period(), want);
        }
        n.divisor_code = 1;
        n.clock_shift = 4;
        assert_eq!(n.period(), 16 << 4);
    }

    #[test]
    fn lfsr_first_steps_from_all_ones() {
        let mut n = Noise::new();
        n.lfsr = 0x7FFF;
        // bits 0,1 are 1,1 -> feedback 0: zeros shift in.
        n.clock_lfsr();
        assert_eq!(n.lfsr, 0x3FFF);
        n.clock_lfsr();
        assert_eq!(n.lfsr, 0x1FFF);
        // After 14 clocks only bit 0 remains; feedback becomes 1.
        let mut n = Noise::new();
        for _ in 0..14 {
            n.clock_lfsr();
        }
        assert_eq!(n.lfsr, 0x0001);
        n.clock_lfsr();
        assert_eq!(n.lfsr, 0x4000);
    }

    #[test]
    fn lfsr_15bit_period_is_32767() {
        let mut n = Noise::new();
        let mut count = 0u32;
        loop {
            n.clock_lfsr();
            count += 1;
            if n.lfsr == 0x7FFF {
                break;
            }
            assert!(count < 40000, "LFSR never returned to seed");
        }
        assert_eq!(count, 32767);
    }

    #[test]
    fn lfsr_7bit_period_is_127() {
        let mut n = Noise::new();
        n.width7 = true;
        // Settle into the 7-bit cycle, then measure the period of the full
        // register state.
        for _ in 0..200 {
            n.clock_lfsr();
        }
        let snapshot = n.lfsr;
        let mut count = 0u32;
        loop {
            n.clock_lfsr();
            count += 1;
            if n.lfsr == snapshot {
                break;
            }
            assert!(count < 1000, "no 7-bit cycle found");
        }
        assert_eq!(count, 127);
    }

    #[test]
    fn output_is_inverted_bit0_times_volume() {
        let mut n = Noise::new();
        n.enabled = true;
        n.envelope.volume = 9;
        n.lfsr = 0x7FFE; // bit 0 clear -> output high
        assert_eq!(n.digital(), 9);
        n.lfsr = 0x7FFF; // bit 0 set -> output 0
        assert_eq!(n.digital(), 0);
        n.enabled = false;
        n.lfsr = 0x7FFE;
        assert_eq!(n.digital(), 0);
    }

    #[test]
    fn shift_14_and_15_freeze_lfsr() {
        for shift in [14u8, 15] {
            let mut n = Noise::new();
            n.envelope.write(0xF0);
            n.dac = true;
            n.write_nr43(shift << 4);
            n.trigger();
            for _ in 0..(8u32 << 15) + 16 {
                n.step();
            }
            assert_eq!(n.lfsr, 0x7FFF, "shift {shift}");
        }
    }

    #[test]
    fn step_clocks_lfsr_every_period() {
        let mut n = Noise::new();
        n.envelope.write(0xF0);
        n.dac = true;
        n.write_nr43(0x01); // divisor 16, shift 0
        n.trigger();
        for _ in 0..15 {
            n.step();
        }
        assert_eq!(n.lfsr, 0x7FFF, "no clock before the period elapses");
        n.step();
        assert_eq!(n.lfsr, 0x3FFF);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "noise frequency timer")]
    fn step_with_zero_timer_panics_in_debug() {
        let mut n = Noise::new();
        n.timer = 0; // violates the "timer always >= 1" invariant
        n.step();
    }

    #[test]
    fn trigger_resets_lfsr_to_all_ones() {
        let mut n = Noise::new();
        n.envelope.write(0xF0);
        n.dac = true;
        n.trigger();
        for _ in 0..100 {
            n.step();
        }
        assert_ne!(n.lfsr, 0x7FFF);
        n.trigger();
        assert_eq!(n.lfsr, 0x7FFF);
        assert!(n.enabled);
    }
}
