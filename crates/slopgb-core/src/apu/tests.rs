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
    assert_eq!(h.apu.div_divider, 0, "no step before DIV bit 4 falls");
    h.tick(); // div: 0x1FFC -> 0x2000
    assert_eq!(h.apu.div_divider, 1);
    h.ticks(2048);
    assert_eq!(h.apu.div_divider, 2);
}

/// A DIV write while the DIV-APU bit is high clocks the frame
/// sequencer in the write's own cycle (Pan Docs "DIV-APU"; see
/// [`Apu::div_write`]), and never when the bit is low.
#[test]
fn div_write_with_div_apu_bit_high_clocks_sequencer_now() {
    let mut h = H::dmg();
    h.ticks(1024); // div 0x1000: bit 12 high
    assert_eq!(h.apu.div_divider, 0);
    h.apu.div_write(false);
    h.div = 0; // the timer-side counter reset the hook accompanies
    assert_eq!(h.apu.div_divider, 1, "reset = falling edge, same cycle");
    h.ticks(512); // counter restarted: bit 12 low through 0x800
    assert_eq!(h.apu.div_divider, 1, "no spurious edge after the reset");
    h.apu.div_write(false); // bit low: no edge
    assert_eq!(h.apu.div_divider, 1);
}

#[test]
fn fs_edge_uses_div_bit_13_in_double_speed() {
    let mut apu = Apu::new(true);
    let mut div = 0u16;
    for _ in 0..4095 {
        div = div.wrapping_add(4);
        apu.tick(div, true);
    }
    assert_eq!(apu.div_divider, 0);
    div = div.wrapping_add(4); // 0x4000: bit 13 falls
    apu.tick(div, true);
    assert_eq!(apu.div_divider, 1);
}

#[test]
fn fs_handles_div_reset_via_stored_previous() {
    // A DIV write resets the counter; if bit 12 was high that is a
    // falling edge, detected by comparing with the stored previous value.
    let mut apu = Apu::new(false);
    apu.tick(0x1000, false); // bit 12 high
    assert_eq!(apu.div_divider, 0);
    apu.tick(0x0004, false); // counter restarted: falling edge
    assert_eq!(apu.div_divider, 1);
}

// ---- APU power-on DIV-event skip glitch ----
//
// SameBoy apu.c (GB_apu_init): "APU glitch: When turning the APU on
// while DIV's bit 4 (or 5 in double speed mode) is on, the first DIV/APU
// event is skipped." Implemented there as skip_div_event=SKIP plus
// div_divider=1: the first DIV-APU event after power-on is consumed
// entirely, the second runs its clocks *without* advancing the divider,
// and the divider parity starts shifted (lengths clock on odd divider,
// and the NRx4 "extra length clock" phase is flipped). Pinned by
// same-suite apu/div_trigger_volume_10, div_write_trigger_10,
// div_write_trigger_volume_10 (the "_10" sync helper at ROM $0630
// phase-locks DIV == $10, i.e. DIV-APU bit high, before NR52 writes).

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

#[test]
fn power_on_with_div_bit_high_skips_first_event() {
    let mut h = power_cycle_with_div_bit_high();
    // Parity is shifted: the next FS step must NOT be a length step, so
    // NR14 trigger+enable with counter 1 takes the extra-clock (1 -> 0)
    // + reload-63 path and the channel survives.
    arm_ch1_len(&mut h, 1);
    assert_eq!(h.apu.ch1.length.counter, 63, "extra clock + reload to 63");
    assert!(h.ch_on(1));
    // 1st DIV-APU event (div 0x1000 -> 0x2000): consumed entirely.
    h.ticks(1024);
    assert_eq!(h.apu.ch1.length.counter, 63, "first event skipped");
    // 2nd event: clocks lengths WITHOUT advancing the step counter.
    h.fs_edge();
    assert_eq!(h.apu.ch1.length.counter, 62, "second event clocks length");
    // 3rd event: a normal non-length step (parity stays shifted).
    h.fs_edge();
    assert_eq!(h.apu.ch1.length.counter, 62);
    // 4th event: length again.
    h.fs_edge();
    assert_eq!(h.apu.ch1.length.counter, 61);
}

#[test]
fn power_on_with_div_bit_high_counter_c_dies_after_2c_minus_2_events() {
    // Decoded div_write_trigger_10 contract (expected tables at ROM
    // $05AB): after powering on with the DIV-APU bit high and writing
    // NR14=$C1, counter 1 NEVER dies; counter c >= 2 dies after exactly
    // 2(c-1) DIV-APU events.
    for c in 1..=8u8 {
        let mut h = power_cycle_with_div_bit_high();
        arm_ch1_len(&mut h, c);
        if c == 1 {
            h.ticks(1024);
            for _ in 0..32 {
                h.fs_edge();
            }
            assert!(h.ch_on(1), "counter 1 must never die");
            continue;
        }
        let death = 2 * (u32::from(c) - 1);
        h.ticks(1024); // event 1
        for e in 2..death {
            h.fs_edge();
            assert!(
                h.ch_on(1),
                "counter {c}: alive before event {death}, dead at {e}"
            );
        }
        h.fs_edge();
        assert!(!h.ch_on(1), "counter {c}: dead at event {death}");
    }
}

#[test]
fn power_on_with_div_bit_low_keeps_plain_event_sequence() {
    // Guard (non-"_10" div_write_trigger table at ROM $05AB): with the
    // DIV-APU bit LOW at power-on there is no skip — NR14=$C1 lands in
    // the length-clocking phase (no extra clock) and counter c dies on
    // event 2c-1.
    for c in 1..=4u8 {
        let mut h = H::dmg();
        h.ticks(2048); // div = 0x2000: bit 12 just fell (low)
        h.w(0xFF26, 0x00);
        h.w(0xFF26, 0x80);
        arm_ch1_len(&mut h, c);
        assert_eq!(h.apu.ch1.length.counter, u16::from(c), "no extra clock");
        let death = 2 * u32::from(c) - 1;
        for e in 1..death {
            h.fs_edge();
            assert!(
                h.ch_on(1),
                "counter {c}: alive before event {death}, dead at {e}"
            );
        }
        h.fs_edge();
        assert!(!h.ch_on(1), "counter {c}: dead at event {death}");
    }
}

#[test]
fn power_on_skip_uses_bit_13_in_double_speed() {
    // Same glitch in double speed: the DIV-APU input is DIV bit 5,
    // internal counter bit 13 (SameBoy apu.c: "or 5 in double speed").
    let mut apu = Apu::new(true);
    let mut div = 0u16;
    let tick = |apu: &mut Apu, div: &mut u16, n: u32| {
        for _ in 0..n {
            *div = div.wrapping_add(4);
            apu.tick(*div, true);
        }
    };
    tick(&mut apu, &mut div, 2048); // div = 0x2000: bit 13 high
    apu.write(0xFF26, 0x00);
    apu.write(0xFF26, 0x80);
    apu.write(0xFF12, 0xF0);
    apu.write(0xFF11, 64 - 2);
    apu.write(0xFF14, 0xC1); // extra clock 2 -> 1 (shifted parity)
    assert_eq!(apu.ch1.length.counter, 1);
    tick(&mut apu, &mut div, 2048); // event 1 (bit 13 falls): skipped
    assert_eq!(apu.ch1.length.counter, 1);
    tick(&mut apu, &mut div, 4096); // event 2: length clocks, channel dies
    assert_eq!(apu.ch1.length.counter, 0);
    assert_eq!(apu.read(0xFF26) & 1, 0);
}

#[test]
fn power_cycle_clears_a_pending_skip() {
    // Power on with the bit high (arming the skip), straight off, then
    // on again with the bit low: the stale skip must not survive — the
    // first event after the second power-on clocks lengths normally.
    let mut h = H::dmg();
    h.ticks(1024); // bit 12 high
    h.w(0xFF26, 0x00);
    h.w(0xFF26, 0x80); // skip armed
    h.w(0xFF26, 0x00);
    h.ticks(1024); // div = 0x2000: bit 12 low (this edge is unpowered)
    h.w(0xFF26, 0x80); // no skip this time
    h.w(0xFF12, 0xF0);
    h.w(0xFF11, 63); // counter 1
    h.w(0xFF14, 0xC1); // length phase: no extra clock
    assert_eq!(h.apu.ch1.length.counter, 1);
    h.fs_edge(); // event 1 must clock length immediately
    assert_eq!(h.apu.ch1.length.counter, 0);
    assert!(!h.ch_on(1));
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
    h.fs_edge(); // step 1 ran: div_divider is now 2 (next step clocks length)
    assert_eq!(h.apu.div_divider, 2);
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

// ---- envelope: DIV-event countdown + secondary-event arming ----
//
// SameBoy apu.c GB_apu_div_event / GB_apu_div_secondary_event +
// timing.c GB_set_internal_div_counter: the envelope countdown
// decrements on DIV-APU events where (div_divider & 7) == 7; when it
// reaches 0 the volume tick is armed at the next RISING edge of the
// DIV-APU bit (the "secondary event", half an event period later) and
// fired at the following falling-edge event. The first-tick distance
// therefore depends on the trigger-vs-DIV phase — what gambatte's
// sound/ch2_init_env_counter_timing boundary scans measure.

#[test]
fn envelope_first_tick_depends_on_trigger_phase() {
    // Trigger right AFTER the divider-7 event: the countdown (period 1)
    // survives until the NEXT divider-7 event — first tick a full
    // envelope period later, NOT at the next "step 7".
    let mut h = H::dmg();
    for _ in 0..7 {
        h.fs_edge(); // divider = 7
    }
    h.w(0xFF12, 0x19); // volume 1, increase, period 1
    h.w(0xFF14, 0x80); // trigger: countdown = 1
    h.fs_edge(); // event 8 (divider 0)
    assert_eq!(
        h.apu.ch1.envelope.volume, 1,
        "countdown only decrements at divider&7==7"
    );
    for _ in 0..7 {
        h.fs_edge(); // events 9..15; event 15 takes the countdown to 0
    }
    assert_eq!(h.apu.ch1.envelope.volume, 1, "armed but not yet ticked");
    h.fs_edge(); // armed at the rising edge before event 16; tick fires
    assert_eq!(h.apu.ch1.envelope.volume, 2);
}

#[test]
fn envelope_ticks_quickly_when_triggered_just_before_divider_7() {
    // Trigger between events 6 and 7: event 7 decrements the fresh
    // countdown to 0, the secondary event arms, event 8 ticks — first
    // tick only 2 events after the trigger.
    let mut h = H::dmg();
    h.ticks(2048 * 6 + 1024); // divider = 6, past the rising edge
    h.w(0xFF12, 0x19); // volume 1, increase, period 1
    h.w(0xFF14, 0x80); // trigger: countdown = 1
    h.ticks(1024); // event 7: countdown 1 -> 0
    assert_eq!(h.apu.ch1.envelope.volume, 1);
    h.fs_edge(); // event 8: armed at the rising edge in between
    assert_eq!(h.apu.ch1.envelope.volume, 2);
}

#[test]
fn envelope_lock_stops_at_15_until_retrigger() {
    // SameBoy set_envelope_clock: arming with the volume already at the
    // add-mode rail (15) locks the envelope — no wrap-around — until a
    // trigger clears the lock.
    let mut h = H::dmg();
    h.w(0xFF12, 0xE9); // volume 14, increase, period 1
    h.w(0xFF14, 0x80);
    for _ in 0..64 {
        h.fs_edge();
    }
    assert_eq!(h.apu.ch1.envelope.volume, 15, "clamped at 15, no wrap");
}

// ---- NRx4 length extra-clock matrix through the register interface ----

/// Put the frame sequencer in the "next step does not clock length"
/// phase by consuming exactly one edge (div_divider becomes 1).
fn h_in_no_length_phase() -> H {
    let mut h = H::dmg();
    h.fs_edge();
    assert_eq!(h.apu.div_divider, 1);
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
    h.ticks(2048 * 3 + 100); // div_divider 3, duty somewhere
    h.apu.ch3.sample_byte = 0xAA;
    h.w(0xFF26, 0x00);
    h.w(0xFF26, 0x80);
    assert_eq!(h.apu.div_divider, 0);
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
    assert_eq!(h.apu.div_divider, 0);
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

// ---- pulse trigger anchoring to the machine 2 MHz grid ----

#[test]
fn pulse_trigger_delay_lands_on_the_machine_grid() {
    // Single-speed register writes always land at the same 2 MHz phase
    // (lf_div = 1: the phase counter starts at 2 and advances 4 dots
    // per tick). Inactive trigger: countdown = (freq^0x7FF)*2 + 6 -
    // lf_div = 5 at freq 2047, and the expiry consumes countdown + 1 =
    // 6 2 MHz cycles = 3 M-cycles (SameBoy apu.c square trigger).
    let mut h = H::cgb();
    h.w(0xFF12, 0xF0);
    h.w(0xFF11, 0xC0); // duty 3: position 1 is the first high cell
    h.w(0xFF13, 0xFF);
    h.w(0xFF14, 0x87); // trigger, freq 2047
    h.tick();
    h.tick();
    assert_eq!(h.apu.pcm12() & 0x0F, 0, "suppressed until first expiry");
    h.tick();
    assert_eq!(h.apu.pcm12() & 0x0F, 15, "position 1 after 3 M-cycles");
    // Steady state: period (2048-2047)*2 = 2 cycles = 1 M-cycle. Duty 3
    // is high through position 6, low at 7 and 0.
    for pos in 2..=6 {
        h.tick();
        assert_eq!(h.apu.pcm12() & 0x0F, 15, "position {pos}");
    }
    h.tick();
    assert_eq!(h.apu.pcm12() & 0x0F, 0, "position 7");
    // Retrigger while active: countdown = (freq^0x7FF)*2 + 4 - lf_div =
    // 3, expiry after 4 cycles = 2 M-cycles, position preserved.
    h.w(0xFF14, 0x87);
    assert_eq!(h.apu.ch1.sample_countdown, 3);
    assert_eq!(h.apu.ch1.duty_pos, 7, "retrigger preserves position");
    h.tick();
    assert_eq!(h.apu.pcm12() & 0x0F, 0, "position 7 still playing");
    h.tick();
    assert_eq!(h.apu.pcm12() & 0x0F, 0, "position 0");
    h.tick();
    assert_eq!(h.apu.pcm12() & 0x0F, 15, "position 1");
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
fn sweep_overflow_on_trigger_clears_status_bit_after_the_delay() {
    let mut h = H::dmg();
    h.w(0xFF10, 0x11);
    h.w(0xFF12, 0xF0);
    h.w(0xFF13, 0x80);
    h.w(0xFF14, 0x87); // freq 0x780 = 1920: overflow check armed
    // The kill is a delayed calculation: reload 3 (2 + 1 inactive)
    // plus shift 1 on the 1 MHz grid (SameBoy apu.c NR14 trigger).
    assert!(h.ch_on(1), "no instant kill at trigger");
    h.ticks(3);
    assert!(h.ch_on(1));
    h.tick();
    assert!(!h.ch_on(1), "kill lands 4 M-cycles after the trigger");
}

// ---- sweep calculation countdown on the machine grid ----
//
// SameBoy apu.c's sweep machinery (trigger_sweep_calculation /
// sweep_calculation_done / square_sweep_calculate_countdown), pinned
// end-to-end by SameSuite channel_1_sweep / channel_1_sweep_restart
// and the gambatte sound/ch1_init_reset_sweep_counter_timing scans.

#[test]
fn sweep_fire_overflow_kill_lands_8_mcycles_after_the_div_apu_event() {
    // SameSuite channel_1_sweep round 3 shape ($27/$7f0): the second
    // 128 Hz fire writes frequency $7ff immediately; the overflow
    // re-check ($7ff + $f) kills 8 M-cycles later — reload 2 + shift
    // 7 on the 1 MHz grid, the first cycle landing inside the event's
    // own M-cycle ("8 cycles after trigger, the APU checks if the
    // NEXT trigger overflows", channel_1_sweep.asm).
    let mut h = H::cgb();
    h.w(0xFF10, 0x27); // period 2, shift 7
    h.w(0xFF12, 0x80);
    h.w(0xFF13, 0xF0);
    h.w(0xFF14, 0x87); // trigger: freq $7f0; $7f0 + $f survives
    h.ticks(2048 * 7); // divider 7: the second sweep tick fires
    assert_eq!(h.apu.ch1.freq, 0x7FF, "fire writes the frequency");
    assert!(h.ch_on(1));
    h.ticks(7);
    assert!(h.ch_on(1), "re-check still counting");
    h.tick();
    assert!(!h.ch_on(1), "overflow kill 8 M-cycles after the fire");
}

#[test]
fn retrigger_after_a_fire_keeps_the_swept_frequency() {
    // SameSuite channel_1_sweep_restart round 1 ($1f/$7ff): the fire
    // subtracts to $7f0; a restart right after must retain it (NR14
    // $87 re-writes frequency bits 10-8 = 7, already their value),
    // and negate-mode sweeps never overflow-kill.
    let mut h = H::cgb();
    h.w(0xFF10, 0x1F); // period 1, negate, shift 7
    h.w(0xFF12, 0x80);
    h.w(0xFF13, 0xFF);
    h.w(0xFF14, 0x87); // freq $7ff
    h.ticks(2048 * 3); // divider 3: fire
    assert_eq!(h.apu.ch1.freq, 0x7F0, "negate fire: $7ff - $f");
    h.w(0xFF14, 0x87); // restart
    assert_eq!(h.apu.ch1.freq, 0x7F0, "restart keeps the frequency");
    h.ticks(50_000);
    assert!(h.ch_on(1), "negate-mode sweep never overflows");
}

#[test]
fn retrigger_replaces_a_pending_overflow_kill() {
    // SameSuite channel_1_sweep_restart round 2 ($17/$7f0): the fire
    // arms a kill 8 M-cycles out; a retrigger before it lands
    // replaces the calculation — the channel survives the original
    // deadline and dies on the NEW one (retrigger reload 2 + shift 7,
    // first machine cycle in the next M-cycle: 9 cycles).
    let mut h = H::cgb();
    h.w(0xFF10, 0x17); // period 1, shift 7
    h.w(0xFF12, 0x80);
    h.w(0xFF13, 0xF0);
    h.w(0xFF14, 0x87); // freq $7f0
    h.ticks(2048 * 3); // divider 3: fire -> freq $7ff, kill armed
    assert_eq!(h.apu.ch1.freq, 0x7FF);
    h.w(0xFF14, 0x87); // restart before the kill lands
    h.ticks(8);
    assert!(h.ch_on(1), "original deadline replaced");
    h.tick();
    assert!(!h.ch_on(1), "new calculation kills 9 cycles after");
}

#[test]
fn clearing_shift_after_a_fire_averts_the_pending_kill() {
    // SameSuite channel_1_sweep_restart round 3 ($17/$7f0 -> NR10=0):
    // clearing the shift bits pauses the armed calculation — the kill
    // never lands — and the negate-clear check sums exactly $7f0 +
    // $f + 0 = $7ff (E form of the old-negate bit; ARCHITECTURE.md
    // §CGB revision policy companion rule).
    let mut h = H::cgb();
    h.w(0xFF10, 0x17); // period 1, shift 7
    h.w(0xFF12, 0x80);
    h.w(0xFF13, 0xF0);
    h.w(0xFF14, 0x87); // freq $7f0
    h.ticks(2048 * 3); // divider 3: fire -> freq $7ff, kill armed
    h.w(0xFF10, 0x00); // disable sweep before the re-check lands
    h.ticks(50_000);
    assert!(h.ch_on(1), "paused calculation must never kill");
    assert_eq!(h.apu.ch1.freq, 0x7FF, "swept frequency survives");
}

#[test]
fn div_write_sweep_fire_uses_lead_1() {
    // A 128 Hz fire raised by a DIV write (the reset is the falling
    // edge) arms the calculation with a 1-cycle lead instead of
    // 1 + lf_div: the write lands later in its M-cycle than a natural
    // edge (SameBoy trigger_sweep_calculation, during_div_write).
    let mut h = H::dmg();
    h.w(0xFF10, 0x17); // period 1, shift 7
    h.w(0xFF12, 0x80);
    h.w(0xFF13, 0xF0);
    h.w(0xFF14, 0x87); // freq $7f0
    h.ticks(2048 * 2); // divider 2
    h.ticks(1024); // DIV-APU bit high
    h.apu.div_write(false); // divider 3: the sweep fire
    h.div = 0;
    assert_eq!(h.apu.ch1.freq, 0x7FF);
    assert_eq!(h.apu.ch1.sweep_reload_timer, 1);
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
