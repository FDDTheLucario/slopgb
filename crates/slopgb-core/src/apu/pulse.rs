//! Pulse (square wave) channels 1 and 2. Channel 1 additionally has the
//! frequency sweep unit (NR10); channel 2's sweep state simply stays inert
//! because no register write ever reaches it and the sweep machinery is
//! only stepped for channel 1.
//!
//! The frequency unit follows SameBoy's hardware-verified countdown model
//! (Core/apu.c): `sample_countdown` counts 2 MHz cycles and an expiry
//! consumes `sample_countdown + 1` of them, reloading `(freq ^ 0x7FF) * 2 +
//! 1` — a period of `(2048 - freq) * 2` cycles. Triggers anchor the first
//! expiry to the machine-global 1 MHz grid via the APU's `lf_div` phase bit
//! instead of restarting a private timer.
//!
//! The sweep unit likewise follows SameBoy's countdown machinery rather
//! than the classic 128 Hz state machine (SameBoy apu.c
//! `trigger_sweep_calculation` / `sweep_calculation_done` /
//! `square_sweep_calculate_countdown`): the frequency *write* lands at the
//! 128 Hz DIV-APU fire, but the shadow/addend refresh and the overflow
//! check are a separate *calculation* that completes only `reload_timer +
//! shift` 1 MHz cycles later — so an overflow kill trails the fire (or the
//! NRx4 trigger) by several M-cycles, NR10 writes in that window hit live
//! machinery, and a retrigger replaces a pending kill (the restart hold).
//! SameSuite channel_1_sweep / channel_1_sweep_restart and the gambatte
//! sound/ch1_init_reset_sweep_counter_timing scans pin this model.

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

#[derive(Clone)]
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
    // Sweep unit (channel 1 only). Field-by-field port of SameBoy apu.c's
    // sweep machinery (see the module docs).
    pub(super) sweep_period: u8,
    pub(super) sweep_negate: bool,
    pub(super) sweep_shift: u8,
    /// 128 Hz fire phase: 3-bit up-counter incremented per sweep DIV-APU
    /// event; the unit fires when it reads 7 and the period is non-zero
    /// (SameBoy `square_sweep_countdown`; reset to `period ^ 7` by fires
    /// and triggers).
    pub(super) sweep_countdown: u8,
    /// 1 MHz cycles until the delayed re-calculation (shadow/addend
    /// refresh + overflow check) completes (SameBoy
    /// `square_sweep_calculate_countdown`).
    pub(super) sweep_calc_countdown: u8,
    /// 1 MHz cycles before `sweep_calc_countdown` starts running (SameBoy
    /// `square_sweep_calculate_countdown_reload_timer`).
    pub(super) sweep_reload_timer: u8,
    /// Shadow frequency the overflow check sums; refreshed from `freq`
    /// only by a *completed* calculation outside the restart hold
    /// (SameBoy `shadow_sweep_sample_length`).
    pub(super) sweep_shadow: u16,
    /// Pre-shifted delta the next fire adds (SameBoy
    /// `sweep_length_addend`); one's-complemented by a completed
    /// calculation in negate mode.
    pub(super) sweep_addend: u16,
    /// `sweep_addend` as of the last completed calculation — the NR10
    /// negate-clear kill check sums it (SameBoy
    /// `channel1_completed_addend`).
    pub(super) sweep_completed_addend: u16,
    /// 2 MHz hold window after a trigger during which completed
    /// calculations and fires do not refresh the shadow register / addend
    /// (SameBoy `channel_1_restart_hold`).
    pub(super) sweep_restart_hold: u8,
    /// The last fire ran with shift 0 (SameBoy `unshifted_sweep`): the
    /// pending calculation keeps counting even though NR10's shift bits
    /// read 0 (otherwise a cleared shift *pauses* it).
    pub(super) sweep_unshifted: bool,
    /// A shift-0 fire armed an "instant" calculation that completes when
    /// the reload timer expires (SameBoy
    /// `square_sweep_instant_calculation_done`).
    pub(super) sweep_instant_done: bool,
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
            sweep_countdown: 0,
            sweep_calc_countdown: 0,
            sweep_reload_timer: 0,
            sweep_shadow: 0,
            sweep_addend: 0,
            sweep_completed_addend: 0,
            sweep_restart_hold: 0,
            sweep_unshifted: false,
            sweep_instant_done: false,
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

    /// NR10 write. `lf_div` is the APU's 2 MHz phase bit at the write,
    /// `double_speed` the machine speed (both feed the in-flight-machinery
    /// glitches). Port of SameBoy apu.c `GB_apu_write` case GB_IO_NR10:
    /// glitch the live machinery with the OLD register, commit the new
    /// value, run the negate-clear kill check, then let the write itself
    /// fire the sweep if the 128 Hz counter is parked at 7.
    pub(super) fn write_nr10(&mut self, value: u8, lf_div: u16, double_speed: bool) {
        debug_assert!(lf_div <= 1, "lf_div is the 2 MHz phase BIT");
        if self.sweep_calc_countdown != 0 || self.sweep_reload_timer != 0 {
            self.nr10_write_glitch(value, lf_div, double_speed);
        }
        let old_negate = self.sweep_negate;
        self.sweep_period = (value >> 4) & 7;
        self.sweep_negate = value & 0x08 != 0;
        self.sweep_shift = value & 7;
        // Clearing negate kills the channel when the last completed
        // calculation's sum (shadow + addend + the OLD negate bit) would
        // overflow — after a negate-mode calculation the addend holds the
        // one's complement, so the sum always crosses 0x7FF: this is the
        // documented "negate calculation followed by negate-clear kills
        // the channel" rule (SameBoy apu.c NR10 write; Blargg dmg_sound
        // 05-sweep details). SameBoy forces `old_negate` to 1 on CGB-C
        // and older; that C-only variant flips the exact-0x7FF boundary
        // (SameSuite channel_1_sweep_restart round 3: $7f0 + $f survives
        // on the E-verified table, dies with the forced bit), so per
        // docs/ARCHITECTURE.md §CGB revision policy (companion rule, the
        // PCM12/34-glitch shape) the E form is used until a revision
        // split.
        if self.sweep_shadow + self.sweep_completed_addend + u16::from(old_negate) > 0x7FF
            && value & 0x08 == 0
        {
            self.enabled = false;
        }
        self.sweep_fire(1 + lf_div as u8);
    }

    pub(super) fn read_nr10(&self) -> u8 {
        0x80 | (self.sweep_period << 4) | (u8::from(self.sweep_negate) << 3) | self.sweep_shift
    }

    /// NR10 write landing while the calculation machinery is in flight
    /// (SameBoy apu.c `nr10_write_glitch`, the `model <= GB_MODEL_CGB_C`
    /// branch — `Model::Cgb` is CPU CGB C and the DMG family shares the
    /// branch upstream; the CGB-D/E/AGB variant stays unmodelled because
    /// no AGB-routed reference writes NR10 mid-sweep). Reads the OLD
    /// register fields (the caller commits `value` afterwards).
    fn nr10_write_glitch(&mut self, value: u8, lf_div: u16, double_speed: bool) {
        if self.sweep_reload_timer == 1 && lf_div == 0 {
            // Upstream documents this double-speed cell as instance-
            // specific data corruption (four different tables across its
            // CGB-C units, one case non-deterministic) — like the NR43
            // LFSR-corruption tables, only deterministic paths are
            // modelled; the countdown is left untouched here.
        } else if self.sweep_reload_timer > 1 {
            if double_speed {
                self.sweep_calc_countdown = value & 7;
            }
        } else if self.sweep_calc_countdown != 0 {
            // "No clue why 1 is a special case here" (upstream comment).
            let zombie_step = if self.sweep_shift == 0 {
                (lf_div == 1) != double_speed
            } else {
                double_speed && self.sweep_calc_countdown == 1
            };
            if zombie_step {
                self.sweep_calc_countdown -= 1;
                if self.sweep_calc_countdown <= 1 {
                    self.sweep_calc_countdown = 0;
                    self.sweep_calculation_done();
                }
            }
        }
    }

    /// Completed sweep calculation (SameBoy apu.c
    /// `sweep_calculation_done`): refresh the shadow register (outside
    /// the restart hold), one's-complement the addend in negate mode, and
    /// run the overflow check — "sweep frequency is checked after adding
    /// the sweep delta twice" (upstream comment): the fire already wrote
    /// `shadow + addend` into `freq`, and this probes one more addend on
    /// top. Negate mode never kills (the complemented sum models the
    /// subtraction).
    fn sweep_calculation_done(&mut self) {
        if self.sweep_restart_hold == 0 {
            self.sweep_shadow = self.freq;
        }
        if self.sweep_negate {
            self.sweep_addend ^= 0x7FF;
        }
        if self.sweep_shadow + self.sweep_addend > 0x7FF && !self.sweep_negate {
            self.enabled = false;
        }
        self.sweep_completed_addend = self.sweep_addend;
    }

    /// Sweep fire (SameBoy apu.c `trigger_sweep_calculation`): runs from
    /// the 128 Hz clock *and* from NR10 writes, gated on a non-zero period
    /// and the up-counter reading 7. Writes the new frequency immediately
    /// (negate mode adds the complemented addend plus the negate bit —
    /// two's-complement subtraction), refreshes the addend outside the
    /// restart hold, and arms the delayed re-calculation: `reload` 1 MHz
    /// cycles of lead, then `shift` more until the overflow check.
    fn sweep_fire(&mut self, reload: u8) {
        if self.sweep_period != 0 && self.sweep_countdown == 7 {
            if self.sweep_shift != 0 {
                self.freq =
                    (self.sweep_addend + self.sweep_shadow + u16::from(self.sweep_negate)) & 0x7FF;
            }
            if self.sweep_restart_hold == 0 {
                self.sweep_addend = self.freq >> self.sweep_shift;
            }
            self.sweep_calc_countdown = self.sweep_shift;
            self.sweep_reload_timer = reload;
            self.sweep_unshifted = self.sweep_shift == 0;
            self.sweep_countdown = self.sweep_period ^ 7;
            if self.sweep_calc_countdown == 0 {
                self.sweep_instant_done = true;
            }
        }
    }

    /// 128 Hz frame-sequencer sweep clock (DIV-APU events with
    /// `divider & 3 == 3`): step the up-counter and try to fire. `reload`
    /// is the calculation lead time the caller derives from the machine
    /// phase (`1 + lf_div`; 1 for a single-speed DIV-write event —
    /// SameBoy apu.c `trigger_sweep_calculation` and its
    /// `during_div_write` compensation).
    pub(super) fn sweep_clock(&mut self, reload: u8) {
        self.sweep_countdown = (self.sweep_countdown + 1) & 7;
        self.sweep_fire(reload);
    }

    /// One 1 MHz cycle of the calculation machinery (SameBoy GB_apu_run's
    /// `sweep_cycles` block): the reload timer leads, then the calculation
    /// countdown runs — unless NR10's shift bits were cleared after a
    /// shifted fire, which pauses it ("Calculation is paused if the lower
    /// bits are 0", SameBoy apu.c).
    pub(super) fn sweep_machine_step(&mut self) {
        if self.sweep_reload_timer > 0 {
            self.sweep_reload_timer -= 1;
            if self.sweep_reload_timer == 0 {
                if self.sweep_calc_countdown == 0 && self.sweep_instant_done {
                    self.sweep_calculation_done();
                }
                self.sweep_instant_done = false;
            }
        } else if self.sweep_calc_countdown != 0 && (self.sweep_shift != 0 || self.sweep_unshifted)
        {
            self.sweep_calc_countdown -= 1;
            if self.sweep_calc_countdown == 0 {
                self.sweep_calculation_done();
            }
        }
    }

    /// One 2 MHz cycle of the post-trigger restart hold (SameBoy
    /// `channel_1_restart_hold` decrements on the full APU cycle clock,
    /// not the 1 MHz sweep grid).
    pub(super) fn sweep_hold_step(&mut self) {
        self.sweep_restart_hold = self.sweep_restart_hold.saturating_sub(1);
    }

    /// NRx4 trigger tail for the sweep unit (channel 1 only; SameBoy
    /// apu.c NR14 trigger, `index == GB_SQUARE_1` block). `was_active` is
    /// the channel state before the trigger.
    pub(super) fn trigger_sweep(
        &mut self,
        lf_div: u16,
        was_active: bool,
        cgb: bool,
        double_speed: bool,
    ) {
        debug_assert!(lf_div <= 1, "lf_div is the 2 MHz phase BIT");
        self.sweep_instant_done = false;
        self.sweep_shadow = 0;
        self.sweep_completed_addend = 0;
        if self.sweep_shift != 0 {
            // "APU bug: if shift is nonzero, overflow check also occurs
            // on trigger" — armed as a delayed calculation, so the kill
            // trails the trigger by `reload + shift` M-cycles (SameSuite
            // channel_1_sweep boundaries). The lead is 3 on CGB-C and
            // older when `lf_div ^ !double_speed` is set (upstream), 2
            // otherwise, plus 1 when the channel was inactive.
            self.sweep_calc_countdown = self.sweep_shift;
            self.sweep_reload_timer = if (lf_div == 1) == double_speed { 3 } else { 2 };
            self.sweep_unshifted = false;
            if !was_active {
                self.sweep_reload_timer += 1;
            }
            self.sweep_addend = self.freq >> self.sweep_shift;
        } else {
            self.sweep_addend = 0;
        }
        // Completed calculations inside this hold do not refresh the
        // shadow register: a quick retrigger re-checks against the reset
        // shadow (0), not the live frequency.
        self.sweep_restart_hold = 2 - lf_div as u8 + if cgb { 2 } else { 0 };
        self.sweep_countdown = self.sweep_period ^ 7;
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
        // Channel 1's sweep-unit trigger tail lives in
        // [`Self::trigger_sweep`], invoked by the APU's NR14 handler only.
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
        self.sweep_countdown = 0;
        self.sweep_calc_countdown = 0;
        self.sweep_reload_timer = 0;
        self.sweep_shadow = 0;
        self.sweep_addend = 0;
        self.sweep_completed_addend = 0;
        self.sweep_restart_hold = 0;
        self.sweep_unshifted = false;
        self.sweep_instant_done = false;
    }
}

// --- Save state (see `crate::state`) ---
impl Pulse {
    pub(super) fn write_state(&self, w: &mut crate::state::Writer) {
        w.bool(self.enabled);
        w.bool(self.dac);
        w.u8(self.duty);
        w.u16(self.freq);
        self.length.write_state(w);
        self.envelope.write_state(w);
        w.u16(self.sample_countdown);
        w.u8(self.duty_pos);
        w.u8(self.current_sample);
        w.bool(self.suppressed);
        w.bool(self.did_tick);
        w.bool(self.just_reloaded);
        w.u8(self.sweep_period);
        w.bool(self.sweep_negate);
        w.u8(self.sweep_shift);
        w.u8(self.sweep_countdown);
        w.u8(self.sweep_calc_countdown);
        w.u8(self.sweep_reload_timer);
        w.u16(self.sweep_shadow);
        w.u16(self.sweep_addend);
        w.u16(self.sweep_completed_addend);
        w.u8(self.sweep_restart_hold);
        w.bool(self.sweep_unshifted);
        w.bool(self.sweep_instant_done);
    }
    pub(super) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.enabled = r.bool()?;
        self.dac = r.bool()?;
        self.duty = r.u8()?;
        self.freq = r.u16()?;
        self.length.read_state(r)?;
        self.envelope.read_state(r)?;
        self.sample_countdown = r.u16()?;
        self.duty_pos = r.u8()?;
        self.current_sample = r.u8()?;
        self.suppressed = r.bool()?;
        self.did_tick = r.bool()?;
        self.just_reloaded = r.bool()?;
        self.sweep_period = r.u8()?;
        self.sweep_negate = r.bool()?;
        self.sweep_shift = r.u8()?;
        self.sweep_countdown = r.u8()?;
        self.sweep_calc_countdown = r.u8()?;
        self.sweep_reload_timer = r.u8()?;
        self.sweep_shadow = r.u16()?;
        self.sweep_addend = r.u16()?;
        self.sweep_completed_addend = r.u16()?;
        self.sweep_restart_hold = r.u8()?;
        self.sweep_unshifted = r.bool()?;
        self.sweep_instant_done = r.bool()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "pulse_tests.rs"]
mod tests;
