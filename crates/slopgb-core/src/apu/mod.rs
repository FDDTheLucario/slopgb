//! APU: 2 pulse channels, wave, noise (FF10-FF3F). APU work package.
//!
//! The frame sequencer (length/envelope/sweep) is clocked by falling edges
//! of DIV bit 4 (bit 5 in CGB double speed) — "DIV-APU". Power-off (NR52)
//! clears registers; length counters are writable while off on DMG only.
//! Emulates obscure behaviors: trigger while sweep negate, wave RAM access
//! during playback, length clocking on enable edge, etc.
//!
//! References: Pan Docs ("Audio Registers", "Audio Details") and the gbdev
//! wiki "Game Boy Sound Operation" page (Blargg's dmg_sound research).

mod envelope;
mod length;
mod noise;
mod pulse;
mod wave;

use envelope::Envelope;
use length::LengthCounter;
use noise::Noise;
use pulse::Pulse;
use wave::Wave;

/// Default output sample rate in Hz for [`GameBoy::drain_audio`], in effect
/// until a frontend overrides it via [`GameBoy::set_sample_rate`]. Exported
/// so frontends can size resamplers against it instead of copying the
/// literal.
///
/// [`GameBoy::drain_audio`]: crate::GameBoy::drain_audio
/// [`GameBoy::set_sample_rate`]: crate::GameBoy::set_sample_rate
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

/// Power-on DIV-event skip state (SameBoy apu.c, `GB_apu_init` /
/// `GB_apu_div_event`): "APU glitch: When turning the APU on while DIV's
/// bit 4 (or 5 in double speed mode) is on, the first DIV/APU event is
/// skipped." The first event after such a power-on is consumed entirely
/// (`Skip` -> `Skipped`), the second runs its clocks without incrementing
/// the divider (`Skipped` -> `Inactive`), and the divider parity starts
/// shifted (div_divider = 1, like SameBoy's `GB_apu_init`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SkipDivEvent {
    Inactive,
    Skip,
    Skipped,
}

#[derive(Clone)]
pub struct Apu {
    cgb: bool,
    /// NR52 bit 7.
    power: bool,
    ch1: Pulse,
    ch2: Pulse,
    ch3: Wave,
    ch4: Noise,
    nr50: u8,
    nr51: u8,
    /// Per-channel mute mask: bit `(channel-1)` set => channel silenced in
    /// [`Self::mix`]. A frontend/debugger control (bgb's "Sound channel"
    /// submenu), NOT hardware — it survives NR52 power cycles and defaults
    /// to 0 (all audible) so it never perturbs golden output.
    mute_mask: u8,
    /// DIV-APU event divider (3 bits), incremented at the start of each
    /// event like SameBoy's `div_divider`: lengths clock on odd values,
    /// sweep at `divider&3 == 3`, envelope countdowns at `divider&7 == 7`.
    div_divider: u8,
    skip_div_event: SkipDivEvent,
    /// Machine-global dot phase within the 1 MHz cycle, low 2 bits only.
    /// Bit 1 is SameBoy's `lf_div` — the 2 MHz phase bit the pulse trigger
    /// delays are anchored to ("To align the square signal to 1MHz",
    /// SameBoy apu.c). The pulse frequency units step once per 2 dots, when
    /// this wraps to even. Reset by APU power-on (the APU's divider chain
    /// is held in reset while off); starts at 2 so `lf_div` reads 1 like
    /// SameBoy's `GB_apu_init`.
    phase: u8,
    prev_div: u16,
    /// `double_speed` of the latest [`Self::tick`]: an NR52 power-on (which
    /// lands between ticks) must test the DIV-APU bit the machine is
    /// currently running on.
    last_double_speed: bool,
    // Output stage.
    cycles_per_sample: f64,
    sample_frac: f64,
    sum_l: f32,
    sum_r: f32,
    sum_count: u32,
    hp_charge: f32,
    hp_cap_l: f32,
    hp_cap_r: f32,
    samples: Vec<(f32, f32)>,
    /// Cap on `samples` (one second of audio) so headless runs that never
    /// call [`Self::drain_samples`] stay flat in memory.
    max_samples: usize,
    /// Raw audio tap: one stereo sample per dot, taken straight off
    /// [`Self::mix`] *before* the box-average resampler and the high-pass
    /// stage of [`Self::output_cycle`] (see [`Self::drain_raw_samples`]).
    raw_samples: Vec<(f32, f32)>,
}

/// Cap on [`Apu::raw_samples`] (two frames of dots): the tap exists for
/// single-frame test assertions — a consumer further behind has lost the
/// window it cares about — and headless runs never drain at all.
const RAW_SAMPLE_CAP: usize = 2 * crate::CYCLES_PER_FRAME as usize;

/// Blargg-style single-pole high-pass ("the output capacitor").
fn high_pass(cap: &mut f32, input: f32, charge: f32) -> f32 {
    let out = input - *cap;
    *cap = input - out * charge;
    out
}

// Per-channel register-write plumbing, shared by all four channels. These
// take the individual fields (not the channel structs) because pulse, wave
// and noise are distinct types whose step/trigger/digital logic differs
// structurally — only the register bookkeeping is common.

/// 256 Hz length clock: disable the channel when its counter expires.
fn clock_length(length: &mut LengthCounter, enabled: &mut bool) {
    if length.clock() {
        *enabled = false;
    }
}

/// NRx1 for the pulse channels: duty in bits 7-6, length load in bits 5-0.
fn write_pulse_nrx1(ch: &mut Pulse, value: u8) {
    ch.duty = value >> 6;
    ch.length.load(value & 0x3F);
}

/// NRx2: store the envelope parameters and refresh the DAC flag; a channel
/// whose DAC turns off (bits 7-3 all zero) is disabled immediately, and a
/// write to an *active* channel goes through the envelope "zombie mode"
/// glitch ([`Envelope::write_active`], SameBoy `_nrx2_glitch`).
fn write_nrx2(envelope: &mut Envelope, dac: &mut bool, enabled: &mut bool, value: u8) {
    if value & 0xF8 == 0 {
        envelope.write(value);
        *dac = false;
        *enabled = false;
    } else {
        if *enabled {
            envelope.write_active(value);
        } else {
            envelope.write(value);
        }
        *dac = envelope.dac_enabled();
    }
}

/// NRx3: frequency low byte.
fn write_freq_low(freq: &mut u16, value: u8) {
    *freq = (*freq & 0x0700) | u16::from(value);
}

/// NRx4 bits 2-0: frequency high bits (pulse and wave channels only).
fn write_freq_high(freq: &mut u16, value: u8) {
    *freq = (*freq & 0x00FF) | (u16::from(value & 7) << 8);
}

/// NRx4 trigger/length plumbing: apply the length-enable write (with its
/// "extra length clock" edge cases, see [`LengthCounter::write_nrx4`]) and
/// return whether the trigger bit was set so the caller can run the
/// channel's own trigger logic afterwards.
fn write_nrx4(
    length: &mut LengthCounter,
    enabled: &mut bool,
    value: u8,
    next_step_clocks_length: bool,
) -> bool {
    let trigger = value & 0x80 != 0;
    if length.write_nrx4(value & 0x40 != 0, trigger, next_step_clocks_length) {
        *enabled = false;
    }
    trigger
}

impl Apu {
    pub fn new(cgb: bool) -> Self {
        let mut apu = Self {
            cgb,
            // The boot ROM leaves the APU powered on; the interconnect's
            // post-boot writes assume it is already accepting writes.
            power: true,
            ch1: Pulse::new(),
            ch2: Pulse::new(),
            ch3: Wave::new(cgb),
            ch4: Noise::new(),
            nr50: 0,
            nr51: 0,
            mute_mask: 0,
            div_divider: 0,
            skip_div_event: SkipDivEvent::Inactive,
            phase: 2,
            prev_div: 0,
            last_double_speed: false,
            cycles_per_sample: 0.0,
            sample_frac: 0.0,
            sum_l: 0.0,
            sum_r: 0.0,
            sum_count: 0,
            hp_charge: 0.0,
            hp_cap_l: 0.0,
            hp_cap_r: 0.0,
            samples: Vec::new(),
            max_samples: 0,
            raw_samples: Vec::new(),
        };
        apu.set_sample_rate(DEFAULT_SAMPLE_RATE);
        apu
    }

    /// A DIV reset reaches the frame sequencer within the write's own
    /// M-cycle: if the DIV-APU bit was high, the reset is its falling edge
    /// and clocks the sequencer now rather than at the next sampled tick
    /// (Pan Docs "DIV-APU": "writing to DIV ... can clock the APU's frame
    /// sequencer"; same shape as [`crate::serial::Serial::div_write`] —
    /// the once-per-M-cycle sampled tick would land the event one cycle
    /// late, which the gambatte speedchange ch2_nr52 a/b phase pairs
    /// resolve). The caller's next [`Self::tick`] passes the restarted
    /// counter.
    pub fn div_write(&mut self, double_speed: bool) {
        let bit = if double_speed { 13 } else { 12 };
        let was_high = (self.prev_div >> bit) & 1 == 1;
        self.prev_div = 0;
        if self.power && was_high {
            self.div_event(true);
        }
    }

    /// Advance one M-cycle (4 T-cycles). `div` is the timer's internal DIV
    /// counter after this cycle; `double_speed` selects the DIV-APU bit.
    pub fn tick(&mut self, div: u16, double_speed: bool) {
        // DIV-APU: falling edge of DIV register bit 4 (bit 5 in double
        // speed). DIV is the top byte of the internal counter, so that is
        // bit 12 (13) here — a 512 Hz edge in real time either way.
        let bit = if double_speed { 13 } else { 12 };
        let was = (self.prev_div >> bit) & 1;
        let now = (div >> bit) & 1;
        self.prev_div = div;
        self.last_double_speed = double_speed;
        if self.power {
            if was == 1 && now == 0 {
                self.div_event(false);
            } else if was == 0 && now == 1 {
                // Rising edge: the "secondary event" (SameBoy timing.c —
                // falling edge of the DIV-APU bit fires GB_apu_div_event,
                // rising edge GB_apu_div_secondary_event) arms the envelope
                // ticks of active channels whose countdown reached 0.
                self.ch1.envelope.arm(self.ch1.enabled);
                self.ch2.envelope.arm(self.ch2.enabled);
                self.ch4.envelope.arm(self.ch4.enabled);
            }
        }
        // One CPU M-cycle is 4 dots of APU time, 2 in double speed.
        let dots = if double_speed { 2 } else { 4 };
        for _ in 0..dots {
            if self.power {
                self.phase = (self.phase + 1) & 3;
                if self.phase & 1 == 0 {
                    // A full 2 MHz cycle elapsed: step the pulse and noise
                    // units (both run on the 2 MHz clock in hardware).
                    // Channel 1's sweep machinery leads, like SameBoy
                    // GB_apu_run (sweep countdowns before the sample
                    // countdowns): the calculation grid is 1 MHz — the
                    // cycle completing as `lf_div` wraps to 0 — and the
                    // restart hold runs on the full 2 MHz clock. Neither
                    // is gated on the channel being enabled (a pending
                    // calculation outlives an overflow kill).
                    if self.phase == 0 {
                        self.ch1.sweep_machine_step();
                    }
                    self.ch1.sweep_hold_step();
                    self.ch1.step();
                    self.ch2.step();
                    self.ch4.step();
                }
                self.ch3.step();
            }
            self.output_cycle();
        }
    }

    /// SameBoy's `lf_div`: the 2 MHz phase bit within the machine's 1 MHz
    /// grid, as seen by a register write landing between ticks. Constant 1
    /// in single speed (writes always land on the same phase); alternates
    /// per M-cycle in double speed.
    fn lf_div(&self) -> u16 {
        u16::from(self.phase >> 1) & 1
    }

    /// One DIV-APU event (falling edge of the DIV-APU bit), structured like
    /// SameBoy's GB_apu_div_event: increment the divider, then gate each
    /// unit on the divider value — envelope countdowns at `divider&7 == 7`,
    /// armed envelope ticks every event, lengths on odd dividers, sweep at
    /// `divider&3 == 3`. `during_div_write` marks the event as raised by a
    /// DIV write (see [`Self::div_write`]): the sweep calculation's lead
    /// time drops to 1 in single speed (SameBoy apu.c
    /// `trigger_sweep_calculation`, the `during_div_write` compensation
    /// for the write landing later in the cycle than a natural edge).
    fn div_event(&mut self, during_div_write: bool) {
        match self.skip_div_event {
            // Power-on glitch (see [`SkipDivEvent`]): the first event is
            // consumed entirely...
            SkipDivEvent::Skip => {
                self.skip_div_event = SkipDivEvent::Skipped;
                return;
            }
            // ...and the second runs its clocks without incrementing the
            // divider.
            SkipDivEvent::Skipped => self.skip_div_event = SkipDivEvent::Inactive,
            SkipDivEvent::Inactive => self.div_divider = (self.div_divider + 1) & 7,
        }
        if self.div_divider & 7 == 7 {
            self.ch1.envelope.countdown_event();
            self.ch2.envelope.countdown_event();
            self.ch4.envelope.countdown_event();
        }
        self.ch1.envelope.tick_event();
        self.ch2.envelope.tick_event();
        self.ch4.envelope.tick_event();
        if self.div_divider & 1 == 1 {
            self.clock_lengths();
        }
        if self.div_divider & 3 == 3 {
            let reload = if during_div_write && !self.last_double_speed {
                1
            } else {
                1 + self.lf_div() as u8
            };
            self.ch1.sweep_clock(reload);
        }
    }

    fn clock_lengths(&mut self) {
        clock_length(&mut self.ch1.length, &mut self.ch1.enabled);
        clock_length(&mut self.ch2.length, &mut self.ch2.enabled);
        clock_length(&mut self.ch3.length, &mut self.ch3.enabled);
        clock_length(&mut self.ch4.length, &mut self.ch4.enabled);
    }

    /// True when the next frame-sequencer step is one of 0/2/4/6. NRx4
    /// writes in the other phase produce the "extra length clock".
    fn next_step_clocks_length(&self) -> bool {
        self.div_divider % 2 == 0
    }

    /// Read FF10-FF3F (unused bits read 1, wave RAM access rules apply).
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF10 => self.ch1.read_nr10(),
            0xFF11 => 0x3F | (self.ch1.duty << 6),
            0xFF12 => self.ch1.envelope.read(),
            0xFF13 => 0xFF,
            0xFF14 => 0xBF | (u8::from(self.ch1.length.enabled) << 6),
            0xFF16 => 0x3F | (self.ch2.duty << 6),
            0xFF17 => self.ch2.envelope.read(),
            0xFF18 => 0xFF,
            0xFF19 => 0xBF | (u8::from(self.ch2.length.enabled) << 6),
            0xFF1A => 0x7F | (u8::from(self.ch3.dac) << 7),
            0xFF1B => 0xFF,
            0xFF1C => 0x9F | (self.ch3.volume_code << 5),
            0xFF1D => 0xFF,
            0xFF1E => 0xBF | (u8::from(self.ch3.length.enabled) << 6),
            0xFF20 => 0xFF,
            0xFF21 => self.ch4.envelope.read(),
            0xFF22 => self.ch4.read_nr43(),
            0xFF23 => 0xBF | (u8::from(self.ch4.length.enabled) << 6),
            0xFF24 => self.nr50,
            0xFF25 => self.nr51,
            0xFF26 => {
                0x70 | (u8::from(self.power) << 7)
                    | u8::from(self.ch1.enabled)
                    | (u8::from(self.ch2.enabled) << 1)
                    | (u8::from(self.ch3.enabled) << 2)
                    | (u8::from(self.ch4.enabled) << 3)
            }
            0xFF30..=0xFF3F => self.ch3.read_ram(usize::from(addr - 0xFF30)),
            // FF15, FF1F, FF27-FF2F: unmapped, read 0xFF.
            _ => 0xFF,
        }
    }

    /// Write FF10-FF3F.
    pub fn write(&mut self, addr: u16, value: u8) {
        // NR52 and wave RAM work regardless of the power state.
        if addr == 0xFF26 {
            self.write_nr52(value);
            return;
        }
        if let 0xFF30..=0xFF3F = addr {
            self.ch3.write_ram(usize::from(addr - 0xFF30), value);
            return;
        }
        if !self.power {
            // While powered off, registers ignore writes — except that on
            // DMG the length counters remain writable (the duty bits of
            // NRx1 are NOT stored). Blargg dmg_sound 08-len ctr during power.
            if !self.cgb {
                match addr {
                    0xFF11 => self.ch1.length.load(value & 0x3F),
                    0xFF16 => self.ch2.length.load(value & 0x3F),
                    0xFF1B => self.ch3.length.load(value),
                    0xFF20 => self.ch4.length.load(value & 0x3F),
                    _ => {}
                }
            }
            return;
        }
        let next_clocks = self.next_step_clocks_length();
        match addr {
            0xFF10 => {
                let lf_div = self.lf_div();
                let ds = self.last_double_speed;
                self.ch1.write_nr10(value, lf_div, ds);
            }
            0xFF11 => write_pulse_nrx1(&mut self.ch1, value),
            0xFF12 => write_nrx2(
                &mut self.ch1.envelope,
                &mut self.ch1.dac,
                &mut self.ch1.enabled,
                value,
            ),
            0xFF13 => self.ch1.write_nrx3(value),
            0xFF14 => {
                // `was_active` feeds the sweep trigger tail (SameBoy
                // captures it at the head of the NR14 case; the length
                // extra-clock path cannot kill the channel when the
                // trigger bit is set, so capturing before `write_nrx4`
                // matches).
                let was_active = self.ch1.enabled;
                self.ch1.write_nrx4_freq(value);
                if write_nrx4(
                    &mut self.ch1.length,
                    &mut self.ch1.enabled,
                    value,
                    next_clocks,
                ) {
                    let lf_div = self.lf_div();
                    self.ch1.trigger(lf_div);
                    self.ch1
                        .trigger_sweep(lf_div, was_active, self.cgb, self.last_double_speed);
                }
            }
            0xFF16 => write_pulse_nrx1(&mut self.ch2, value),
            0xFF17 => write_nrx2(
                &mut self.ch2.envelope,
                &mut self.ch2.dac,
                &mut self.ch2.enabled,
                value,
            ),
            0xFF18 => self.ch2.write_nrx3(value),
            0xFF19 => {
                self.ch2.write_nrx4_freq(value);
                if write_nrx4(
                    &mut self.ch2.length,
                    &mut self.ch2.enabled,
                    value,
                    next_clocks,
                ) {
                    self.ch2.trigger(self.lf_div());
                }
            }
            0xFF1A => {
                self.ch3.dac = value & 0x80 != 0;
                if !self.ch3.dac {
                    self.ch3.enabled = false;
                }
            }
            0xFF1B => self.ch3.length.load(value),
            0xFF1C => self.ch3.volume_code = (value >> 5) & 3,
            0xFF1D => write_freq_low(&mut self.ch3.freq, value),
            0xFF1E => {
                write_freq_high(&mut self.ch3.freq, value);
                if write_nrx4(
                    &mut self.ch3.length,
                    &mut self.ch3.enabled,
                    value,
                    next_clocks,
                ) {
                    self.ch3.trigger();
                }
            }
            0xFF20 => self.ch4.length.load(value & 0x3F),
            0xFF21 => write_nrx2(
                &mut self.ch4.envelope,
                &mut self.ch4.dac,
                &mut self.ch4.enabled,
                value,
            ),
            0xFF22 => self.ch4.write_nr43(value),
            // Channel 4 has no frequency; NR44 is trigger/length only.
            0xFF23 => {
                if write_nrx4(
                    &mut self.ch4.length,
                    &mut self.ch4.enabled,
                    value,
                    next_clocks,
                ) {
                    self.ch4.trigger(!self.cgb, self.last_double_speed);
                }
            }
            0xFF24 => self.nr50 = value,
            0xFF25 => self.nr51 = value,
            // FF15, FF1F, FF27-FF2F: unmapped.
            _ => {}
        }
    }

    fn write_nr52(&mut self, value: u8) {
        let on = value & 0x80 != 0;
        if self.power && !on {
            self.power_off();
        } else if !self.power && on {
            self.power_on();
        }
    }

    /// NR52 bit 7 cleared: zero every register FF10-FF25 and stop all
    /// channels. On DMG the length counters survive; on CGB they are
    /// cleared too. Wave RAM is unaffected.
    fn power_off(&mut self) {
        self.power = false;
        let clear_len = self.cgb;
        self.ch1.power_off(clear_len);
        self.ch2.power_off(clear_len);
        self.ch3.power_off(clear_len);
        self.ch4.power_off(clear_len);
        self.nr50 = 0;
        self.nr51 = 0;
    }

    /// NR52 bit 7 set: the divider chain restarts (div_divider, the 2 MHz
    /// phase), the pulse duty units restart, and the wave sample buffer is
    /// cleared (gbdev wiki, "Power Control"; SameBoy `GB_apu_init` runs on
    /// every NR52 power-on and resets `lf_div`/`div_divider`).
    ///
    /// Power-on glitch (see [`SkipDivEvent`]): if the DIV-APU input bit is
    /// HIGH right now, the first DIV-APU event is skipped and the divider
    /// parity starts shifted — div_divider = 1 like SameBoy — which also
    /// flips the NRx4 "extra length clock" phase.
    fn power_on(&mut self) {
        self.power = true;
        self.phase = 2; // divider chain reset: lf_div restarts at 1
        let bit = if self.last_double_speed { 13 } else { 12 };
        if (self.prev_div >> bit) & 1 == 1 {
            self.skip_div_event = SkipDivEvent::Skip;
            self.div_divider = 1;
        } else {
            self.skip_div_event = SkipDivEvent::Inactive;
            self.div_divider = 0;
        }
        self.ch1.duty_pos = 0;
        self.ch2.duty_pos = 0;
        self.ch3.sample_byte = 0;
    }

    /// Mute or un-mute one APU channel (1-4) in the mixer. A frontend/
    /// debugger control, not hardware: see [`Self::mute_mask`]. Channels
    /// outside 1..=4 are ignored.
    pub fn set_channel_mute(&mut self, channel: u8, muted: bool) {
        if let 1..=4 = channel {
            let bit = 1u8 << (channel - 1);
            if muted {
                self.mute_mask |= bit;
            } else {
                self.mute_mask &= !bit;
            }
        }
    }

    /// Whether channel `channel` (1-4) is currently muted by
    /// [`Self::set_channel_mute`]. Out-of-range channels read `false`.
    #[must_use]
    pub fn channel_muted(&self, channel: u8) -> bool {
        matches!(channel, 1..=4) && self.mute_mask & (1 << (channel - 1)) != 0
    }

    /// The raw 16 stored wave-RAM bytes (FF30-FF3F), for the debug I/O viewer.
    /// Bypasses the CPU read gating of [`Self::read`] (which returns 0xFF or the
    /// volatile current sample byte while the channel plays). Side-effect-free.
    #[must_use]
    pub fn wave_ram(&self) -> [u8; 16] {
        self.ch3.ram()
    }

    /// Output sample rate for [`Self::drain_samples`]. Default
    /// [`DEFAULT_SAMPLE_RATE`].
    pub fn set_sample_rate(&mut self, hz: u32) {
        let hz = hz.max(1);
        self.cycles_per_sample = f64::from(crate::CLOCK_HZ) / f64::from(hz);
        self.max_samples = hz as usize;
        // Blargg measured the DMG output capacitor as a charge factor of
        // ~0.999958 per T-cycle; scale it to one factor per output sample.
        self.hp_charge = 0.999_958_f64.powf(self.cycles_per_sample) as f32;
        self.sample_frac = 0.0;
        self.sum_l = 0.0;
        self.sum_r = 0.0;
        self.sum_count = 0;
        // Restart the output stage cleanly: samples already queued at the
        // old rate and the capacitor charge (scaled per-sample, so wrong
        // for the new rate) must not leak into the new stream.
        self.hp_cap_l = 0.0;
        self.hp_cap_r = 0.0;
        self.samples.clear();
    }

    /// Move all accumulated stereo samples into `out`.
    pub fn drain_samples(&mut self, out: &mut Vec<(f32, f32)>) {
        out.append(&mut self.samples);
    }

    /// Move the raw audio tap into `out`: one stereo sample per dot,
    /// captured in [`Self::output_cycle`] straight off the channel mixer
    /// *before* the box-average resampler and the high-pass "output
    /// capacitor". This is the stream gambatte's testrunner inspects for
    /// its `_outaudio` sample-equality verdicts — equality there must not
    /// be broken by a decaying high-pass tail (false "sound") or created
    /// by the filter flattening distinct inputs (false "silence"). Capped
    /// at [`RAW_SAMPLE_CAP`]; drain right before the frame under test.
    pub fn drain_raw_samples(&mut self, out: &mut Vec<(f32, f32)>) {
        out.append(&mut self.raw_samples);
    }

    /// Accumulate one T-cycle of output; emit an averaged sample whenever
    /// `CLOCK_HZ / sample_rate` cycles have been gathered.
    fn output_cycle(&mut self) {
        let (l, r) = self.mix();
        if self.raw_samples.len() < RAW_SAMPLE_CAP {
            self.raw_samples.push((l, r));
        }
        self.sum_l += l;
        self.sum_r += r;
        self.sum_count += 1;
        self.sample_frac += 1.0;
        if self.sample_frac >= self.cycles_per_sample {
            self.sample_frac -= self.cycles_per_sample;
            let n = self.sum_count as f32;
            let l = self.sum_l / n;
            let r = self.sum_r / n;
            self.sum_l = 0.0;
            self.sum_r = 0.0;
            self.sum_count = 0;
            let l = high_pass(&mut self.hp_cap_l, l, self.hp_charge);
            let r = high_pass(&mut self.hp_cap_r, r, self.hp_charge);
            // Drop new samples once one second of audio has piled up: a
            // consumer that far behind has lost real-time anyway, and
            // headless runs (e.g. the mooneye harness) never drain at all.
            if self.samples.len() < self.max_samples {
                self.samples.push((l, r));
            }
        }
    }

    /// CGB PCM12 (FF76): channel 1 digital output in the low nibble,
    /// channel 2 in the high nibble (Pan Docs "PCM amplitude readouts").
    /// A channel with its DAC off reads 0.
    pub fn pcm12(&self) -> u8 {
        let c1 = if self.ch1.dac { self.ch1.digital() } else { 0 };
        let c2 = if self.ch2.dac { self.ch2.digital() } else { 0 };
        c1 | (c2 << 4)
    }

    /// CGB PCM34 (FF77): channel 3 low nibble, channel 4 high nibble.
    pub fn pcm34(&self) -> u8 {
        let c3 = if self.ch3.dac { self.ch3.digital() } else { 0 };
        let c4 = if self.ch4.dac { self.ch4.digital() } else { 0 };
        c3 | (c4 << 4)
    }

    /// Sum one channel into the stereo accumulators per NR51 routing.
    /// `ch` is the channel index 0-3 selecting the NR51 bits.
    fn mix_channel(&self, dac: bool, digital: u8, ch: u8, left: &mut f32, right: &mut f32) {
        if !dac || self.mute_mask & (1 << ch) != 0 {
            // DAC off, or muted by the frontend ([`Self::mute_mask`]): the
            // channel contributes nothing at all.
            return;
        }
        // DAC: digital 0-15 to analog with a *negative* slope — Pan
        // Docs "Audio Details" (DACs): digital 0 maps to analog +1,
        // digital 15 to analog -1. (A disabled channel with a live DAC
        // outputs digital 0, i.e. a DC offset — that is hardware.)
        let analog = 1.0 - f32::from(digital) / 7.5;
        if self.nr51 & (0x10 << ch) != 0 {
            *left += analog;
        }
        if self.nr51 & (0x01 << ch) != 0 {
            *right += analog;
        }
    }

    /// Instantaneous analog output of both terminals, each in [-1, 1].
    /// Runs every T-cycle, hence the straight per-channel calls instead of
    /// building per-call channel arrays.
    fn mix(&self) -> (f32, f32) {
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        self.mix_channel(self.ch1.dac, self.ch1.digital(), 0, &mut left, &mut right);
        self.mix_channel(self.ch2.dac, self.ch2.digital(), 1, &mut left, &mut right);
        self.mix_channel(self.ch3.dac, self.ch3.digital(), 2, &mut left, &mut right);
        self.mix_channel(self.ch4.dac, self.ch4.digital(), 3, &mut left, &mut right);
        // NR50 master volume scales by (vol+1)/8 — it never mutes. The
        // extra /4 normalises the 4-channel sum into [-1, 1].
        let lvol = f32::from((self.nr50 >> 4) & 7) + 1.0;
        let rvol = f32::from(self.nr50 & 7) + 1.0;
        (left * lvol / 32.0, right * rvol / 32.0)
    }
}

// --- Save state (see `crate::state`). The output-stage config
// (cycles_per_sample / max_samples) is NOT serialized: it is re-derived from
// the live sample rate, so a state loads at the host's current rate. ---
impl Apu {
    pub(super) fn write_state(&self, w: &mut crate::state::Writer) {
        w.bool(self.cgb);
        w.bool(self.power);
        self.ch1.write_state(w);
        self.ch2.write_state(w);
        self.ch3.write_state(w);
        self.ch4.write_state(w);
        w.u8(self.nr50);
        w.u8(self.nr51);
        w.u8(self.mute_mask);
        w.u8(self.div_divider);
        w.u8(match self.skip_div_event {
            SkipDivEvent::Inactive => 0,
            SkipDivEvent::Skip => 1,
            SkipDivEvent::Skipped => 2,
        });
        w.u8(self.phase);
        w.u16(self.prev_div);
        w.bool(self.last_double_speed);
        w.u64(self.sample_frac.to_bits());
        w.u32(self.sum_l.to_bits());
        w.u32(self.sum_r.to_bits());
        w.u32(self.sum_count);
        w.u32(self.hp_charge.to_bits());
        w.u32(self.hp_cap_l.to_bits());
        w.u32(self.hp_cap_r.to_bits());
        // `samples`/`raw_samples` are the drained-per-frame OUTPUT queues, not
        // emulation state — a save must not carry them (raw_samples alone caps
        // at ~2 frames ≈ 1 MB of transient audio). Reset empty on load; the
        // stream resumes fresh, an imperceptible gap. (cf. `cycles_per_sample`,
        // also re-derived not serialized.)
    }
    pub(super) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.cgb = r.bool()?;
        self.power = r.bool()?;
        self.ch1.read_state(r)?;
        self.ch2.read_state(r)?;
        self.ch3.read_state(r)?;
        self.ch4.read_state(r)?;
        self.nr50 = r.u8()?;
        self.nr51 = r.u8()?;
        self.mute_mask = r.u8()?;
        self.div_divider = r.u8()?;
        self.skip_div_event = match r.u8()? {
            0 => SkipDivEvent::Inactive,
            1 => SkipDivEvent::Skip,
            _ => SkipDivEvent::Skipped,
        };
        self.phase = r.u8()?;
        self.prev_div = r.u16()?;
        self.last_double_speed = r.bool()?;
        self.sample_frac = f64::from_bits(r.u64()?);
        self.sum_l = f32::from_bits(r.u32()?);
        self.sum_r = f32::from_bits(r.u32()?);
        self.sum_count = r.u32()?;
        self.hp_charge = f32::from_bits(r.u32()?);
        self.hp_cap_l = f32::from_bits(r.u32()?);
        self.hp_cap_r = f32::from_bits(r.u32()?);
        // Output queues are not serialized (see `write_state`) — start fresh.
        self.samples.clear();
        self.raw_samples.clear();
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
