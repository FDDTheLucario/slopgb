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
