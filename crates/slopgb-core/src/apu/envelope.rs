//! Volume envelope (NRx2) shared by the pulse and noise channels.
//!
//! Hardware model (SameBoy apu.c `GB_apu_div_event` /
//! `GB_apu_div_secondary_event` / `set_envelope_clock`, hardware-verified):
//! the envelope is not a simple "clock at frame-sequencer step 7" — a
//! per-channel 3-bit countdown decrements on DIV-APU events where
//! `(div_divider & 7) == 7`; once it reaches 0 the volume tick is *armed*
//! at the next RISING edge of the DIV-APU bit (the "secondary event",
//! timing.c: falling edge → primary event, rising edge → secondary) which
//! also reloads the countdown, and the tick itself fires on the following
//! falling-edge event. Arming at a volume rail (15 increasing / 0
//! decreasing) locks the envelope until the next trigger — that is how the
//! hardware clamps without a comparator.

pub(super) struct Envelope {
    /// NRx2 bits 7-4.
    pub(super) initial_volume: u8,
    /// NRx2 bit 3: 1 = increase.
    pub(super) add_mode: bool,
    /// NRx2 bits 2-0.
    pub(super) period: u8,
    /// Current output volume, 0-15.
    pub(super) volume: u8,
    /// 3-bit countdown in 64 Hz units (SameBoy `volume_countdown`).
    pub(super) countdown: u8,
    /// Tick armed by the secondary event, consumed by the next DIV event.
    clock: bool,
    /// Set at arming when the volume sits at the direction's rail; merged
    /// into `locked` when the armed tick is consumed.
    should_lock: bool,
    /// Envelope frozen until the next trigger.
    locked: bool,
}

impl Envelope {
    pub(super) fn new() -> Self {
        Self {
            initial_volume: 0,
            add_mode: false,
            period: 0,
            volume: 0,
            countdown: 0,
            clock: false,
            should_lock: false,
            locked: false,
        }
    }

    pub(super) fn write(&mut self, value: u8) {
        self.initial_volume = value >> 4;
        self.add_mode = value & 0x08 != 0;
        self.period = value & 0x07;
    }

    pub(super) fn read(&self) -> u8 {
        (self.initial_volume << 4) | (u8::from(self.add_mode) << 3) | self.period
    }

    /// NRx2 high 5 bits non-zero <=> the channel's DAC is powered.
    pub(super) fn dac_enabled(&self) -> bool {
        self.initial_volume != 0 || self.add_mode
    }

    /// NRx4 trigger: reload volume and countdown, drop any armed tick and
    /// clear the rail lock (SameBoy NRx4 trigger:
    /// `envelope_clock.locked = false; clock = false;
    /// current_volume = nrx2 >> 4; volume_countdown = nrx2 & 7`).
    pub(super) fn trigger(&mut self) {
        self.volume = self.initial_volume;
        self.countdown = self.period;
        self.clock = false;
        self.should_lock = false;
        self.locked = false;
    }

    /// SameBoy `set_envelope_clock`: arming records whether the volume sits
    /// at the direction's rail; disarming merges that into the lock.
    fn set_clock(&mut self, value: bool) {
        if self.clock == value {
            return;
        }
        if value {
            self.clock = true;
            self.should_lock =
                (self.volume == 0xF && self.add_mode) || (self.volume == 0 && !self.add_mode);
        } else {
            self.clock = false;
            self.locked |= self.should_lock;
        }
    }

    /// NRx2 write while the channel is active — envelope "zombie mode".
    ///
    /// Port of SameBoy apu.c `_nrx2_glitch` (hardware-verified): the
    /// envelope's counter wiring reacts to the written value immediately —
    /// conditional ±1 ticks, two inversion forms (`v ^ 0xF` / `0xE - v`)
    /// keyed on direction-bit changes, and an armed-clock countdown reload.
    /// Applied ONCE: the SameSuite channel_1_volume expectation table
    /// (CGB-E-verified, run on `Model::Cgb` per docs/ARCHITECTURE.md §CGB
    /// revision policy) matches SameBoy's single-application (D/E) branch;
    /// its ≤C double-application through an intermediate $FF write does
    /// not, and pre-CGB $x0 writes are per-unit non-deterministic upstream
    /// (apu.c comment), so the deterministic single form is used for every
    /// model.
    pub(super) fn write_active(&mut self, value: u8) {
        let old = self.read();
        if self.clock {
            self.countdown = value & 7;
        }
        let mut should_tick = (value & 7) != 0 && (old & 7) == 0 && !self.locked;
        let should_invert = ((value ^ old) & 8) != 0;
        // "The weird and over-the-top way clocks for this counter are
        // connected" (SameBoy): an $x8 -> $x8 write ticks too.
        if (value & 0xF) == 8 && (old & 0xF) == 8 && !self.locked {
            should_tick = true;
        }
        if should_invert {
            if value & 8 != 0 {
                if (old & 7) == 0 && !self.locked {
                    self.volume ^= 0xF;
                } else {
                    self.volume = 0xEu8.wrapping_sub(self.volume) & 0xF;
                }
                should_tick = false;
            } else {
                self.volume = 0x10u8.wrapping_sub(self.volume) & 0xF;
            }
        }
        if should_tick {
            if value & 8 != 0 {
                self.volume = (self.volume + 1) & 0xF;
            } else {
                self.volume = self.volume.wrapping_sub(1) & 0xF;
            }
        } else if value & 7 == 0 && self.clock {
            self.set_clock(false);
        }
        self.write(value);
    }

    /// DIV event with `(div_divider & 7) == 7`: the countdown decrements
    /// (wrapping in 3 bits — a zero countdown with period 0 walks 7,6,...,
    /// the "period 0 acts as 8" behavior) unless a tick is armed.
    pub(super) fn countdown_event(&mut self) {
        if !self.clock {
            self.countdown = self.countdown.wrapping_sub(1) & 7;
        }
    }

    /// Secondary event (rising edge of the DIV-APU bit): on an active
    /// channel whose countdown reached 0, reload the countdown and arm the
    /// tick (a period of 0 reloads 0 and arms nothing).
    pub(super) fn arm(&mut self, channel_active: bool) {
        if channel_active && self.countdown == 0 {
            self.countdown = self.period;
            self.set_clock(self.period != 0);
        }
    }

    /// DIV event: consume an armed tick — volume moves one step in the
    /// NRx2 direction unless the envelope is locked at a rail (SameBoy
    /// `tick_square_envelope` / `tick_noise_envelope`).
    pub(super) fn tick_event(&mut self) {
        if !self.clock {
            return;
        }
        self.set_clock(false);
        if self.locked || self.period == 0 {
            return;
        }
        if self.add_mode {
            self.volume = (self.volume + 1) & 0xF;
        } else {
            self.volume = self.volume.wrapping_sub(1) & 0xF;
        }
    }

    pub(super) fn power_off(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive one full DIV-APU envelope period: seven plain events, the
    /// divider-7 countdown event, the secondary arming event, then report
    /// after the next event's tick — `events` counts events consumed.
    fn run_events(e: &mut Envelope, active: bool, n: u32) {
        for k in 1..=n {
            if k % 8 == 7 {
                e.countdown_event();
            }
            e.tick_event();
            // Secondary event between this DIV event and the next.
            e.arm(active);
        }
    }

    #[test]
    fn readback_round_trips() {
        let mut e = Envelope::new();
        e.write(0xA5);
        assert_eq!(e.read(), 0xA5);
        assert_eq!(e.initial_volume, 0xA);
        assert!(!e.add_mode);
        assert_eq!(e.period, 5);
    }

    #[test]
    fn dac_enabled_iff_high_5_bits() {
        let mut e = Envelope::new();
        e.write(0x00);
        assert!(!e.dac_enabled());
        e.write(0x08); // volume 0 but add mode: DAC on
        assert!(e.dac_enabled());
        e.write(0x10);
        assert!(e.dac_enabled());
        e.write(0x07); // only period bits: DAC off
        assert!(!e.dac_enabled());
    }

    #[test]
    fn increases_once_per_period_and_locks_at_15() {
        let mut e = Envelope::new();
        e.write(0xDA); // volume 13, increase, period 2
        e.trigger();
        assert_eq!(e.volume, 13);
        run_events(&mut e, true, 8);
        assert_eq!(e.volume, 13, "period 2: one divider-7 event only arms");
        run_events(&mut e, true, 8);
        assert_eq!(e.volume, 14);
        run_events(&mut e, true, 16);
        assert_eq!(e.volume, 15);
        run_events(&mut e, true, 64);
        assert_eq!(e.volume, 15, "locked at the rail, no wrap");
    }

    #[test]
    fn envelope_decrease_locks_at_zero() {
        let mut e = Envelope::new();
        e.write(0x21); // volume 2, decrease, period 1
        e.trigger();
        run_events(&mut e, true, 8);
        assert_eq!(e.volume, 1);
        run_events(&mut e, true, 8);
        assert_eq!(e.volume, 0);
        run_events(&mut e, true, 32);
        assert_eq!(e.volume, 0, "locked at the rail, no wrap");
    }

    #[test]
    fn period_zero_never_changes_volume() {
        let mut e = Envelope::new();
        e.write(0x50); // volume 5, decrease, period 0
        e.trigger();
        run_events(&mut e, true, 64);
        assert_eq!(e.volume, 5);
    }

    #[test]
    fn period_zero_countdown_walks_all_8_steps() {
        // "Period 0 acts as 8": the 3-bit countdown wraps 0 -> 7 and a
        // later NRx2 period write resumes from wherever it got to.
        let mut e = Envelope::new();
        e.write(0x18); // volume 1, increase, period 0
        e.trigger();
        run_events(&mut e, true, 8); // countdown wrapped to 7
        e.write(0x19); // period 1 — the countdown keeps walking down
        run_events(&mut e, true, 8 * 6); // countdown 7 -> 1
        assert_eq!(e.volume, 1, "still counting the wrapped value down");
        run_events(&mut e, true, 8); // countdown expires, arm, tick
        assert_eq!(e.volume, 2, "ticks once the wrapped countdown expires");
    }

    #[test]
    fn inactive_channel_never_arms() {
        let mut e = Envelope::new();
        e.write(0x19);
        e.trigger();
        run_events(&mut e, false, 64);
        assert_eq!(e.volume, 1);
    }

    #[test]
    fn zombie_nrx2_write_table() {
        // SameSuite channel_1_volume: NRx2 written right after a trigger
        // (no envelope clock armed, no lock), decoded from the ROM's
        // CorrectResults table at $0149 — volumes v ∈ {0,1,4,7,8,10,14,15}
        // per row. Each case: (old NRx2 low byte form, written value,
        // transformation).
        type Xform = fn(u8) -> u8;
        let cases: [(u8, u8, Xform); 16] = [
            // write $F0: period-0 old unchanged; period-1 old unchanged;
            // add-mode olds invert 0x10 - v.
            (0x00, 0xF0, |v| v),
            (0x01, 0xF0, |v| v),
            (0x08, 0xF0, |v| 0x10u8.wrapping_sub(v) & 0xF),
            (0x09, 0xF0, |v| 0x10u8.wrapping_sub(v) & 0xF),
            // write $F1: period-0 old ticks down; period-1 unchanged;
            // $x8 old inverts then ticks (0xF - v); $x9 old inverts.
            (0x00, 0xF1, |v| v.wrapping_sub(1) & 0xF),
            (0x01, 0xF1, |v| v),
            (0x08, 0xF1, |v| v ^ 0xF),
            (0x09, 0xF1, |v| 0x10u8.wrapping_sub(v) & 0xF),
            // write $F8: xor-invert for period-0 old, 0xE - v for
            // period-1, the 8/8 special tick for $x8, unchanged for $x9.
            (0x00, 0xF8, |v| v ^ 0xF),
            (0x01, 0xF8, |v| 0xEu8.wrapping_sub(v) & 0xF),
            (0x08, 0xF8, |v| (v + 1) & 0xF),
            (0x09, 0xF8, |v| v),
            // write $F9: like $F8 except the $x8 tick comes from the
            // should_tick path.
            (0x00, 0xF9, |v| v ^ 0xF),
            (0x01, 0xF9, |v| 0xEu8.wrapping_sub(v) & 0xF),
            (0x08, 0xF9, |v| (v + 1) & 0xF),
            (0x09, 0xF9, |v| v),
        ];
        for (old_low, value, want) in cases {
            for v in [0u8, 1, 4, 7, 8, 10, 14, 15] {
                let old = (v << 4) | old_low;
                if old & 0xF8 == 0 {
                    continue; // DAC off: the channel would not be active
                }
                let mut e = Envelope::new();
                e.write(old);
                e.trigger();
                assert_eq!(e.volume, v);
                e.write_active(value);
                assert_eq!(
                    e.volume,
                    want(v),
                    "old {old:02X} write {value:02X} from volume {v}"
                );
                assert_eq!(e.read(), value, "register stores the new value");
            }
        }
    }

    #[test]
    fn trigger_clears_the_rail_lock() {
        let mut e = Envelope::new();
        e.write(0xF9); // volume 15, increase, period 1: arms straight into lock
        e.trigger();
        run_events(&mut e, true, 32);
        assert_eq!(e.volume, 15);
        e.write(0x19); // volume 1 on the next trigger
        e.trigger();
        run_events(&mut e, true, 8);
        assert_eq!(e.volume, 2, "lock cleared by the trigger");
    }
}
