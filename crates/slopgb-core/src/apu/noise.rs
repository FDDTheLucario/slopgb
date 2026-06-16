//! Noise channel (channel 4): 15-bit LFSR clocked from a free-running
//! counter.
//!
//! Hardware model (SameBoy apu.c, hardware-verified): the LFSR does NOT
//! have a private period timer — a FREE-RUNNING 14-bit counter increments
//! every `divisor` 2 MHz cycles (divisor = (NR43 & 7) * 4, or 2 for code
//! 0), and the LFSR steps on each RISING edge of counter bit `NR43 >> 4`.
//! A trigger resets the LFSR but NOT the counter (SameSuite
//! channel_4_equivalent_frequencies / channel_4_frequency_alignment: equal
//! effective rates stay phase-identical across NR43 writes), so once the
//! channel has been started the noise phase is a machine-global property.
//! The LFSR itself is SameBoy's polarity: seeded 0, inverted feedback
//! `(lfsr ^ lfsr>>1 ^ 1) & 1` into bit 14 (and bit 6 in 7-bit mode), output
//! is bit 0 (set = volume).

use super::envelope::Envelope;
use super::length::LengthCounter;

#[derive(Clone)]
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
    /// Free-running 14-bit counter (SameBoy `noise_channel.counter`).
    pub(super) counter: u16,
    /// 2 MHz cycles until the next counter increment.
    pub(super) counter_countdown: u16,
    /// The counter runs from the first trigger after APU power-on even
    /// while the channel itself is silent (SameBoy
    /// `noise_background_counter_active`).
    pub(super) background_counting: bool,
    /// NR42's DAC bits were non-zero at the last trigger (SameBoy
    /// `noise_counter_active`; reset by power-off and DAC-off).
    pub(super) counter_active: bool,
    /// The last trigger happened with the DAC disabled (SameBoy
    /// `noise_started_with_dac_disabled`).
    started_with_dac_disabled: bool,
    /// The counter incremented at least once since the last trigger.
    pub(super) did_step_counter: bool,
    /// The last 2 MHz cycle processed reloaded the countdown (a counter
    /// increment happened on it).
    pub(super) countdown_reloaded: bool,
    /// Free-running 2 MHz cycle counter, low bits only (SameBoy
    /// `noise_channel.alignment`); the trigger corrections key on it.
    pub(super) alignment: u8,
    /// DMG only: a trigger landing at `alignment & 3 != 0` is deferred this
    /// many 2 MHz cycles (SameBoy `dmg_delayed_start`).
    dmg_delayed_start: u8,
    pub(super) lfsr: u16,
    /// LFSR bit 0 latched at the last LFSR step.
    pub(super) current_sample: u8,
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
            counter: 0,
            counter_countdown: 0,
            background_counting: false,
            counter_active: false,
            started_with_dac_disabled: false,
            did_step_counter: false,
            countdown_reloaded: false,
            alignment: 0,
            dmg_delayed_start: 0,
            lfsr: 0,
            current_sample: 0,
        }
    }

    /// Counter increment period in 2 MHz cycles: `(NR43 & 7) * 4`, with
    /// divisor code 0 counting as 2 (SameBoy GB_apu_run).
    fn divisor_2mhz(&self) -> u16 {
        let d = u16::from(self.divisor_code) << 2;
        if d == 0 { 2 } else { d }
    }

    pub(super) fn write_nr43(&mut self, value: u8) {
        // A write landing on a counter-increment cycle reloads the
        // countdown from the new divisor with an alignment-dependent
        // offset — {2,1,4,3}[alignment & 3] on CGB ≤ C and DMG (SameBoy
        // GB_IO_NR43 handler; the > C table is {2,1,0,3}; `Model::Cgb` is
        // CPU CGB C per docs/ARCHITECTURE.md §CGB revision policy).
        if self.countdown_reloaded {
            let d = u16::from(value & 7) << 2;
            let divisor = if d == 0 { 2 } else { d };
            self.counter_countdown = divisor
                + if divisor == 2 {
                    0
                } else {
                    [2, 1, 4, 3][usize::from(self.alignment & 3)]
                };
        }
        // ≤C: a same-cycle write whose shift change makes the tapped bit
        // rise while counter bit 7 is set steps the LFSR once more if the
        // previous counter value shows a matching falling edge (SameBoy
        // GB_IO_NR43 handler, ≤C branch).
        if self.countdown_reloaded {
            let old_bit = (self.counter >> self.clock_shift) & 1 != 0;
            let glitch_bit = (self.counter >> 7) & 1 != 0;
            let new_bit = (self.counter >> (value >> 4)) & 1 != 0;
            if !old_bit && new_bit && glitch_bit {
                let prev = self.counter.wrapping_sub(1) & 0x3FFF;
                let old_bit = (prev >> self.clock_shift) & 1 != 0;
                let glitch_bit = (prev >> 7) & 1 != 0;
                let new_bit = (prev >> (value >> 4)) & 1 != 0;
                if old_bit && !new_bit && glitch_bit {
                    self.step_lfsr();
                }
            }
        }
        // The ≤C double-write-through-$FF LFSR corruption tables (SameBoy
        // nr43_write) are explicitly documented upstream as revision- and
        // unit-specific with non-deterministic variants; only the
        // deterministic paths above are modelled.
        self.clock_shift = value >> 4;
        self.width7 = value & 0x08 != 0;
        self.divisor_code = value & 0x07;
    }

    pub(super) fn read_nr43(&self) -> u8 {
        (self.clock_shift << 4) | (u8::from(self.width7) << 3) | self.divisor_code
    }

    /// One LFSR step: inverted-feedback 15-bit shift (SameBoy `step_lfsr`),
    /// latching bit 0 as the output sample.
    fn step_lfsr(&mut self) {
        let high_mask: u16 = if self.width7 { 0x4040 } else { 0x4000 };
        let new_high = (self.lfsr ^ (self.lfsr >> 1) ^ 1) & 1 == 1;
        self.lfsr >>= 1;
        if new_high {
            self.lfsr |= high_mask;
        } else {
            // Not redundant: relevant when switching LFSR widths.
            self.lfsr &= !high_mask;
        }
        self.current_sample = (self.lfsr & 1) as u8;
    }

    /// Advance one 2 MHz cycle.
    pub(super) fn step(&mut self) {
        self.alignment = self.alignment.wrapping_add(1);
        if self.dmg_delayed_start > 0 {
            self.dmg_delayed_start -= 1;
            if self.dmg_delayed_start == 0 {
                self.start();
            }
        }
        if !(self.counter_active || self.background_counting) {
            return;
        }
        let divisor = self.divisor_2mhz();
        if self.counter_countdown == 0 {
            self.counter_countdown = divisor;
        }
        if self.counter_countdown == 1 {
            self.counter_countdown = divisor;
            let mask = 1u16 << self.clock_shift;
            let old_bit = self.counter & mask != 0;
            self.counter = (self.counter + 1) & 0x3FFF;
            self.did_step_counter = true;
            let new_bit = self.counter & mask != 0;
            // Clock shifts 14/15 tap above the 14-bit counter: no edges,
            // the LFSR freezes (Pan Docs: "no clocks").
            if new_bit && !old_bit && self.enabled {
                self.step_lfsr();
            }
            self.countdown_reloaded = true;
        } else {
            self.counter_countdown -= 1;
            self.countdown_reloaded = false;
        }
    }

    /// Current digital output, 0-15: the latched LFSR bit times the live
    /// envelope volume.
    pub(super) fn digital(&self) -> u8 {
        if self.enabled && self.current_sample == 1 {
            self.envelope.volume
        } else {
            0
        }
    }

    /// NR44 trigger. On DMG a trigger at `alignment & 3 != 0` is deferred
    /// by 6 2 MHz cycles (SameBoy GB_IO_NR44 handler `dmg_delayed_start`);
    /// everything else starts immediately.
    pub(super) fn trigger(&mut self, dmg: bool, double_speed: bool) {
        if dmg && self.alignment & 3 != 0 {
            self.dmg_delayed_start = 6;
        } else {
            self.start_with_speed(double_speed);
        }
    }

    /// The deferred DMG start path never runs in double speed (KEY1 is
    /// CGB-only), so the plain start uses single-speed corrections.
    fn start(&mut self) {
        self.start_with_speed(false);
    }

    /// Port of SameBoy `prepare_noise_start` (+ the NR44 trigger body), ≤C
    /// deterministic paths: reset the LFSR (NOT the counter) and reload the
    /// countdown with the alignment corrections measured on hardware.
    fn start_with_speed(&mut self, ds: bool) {
        let was_active = self.enabled;
        self.enabled = self.dac;
        self.envelope.trigger();

        self.counter_active = self.dac;
        let was_started_with_dac_disabled = self.started_with_dac_disabled;
        self.started_with_dac_disabled = !self.counter_active;
        let was_background = self.background_counting;
        self.background_counting = true;
        let mut divisor = u16::from(self.divisor_code);
        let mut instant_step = false;
        let mut div_1_glitch = false;

        if divisor > 1 && self.counter_countdown == 1 {
            self.counter = (self.counter + 1) & 0x3FFF;
        } else if divisor > 1 && self.counter_countdown == 2 && was_active && ds {
            // ≤C double-speed restart quirk.
            self.counter = (self.counter + 1) & 0x3FFF;
        } else if self.counter_countdown == 2 && self.alignment & 3 == 0 && was_active {
            if divisor == 0 {
                divisor = 8;
            } else if divisor == 1 {
                if !self.did_step_counter {
                    div_1_glitch = true;
                }
                let mask = 1u16 << self.clock_shift;
                let old_bit = self.counter & mask != 0;
                self.counter = (self.counter + 1) & 0x3FFF;
                let new_bit = self.counter & mask != 0;
                if new_bit && !old_bit {
                    instant_step = true;
                }
            }
        }
        self.counter_countdown = if divisor == 0 { 6 } else { divisor * 4 + 6 };
        if self.alignment & 1 == 1 {
            if divisor == 0 {
                // ≤C takes the +1 branch unconditionally.
                self.counter_countdown += 1;
            } else if self.alignment & 2 != 0 {
                if divisor == 1 && !was_active {
                    self.counter_countdown += 1;
                } else {
                    self.counter_countdown = self.counter_countdown.wrapping_sub(3);
                }
            } else {
                self.counter_countdown = self.counter_countdown.wrapping_sub(1);
                if divisor == 1 && was_active {
                    self.counter_countdown = self.counter_countdown.wrapping_sub(4);
                }
            }
        } else if divisor != 0 {
            if self.alignment & 2 != 0 {
                if ds && divisor == 1 {
                    // ≤C double-speed sign flip.
                    self.counter_countdown += 2;
                } else {
                    self.counter_countdown = self.counter_countdown.wrapping_sub(2);
                }
            } else if (divisor > 1 && !ds) || (divisor == 1 && was_active && self.clock_shift == 0)
            {
                // Two distinct hardware conditions upstream (SameBoy keeps
                // them apart with a "way too specific" note), same -4.
                self.counter_countdown = self.counter_countdown.wrapping_sub(4);
            }
        } else if ds {
            // ≤C double-speed, divisor 0.
            self.counter_countdown += 2;
        }

        // Background counting glitches.
        if divisor > 1 {
            if !self.counter_active && self.alignment & 3 == 0 {
                self.counter_countdown += 4;
            }
        } else if was_background && !was_active && self.alignment & 3 == 0 {
            if divisor == 0 {
                if was_started_with_dac_disabled {
                    self.counter_countdown += 28;
                }
            } else {
                self.counter_countdown = self.counter_countdown.wrapping_sub(4);
            }
        }
        // ≤C double-speed background-counting drift.
        if divisor == 0 && was_background && !was_active && ds {
            self.counter_countdown = self.counter_countdown.wrapping_sub(1);
        }
        if div_1_glitch {
            self.counter_countdown = self.counter_countdown.wrapping_sub(4);
        }
        // The correction branches above are mutually exclusive in ways
        // that keep the countdown non-negative (`wrapping_sub` mirrors
        // SameBoy's uint16 arithmetic); catch an unreachable combination
        // early instead of free-running for ~65k cycles.
        debug_assert!(
            self.counter_countdown < 0x8000,
            "noise trigger corrections underflowed the countdown"
        );

        if divisor == 0 && was_active && self.alignment & 3 == 3 {
            // SameBoy: "no clue where this number comes from, but ...
            // confirmed for this edge case".
            self.lfsr = 0x0055;
        } else {
            self.lfsr = 0;
        }
        self.current_sample = 0;
        self.did_step_counter = self.alignment & 3 == 2;
        if instant_step {
            self.step_lfsr();
        }
    }

    pub(super) fn power_off(&mut self, clear_length_counter: bool) {
        let length = std::mem::replace(&mut self.length, LengthCounter::new(64));
        *self = Self::new();
        self.length = length;
        self.length.power_off(clear_length_counter);
    }
}

// --- Save state (see `crate::state`) ---
impl Noise {
    pub(super) fn write_state(&self, w: &mut crate::state::Writer) {
        w.bool(self.enabled);
        w.bool(self.dac);
        self.length.write_state(w);
        self.envelope.write_state(w);
        w.u8(self.clock_shift);
        w.bool(self.width7);
        w.u8(self.divisor_code);
        w.u16(self.counter);
        w.u16(self.counter_countdown);
        w.bool(self.background_counting);
        w.bool(self.counter_active);
        w.bool(self.started_with_dac_disabled);
        w.bool(self.did_step_counter);
        w.bool(self.countdown_reloaded);
        w.u8(self.alignment);
        w.u8(self.dmg_delayed_start);
        w.u16(self.lfsr);
        w.u8(self.current_sample);
    }
    pub(super) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.enabled = r.bool()?;
        self.dac = r.bool()?;
        self.length.read_state(r)?;
        self.envelope.read_state(r)?;
        self.clock_shift = r.u8()?;
        self.width7 = r.bool()?;
        self.divisor_code = r.u8()?;
        self.counter = r.u16()?;
        self.counter_countdown = r.u16()?;
        self.background_counting = r.bool()?;
        self.counter_active = r.bool()?;
        self.started_with_dac_disabled = r.bool()?;
        self.did_step_counter = r.bool()?;
        self.countdown_reloaded = r.bool()?;
        self.alignment = r.u8()?;
        self.dmg_delayed_start = r.u8()?;
        self.lfsr = r.u16()?;
        self.current_sample = r.u8()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing_noise(nr43: u8) -> Noise {
        let mut n = Noise::new();
        n.envelope.write(0xF0); // volume 15
        n.dac = true;
        n.write_nr43(nr43);
        n.trigger(false, false);
        n
    }

    /// Step until the LFSR changes; returns the 2 MHz cycle count taken.
    fn cycles_to_lfsr_step(n: &mut Noise) -> u32 {
        let lfsr = n.lfsr;
        let mut c = 0;
        while n.lfsr == lfsr {
            n.step();
            c += 1;
            assert!(c < 100_000, "LFSR never stepped");
        }
        c
    }

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
    fn lfsr_steps_on_rising_edge_of_tapped_counter_bit() {
        // Divisor code 1 (4 2MHz cycles per counter increment), shift 0:
        // bit 0 of the counter rises every second increment — the LFSR
        // steps every 8 cycles in steady state.
        let mut n = playing_noise(0x01);
        cycles_to_lfsr_step(&mut n); // consume the trigger-time alignment
        assert_eq!(cycles_to_lfsr_step(&mut n), 8);
        assert_eq!(cycles_to_lfsr_step(&mut n), 8);
        // Effective LFSR rate: divisor * 2^(shift+1) cycles. Shift 1
        // doubles it.
        let mut n = playing_noise(0x11);
        cycles_to_lfsr_step(&mut n);
        assert_eq!(cycles_to_lfsr_step(&mut n), 16);
    }

    #[test]
    fn lfsr_15bit_sequence_from_zero_seed() {
        // SameBoy polarity: seed 0, inverted feedback into bit 14, output
        // bit 0 (set = audible). First step turns bit 14 on; ones reach
        // bit 0 after 15 steps.
        let mut n = Noise::new();
        n.enabled = true;
        n.step_lfsr();
        assert_eq!(n.lfsr, 0x4000);
        n.step_lfsr();
        assert_eq!(n.lfsr, 0x6000);
        let mut n = Noise::new();
        n.enabled = true;
        for _ in 0..14 {
            n.step_lfsr();
        }
        assert_eq!(n.lfsr, 0x7FFE);
        assert_eq!(n.current_sample, 0);
        n.step_lfsr();
        assert_eq!(n.current_sample, 1, "ones reach bit 0 after 15 steps");
    }

    #[test]
    fn lfsr_15bit_period_is_32767() {
        let mut n = Noise::new();
        n.step_lfsr(); // leave the all-zero state
        let snapshot = n.lfsr;
        let mut count = 0u32;
        loop {
            n.step_lfsr();
            count += 1;
            if n.lfsr == snapshot {
                break;
            }
            assert!(count < 40_000, "LFSR never returned to snapshot");
        }
        assert_eq!(count, 32767);
    }

    #[test]
    fn lfsr_7bit_period_is_127() {
        let mut n = Noise::new();
        n.width7 = true;
        for _ in 0..200 {
            n.step_lfsr();
        }
        let snapshot = n.lfsr;
        let mut count = 0u32;
        loop {
            n.step_lfsr();
            count += 1;
            if n.lfsr == snapshot {
                break;
            }
            assert!(count < 1000, "no 7-bit cycle found");
        }
        assert_eq!(count, 127);
    }

    #[test]
    fn output_is_latched_bit0_times_volume() {
        let mut n = Noise::new();
        n.enabled = true;
        n.envelope.write(0x90);
        n.envelope.trigger(); // volume 9
        n.lfsr = 0x7FFE;
        n.step_lfsr(); // bit 0 becomes 1 (15 ones case ends in bit 0 set)
        assert_eq!(n.current_sample, 1);
        assert_eq!(n.digital(), 9);
        n.enabled = false;
        assert_eq!(n.digital(), 0);
    }

    #[test]
    fn shift_14_and_15_freeze_lfsr() {
        // The tapped bit sits above the 14-bit counter: no rising edges.
        for shift in [14u8, 15] {
            let mut n = playing_noise(shift << 4);
            for _ in 0..(8u32 << 15) + 16 {
                n.step();
            }
            assert_eq!(n.lfsr, 0, "shift {shift}");
        }
    }

    #[test]
    fn trigger_resets_lfsr_but_not_the_counter() {
        // SameSuite channel_4_equivalent_frequencies /
        // channel_4_frequency_alignment: the 14-bit counter free-runs
        // through triggers — only the LFSR restarts.
        // 99 steps leaves the countdown mid-period so none of the
        // countdown-boundary retrigger quirks fire.
        let mut n = playing_noise(0x01);
        for _ in 0..99 {
            n.step();
        }
        let counter = n.counter;
        assert_ne!(n.lfsr, 0);
        assert!(counter > 0);
        n.trigger(false, false);
        assert_eq!(n.lfsr, 0);
        assert_eq!(n.counter, counter, "counter must keep free-running");
    }

    #[test]
    fn counter_keeps_running_while_channel_is_silent() {
        // Once started, the counter runs in the background even after the
        // channel dies (SameBoy noise_background_counter_active).
        let mut n = playing_noise(0x01);
        for _ in 0..50 {
            n.step();
        }
        n.enabled = false; // e.g. length expiry
        let counter = n.counter;
        for _ in 0..64 {
            n.step();
        }
        assert!(n.counter != counter, "background counting continues");
        let lfsr = n.lfsr;
        for _ in 0..64 {
            n.step();
        }
        assert_eq!(n.lfsr, lfsr, "but the LFSR only steps while enabled");
    }

    #[test]
    fn dmg_trigger_is_delayed_6_cycles_when_misaligned() {
        // SameBoy GB_IO_NR44: on DMG a trigger at alignment & 3 != 0 defers
        // the actual start by 6 2 MHz cycles.
        let mut n = Noise::new();
        n.envelope.write(0xF0);
        n.dac = true;
        n.write_nr43(0x01);
        n.step();
        n.step(); // alignment = 2
        n.trigger(true, false);
        assert!(!n.background_counting, "start deferred");
        for _ in 0..5 {
            n.step();
        }
        assert!(!n.background_counting, "still deferred");
        n.step();
        assert!(n.background_counting, "started after 6 cycles");
    }

    #[test]
    fn power_off_resets_noise_state_but_keeps_dmg_length() {
        let mut n = playing_noise(0x35);
        n.length.load(64 - 12);
        for _ in 0..100 {
            n.step();
        }
        n.power_off(false);
        assert!(!n.enabled);
        assert!(!n.background_counting);
        assert_eq!(n.counter, 0);
        assert_eq!(n.alignment, 0);
        assert_eq!(n.read_nr43(), 0);
        assert_eq!(n.length.counter, 12, "DMG keeps the length counter");
        let mut n = playing_noise(0x35);
        n.length.load(64 - 12);
        n.power_off(true);
        assert_eq!(n.length.counter, 0, "CGB clears it");
    }
}
