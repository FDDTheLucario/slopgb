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

use noise::Noise;
use pulse::Pulse;
use wave::Wave;

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
}

/// Blargg-style single-pole high-pass ("the output capacitor").
fn high_pass(cap: &mut f32, input: f32, charge: f32) -> f32 {
    let out = input - *cap;
    *cap = input - out * charge;
    out
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
        };
        apu.set_sample_rate(48000);
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
        if self.ch1.length.clock() {
            self.ch1.enabled = false;
        }
        if self.ch2.length.clock() {
            self.ch2.enabled = false;
        }
        if self.ch3.length.clock() {
            self.ch3.enabled = false;
        }
        if self.ch4.length.clock() {
            self.ch4.enabled = false;
        }
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
        match addr {
            0xFF10 => self.ch1.write_nr10(value),
            0xFF11 => {
                self.ch1.duty = value >> 6;
                self.ch1.length.load(value & 0x3F);
            }
            0xFF12 => {
                self.ch1.envelope.write(value);
                self.ch1.dac = self.ch1.envelope.dac_enabled();
                if !self.ch1.dac {
                    self.ch1.enabled = false;
                }
            }
            0xFF13 => self.ch1.freq = (self.ch1.freq & 0x0700) | u16::from(value),
            0xFF14 => {
                self.ch1.freq = (self.ch1.freq & 0x00FF) | (u16::from(value & 7) << 8);
                let next_clocks = self.next_step_clocks_length();
                let trigger = value & 0x80 != 0;
                if self
                    .ch1
                    .length
                    .write_nrx4(value & 0x40 != 0, trigger, next_clocks)
                {
                    self.ch1.enabled = false;
                }
                if trigger {
                    self.ch1.trigger();
                }
            }
            0xFF16 => {
                self.ch2.duty = value >> 6;
                self.ch2.length.load(value & 0x3F);
            }
            0xFF17 => {
                self.ch2.envelope.write(value);
                self.ch2.dac = self.ch2.envelope.dac_enabled();
                if !self.ch2.dac {
                    self.ch2.enabled = false;
                }
            }
            0xFF18 => self.ch2.freq = (self.ch2.freq & 0x0700) | u16::from(value),
            0xFF19 => {
                self.ch2.freq = (self.ch2.freq & 0x00FF) | (u16::from(value & 7) << 8);
                let next_clocks = self.next_step_clocks_length();
                let trigger = value & 0x80 != 0;
                if self
                    .ch2
                    .length
                    .write_nrx4(value & 0x40 != 0, trigger, next_clocks)
                {
                    self.ch2.enabled = false;
                }
                if trigger {
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
            0xFF1D => self.ch3.freq = (self.ch3.freq & 0x0700) | u16::from(value),
            0xFF1E => {
                self.ch3.freq = (self.ch3.freq & 0x00FF) | (u16::from(value & 7) << 8);
                let next_clocks = self.next_step_clocks_length();
                let trigger = value & 0x80 != 0;
                if self
                    .ch3
                    .length
                    .write_nrx4(value & 0x40 != 0, trigger, next_clocks)
                {
                    self.ch3.enabled = false;
                }
                if trigger {
                    self.ch3.trigger();
                }
            }
            0xFF20 => self.ch4.length.load(value & 0x3F),
            0xFF21 => {
                self.ch4.envelope.write(value);
                self.ch4.dac = self.ch4.envelope.dac_enabled();
                if !self.ch4.dac {
                    self.ch4.enabled = false;
                }
            }
            0xFF22 => self.ch4.write_nr43(value),
            0xFF23 => {
                let next_clocks = self.next_step_clocks_length();
                let trigger = value & 0x80 != 0;
                if self
                    .ch4
                    .length
                    .write_nrx4(value & 0x40 != 0, trigger, next_clocks)
                {
                    self.ch4.enabled = false;
                }
                if trigger {
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

    /// Output sample rate for [`Self::drain_samples`]. Default 48000.
    pub fn set_sample_rate(&mut self, hz: u32) {
        let hz = hz.max(1);
        self.cycles_per_sample = f64::from(crate::CLOCK_HZ) / f64::from(hz);
        // Blargg measured the DMG output capacitor as a charge factor of
        // ~0.999958 per T-cycle; scale it to one factor per output sample.
        self.hp_charge = 0.999_958_f64.powf(self.cycles_per_sample) as f32;
        self.sample_frac = 0.0;
        self.sum_l = 0.0;
        self.sum_r = 0.0;
        self.sum_count = 0;
    }

    /// Move all accumulated stereo samples into `out`.
    pub fn drain_samples(&mut self, out: &mut Vec<(f32, f32)>) {
        out.append(&mut self.samples);
    }

    /// Accumulate one T-cycle of output; emit an averaged sample whenever
    /// `CLOCK_HZ / sample_rate` cycles have been gathered.
    fn output_cycle(&mut self) {
        let (l, r) = self.mix();
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
            self.samples.push((l, r));
        }
    }

    /// Instantaneous analog output of both terminals, each in [-1, 1].
    fn mix(&self) -> (f32, f32) {
        let digital = [
            self.ch1.digital(),
            self.ch2.digital(),
            self.ch3.digital(),
            self.ch4.digital(),
        ];
        let dac_on = [self.ch1.dac, self.ch2.dac, self.ch3.dac, self.ch4.dac];
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for (i, (&d, &on)) in digital.iter().zip(&dac_on).enumerate() {
            if !on {
                // DAC off: the channel contributes nothing at all.
                continue;
            }
            // DAC: digital 0-15 to analog. (A disabled channel with a live
            // DAC outputs digital 0, i.e. a DC offset — that is hardware.)
            let analog = f32::from(d) / 7.5 - 1.0;
            if self.nr51 & (0x10 << i) != 0 {
                left += analog;
            }
            if self.nr51 & (0x01 << i) != 0 {
                right += analog;
            }
        }
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
            let h = {
                let mut h = H::dmg();
                h.w(addr, 0x00);
                h
            };
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
