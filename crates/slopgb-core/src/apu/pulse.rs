//! Pulse (square wave) channels 1 and 2. Channel 1 additionally has the
//! frequency sweep unit (NR10); channel 2's sweep state simply stays inert
//! because no register write ever reaches it.
//!
//! The frequency unit follows SameBoy's hardware-verified countdown model
//! (Core/apu.c): `sample_countdown` counts 2 MHz cycles and an expiry
//! consumes `sample_countdown + 1` of them, reloading `(freq ^ 0x7FF) * 2 +
//! 1` — a period of `(2048 - freq) * 2` cycles. Triggers anchor the first
//! expiry to the machine-global 1 MHz grid via the APU's `lf_div` phase bit
//! instead of restarting a private timer.

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
    /// 2 MHz cycles until the duty position advances; the advance itself
    /// consumes one more cycle (SameBoy apu.c: an expiry consumes
    /// `sample_countdown + 1` cycles). Frozen while the channel is off.
    pub(super) sample_countdown: u16,
    pub(super) duty_pos: u8,
    /// Duty bit latched at the last countdown expiry. NRx1 duty writes only
    /// take effect at the next expiry: "Changing the duty becomes effective
    /// only after the current sample finishes" (SameSuite
    /// channel_1/2_duty_delay; SameBoy apu.c latches the sample per step).
    pub(super) current_sample: u8,
    /// Output forced to digital 0 between a trigger-from-inactive and the
    /// first countdown expiry: the preserved duty position must not become
    /// audible at trigger time (SameBoy apu.c `sample_surpressed`).
    pub(super) suppressed: bool,
    /// The duty position advanced at least once since the last trigger
    /// (SameBoy `did_tick`).
    pub(super) did_tick: bool,
    /// The last 2 MHz cycle processed was an expiry — the countdown holds a
    /// freshly reloaded period (SameBoy `just_reloaded`); frequency writes
    /// landing here take effect immediately.
    pub(super) just_reloaded: bool,
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
            sample_countdown: 0,
            duty_pos: 0,
            current_sample: 0,
            suppressed: false,
            did_tick: false,
            just_reloaded: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_timer: 0,
            sweep_enabled: false,
            sweep_shadow: 0,
            sweep_negate_used: false,
        }
    }

    /// Steady-state countdown reload: period `(2048 - freq) * 2` 2 MHz
    /// cycles counting the expiry cycle itself.
    fn countdown_reload(&self) -> u16 {
        (self.freq ^ 0x7FF) * 2 + 1
    }

    /// Advance one 2 MHz cycle. The frequency unit only runs while the
    /// channel is on (SameBoy apu.c steps square channels under
    /// `is_active`); a disabled channel's countdown and duty position
    /// freeze until the next trigger re-anchors them.
    pub(super) fn step(&mut self) {
        if !self.enabled {
            return;
        }
        if self.sample_countdown == 0 {
            self.sample_countdown = self.countdown_reload();
            self.duty_pos = (self.duty_pos + 1) & 7;
            self.current_sample = DUTY_TABLE[usize::from(self.duty)][usize::from(self.duty_pos)];
            self.suppressed = false;
            self.did_tick = true;
            self.just_reloaded = true;
        } else {
            self.sample_countdown -= 1;
            self.just_reloaded = false;
        }
    }

    /// NRx3: frequency low byte; a write in the reload cycle takes effect
    /// immediately (SameBoy NR13/NR23 `just_reloaded` path).
    pub(super) fn write_nrx3(&mut self, value: u8) {
        self.freq = (self.freq & 0x0700) | u16::from(value);
        if self.just_reloaded {
            self.sample_countdown = self.countdown_reload();
        }
    }

    /// NRx4 frequency bits 2-0, with the non-trigger "frequency high 7 ->
    /// ≠7" glitch (SameBoy apu.c NR14/NR24): such a write on an active,
    /// already-ticked channel whose countdown holds a freshly reloaded
    /// period steps the sample index BACKWARDS (upstream models its
    /// T-cycle-imprecise write timing that way) and lifts suppression. On
    /// CGB-D/E the glitch is unconditional; on every other model it only
    /// fires with an odd countdown — `Model::Cgb` is CPU CGB C
    /// (docs/ARCHITECTURE.md §CGB revision policy), so the odd-countdown
    /// form is used here.
    pub(super) fn write_nrx4_freq(&mut self, value: u8) {
        if value & 0x80 == 0
            && self.enabled
            && (self.freq >> 8) & 7 == 7
            && value & 7 != 7
            && self.sample_countdown & 1 == 1
            && self.did_tick
            && self.sample_countdown >> 1 == (self.freq ^ 0x7FF)
        {
            self.duty_pos = self.duty_pos.wrapping_sub(1) & 7;
            self.suppressed = false;
        }
        self.freq = (self.freq & 0x00FF) | (u16::from(value & 7) << 8);
        if self.just_reloaded {
            self.sample_countdown = self.countdown_reload();
        }
    }

    /// Current digital output, 0-15: the latched duty bit times the live
    /// envelope volume (volume changes apply immediately; the duty bit only
    /// re-latches at expiries).
    pub(super) fn digital(&self) -> u8 {
        if self.enabled && !self.suppressed && self.current_sample == 1 {
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

    /// NRx4 trigger. `lf_div` is the APU's machine-global 2 MHz phase bit.
    ///
    /// SameBoy apu.c (NR14/NR24 trigger), hardware-verified:
    /// - inactive channel: `sample_countdown = (freq ^ 0x7FF) * 2 + 6 -
    ///   lf_div` and the output is suppressed until the first expiry;
    /// - active channel: "sound starts 2 (2MHz) ticks earlier" —
    ///   `sample_countdown = (freq ^ 0x7FF) * 2 + 4 - lf_div`, with the
    ///   current duty cell left audible (no suppression);
    /// - the duty position itself is preserved in both cases (it is only
    ///   reset by APU power-off).
    ///
    /// SameBoy additionally flips the lf_div sign on CGB ≤ C in double
    /// speed; under this core's tick-then-access conventions (writes land
    /// after the full M-cycle) the plain `6 - lf_div` form is what matches
    /// the CGB-C-verified SameSuite channel_1/2_align and channel_1/2_duty
    /// double-speed expectation tables cycle-for-cycle — the upstream sign
    /// flip is an artifact of SameBoy's mid-cycle write timing.
    pub(super) fn trigger(&mut self, lf_div: u16) {
        let was_active = self.enabled;
        self.enabled = self.dac;
        let base = (self.freq ^ 0x7FF) * 2;
        self.sample_countdown = if was_active {
            base + 4 - lf_div
        } else {
            self.suppressed = true;
            base + 6 - lf_div
        };
        self.did_tick = false;
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
        self.sample_countdown = 0;
        self.current_sample = 0;
        self.suppressed = false;
        self.did_tick = false;
        self.just_reloaded = false;
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
        p.trigger(0);
        p
    }

    /// Step until the duty position advances; returns how many 2 MHz cycles
    /// that took.
    fn cycles_to_next_duty_step(p: &mut Pulse) -> u32 {
        let pos = p.duty_pos;
        let mut n = 0;
        while p.duty_pos == pos {
            p.step();
            n += 1;
            assert!(n < 10_000, "duty position never advanced");
        }
        n
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
            let got: Vec<u8> = (0..8)
                .map(|_| {
                    cycles_to_next_duty_step(&mut p);
                    p.digital() / 15
                })
                .collect();
            // `want` is DUTY_TABLE[duty] rotated left by one.
            assert_eq!(got, want, "duty {duty}");
        }
    }

    #[test]
    fn steady_period_is_2048_minus_f_2mhz_cycles_times_2() {
        let mut p = playing_pulse(0, 2040);
        cycles_to_next_duty_step(&mut p); // consume the trigger delay
        assert_eq!(cycles_to_next_duty_step(&mut p), 16); // (2048-2040)*2
        assert_eq!(cycles_to_next_duty_step(&mut p), 16);
    }

    #[test]
    fn trigger_from_inactive_suppresses_output_until_first_expiry() {
        // SameBoy apu.c (`sample_surpressed`): a trigger of an INACTIVE
        // pulse channel keeps the duty position but forces digital 0 until
        // the first frequency-countdown expiry — the stale duty cell must
        // not become audible at trigger time.
        let mut p = playing_pulse(2, 2047); // duty 2: position 0 outputs 1
        assert_eq!(p.duty_pos, 0);
        assert_eq!(p.digital(), 0, "suppressed despite duty table high");
        // countdown = (2047^0x7FF)*2 + 6 - 0 = 6; the expiry consumes
        // countdown + 1 = 7 cycles.
        for i in 0..6 {
            p.step();
            assert_eq!(p.digital(), 0, "still suppressed at cycle {i}");
            assert_eq!(p.duty_pos, 0);
        }
        p.step(); // expiry: position advances, suppression lifts
        assert_eq!(p.duty_pos, 1);
        assert!(!p.suppressed);
        // Duty 2 position 1 is low; position 5 is the next high cell.
        for _ in 0..4 {
            cycles_to_next_duty_step(&mut p);
        }
        assert_eq!(p.duty_pos, 5);
        assert_eq!(p.digital(), 15, "audible after the first expiry");
    }

    #[test]
    fn duty_change_takes_effect_at_next_expiry() {
        // SameSuite channel_1/2_duty_delay: "Changing the duty becomes
        // effective only after the current sample finishes" — the output
        // is the duty bit LATCHED at the last countdown expiry, so an NRx1
        // duty write neither silences a playing cell nor un-silences a low
        // one until the next expiry re-latches.
        let mut p = playing_pulse(3, 2040); // duty 3: position 1 high
        cycles_to_next_duty_step(&mut p);
        assert_eq!(p.duty_pos, 1);
        assert_eq!(p.digital(), 15);
        p.duty = 0; // duty 0: position 1 low — but the latch holds
        assert_eq!(p.digital(), 15, "old sample keeps playing");
        cycles_to_next_duty_step(&mut p); // position 2 latches duty 0
        assert_eq!(p.digital(), 0, "new duty latched at the expiry");
        // And the reverse: a low latch is not raised by a duty change.
        let mut p = playing_pulse(0, 2040); // duty 0: position 1 low
        cycles_to_next_duty_step(&mut p);
        assert_eq!(p.digital(), 0);
        p.duty = 3;
        assert_eq!(p.digital(), 0, "low latch holds despite duty 3");
        cycles_to_next_duty_step(&mut p);
        assert_eq!(p.digital(), 15);
    }

    #[test]
    fn trigger_delay_depends_on_lf_div() {
        // Inactive trigger: countdown = (freq ^ 0x7FF)*2 + 6 - lf_div. The
        // lf_div term is what the SameSuite channel_1/2_align double-speed
        // tables measure (their \2 = 0 vs 1 nop groups shift the threshold
        // by exactly one M-cycle).
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 2047;
        p.trigger(1);
        assert_eq!(p.sample_countdown, 5);
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 2047;
        p.trigger(0);
        assert_eq!(p.sample_countdown, 6);
    }

    #[test]
    fn retrigger_while_active_is_two_cycles_earlier_and_not_suppressed() {
        // SameBoy apu.c: "Timing quirk: if already active, sound starts 2
        // (2MHz) ticks earlier" — delay 4 - lf_div instead of 6 - lf_div —
        // and the current duty cell keeps playing (no suppression).
        let mut p = playing_pulse(2, 2047);
        cycles_to_next_duty_step(&mut p); // suppression lifted, pos 1
        for _ in 0..4 {
            cycles_to_next_duty_step(&mut p);
        }
        assert_eq!(p.duty_pos, 5);
        assert_eq!(p.digital(), 15);
        p.trigger(0);
        assert_eq!(p.sample_countdown, 4); // (2047^0x7FF)*2 + 4 - 0
        assert_eq!(p.duty_pos, 5, "duty position preserved");
        assert_eq!(p.digital(), 15, "no suppression on retrigger");
    }

    #[test]
    fn disabled_channel_freezes_frequency_unit() {
        let mut p = playing_pulse(2, 2040);
        cycles_to_next_duty_step(&mut p);
        let (pos, countdown) = (p.duty_pos, p.sample_countdown);
        p.enabled = false;
        for _ in 0..100 {
            p.step();
        }
        assert_eq!(p.duty_pos, pos);
        assert_eq!(p.sample_countdown, countdown);
        assert_eq!(p.digital(), 0);
    }

    #[test]
    fn disabled_channel_outputs_zero() {
        let mut p = playing_pulse(3, 1000);
        // duty 3 position 0 outputs 0; advance to a high position.
        cycles_to_next_duty_step(&mut p);
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
        p.trigger(0);
        assert!(!p.enabled);

        // Same with shift 0: no immediate check, channel stays on.
        let mut p = Pulse::new();
        p.envelope.write(0xF0);
        p.dac = true;
        p.freq = 1920;
        p.write_nr10(0x10);
        p.trigger(0);
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
        p.trigger(0);
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
        p.trigger(0);
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
        p.trigger(0);
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
        p.trigger(0);
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
        p.trigger(0);
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
        p.trigger(0);
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
        p.trigger(0);
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
        p.trigger(0);
        p.write_nr10(0x11); // period 1, shift 1
        for i in 0..7 {
            p.sweep_clock();
            assert_eq!(p.freq, 512, "no update on sweep clock {i}");
        }
        p.sweep_clock();
        assert_eq!(p.freq, 768);
    }

    #[test]
    fn freq_write_in_reload_cycle_takes_effect_immediately() {
        // SameBoy apu.c NR13/NR23 (and the NRx4 frequency bits): a write
        // landing on the cycle where the countdown just reloaded
        // (`just_reloaded`) re-loads the countdown from the new frequency
        // immediately instead of letting the stale period play out.
        let mut p = playing_pulse(2, 2047);
        cycles_to_next_duty_step(&mut p); // expiry: countdown reloaded to 1
        assert!(p.just_reloaded);
        p.write_nrx3(0x00); // freq low 0 -> freq 0x700
        assert_eq!(p.sample_countdown, (0x700u16 ^ 0x7FF) * 2 + 1);
        // One cycle later the write would be too late.
        let mut p = playing_pulse(2, 2046);
        cycles_to_next_duty_step(&mut p);
        p.step(); // plain countdown cycle: just_reloaded clears
        assert!(!p.just_reloaded);
        let before = p.sample_countdown;
        p.write_nrx3(0x00);
        assert_eq!(p.sample_countdown, before, "no immediate reload");
    }

    #[test]
    fn nrx4_freq_high_7_to_other_steps_sample_back() {
        // SameBoy apu.c NR14/NR24: a NON-trigger write taking frequency
        // bits 10-8 from 7 to another value while the channel is active
        // steps the sample index BACKWARDS when the channel has ticked
        // since its trigger and the countdown holds a freshly reloaded
        // period (odd countdown on non-D/E revisions).
        let mut p = playing_pulse(2, 0x7FF);
        cycles_to_next_duty_step(&mut p); // pos 1, countdown = 1, did_tick
        assert_eq!(p.duty_pos, 1);
        p.write_nrx4_freq(0x00); // freq high 7 -> 0, no trigger
        assert_eq!(p.duty_pos, 0, "sample index stepped back");
        // Without a tick since the trigger the glitch does not fire.
        let mut p = playing_pulse(2, 0x7FF);
        p.sample_countdown = 1; // same countdown state, but did_tick false
        p.write_nrx4_freq(0x00);
        assert_eq!(p.duty_pos, 0, "no backward step before the first tick");
    }

    #[test]
    fn trigger_with_dac_off_leaves_channel_disabled() {
        let mut p = Pulse::new();
        p.envelope.write(0x00);
        p.dac = p.envelope.dac_enabled();
        p.trigger(0);
        assert!(!p.enabled);
    }
}
