//! Pulse (square wave) channels 1 and 2. Channel 1 additionally has the
//! frequency sweep unit (NR10); channel 2's sweep state simply stays inert
//! because no register write ever reaches it.

use super::envelope::Envelope;
use super::length::LengthCounter;

/// Duty waveforms, indexed `[duty][position]` (Pan Docs "Sound Channel 1").
/// 0: 12.5% `00000001`, 1: 25% `10000001`, 2: 50% `10000111`, 3: 75% `01111110`.
pub(super) const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 0],
];

pub(super) struct Pulse {
    pub(super) enabled: bool,
    pub(super) dac: bool,
    /// NRx1 bits 7-6.
    pub(super) duty: u8,
    /// 11-bit frequency from NRx3/NRx4.
    pub(super) freq: u16,
    pub(super) length: LengthCounter,
    pub(super) envelope: Envelope,
    /// T-cycles until the duty position advances (always >= 1).
    pub(super) timer: u32,
    pub(super) duty_pos: u8,
    // Sweep unit (channel 1 only).
    pub(super) sweep_period: u8,
    pub(super) sweep_negate: bool,
    pub(super) sweep_shift: u8,
    pub(super) sweep_timer: u8,
    pub(super) sweep_enabled: bool,
    pub(super) sweep_shadow: u16,
    /// At least one frequency calculation used negate mode since the last
    /// trigger. Clearing negate afterwards disables the channel.
    pub(super) sweep_negate_used: bool,
}

impl Pulse {
    pub(super) fn new() -> Self {
        Self {
            enabled: false,
            dac: false,
            duty: 0,
            freq: 0,
            length: LengthCounter::new(64),
            envelope: Envelope::new(),
            // period() at frequency 0: (2048 - 0) * 4.
            timer: 8192,
            duty_pos: 0,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_timer: 0,
            sweep_enabled: false,
            sweep_shadow: 0,
            sweep_negate_used: false,
        }
    }

    /// Duty step period in T-cycles.
    pub(super) fn period(&self) -> u32 {
        (2048 - u32::from(self.freq)) * 4
    }

    /// Advance one T-cycle.
    pub(super) fn step(&mut self) {
        debug_assert!(
            self.timer > 0,
            "pulse frequency timer invariant violated: must stay >= 1"
        );
        self.timer -= 1;
        if self.timer == 0 {
            self.timer = self.period();
            self.duty_pos = (self.duty_pos + 1) & 7;
        }
    }

    /// Current digital output, 0-15.
    pub(super) fn digital(&self) -> u8 {
        if self.enabled && DUTY_TABLE[usize::from(self.duty)][usize::from(self.duty_pos)] == 1 {
            self.envelope.volume
        } else {
            0
        }
    }

    pub(super) fn write_nr10(&mut self, value: u8) {
        let negate = value & 0x08 != 0;
        // Clearing negate after at least one negate-mode calculation since
        // the last trigger immediately disables the channel (gbdev wiki,
        // Obscure Behavior; Blargg dmg_sound 05-sweep details).
        if self.sweep_negate && !negate && self.sweep_negate_used {
            self.enabled = false;
        }
        self.sweep_period = (value >> 4) & 7;
        self.sweep_negate = negate;
        self.sweep_shift = value & 7;
    }

    pub(super) fn read_nr10(&self) -> u8 {
        0x80 | (self.sweep_period << 4) | (u8::from(self.sweep_negate) << 3) | self.sweep_shift
    }

    /// New frequency from the shadow register. Marks negate as used.
    fn sweep_calc(&mut self) -> u16 {
        let delta = self.sweep_shadow >> self.sweep_shift;
        if self.sweep_negate {
            self.sweep_negate_used = true;
            self.sweep_shadow - delta
        } else {
            self.sweep_shadow + delta
        }
    }

    /// 128 Hz frame-sequencer clock (steps 2 and 6).
    pub(super) fn sweep_clock(&mut self) {
        if self.sweep_timer > 0 {
            self.sweep_timer -= 1;
        }
        if self.sweep_timer == 0 {
            // Timer treats period 0 as 8, but no frequency updates happen.
            self.sweep_timer = if self.sweep_period == 0 {
                8
            } else {
                self.sweep_period
            };
            if self.sweep_enabled && self.sweep_period != 0 {
                let f = self.sweep_calc();
                if f > 2047 {
                    self.enabled = false;
                } else if self.sweep_shift != 0 {
                    // Write back to shadow and the channel frequency, then
                    // run the overflow check again without writing back.
                    self.sweep_shadow = f;
                    self.freq = f;
                    if self.sweep_calc() > 2047 {
                        self.enabled = false;
                    }
                }
            }
        }
    }

    pub(super) fn trigger(&mut self) {
        self.enabled = self.dac;
        // The low two bits of the frequency timer are NOT modified by a
        // trigger (gbdev wiki, Obscure Behavior). `period()` is a multiple
        // of 4, so OR-ing the old low bits in is exact.
        self.timer = self.period() | (self.timer & 3);
        self.envelope.trigger();
        // Sweep init (Pan Docs / gbdev wiki):
        self.sweep_shadow = self.freq;
        self.sweep_timer = if self.sweep_period == 0 {
            8
        } else {
            self.sweep_period
        };
        self.sweep_enabled = self.sweep_period != 0 || self.sweep_shift != 0;
        self.sweep_negate_used = false;
        // With a non-zero shift the overflow check runs immediately.
        if self.sweep_shift != 0 && self.sweep_calc() > 2047 {
            self.enabled = false;
        }
    }

    pub(super) fn power_off(&mut self, clear_length_counter: bool) {
        self.enabled = false;
        self.dac = false;
        self.duty = 0;
        self.freq = 0;
        self.length.power_off(clear_length_counter);
        self.envelope.power_off();
        self.sweep_period = 0;
        self.sweep_negate = false;
        self.sweep_shift = 0;
        self.sweep_timer = 0;
        self.sweep_enabled = false;
        self.sweep_shadow = 0;
        self.sweep_negate_used = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing_pulse(duty: u8, freq: u16) -> Pulse {
        let mut p = Pulse::new();
        p.duty = duty;
        p.freq = freq;
        p.envelope.write(0xF0); // volume 15
        p.dac = true;
        p.trigger();
        p
    }

    /// Step one full duty period and return the output level (0 or 15).
    fn next_duty_output(p: &mut Pulse) -> u8 {
        for _ in 0..p.period() {
            p.step();
        }
        p.digital()
    }

    #[test]
    fn duty_sequences_match_hardware_table() {
        for (duty, want) in [
            (0u8, [0u8, 0, 0, 0, 0, 0, 1, 0]),
            (1, [0, 0, 0, 0, 0, 0, 1, 1]),
            (2, [0, 0, 0, 0, 1, 1, 1, 1]),
            (3, [1, 1, 1, 1, 1, 1, 0, 0]),
        ] {
            // Trigger does not reset duty_pos; fresh channel starts at 0,
            // so the first observed step is position 1.
            let mut p = playing_pulse(duty, 2047);
            let got: Vec<u8> = (0..8).map(|_| next_duty_output(&mut p) / 15).collect();
            // `want` is DUTY_TABLE[duty] rotated left by one.
            assert_eq!(got, want, "duty {duty}");
        }
    }

    #[test]
    fn frequency_timer_period_is_2048_minus_f_times_4() {
        let mut p = playing_pulse(0, 2046);
        assert_eq!(p.period(), 8);
        let start = p.duty_pos;
        for _ in 0..7 {
            p.step();
        }
        assert_eq!(p.duty_pos, start, "no advance before the period elapses");
        p.step();
        assert_eq!(p.duty_pos, (start + 1) & 7);
    }

    #[test]
    fn trigger_preserves_frequency_timer_low_bits() {
        let mut p = playing_pulse(0, 2040);
        // Walk the timer to a value with non-zero low bits.
        p.step();
        p.step();
        p.step();
        let low = p.timer & 3;
        assert_ne!(low, 0);
        p.trigger();
        assert_eq!(p.timer, p.period() | low);
    }

    #[test]
    fn trigger_does_not_reset_duty_position() {
        let mut p = playing_pulse(2, 2047);
        for _ in 0..3 {
            next_duty_output(&mut p);
        }
        let pos = p.duty_pos;
        p.trigger();
        assert_eq!(p.duty_pos, pos);
    }

    #[test]
    fn disabled_channel_outputs_zero() {
        let mut p = playing_pulse(3, 1000);
        // duty 3 position 0 outputs 0; advance to a high position.
        next_duty_output(&mut p);
        assert_eq!(p.digital(), 15);
        p.enabled = false;
        assert_eq!(p.digital(), 0);
    }

    #[test]
    fn sweep_trigger_overflow_check_only_with_nonzero_shift() {
        // freq 1920 + (1920 >> 1) = 2880 > 2047: dies on trigger.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 1920;
        p.write_nr10(0x11); // period 1, shift 1
        p.trigger();
        assert!(!p.enabled);

        // Same with shift 0: no immediate check, channel stays on.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 1920;
        p.write_nr10(0x10);
        p.trigger();
        assert!(p.enabled);
    }

    #[test]
    fn sweep_clock_updates_frequency_and_runs_second_check() {
        // 1024 -> 1536 (ok), again-check 1536 + 768 = 2304 overflows:
        // the new frequency is written but the channel dies immediately.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 1024;
        p.write_nr10(0x11); // period 1, shift 1
        p.trigger();
        assert!(p.enabled);
        p.sweep_clock();
        assert_eq!(p.freq, 1536);
        assert!(!p.enabled, "second overflow check must disable");

        // 256 -> 320 with shift 2, again-check 400: survives.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 256;
        p.write_nr10(0x12);
        p.trigger();
        p.sweep_clock();
        assert_eq!(p.freq, 320);
        assert!(p.enabled);
    }

    #[test]
    fn sweep_negate_mode_subtracts() {
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 1024;
        p.write_nr10(0x19); // period 1, negate, shift 1
        p.trigger();
        p.sweep_clock();
        assert_eq!(p.freq, 512);
        assert!(p.enabled);
    }

    #[test]
    fn clearing_negate_after_negate_calc_disables_channel() {
        // Trigger with shift != 0 performs a negate-mode calculation.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 1024;
        p.write_nr10(0x19);
        p.trigger();
        assert!(p.enabled);
        p.write_nr10(0x11); // clear negate
        assert!(!p.enabled);
    }

    #[test]
    fn clearing_negate_without_any_calc_keeps_channel() {
        // Shift 0: trigger does not calculate, so negate was never used.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 1024;
        p.write_nr10(0x18); // period 1, negate, shift 0
        p.trigger();
        p.write_nr10(0x10);
        assert!(p.enabled);
    }

    #[test]
    fn negate_calc_on_sweep_tick_with_shift_zero_counts() {
        // Sweep clocks calculate (for the overflow check) even with shift 0.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 1024;
        p.write_nr10(0x18); // period 1, negate, shift 0
        p.trigger();
        p.sweep_clock(); // negate-mode calculation happens here
        assert!(p.enabled);
        p.write_nr10(0x10);
        assert!(!p.enabled);
    }

    #[test]
    fn sweep_period_zero_never_updates_frequency() {
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 512;
        p.write_nr10(0x01); // period 0, shift 1
        p.trigger();
        for _ in 0..32 {
            p.sweep_clock();
        }
        assert_eq!(p.freq, 512);
        assert!(p.enabled);
    }

    #[test]
    fn sweep_timer_uses_8_when_period_zero() {
        // Trigger with period 0 loads the timer with 8; raising the period
        // afterwards doesn't reload it, so the first update lands on the
        // 8th sweep clock.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 512;
        p.write_nr10(0x01); // period 0, shift 1
        p.trigger();
        p.write_nr10(0x11); // period 1, shift 1
        for i in 0..7 {
            p.sweep_clock();
            assert_eq!(p.freq, 512, "no update on sweep clock {i}");
        }
        p.sweep_clock();
        assert_eq!(p.freq, 768);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "pulse frequency timer")]
    fn step_with_zero_timer_panics_in_debug() {
        let mut p = Pulse::new();
        p.timer = 0; // violates the "timer always >= 1" invariant
        p.step();
    }

    #[test]
    fn trigger_with_dac_off_leaves_channel_disabled() {
        let mut p = Pulse::new();
        p.envelope.write(0x00);
        p.dac = p.envelope.dac_enabled();
        p.trigger();
        assert!(!p.enabled);
    }
}
