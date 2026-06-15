//! `tests` — timing tests (split for file size).

use super::*;

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
