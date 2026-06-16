//! Length counter: silences a channel after a programmable time.
//!
//! Clocked at 256 Hz by frame-sequencer steps 0/2/4/6. Pulse and noise
//! channels count down from up to 64, the wave channel from up to 256.
//! The infamous "extra length clock" NRx4 edge cases (Blargg dmg_sound
//! 03-trigger, gbdev wiki "Game Boy Sound Operation", Obscure Behavior)
//! are implemented in [`LengthCounter::write_nrx4`].

#[derive(Clone)]
pub(super) struct LengthCounter {
    /// 64 for pulse/noise, 256 for wave.
    pub(super) max: u16,
    pub(super) counter: u16,
    /// NRx4 bit 6.
    pub(super) enabled: bool,
}

impl LengthCounter {
    pub(super) fn new(max: u16) -> Self {
        Self {
            max,
            counter: 0,
            enabled: false,
        }
    }

    /// NRx1 write: the counter is reloaded with `max - data` (caller masks
    /// `data` to 6 bits for pulse/noise channels).
    pub(super) fn load(&mut self, data: u8) {
        debug_assert!(
            u16::from(data) < self.max,
            "length data must be masked by caller"
        );
        self.counter = self.max - u16::from(data);
    }

    /// 256 Hz frame-sequencer clock. Returns true when the counter just hit
    /// zero, i.e. the channel must be disabled. The counter counts whenever
    /// it is enabled and non-zero, regardless of the channel being on.
    pub(super) fn clock(&mut self) -> bool {
        if self.enabled && self.counter > 0 {
            self.counter -= 1;
            self.counter == 0
        } else {
            false
        }
    }

    /// Length-related side effects of writing NRx4. Must be called *before*
    /// the trigger event itself. `next_step_clocks_length` tells whether the
    /// next frame-sequencer step is one of 0/2/4/6.
    ///
    /// Returns true if the channel must be disabled (the "extra length
    /// clock" reached zero without a trigger).
    ///
    /// Hardware (gbdev wiki, Obscure Behavior):
    /// - Enabling the counter (0 -> 1) while the next FS step does not clock
    ///   lengths gives one extra decrement; if that reaches 0 and the write
    ///   does not also trigger, the channel is disabled.
    /// - Triggering with a zero counter reloads it with `max`; if the write
    ///   also enables the counter in the same no-length-clock phase, it is
    ///   loaded with `max - 1` instead.
    pub(super) fn write_nrx4(
        &mut self,
        enable: bool,
        trigger: bool,
        next_step_clocks_length: bool,
    ) -> bool {
        let was_enabled = self.enabled;
        self.enabled = enable;
        let mut disable_channel = false;
        if !next_step_clocks_length && !was_enabled && enable && self.counter > 0 {
            self.counter -= 1;
            if self.counter == 0 && !trigger {
                disable_channel = true;
            }
        }
        if trigger && self.counter == 0 {
            self.counter = self.max;
            if enable && !next_step_clocks_length {
                self.counter -= 1;
            }
        }
        disable_channel
    }

    /// NR52 power-off: the enable flag lives in NRx4 and is always cleared;
    /// the counter itself survives on DMG but is cleared on CGB.
    pub(super) fn power_off(&mut self, clear_counter: bool) {
        self.enabled = false;
        if clear_counter {
            self.counter = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_is_max_minus_data() {
        let mut l = LengthCounter::new(64);
        l.load(0);
        assert_eq!(l.counter, 64);
        l.load(63);
        assert_eq!(l.counter, 1);
        let mut w = LengthCounter::new(256);
        w.load(0);
        assert_eq!(w.counter, 256);
        w.load(255);
        assert_eq!(w.counter, 1);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "length data must be masked by caller")]
    fn load_rejects_unmasked_data_in_debug() {
        let mut l = LengthCounter::new(64);
        l.load(64); // callers must mask to 6 bits first
    }

    #[test]
    fn clock_counts_only_when_enabled_and_nonzero() {
        let mut l = LengthCounter::new(64);
        l.load(62); // counter = 2
        assert!(!l.clock()); // disabled: no count
        assert_eq!(l.counter, 2);
        l.enabled = true;
        assert!(!l.clock());
        assert_eq!(l.counter, 1);
        assert!(l.clock()); // hits zero: disable channel
        assert_eq!(l.counter, 0);
        assert!(!l.clock()); // stays at zero
    }

    /// The canonical extra-clock matrix, phase = next FS step does NOT clock
    /// length. (enable_before, counter, write enable, write trigger) ->
    /// (counter after, channel killed).
    #[test]
    fn nrx4_extra_clock_matrix_no_length_phase() {
        // Enabling with counter 1, no trigger: extra clock kills channel.
        let mut l = LengthCounter::new(64);
        l.counter = 1;
        assert!(l.write_nrx4(true, false, false));
        assert_eq!(l.counter, 0);

        // Enabling with counter 1 + trigger: extra clock to 0, then trigger
        // reloads to max-1 because enable is set in this phase.
        let mut l = LengthCounter::new(64);
        l.counter = 1;
        assert!(!l.write_nrx4(true, true, false));
        assert_eq!(l.counter, 63);

        // Trigger with counter 0, enable clear: plain reload to max.
        let mut l = LengthCounter::new(64);
        assert!(!l.write_nrx4(false, true, false));
        assert_eq!(l.counter, 64);

        // Trigger with counter 0, enable set: reload to max-1.
        let mut l = LengthCounter::new(64);
        assert!(!l.write_nrx4(true, true, false));
        assert_eq!(l.counter, 63);

        // Enabling with counter 2: extra clock to 1, channel survives.
        let mut l = LengthCounter::new(64);
        l.counter = 2;
        assert!(!l.write_nrx4(true, false, false));
        assert_eq!(l.counter, 1);

        // Already enabled: no extra clock (only a 0 -> 1 edge clocks).
        let mut l = LengthCounter::new(64);
        l.counter = 2;
        l.enabled = true;
        assert!(!l.write_nrx4(true, false, false));
        assert_eq!(l.counter, 2);

        // Disabling never clocks.
        let mut l = LengthCounter::new(64);
        l.counter = 1;
        l.enabled = true;
        assert!(!l.write_nrx4(false, false, false));
        assert_eq!(l.counter, 1);

        // Trigger with non-zero counter: no reload.
        let mut l = LengthCounter::new(64);
        l.counter = 10;
        assert!(!l.write_nrx4(false, true, false));
        assert_eq!(l.counter, 10);
    }

    /// Same writes with phase = next FS step DOES clock length: no extra
    /// clocking, plain reloads.
    #[test]
    fn nrx4_no_extra_clock_in_length_phase() {
        let mut l = LengthCounter::new(64);
        l.counter = 1;
        assert!(!l.write_nrx4(true, false, true));
        assert_eq!(l.counter, 1);

        let mut l = LengthCounter::new(64);
        assert!(!l.write_nrx4(true, true, true));
        assert_eq!(l.counter, 64); // no max-1 adjustment

        let mut l = LengthCounter::new(256);
        assert!(!l.write_nrx4(true, true, false));
        assert_eq!(l.counter, 255); // wave variant of max-1
    }

    #[test]
    fn power_off_clears_enable_and_optionally_counter() {
        let mut l = LengthCounter::new(64);
        l.counter = 12;
        l.enabled = true;
        l.power_off(false); // DMG
        assert!(!l.enabled);
        assert_eq!(l.counter, 12);
        l.power_off(true); // CGB
        assert_eq!(l.counter, 0);
    }
}
