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
    /// Next frame-sequencer step (0-7) to run on the next DIV-APU tick.
    /// Steps 0/2/4/6 clock lengths, 2/6 sweep, 7 envelopes.
    fs_step: u8,
    prev_div: u16,
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
/// whose DAC turns off (bits 7-3 all zero) is disabled immediately.
///
/// Envelope "zombie mode" — live volume manipulation when NRx2 is written
/// while the channel is active — is intentionally unimplemented: it is not
/// pinned by mooneye or Blargg dmg_sound, and it varies across hardware
/// revisions (gbdev wiki, Obscure Behavior).
fn write_nrx2(envelope: &mut Envelope, dac: &mut bool, enabled: &mut bool, value: u8) {
    envelope.write(value);
    *dac = envelope.dac_enabled();
    if !*dac {
        *enabled = false;
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
            fs_step: 0,
            prev_div: 0,
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

    /// Advance one M-cycle (4 T-cycles). `div` is the timer's internal DIV
    /// counter after this cycle; `double_speed` selects the DIV-APU bit.
    pub fn tick(&mut self, div: u16, double_speed: bool) {
        // DIV-APU: falling edge of DIV register bit 4 (bit 5 in double
        // speed). DIV is the top byte of the internal counter, so that is
        // bit 12 (13) here — a 512 Hz edge in real time either way.
        let bit = if double_speed { 13 } else { 12 };
        let fell = (self.prev_div >> bit) & 1 == 1 && (div >> bit) & 1 == 0;
        self.prev_div = div;
        if fell && self.power {
            self.frame_sequencer_step();
        }
        // One CPU M-cycle is 4 dots of APU time, 2 in double speed.
        let dots = if double_speed { 2 } else { 4 };
        for _ in 0..dots {
            if self.power {
                self.ch1.step();
                self.ch2.step();
                self.ch3.step();
                self.ch4.step();
            }
            self.output_cycle();
        }
    }

    fn frame_sequencer_step(&mut self) {
        match self.fs_step {
            0 | 4 => self.clock_lengths(),
            2 | 6 => {
                self.clock_lengths();
                self.ch1.sweep_clock();
            }
            7 => {
                self.ch1.envelope.clock();
                self.ch2.envelope.clock();
                self.ch4.envelope.clock();
            }
            _ => {}
        }
        self.fs_step = (self.fs_step + 1) & 7;
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
        self.fs_step % 2 == 0
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
            0xFF10 => self.ch1.write_nr10(value),
            0xFF11 => write_pulse_nrx1(&mut self.ch1, value),
            0xFF12 => write_nrx2(
                &mut self.ch1.envelope,
                &mut self.ch1.dac,
                &mut self.ch1.enabled,
                value,
            ),
            0xFF13 => write_freq_low(&mut self.ch1.freq, value),
            0xFF14 => {
                write_freq_high(&mut self.ch1.freq, value);
                if write_nrx4(
                    &mut self.ch1.length,
                    &mut self.ch1.enabled,
                    value,
                    next_clocks,
                ) {
                    self.ch1.trigger();
                }
            }
            0xFF16 => write_pulse_nrx1(&mut self.ch2, value),
            0xFF17 => write_nrx2(
                &mut self.ch2.envelope,
                &mut self.ch2.dac,
                &mut self.ch2.enabled,
                value,
            ),
            0xFF18 => write_freq_low(&mut self.ch2.freq, value),
            0xFF19 => {
                write_freq_high(&mut self.ch2.freq, value);
                if write_nrx4(
                    &mut self.ch2.length,
                    &mut self.ch2.enabled,
                    value,
                    next_clocks,
                ) {
                    self.ch2.trigger();
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
                    self.ch4.trigger();
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

    /// NR52 bit 7 set: the frame sequencer restarts at step 0, the pulse
    /// duty units restart, and the wave sample buffer is cleared (gbdev
    /// wiki, "Power Control").
    fn power_on(&mut self) {
        self.power = true;
        self.fs_step = 0;
        self.ch1.duty_pos = 0;
        self.ch2.duty_pos = 0;
        self.ch3.sample_byte = 0;
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
        if !dac {
            // DAC off: the channel contributes nothing at all.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives the APU like the interconnect does: one tick per M-cycle with
    /// a DIV counter that advances 4 T-cycles per tick from 0, so a frame-
    /// sequencer DIV-APU edge lands exactly every 2048 ticks.
    struct H {
        apu: Apu,
        div: u16,
    }

    impl H {
        fn dmg() -> Self {
            H {
                apu: Apu::new(false),
                div: 0,
            }
        }

        fn cgb() -> Self {
            H {
                apu: Apu::new(true),
                div: 0,
            }
        }

        fn tick(&mut self) {
            self.div = self.div.wrapping_add(4);
            self.apu.tick(self.div, false);
        }

        fn ticks(&mut self, n: u32) {
            for _ in 0..n {
                self.tick();
            }
        }

        /// Advance exactly one frame-sequencer edge.
        fn fs_edge(&mut self) {
            self.ticks(2048);
        }

        fn w(&mut self, addr: u16, v: u8) {
            self.apu.write(addr, v);
        }

        fn r(&self, addr: u16) -> u8 {
            self.apu.read(addr)
        }

        fn ch_on(&self, ch: u8) -> bool {
            self.r(0xFF26) & (1 << (ch - 1)) != 0
        }

        /// Minimal "channel 1 playing" setup.
        fn start_ch1(&mut self) {
            self.w(0xFF12, 0xF0);
            self.w(0xFF14, 0x80);
        }
    }

    // ---- register read-back masks ----

    const MASKS: [(u16, u8); 22] = [
        (0xFF10, 0x80),
        (0xFF11, 0x3F),
        (0xFF12, 0x00),
        (0xFF13, 0xFF),
        (0xFF14, 0xBF),
        (0xFF15, 0xFF),
        (0xFF16, 0x3F),
        (0xFF17, 0x00),
        (0xFF18, 0xFF),
        (0xFF19, 0xBF),
        (0xFF1A, 0x7F),
        (0xFF1B, 0xFF),
        (0xFF1C, 0x9F),
        (0xFF1D, 0xFF),
        (0xFF1E, 0xBF),
        (0xFF1F, 0xFF),
        (0xFF20, 0xFF),
        (0xFF21, 0x00),
        (0xFF22, 0x00),
        (0xFF23, 0xBF),
        (0xFF24, 0x00),
        (0xFF25, 0x00),
    ];

    #[test]
    fn register_readback_masks_after_writing_zero() {
        for (addr, mask) in MASKS {
            let mut h = H::dmg();
            h.w(addr, 0x00);
            assert_eq!(h.r(addr), mask, "addr {addr:04X}");
        }
    }

    #[test]
    fn register_readback_all_ones_after_writing_ff() {
        for (addr, _) in MASKS {
            let mut h = H::dmg();
            h.w(addr, 0xFF);
            assert_eq!(h.r(addr), 0xFF, "addr {addr:04X}");
        }
    }

    #[test]
    fn unmapped_ff27_to_ff2f_read_ff_and_ignore_writes() {
        let mut h = H::dmg();
        for addr in 0xFF27..=0xFF2F {
            h.w(addr, 0x00);
            assert_eq!(h.r(addr), 0xFF, "addr {addr:04X}");
        }
    }

    #[test]
    fn nr52_reads_70_plus_power_and_status() {
        let mut h = H::dmg();
        assert_eq!(h.r(0xFF26), 0xF0); // powered on, no channels
        h.start_ch1();
        assert_eq!(h.r(0xFF26), 0xF1);
        h.w(0xFF26, 0x00);
        assert_eq!(h.r(0xFF26), 0x70);
        h.w(0xFF26, 0xFF); // only bit 7 is writable
        assert_eq!(h.r(0xFF26), 0xF0);
    }

    #[test]
    fn wave_ram_round_trips_while_channel_off() {
        let mut h = H::dmg();
        for i in 0..16u16 {
            h.w(0xFF30 + i, (i as u8) << 4 | 0x0A);
        }
        for i in 0..16u16 {
            assert_eq!(h.r(0xFF30 + i), (i as u8) << 4 | 0x0A);
        }
    }

    // ---- frame sequencer / DIV-APU ----

    #[test]
    fn fs_edge_is_falling_div_bit_12() {
        let mut h = H::dmg();
        h.ticks(2047);
        assert_eq!(h.apu.fs_step, 0, "no step before DIV bit 4 falls");
        h.tick(); // div: 0x1FFC -> 0x2000
        assert_eq!(h.apu.fs_step, 1);
        h.ticks(2048);
        assert_eq!(h.apu.fs_step, 2);
    }

    #[test]
    fn fs_edge_uses_div_bit_13_in_double_speed() {
        let mut apu = Apu::new(true);
        let mut div = 0u16;
        for _ in 0..4095 {
            div = div.wrapping_add(4);
            apu.tick(div, true);
        }
        assert_eq!(apu.fs_step, 0);
        div = div.wrapping_add(4); // 0x4000: bit 13 falls
        apu.tick(div, true);
        assert_eq!(apu.fs_step, 1);
    }

    #[test]
    fn fs_handles_div_reset_via_stored_previous() {
        // A DIV write resets the counter; if bit 12 was high that is a
        // falling edge, detected by comparing with the stored previous value.
        let mut apu = Apu::new(false);
        apu.tick(0x1000, false); // bit 12 high
        assert_eq!(apu.fs_step, 0);
        apu.tick(0x0004, false); // counter restarted: falling edge
        assert_eq!(apu.fs_step, 1);
    }

    #[test]
    fn length_expiry_disables_channel_at_256hz() {
        let mut h = H::dmg();
        h.w(0xFF12, 0xF0);
        h.w(0xFF11, 64 - 8); // counter 8
        h.w(0xFF14, 0xC0 | 0x80); // trigger + enable; next step (0) clocks
        assert!(h.ch_on(1));
        // Length clocks on edges 1,3,5,7,9,11,13,15 (steps 0,2,4,6,...).
        for _ in 0..14 {
            h.fs_edge();
        }
        assert!(h.ch_on(1), "still alive after 7 length clocks");
        h.fs_edge();
        assert!(!h.ch_on(1), "dead on the 8th length clock");
    }

    #[test]
    fn length_freezes_when_disabled_and_resumes() {
        let mut h = H::dmg();
        h.w(0xFF12, 0xF0);
        h.w(0xFF11, 64 - 4); // counter 4
        h.w(0xFF14, 0xC0 | 0x80); // trigger + enable
        h.fs_edge(); // step 0: counter 3
        assert_eq!(h.apu.ch1.length.counter, 3);
        h.w(0xFF14, 0x00); // disable length
        for _ in 0..16 {
            h.fs_edge();
        }
        assert_eq!(h.apu.ch1.length.counter, 3, "frozen while disabled");
        assert!(h.ch_on(1));
        // Re-enable in a phase where the next FS step clocks length so the
        // NRx4 write itself causes no extra clock, then resume counting.
        h.fs_edge(); // step 1 ran: fs_step is now 2 (next step clocks length)
        assert_eq!(h.apu.fs_step, 2);
        h.w(0xFF14, 0x40); // re-enable length, no trigger
        assert_eq!(h.apu.ch1.length.counter, 3, "no extra clock on re-enable");
        h.fs_edge(); // step 2 clocks length
        assert_eq!(h.apu.ch1.length.counter, 2, "resumes once re-enabled");
        assert!(h.ch_on(1));
    }

    #[test]
    fn sweep_clocks_on_steps_2_and_6() {
        let mut h = H::dmg();
        h.w(0xFF10, 0x11); // period 1, shift 1
        h.w(0xFF12, 0xF0);
        h.w(0xFF13, 0x00);
        h.w(0xFF14, 0x81); // trigger, freq 0x100
        h.fs_edge(); // step 0
        h.fs_edge(); // step 1
        assert_eq!(h.apu.ch1.freq, 0x100, "no sweep before step 2");
        h.fs_edge(); // step 2: sweep
        assert_eq!(h.apu.ch1.freq, 0x180);
        h.ticks(2048 * 3); // steps 3,4,5
        assert_eq!(h.apu.ch1.freq, 0x180);
        h.fs_edge(); // step 6: sweep
        assert_eq!(h.apu.ch1.freq, 0x240);
    }

    #[test]
    fn envelope_clocks_on_step_7() {
        let mut h = H::dmg();
        h.w(0xFF12, 0x19); // volume 1, increase, period 1
        h.w(0xFF14, 0x80);
        for _ in 0..7 {
            h.fs_edge();
        }
        assert_eq!(h.apu.ch1.envelope.volume, 1, "no envelope before step 7");
        h.fs_edge(); // step 7
        assert_eq!(h.apu.ch1.envelope.volume, 2);
        for _ in 0..8 {
            h.fs_edge();
        }
        assert_eq!(h.apu.ch1.envelope.volume, 3, "64 Hz: once per 8 steps");
    }

    // ---- NRx4 length extra-clock matrix through the register interface ----

    /// Put the frame sequencer in the "next step does not clock length"
    /// phase by consuming exactly one edge (fs_step becomes 1).
    fn h_in_no_length_phase() -> H {
        let mut h = H::dmg();
        h.fs_edge();
        assert_eq!(h.apu.fs_step, 1);
        h
    }

    #[test]
    fn enabling_length_in_no_length_phase_extra_clocks() {
        let mut h = h_in_no_length_phase();
        h.w(0xFF12, 0xF0);
        h.w(0xFF11, 63); // counter 1
        h.w(0xFF14, 0x80); // trigger, length disabled
        assert!(h.ch_on(1));
        h.w(0xFF14, 0x40); // enable: extra clock 1 -> 0 kills the channel
        assert!(!h.ch_on(1));
        assert_eq!(h.apu.ch1.length.counter, 0);
    }

    #[test]
    fn enabling_length_in_length_phase_does_not_extra_clock() {
        let mut h = H::dmg(); // fresh: next step is 0 (clocks length)
        h.w(0xFF12, 0xF0);
        h.w(0xFF11, 63); // counter 1
        h.w(0xFF14, 0x80);
        h.w(0xFF14, 0x40);
        assert!(h.ch_on(1));
        assert_eq!(h.apu.ch1.length.counter, 1);
    }

    #[test]
    fn trigger_with_zero_length_reloads_64_or_63() {
        // Phase: next step clocks length -> plain reload of 64.
        let mut h = H::dmg();
        h.w(0xFF12, 0xF0);
        h.w(0xFF14, 0xC0); // enable length with counter 0
        h.w(0xFF14, 0xC0 | 0x80); // trigger
        assert_eq!(h.apu.ch1.length.counter, 64);

        // Phase: next step does not clock length and enable set -> 63.
        let mut h = h_in_no_length_phase();
        h.w(0xFF12, 0xF0);
        h.w(0xFF14, 0xC0 | 0x80);
        assert_eq!(h.apu.ch1.length.counter, 63);

        // Same but enable clear -> 64.
        let mut h = h_in_no_length_phase();
        h.w(0xFF12, 0xF0);
        h.w(0xFF14, 0x80);
        assert_eq!(h.apu.ch1.length.counter, 64);
    }

    #[test]
    fn trigger_plus_enable_with_counter_1_gives_63() {
        // The enable edge clocks 1 -> 0, then the trigger reload gives
        // 64 - 1 = 63 and the channel stays alive.
        let mut h = h_in_no_length_phase();
        h.w(0xFF12, 0xF0);
        h.w(0xFF11, 63); // counter 1
        h.w(0xFF14, 0xC0 | 0x80);
        assert_eq!(h.apu.ch1.length.counter, 63);
        assert!(h.ch_on(1));
    }

    #[test]
    fn wave_length_reloads_256_or_255() {
        let mut h = h_in_no_length_phase();
        h.w(0xFF1A, 0x80);
        h.w(0xFF1E, 0xC0 | 0x80);
        assert_eq!(h.apu.ch3.length.counter, 255);
        let mut h = H::dmg();
        h.w(0xFF1A, 0x80);
        h.w(0xFF1E, 0xC0 | 0x80);
        assert_eq!(h.apu.ch3.length.counter, 256);
    }

    // ---- DAC / NR52 status ----

    #[test]
    fn dac_off_kills_channel_and_trigger_cannot_revive() {
        let mut h = H::dmg();
        h.start_ch1();
        assert!(h.ch_on(1));
        h.w(0xFF12, 0x00); // DAC off
        assert!(!h.ch_on(1));
        h.w(0xFF14, 0x80); // trigger with DAC off
        assert!(!h.ch_on(1));
        // But trigger side effects still ran: zero length reloaded.
        assert_eq!(h.apu.ch1.length.counter, 64);
    }

    #[test]
    fn wave_dac_is_nr30_bit7() {
        let mut h = H::dmg();
        h.w(0xFF1A, 0x80);
        h.w(0xFF1E, 0x80);
        assert!(h.ch_on(3));
        h.w(0xFF1A, 0x00);
        assert!(!h.ch_on(3));
    }

    #[test]
    fn all_four_status_bits() {
        let mut h = H::dmg();
        h.w(0xFF12, 0xF0);
        h.w(0xFF14, 0x80);
        h.w(0xFF17, 0xF0);
        h.w(0xFF19, 0x80);
        h.w(0xFF1A, 0x80);
        h.w(0xFF1E, 0x80);
        h.w(0xFF21, 0xF0);
        h.w(0xFF23, 0x80);
        assert_eq!(h.r(0xFF26), 0xFF);
    }

    // ---- power control ----

    #[test]
    fn power_off_clears_all_registers() {
        let mut h = H::dmg();
        for (addr, _) in MASKS {
            h.w(addr, 0xFF);
        }
        h.w(0xFF26, 0x00);
        h.w(0xFF26, 0x80);
        for (addr, mask) in MASKS {
            assert_eq!(h.r(addr), mask, "addr {addr:04X}");
        }
    }

    #[test]
    fn writes_ignored_while_powered_off() {
        let mut h = H::dmg();
        h.w(0xFF26, 0x00);
        h.w(0xFF12, 0xF0);
        h.w(0xFF24, 0x77);
        h.w(0xFF25, 0xFF);
        h.w(0xFF26, 0x80);
        assert_eq!(h.r(0xFF12), 0x00);
        assert_eq!(h.r(0xFF24), 0x00);
        assert_eq!(h.r(0xFF25), 0x00);
    }

    #[test]
    fn dmg_length_counters_writable_while_off() {
        let mut h = H::dmg();
        h.w(0xFF26, 0x00);
        h.w(0xFF11, 64 - 12);
        h.w(0xFF1B, 0x00); // wave: 256
        assert_eq!(h.apu.ch1.length.counter, 12);
        assert_eq!(h.apu.ch3.length.counter, 256);
        // The duty bits are NOT stored.
        h.w(0xFF26, 0x80);
        assert_eq!(h.r(0xFF11), 0x3F);
    }

    #[test]
    fn cgb_length_writes_ignored_and_counters_cleared_while_off() {
        let mut h = H::cgb();
        h.w(0xFF11, 64 - 12); // counter 12 while on
        h.w(0xFF26, 0x00);
        assert_eq!(h.apu.ch1.length.counter, 0, "CGB power-off clears");
        h.w(0xFF11, 64 - 30);
        assert_eq!(h.apu.ch1.length.counter, 0, "write while off ignored");
    }

    #[test]
    fn dmg_length_counters_survive_power_off() {
        let mut h = H::dmg();
        h.w(0xFF11, 64 - 12);
        h.w(0xFF26, 0x00);
        assert_eq!(h.apu.ch1.length.counter, 12);
    }

    #[test]
    fn power_on_resets_frame_sequencer_duty_and_wave_buffer() {
        let mut h = H::dmg();
        h.start_ch1();
        h.ticks(2048 * 3 + 100); // fs_step 3, duty somewhere
        h.apu.ch3.sample_byte = 0xAA;
        h.w(0xFF26, 0x00);
        h.w(0xFF26, 0x80);
        assert_eq!(h.apu.fs_step, 0);
        assert_eq!(h.apu.ch1.duty_pos, 0);
        assert_eq!(h.apu.ch2.duty_pos, 0);
        assert_eq!(h.apu.ch3.sample_byte, 0);
    }

    #[test]
    fn frame_sequencer_does_not_run_while_off() {
        let mut h = H::dmg();
        h.w(0xFF26, 0x00);
        // Re-arm a length counter on DMG and make sure nothing clocks it.
        h.w(0xFF11, 63);
        for _ in 0..32 {
            h.fs_edge();
        }
        assert_eq!(h.apu.ch1.length.counter, 1);
        assert_eq!(h.apu.fs_step, 0);
    }

    #[test]
    fn wave_ram_writable_while_powered_off() {
        let mut h = H::dmg();
        h.w(0xFF26, 0x00);
        h.w(0xFF30, 0x12);
        assert_eq!(h.r(0xFF30), 0x12);
        h.w(0xFF26, 0x80);
        assert_eq!(h.r(0xFF30), 0x12, "wave RAM survives power off");
    }

    // ---- wave channel through the bus interface ----

    #[test]
    fn wave_ram_reads_current_byte_at_max_frequency_on_dmg() {
        let mut h = H::dmg();
        for i in 0..16u16 {
            h.w(0xFF30 + i, i as u8);
        }
        h.w(0xFF1A, 0x80);
        h.w(0xFF1C, 0x20);
        h.w(0xFF1D, 0xFF);
        h.w(0xFF1E, 0x87); // trigger, freq 0x7FF: fetch every 2 T-cycles
        h.ticks(2); // 8 T: first fetch happened
        let current = h.apu.ch3.ram[usize::from(h.apu.ch3.position >> 1)];
        for i in 0..16u16 {
            assert_eq!(h.r(0xFF30 + i), current);
        }
    }

    #[test]
    fn wave_ram_reads_ff_at_low_frequency_on_dmg() {
        let mut h = H::dmg();
        h.w(0xFF1A, 0x80);
        h.w(0xFF1D, 0x00);
        h.w(0xFF1E, 0x80); // freq 0: period 4096
        h.ticks(4);
        assert_eq!(h.r(0xFF30), 0xFF);
        h.w(0xFF30, 0x55);
        assert_eq!(h.apu.ch3.ram[0], 0x00, "write lost outside window");
    }

    #[test]
    fn wave_retrigger_corrupts_ram_on_dmg_only() {
        for cgb in [false, true] {
            let mut h = if cgb { H::cgb() } else { H::dmg() };
            for i in 0..16u16 {
                h.w(0xFF30 + i, i as u8);
            }
            h.w(0xFF1A, 0x80);
            h.w(0xFF1D, 0xFF);
            h.w(0xFF1E, 0x87);
            h.ticks(3); // 12 T: position 3, fetch just happened
            h.w(0xFF1E, 0x87); // retrigger: next read would be byte 2
            if cgb {
                assert_eq!(h.apu.ch3.ram[0], 0, "no corruption on CGB");
            } else {
                assert_eq!(h.apu.ch3.ram[0], 2, "byte 0 takes the read byte");
            }
        }
    }

    // ---- output stage ----

    #[test]
    fn default_sample_rate_produces_48000_per_second() {
        let mut h = H::dmg();
        h.ticks(1_048_576); // one second of M-cycles
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        assert!((47999..=48001).contains(&out.len()), "got {}", out.len());
    }

    #[test]
    fn set_sample_rate_changes_output_rate() {
        let mut h = H::dmg();
        h.apu.set_sample_rate(22050);
        h.ticks(1_048_576);
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        assert!((22049..=22051).contains(&out.len()), "got {}", out.len());
    }

    #[test]
    fn set_sample_rate_resets_capacitors_and_drops_stale_samples() {
        let mut h = H::dmg();
        h.w(0xFF24, 0x77);
        h.w(0xFF25, 0xFF);
        h.w(0xFF12, 0xF0); // ch1 DAC on: a DC offset charges the capacitors
        h.ticks(10_000);
        assert!(!h.apu.samples.is_empty());
        assert_ne!(h.apu.hp_cap_l, 0.0);
        assert_ne!(h.apu.hp_cap_r, 0.0);
        // A mid-run rate change must not mix stale state into the new
        // stream: pending samples at the old rate are dropped and the
        // high-pass capacitors restart discharged.
        h.apu.set_sample_rate(22_050);
        assert!(h.apu.samples.is_empty(), "stale samples must be dropped");
        assert_eq!(h.apu.hp_cap_l, 0.0);
        assert_eq!(h.apu.hp_cap_r, 0.0);
    }

    #[test]
    fn drain_moves_the_buffer() {
        let mut h = H::dmg();
        h.ticks(10_000);
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        assert!(!out.is_empty());
        let n = out.len();
        h.apu.drain_samples(&mut out);
        assert_eq!(out.len(), n, "second drain adds nothing");
    }

    #[test]
    fn silence_when_all_dacs_off() {
        let mut h = H::dmg();
        h.w(0xFF24, 0x77);
        h.w(0xFF25, 0xFF);
        h.ticks(50_000);
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        assert!(out.iter().all(|&(l, r)| l == 0.0 && r == 0.0));
    }

    #[test]
    fn playing_pulse_is_audible_and_routed_by_nr51() {
        let mut h = H::dmg();
        h.w(0xFF24, 0x77);
        h.w(0xFF25, 0x10); // ch1 left only
        h.w(0xFF11, 0x80); // 50% duty
        h.w(0xFF12, 0xF0);
        h.w(0xFF13, 0x00);
        h.w(0xFF14, 0x84); // trigger, freq 0x400: audible period
        h.ticks(100_000);
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        let energy_l: f32 = out.iter().map(|&(l, _)| l * l).sum();
        let energy_r: f32 = out.iter().map(|&(_, r)| r * r).sum();
        assert!(energy_l > 1.0, "left should carry the square wave");
        assert!(
            energy_r < energy_l / 100.0,
            "right is unrouted: {energy_r} vs {energy_l}"
        );
    }

    #[test]
    fn nr50_zero_does_not_mute() {
        let mut h = H::dmg();
        h.w(0xFF24, 0x00); // volume 0 = gain 1/8
        h.w(0xFF25, 0xFF);
        h.w(0xFF11, 0x80);
        h.w(0xFF12, 0xF0);
        h.w(0xFF14, 0x84);
        h.ticks(100_000);
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        let energy: f32 = out.iter().map(|&(l, _)| l * l).sum();
        assert!(energy > 0.01, "NR50 never mutes, got {energy}");
    }

    #[test]
    fn sample_buffer_is_capped_without_a_consumer() {
        // Headless runs (the mooneye harness never drains audio) must not
        // grow the buffer without bound: capped at one second of audio.
        let mut h = H::dmg();
        h.apu.set_sample_rate(1000);
        h.ticks(2 * 1_048_576); // two emulated seconds, never drained
        assert_eq!(h.apu.samples.len(), 1000);
        // Draining frees the cap and output resumes.
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        assert_eq!(out.len(), 1000);
        h.ticks(10_000);
        assert!(!h.apu.samples.is_empty());
    }

    #[test]
    fn dac_maps_digital_zero_to_positive_analog() {
        // Pan Docs "Audio Details" (DACs): the DAC slope is negative —
        // digital 0 is analog +1, digital 15 is analog -1. A live DAC on a
        // silent channel is therefore a *positive* DC offset.
        let mut h = H::dmg();
        h.w(0xFF24, 0x77);
        h.w(0xFF25, 0xFF);
        h.w(0xFF12, 0xF0); // ch1 DAC on, channel not triggered: digital 0
        h.ticks(100);
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        let first = out[0].0;
        assert!(first > 0.05, "digital 0 must map to analog +1, got {first}");
    }

    #[test]
    fn pcm_readouts_expose_channel_digital_outputs() {
        // Pan Docs "PCM amplitude readouts": PCM12 low nibble = ch1 digital
        // output, high nibble = ch2; PCM34 likewise for ch3/ch4. DAC-off
        // channels read 0.
        let mut h = H::dmg();
        assert_eq!(h.apu.pcm12(), 0x00, "all DACs off at power-on");
        assert_eq!(h.apu.pcm34(), 0x00);
        h.w(0xFF24, 0x77);
        h.w(0xFF25, 0xFF);
        // ch2: max volume, no envelope; duty 2 (50%); trigger.
        h.w(0xFF17, 0xF0);
        h.w(0xFF18, 0x00);
        h.w(0xFF19, 0x87);
        // A full duty cycle is 8 steps of (2048-1024)*4 T-cycles; sample the
        // high nibble across one cycle and expect both 0 and 15 phases.
        let mut seen = [false; 16];
        for _ in 0..8 * 1024 {
            h.apu.tick(0, false);
            seen[usize::from(h.apu.pcm12() >> 4)] = true;
        }
        assert!(seen[0] && seen[15], "50% duty must swing 0<->15: {seen:?}");
        assert_eq!(h.apu.pcm12() & 0x0F, 0, "ch1 DAC off reads 0");
    }

    #[test]
    fn high_pass_removes_dc_offset() {
        // A DAC turned on with the channel silent is a pure DC offset; the
        // output capacitor must drain it to (near) zero.
        let mut h = H::dmg();
        h.w(0xFF24, 0x77);
        h.w(0xFF25, 0xFF);
        h.w(0xFF12, 0xF0); // ch1 DAC on, channel not triggered
        h.ticks(1_048_576); // one second
        let mut out = Vec::new();
        h.apu.drain_samples(&mut out);
        let tail = &out[out.len() - 100..];
        assert!(
            tail.iter().all(|&(l, r)| l.abs() < 0.01 && r.abs() < 0.01),
            "DC offset must decay"
        );
        // ...but the first samples did see the offset (DAC actually mixes).
        assert!(out[0].0.abs() > 0.05);
    }

    #[test]
    fn raw_tap_is_pre_average_pre_high_pass() {
        // Constant DC input (DAC on, channel silent): the raw pre-filter
        // tap must report bit-identical samples for the whole run —
        // gambatte's testrunner judges silence by raw-sample equality —
        // while the filtered drain_samples output decays through the
        // output capacitor (i.e. varies).
        let mut h = H::dmg();
        h.w(0xFF24, 0x77);
        h.w(0xFF25, 0xFF);
        h.w(0xFF12, 0xF0); // ch1 DAC on, channel not triggered -> pure DC
        h.ticks(8192);
        let mut raw = Vec::new();
        h.apu.drain_raw_samples(&mut raw);
        assert_eq!(raw.len(), 8192 * 4, "one raw sample per dot");
        let (l0, r0) = raw[0];
        assert!(l0 != 0.0, "the DC offset must reach the tap");
        assert!(
            raw.iter()
                .all(|&(l, r)| l.to_bits() == l0.to_bits() && r.to_bits() == r0.to_bits()),
            "raw samples must be bit-identical under constant DC"
        );
        let mut filtered = Vec::new();
        h.apu.drain_samples(&mut filtered);
        let f0 = filtered[0].0;
        assert!(
            filtered.iter().any(|&(l, _)| l.to_bits() != f0.to_bits()),
            "high-passed output must decay (vary) under constant DC"
        );
    }

    #[test]
    fn raw_tap_is_capped_and_draining_restarts_collection() {
        let mut h = H::dmg();
        // Run far past the cap: the buffer must stop growing, not OOM.
        h.ticks(RAW_SAMPLE_CAP as u32 / 4 + 10_000);
        assert_eq!(h.apu.raw_samples.len(), RAW_SAMPLE_CAP);
        let mut out = Vec::new();
        h.apu.drain_raw_samples(&mut out);
        assert_eq!(out.len(), RAW_SAMPLE_CAP);
        assert!(h.apu.raw_samples.is_empty());
        // Collection resumes after a drain (the gambatte harness drains the
        // 15 warm-up frames, then captures exactly the final frame).
        h.ticks(100);
        assert_eq!(h.apu.raw_samples.len(), 400);
    }

    #[test]
    fn double_speed_ticks_advance_two_dots() {
        // 4096 ticks at double speed = 8192 dots = 8192/87.38 samples.
        let mut apu = Apu::new(true);
        let mut div = 0u16;
        for _ in 0..524_288 {
            div = div.wrapping_add(4);
            apu.tick(div, true);
        }
        let mut out = Vec::new();
        apu.drain_samples(&mut out);
        // 524288 M-cycles * 2 dots = 1048576 dots = 0.25 s = 12000 samples.
        assert!((11999..=12001).contains(&out.len()), "got {}", out.len());
    }

    // ---- misc cross-checks ----

    #[test]
    fn nrx3_writes_change_frequency_low_bits() {
        let mut h = H::dmg();
        h.w(0xFF13, 0xAB);
        h.w(0xFF14, 0x05);
        assert_eq!(h.apu.ch1.freq, 0x5AB);
        h.w(0xFF18, 0x34);
        h.w(0xFF19, 0x02);
        assert_eq!(h.apu.ch2.freq, 0x234);
        h.w(0xFF1D, 0xCD);
        h.w(0xFF1E, 0x07);
        assert_eq!(h.apu.ch3.freq, 0x7CD);
    }

    #[test]
    fn sweep_overflow_on_trigger_clears_status_bit() {
        let mut h = H::dmg();
        h.w(0xFF10, 0x11);
        h.w(0xFF12, 0xF0);
        h.w(0xFF13, 0x80);
        h.w(0xFF14, 0x87); // freq 0x780 = 1920: overflows immediately
        assert!(!h.ch_on(1));
    }

    #[test]
    fn noise_length_works_via_nr44() {
        let mut h = H::dmg();
        h.w(0xFF21, 0xF0);
        h.w(0xFF20, 63); // counter 1
        h.w(0xFF23, 0xC0 | 0x80); // trigger + enable (phase: step 0 next)
        assert!(h.ch_on(4));
        h.fs_edge();
        assert!(!h.ch_on(4));
    }
}
