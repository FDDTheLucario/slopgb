//! Unit tests split out of `pulse.rs` for the file-size rule;
//! compiled as `super::tests` via the `#[path]` attribute.

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

/// Channel-1 setup with the sweep trigger tail, single-speed DMG
/// conventions (`lf_div` = 1 at every register write).
fn sweep_pulse(nr10: u8, freq: u16) -> Pulse {
    let mut p = Pulse::new();
    p.envelope.write(0xF0);
    p.dac = true;
    p.freq = freq;
    p.write_nr10(nr10, 1, false);
    p.trigger(1);
    p.trigger_sweep(1, false, false, false);
    p
}

/// One single-speed M-cycle of the APU dot loop as `Apu::tick` drives
/// channel 1, positioned right after a register write (phase = 2): the
/// 1 MHz machine step lands on the second dot, the 2 MHz restart-hold
/// steps on the second and fourth.
fn sweep_mcycle(p: &mut Pulse) {
    p.sweep_machine_step();
    p.sweep_hold_step();
    p.step();
    p.sweep_hold_step();
    p.step();
}

#[test]
fn sweep_trigger_overflow_kill_is_a_delayed_calculation() {
    // freq 1920 + (1920 >> 1) = 2880 > 2047 — but the trigger only
    // ARMS the overflow check: reload lead 3 (2 + 1 for an inactive
    // channel) plus shift 1 on the 1 MHz grid, so the kill lands 4
    // M-cycles after the trigger, not instantly (SameBoy apu.c NR14
    // trigger: "overflow check also occurs on trigger" via
    // square_sweep_calculate_countdown; SameSuite channel_1_sweep
    // measures the analogous post-fire delay).
    let mut p = sweep_pulse(0x11, 1920); // period 1, shift 1
    assert!(p.enabled, "no instant kill at trigger");
    for i in 0..3 {
        sweep_mcycle(&mut p);
        assert!(p.enabled, "still counting at M-cycle {i}");
    }
    sweep_mcycle(&mut p);
    assert!(!p.enabled, "kill lands reload+shift M-cycles after");

    // Same with shift 0: no calculation is armed, channel stays on.
    let mut p = sweep_pulse(0x10, 1920);
    for _ in 0..100 {
        sweep_mcycle(&mut p);
    }
    assert!(p.enabled);
}

#[test]
fn sweep_fire_writes_frequency_then_recheck_kills_after_shift_cycles() {
    // 1024 -> 1536 at the 128 Hz fire (immediate frequency write);
    // the re-check 1536 + 768 = 2304 > 2047 completes reload(2) +
    // shift(1) 1 MHz cycles later and kills (SameBoy
    // trigger_sweep_calculation / sweep_calculation_done; "sweep
    // frequency is checked after adding the sweep delta twice").
    let mut p = sweep_pulse(0x11, 1024); // period 1, shift 1
    for _ in 0..4 {
        sweep_mcycle(&mut p); // trigger calc: 1024 + 512 survives
    }
    assert!(p.enabled);
    p.sweep_clock(2); // counter 6 -> 7: fire
    assert_eq!(p.freq, 1536, "frequency written at the fire");
    assert!(p.enabled, "overflow re-check has not landed yet");
    sweep_mcycle(&mut p);
    sweep_mcycle(&mut p);
    assert!(p.enabled);
    sweep_mcycle(&mut p);
    assert!(!p.enabled, "re-check kills after reload+shift cycles");

    // 256 -> 320 with shift 2, re-check 320 + 80 = 400: survives.
    let mut p = sweep_pulse(0x12, 256);
    for _ in 0..5 {
        sweep_mcycle(&mut p);
    }
    p.sweep_clock(2);
    assert_eq!(p.freq, 320);
    for _ in 0..4 {
        sweep_mcycle(&mut p);
    }
    assert!(p.enabled);
}

#[test]
fn sweep_negate_mode_subtracts() {
    // The completed trigger calculation one's-complements the addend
    // (512 ^ 0x7FF = 1535); the fire then adds it plus the negate bit
    // — two's-complement subtraction: 1024 - 512 = 512.
    let mut p = sweep_pulse(0x19, 1024); // period 1, negate, shift 1
    for _ in 0..4 {
        sweep_mcycle(&mut p);
    }
    assert!(p.enabled);
    p.sweep_clock(2);
    assert_eq!(p.freq, 512);
    assert!(p.enabled);
}

#[test]
fn clearing_negate_after_negate_calc_disables_channel() {
    // After the trigger-armed negate calculation completes, the
    // completed addend holds the one's complement: an NR10 write
    // clearing negate sums shadow(1024) + 1535 + old-negate(1) >
    // 0x7FF and kills (SameBoy NR10 write; Blargg dmg_sound 05).
    let mut p = sweep_pulse(0x19, 1024);
    for _ in 0..4 {
        sweep_mcycle(&mut p); // let the calculation complete
    }
    assert!(p.enabled);
    p.write_nr10(0x11, 1, false); // clear negate
    assert!(!p.enabled);
}

#[test]
fn clearing_negate_without_any_calc_keeps_channel() {
    // Shift 0 arms no calculation: shadow and the completed addend
    // stay 0, so the negate-clear check cannot cross 0x7FF.
    let mut p = sweep_pulse(0x18, 1024); // period 1, negate, shift 0
    for _ in 0..4 {
        sweep_mcycle(&mut p);
    }
    p.write_nr10(0x10, 1, false);
    assert!(p.enabled);
}

#[test]
fn negate_calc_on_shift_zero_fire_counts_for_the_negate_clear_kill() {
    // A shift-0 fire arms an "instant" calculation that completes
    // when the reload lead expires (SameBoy
    // square_sweep_instant_calculation_done): no frequency write, no
    // overflow kill (negate), but the completed addend (1024 ^ 0x7FF
    // = 1023) pins the later negate-clear kill: 1024 + 1023 + 1 >
    // 0x7FF.
    let mut p = sweep_pulse(0x18, 1024); // period 1, negate, shift 0
    p.sweep_clock(2); // counter 6 -> 7: shift-0 fire
    assert_eq!(p.freq, 1024, "shift 0 never writes the frequency");
    sweep_mcycle(&mut p);
    sweep_mcycle(&mut p); // reload expires: instant calculation done
    assert!(p.enabled);
    p.write_nr10(0x10, 1, false); // clear negate
    assert!(!p.enabled);
}

#[test]
fn sweep_period_zero_never_updates_frequency() {
    let mut p = sweep_pulse(0x01, 512); // period 0, shift 1
    for _ in 0..32 {
        p.sweep_clock(2);
        for _ in 0..16 {
            sweep_mcycle(&mut p);
        }
    }
    assert_eq!(p.freq, 512);
    assert!(p.enabled);
}

#[test]
fn nr10_write_fires_sweep_when_counter_parked_at_7() {
    // A trigger with period 0 parks the 128 Hz up-counter at 7
    // (period ^ 7); a later NR10 write with a non-zero period fires
    // the sweep unit from the write itself — SameBoy runs
    // trigger_sweep_calculation at the end of every NR10 write. The
    // restart hold has not expired here (no machine cycles ran), so
    // the fire adds the trigger-time addend to the reset shadow (0):
    // freq = 512 >> 1 = 256.
    let mut p = sweep_pulse(0x01, 512); // period 0, shift 1
    assert_eq!(p.sweep_countdown, 7);
    p.write_nr10(0x11, 1, false); // period 1, shift 1: fires NOW
    assert_eq!(p.freq, 256);
    assert_eq!(p.sweep_countdown, 1 ^ 7, "counter reset by the fire");
    assert!(p.enabled);
}

#[test]
fn cleared_shift_pauses_a_pending_calculation() {
    // SameSuite channel_1_sweep_restart round 3: an armed overflow
    // kill never lands once NR10's shift bits are cleared — the
    // calculation countdown pauses ("Calculation is paused if the
    // lower bits are 0", SameBoy GB_apu_run) — and the negate-clear
    // check sums exactly shadow + addend + 0 = 0x7FF: no kill (the
    // SameBoy <=CGB-C forced old-negate bit would cross 0x7FF; the E
    // form applies per docs/ARCHITECTURE.md §CGB revision policy).
    let mut p = sweep_pulse(0x17, 0x7F0); // period 1, shift 7
    for _ in 0..10 {
        sweep_mcycle(&mut p); // trigger calc: $7f0 + $f = $7ff survives
    }
    assert!(p.enabled);
    p.sweep_clock(2); // fire: freq $7ff, kill armed 9 cycles out
    assert_eq!(p.freq, 0x7FF);
    p.write_nr10(0x00, 1, false); // disable sweep before it lands
    assert!(p.enabled, "negate-clear check reads exactly 0x7FF");
    for _ in 0..100 {
        sweep_mcycle(&mut p);
    }
    assert!(p.enabled, "paused calculation must never kill");
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
