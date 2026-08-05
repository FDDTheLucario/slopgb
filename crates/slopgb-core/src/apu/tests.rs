// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Unit tests for the APU. Split out of `mod.rs` for file size;
//! compiled as `super::tests` via the `#[path]` attribute there.

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

/// Power the APU off and back on via NR52 with DIV-APU bit 12 HIGH.
fn power_cycle_with_div_bit_high() -> H {
    let mut h = H::dmg();
    h.ticks(1024); // div = 0x1000: bit 12 high
    h.w(0xFF26, 0x00);
    h.w(0xFF26, 0x80);
    h
}

/// Arm channel 1 with length counter `c` and write NR14 = $C1
/// (trigger + length enable).
fn arm_ch1_len(h: &mut H, c: u8) {
    h.w(0xFF12, 0xF0);
    h.w(0xFF11, 64 - c);
    h.w(0xFF14, 0xC1);
}

/// Put the frame sequencer in the "next step does not clock length"
/// phase by consuming exactly one edge (div_divider becomes 1).
fn h_in_no_length_phase() -> H {
    let mut h = H::dmg();
    h.fs_edge();
    assert_eq!(h.apu.div_divider, 1);
    h
}

#[path = "tests/core.rs"]
mod core;

#[path = "tests/output.rs"]
mod output;

#[path = "tests/timing.rs"]
mod timing;

/// A STOP that leaves double speed restarts DIV without clocking the frame
/// sequencer, whichever side of the bit-13 boundary the counter sits on — the
/// gambatte `speedchange*_ch2_nr52` `a` rungs stop on both and neither may lose
/// a length step (see [`Apu::div_write_switching`]).
#[test]
fn leaving_double_speed_restarts_div_without_a_frame_event() {
    for prev in [0x1FFCu16, 0x2000] {
        let mut a = Apu::new(true);
        a.write(0xFF26, 0x80); // power on
        a.tick(prev, true);
        let before = a.div_divider;
        a.div_write_switching();
        assert_eq!(a.div_divider, before, "prev {prev:04X}: no frame event");
        assert_eq!(a.prev_div, 0, "prev {prev:04X}: counter restarted");
    }
}

/// The APU advances in whole 2 MHz granules, on a grid that an NR52 power-on
/// can leave trailing the CPU's cycle counter ([`Apu::lag`]). A DIV-APU edge
/// raised inside a granule takes effect only at the next boundary, which in
/// double speed is the next machine cycle: that is what lets an FF26 read in
/// the length clock's own cycle still see the channel on (the gambatte
/// `speedchange*_ch2_nr52` `1a`/`1b` straddle).
#[test]
fn a_trailing_granule_grid_defers_the_frame_sequencer_step_one_cycle() {
    for trailing in [false, true] {
        let mut a = Apu::new(true);
        a.write(0xFF26, 0x00); // power off
        // The power-on anchors the grid against the speed it lands in: single
        // speed leaves it a cycle short of the machine-cycle grid.
        a.tick(0, !trailing);
        a.write(0xFF26, 0x80);
        assert_eq!(a.lag, u8::from(trailing));
        let before = a.div_divider;
        a.tick(0x2000, true); // DIV-APU bit 13 rises
        a.tick(0x4000, true); // ...and falls: the frame-sequencer edge
        assert_eq!(
            a.div_divider == before,
            trailing,
            "trailing {trailing}: step deferred past its own machine cycle"
        );
        a.tick(0x4004, true);
        assert_ne!(a.div_divider, before, "the step lands at the next boundary");
    }
}

/// A STOP that leaves double speed re-anchors the granule grid across the
/// CPU's cycle counter without leaving the APU a granule in debt
/// ([`Apu::leave_double_speed`]): the pace stays one granule per double-speed
/// machine cycle either side of the switch.
#[test]
fn leaving_double_speed_flips_the_granule_grid_without_owing_a_granule() {
    let mut a = Apu::new(true);
    a.write(0xFF26, 0x00);
    a.tick(0, false);
    a.write(0xFF26, 0x80); // power on in single speed: the grid trails by one
    a.write(0xFF12, 0xF0); // NR12: DAC on
    a.write(0xFF14, 0x80); // NR14: trigger, frequency 0 (longest period)
    assert_eq!(a.lag, 1);
    for want_lag in [0, 1] {
        a.leave_double_speed();
        assert_eq!(a.lag, want_lag);
        let before = a.ch1.sample_countdown;
        a.tick(0, true);
        assert_eq!(
            before - a.ch1.sample_countdown,
            1,
            "one 2 MHz cycle per double-speed machine cycle across the re-anchor"
        );
    }
}

/// A STOP that ENTERS double speed re-paces the frequency units one machine
/// cycle after the CPU and PPU, so the first cycle of its pause still divides
/// for single speed and the 2 MHz grid gains one cycle over that pause (see
/// [`Apu::set_double_speed_lag`]).
#[test]
fn entering_double_speed_lags_the_frequency_units_one_machine_cycle() {
    for (lag, want) in [(false, 1u16), (true, 2)] {
        let mut a = Apu::new(true);
        a.write(0xFF26, 0x80); // power on
        a.write(0xFF12, 0xF0); // NR12: DAC on
        a.write(0xFF14, 0x80); // NR14: trigger, frequency 0 (longest period)
        if lag {
            a.set_double_speed_lag(true);
        }
        let before = a.ch1.sample_countdown;
        a.tick(0, true);
        assert_eq!(
            before - a.ch1.sample_countdown,
            want,
            "lag {lag}: 2 MHz cycles per double-speed machine cycle"
        );
    }
}
