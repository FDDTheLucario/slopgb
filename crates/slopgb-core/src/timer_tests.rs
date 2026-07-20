//! Unit tests split out of `timer.rs` for the file-size rule;
//! compiled as `super::tests` via the `#[path]` attribute.

use super::*;

/// Build a timer with div = 0 and the given registers, with no edges
/// produced (matches the mooneye tests' state right after their final
/// `ldh (DIV),a` reference reset).
fn timer_with(tac: u8, tima: u8, tma: u8) -> Timer {
    let mut t = Timer::new();
    t.write(0xFF07, tac);
    t.write(0xFF06, tma);
    t.write(0xFF05, tima);
    t
}

/// Run `n` M-cycles, OR-ing the produced IF bits. Accesses in the
/// mooneye sequences happen *after* the final tick of their M-cycle.
fn ticks(t: &mut Timer, n: u32) -> u8 {
    let mut iff = 0;
    for _ in 0..n {
        iff |= t.tick().iff;
    }
    iff
}

/// The per-substep primitive [`Timer::tick_substep`] composes back into the
/// whole-M-cycle [`Timer::tick`]: driving two identically-initialised
/// timers in lockstep — one through `tick`, the other through 4
/// `tick_substep` calls per M-cycle (after a `begin_mcycle` reset) — yields
/// byte-identical IF / `late` / div / TIMA across an overflow + reload window.
/// Pins that `tick` stays byte-identical to the T-granular advance.
#[test]
fn tick_substep_composes_into_tick() {
    let mut whole = timer_with(0x05, 0xFD, 0x37); // enabled, freq bit 3, near overflow
    let mut split = timer_with(0x05, 0xFD, 0x37);
    for m in 0..600u32 {
        let tw = whole.tick();
        split.begin_mcycle();
        let (mut iff, mut late) = (0u8, false);
        for s in 0..4 {
            let (i, l) = split.tick_substep(s);
            iff |= i;
            late |= l;
        }
        assert_eq!(tw.iff, iff, "iff mismatch at M-cycle {m}");
        assert_eq!(tw.late, late, "late mismatch at M-cycle {m}");
        assert_eq!(whole.div_counter(), split.div_counter(), "div at {m}");
        assert_eq!(whole.read(0xFF05), split.read(0xFF05), "tima at {m}");
    }
}

#[test]
fn div_reads_high_byte_of_internal_counter() {
    let mut t = Timer::new();
    t.set_div(0xABCD);
    assert_eq!(t.div_counter(), 0xABCD);
    assert_eq!(t.read(0xFF04), 0xAB);
}

#[test]
fn div_increments_four_per_m_cycle() {
    let mut t = Timer::new();
    ticks(&mut t, 3);
    assert_eq!(t.div_counter(), 12);
    ticks(&mut t, 61);
    assert_eq!(t.read(0xFF04), 1); // 64 M-cycles = 256 T-cycles
}

#[test]
fn div_write_resets_counter() {
    let mut t = Timer::new();
    t.set_div(0x1234);
    t.write(0xFF04, 0x99); // written value is irrelevant
    assert_eq!(t.div_counter(), 0);
    assert_eq!(t.read(0xFF04), 0);
}

#[test]
fn div_counter_wraps() {
    let mut t = Timer::new();
    t.set_div(0xFFFE);
    t.tick();
    assert_eq!(t.div_counter(), 2);
}

#[test]
fn register_readback_and_unused_bits() {
    let mut t = Timer::new();
    t.write(0xFF05, 0x12);
    t.write(0xFF06, 0x34);
    t.write(0xFF07, 0x05);
    assert_eq!(t.read(0xFF05), 0x12);
    assert_eq!(t.read(0xFF06), 0x34);
    // TAC upper 5 bits read 1.
    assert_eq!(t.read(0xFF07), 0xFD);
    t.write(0xFF07, 0xF8); // unused bits written are dropped
    assert_eq!(t.read(0xFF07), 0xF8);
}

/// mooneye tim00/tim01/tim10/tim11: TIMA increments exactly every
/// 1024/16/64/256 T-cycles after a DIV reset.
#[test]
fn tima_increment_periods() {
    for (tac, period_mcycles) in [(0x04u8, 256u32), (0x05, 4), (0x06, 16), (0x07, 64)] {
        let mut t = timer_with(tac, 4, 4);
        ticks(&mut t, period_mcycles - 1);
        assert_eq!(t.read(0xFF05), 4, "tac {tac:#04x}: one cycle early");
        t.tick();
        assert_eq!(t.read(0xFF05), 5, "tac {tac:#04x}: on the boundary");
        ticks(&mut t, period_mcycles - 1);
        assert_eq!(t.read(0xFF05), 5, "tac {tac:#04x}: second period early");
        t.tick();
        assert_eq!(t.read(0xFF05), 6, "tac {tac:#04x}: second boundary");
    }
}

/// mooneye tim00_div_trigger etc.: a DIV write while the selected bit is
/// high produces a falling edge and clocks TIMA; while low it does not.
#[test]
fn div_write_triggers_increment_when_selected_bit_high() {
    // M-cycles after which the selected bit has just gone high
    // (half a period after reset).
    for (tac, half_period) in [(0x04u8, 128u32), (0x05, 2), (0x06, 8), (0x07, 32)] {
        let mut t = timer_with(tac, 4, 4);
        ticks(&mut t, half_period / 2); // selected bit still 0
        t.write(0xFF04, 0);
        assert_eq!(t.read(0xFF05), 4, "tac {tac:#04x}: bit low, no edge");

        let mut t = timer_with(tac, 4, 4);
        ticks(&mut t, half_period); // selected bit now 1
        t.write(0xFF04, 0);
        assert_eq!(t.read(0xFF05), 5, "tac {tac:#04x}: bit high, edge");
    }
}

/// mooneye rapid_toggle: disabling the timer while the selected bit is
/// high clocks TIMA; re-enabling (rising edge) does not, and the internal
/// counter is not reset by TAC writes.
#[test]
fn tac_disable_with_selected_bit_high_increments() {
    let mut t = timer_with(0x04, 4, 4);
    ticks(&mut t, 128); // div = 512, bit 9 high
    t.write(0xFF07, 0x00);
    assert_eq!(t.read(0xFF05), 5);
    t.write(0xFF07, 0x04); // rising edge: no increment
    assert_eq!(t.read(0xFF05), 5);
}

#[test]
fn tac_disable_with_selected_bit_low_does_not_increment() {
    let mut t = timer_with(0x04, 4, 4);
    ticks(&mut t, 64); // div = 256, bit 9 low
    t.write(0xFF07, 0x00);
    assert_eq!(t.read(0xFF05), 4);
}

#[test]
fn disabled_timer_does_not_count() {
    let mut t = timer_with(0x00, 4, 4);
    assert_eq!(ticks(&mut t, 1024), 0);
    assert_eq!(t.read(0xFF05), 4);
}

/// A TAC frequency switch from a high selected bit to a low one is a
/// falling edge too (same edge detector as enable).
#[test]
fn tac_frequency_change_can_increment() {
    let mut t = timer_with(0x07, 4, 4); // bit 7
    ticks(&mut t, 32); // div = 128: bit 7 high, bit 9 low
    t.write(0xFF07, 0x04); // switch to bit 9
    assert_eq!(t.read(0xFF05), 5);
}

/// mooneye tima_reload: after overflow TIMA reads 0x00 for 4 T-cycles
/// (one M-cycle at the observable access points), then TMA. Increments
/// keep their 64-T-cycle phase, no extra delay.
///
/// Reference state: div = 0, TIMA = TMA = 0xFE, TAC = freq 10 (bit 5,
/// 64 T-cycles). Reads happen after the tick of M-cycle:
///   28 nops + 3  -> div 124 -> 0xFF   (d)
///   29 nops + 3  -> div 128 -> 0x00   (e, overflow this cycle)
///   30 nops + 3  -> div 132 -> 0xFE   (c, reload this cycle)
///   60 nops + 3  -> div 252 -> 0xFF   (h)
///   61 nops + 3  -> div 256 -> 0x00   (l, second overflow)
///   62 nops + 3  -> div 260 -> 0xFE   (b)
#[test]
fn tima_reload_sequence() {
    for (mcycles, expected) in [
        (31u32, 0xFFu8),
        (32, 0x00),
        (33, 0xFE),
        (63, 0xFF),
        (64, 0x00),
        (65, 0xFE),
    ] {
        let mut t = timer_with(0x06, 0xFE, 0xFE);
        ticks(&mut t, mcycles);
        assert_eq!(t.read(0xFF05), expected, "after {mcycles} M-cycles");
    }
}

/// The timer interrupt is requested in the reload M-cycle, not in the
/// overflow M-cycle.
#[test]
fn tima_reload_irq_timing() {
    let mut t = timer_with(0x06, 0xFE, 0xFE);
    assert_eq!(ticks(&mut t, 32), 0); // includes the overflow cycle
    assert_eq!(t.read(0xFF05), 0x00);
    assert_eq!(t.tick().iff, 0x04); // reload cycle raises IF bit 2
    assert_eq!(t.read(0xFF05), 0xFE);
}

/// mooneye tima_write_reloading. Writes of 0x7F to TIMA at the access
/// point of M-cycle W (reference state as in `tima_reload_sequence`),
/// then a read 3 M-cycles later:
///   W=31 (div 124, before overflow): normal write, +1 at div 128 -> 0x80
///   W=32 (div 128, overflow cycle):  write wins, reload cancelled -> 0x7F
///   W=33 (div 132, reload cycle):    write ignored, TMA wins      -> 0xFE
///   W=34 (div 136, after reload):    normal write                 -> 0x7F
#[test]
fn tima_write_reloading_cases() {
    for (w, expected) in [(31u32, 0x80u8), (32, 0x7F), (33, 0xFE), (34, 0x7F)] {
        let mut t = timer_with(0x06, 0xFE, 0xFE);
        ticks(&mut t, w);
        t.write(0xFF05, 0x7F);
        let iff = ticks(&mut t, 3);
        assert_eq!(t.read(0xFF05), expected, "write at M-cycle {w}");
        assert_eq!(iff, 0, "no IF after the write at M-cycle {w}");
    }
}

/// A TIMA write in the overflow window cancels both the reload and the
/// interrupt; counting continues from the written value in phase.
#[test]
fn tima_write_in_overflow_window_cancels_reload_and_irq() {
    let mut t = timer_with(0x06, 0xFE, 0xFE);
    ticks(&mut t, 32); // overflow at div 128
    t.write(0xFF05, 0x7F);
    // No reload, no IRQ; next increment still at div 192 (16 cycles on).
    assert_eq!(ticks(&mut t, 15), 0);
    assert_eq!(t.read(0xFF05), 0x7F);
    assert_eq!(t.tick().iff, 0);
    assert_eq!(t.read(0xFF05), 0x80);
}

/// mooneye tma_write_reloading. Writes of 0x7F to TMA at M-cycle W:
///   W=32 (overflow cycle): reload one cycle later picks up new TMA -> 0x7F
///   W=33 (reload cycle):   forwarded to TIMA as well               -> 0x7F
///   W=34, W=35 (after):    too late, TIMA keeps old TMA            -> 0xFE
#[test]
fn tma_write_reloading_cases() {
    for (w, expected) in [(32u32, 0x7Fu8), (33, 0x7F), (34, 0xFE), (35, 0xFE)] {
        let mut t = timer_with(0x06, 0xFE, 0xFE);
        ticks(&mut t, w);
        t.write(0xFF06, 0x7F);
        ticks(&mut t, 3);
        assert_eq!(t.read(0xFF05), expected, "write at M-cycle {w}");
        assert_eq!(t.read(0xFF06), 0x7F, "TMA itself always updated");
    }
}

/// A DIV-write-induced increment that overflows TIMA also delays the
/// reload + IRQ by 4 T-cycles (one observable M-cycle).
#[test]
fn div_write_overflow_delays_reload() {
    let mut t = timer_with(0x04, 0xFF, 0x42);
    ticks(&mut t, 128); // div = 512, bit 9 high, no edge yet
    assert_eq!(t.read(0xFF05), 0xFF);
    t.write(0xFF04, 0); // edge -> overflow, IF delayed
    assert_eq!(t.read(0xFF05), 0x00);
    assert_eq!(t.tick().iff, 0x04);
    assert_eq!(t.read(0xFF05), 0x42);
}

/// Same as above via TAC disable, and the reload window write rules
/// apply to write-induced overflows too.
#[test]
fn tac_write_overflow_delays_reload_and_reload_cycle_write_ignored() {
    let mut t = timer_with(0x04, 0xFF, 0x10);
    ticks(&mut t, 128); // div = 512, bit 9 high
    t.write(0xFF07, 0x00); // disable -> edge -> overflow
    assert_eq!(t.read(0xFF05), 0x00);
    assert_eq!(t.tick().iff, 0x04); // reload still completes when disabled
    assert_eq!(t.read(0xFF05), 0x10);
    t.write(0xFF05, 0x99); // same M-cycle as the reload: ignored
    assert_eq!(t.read(0xFF05), 0x10);
}

/// Edges are detected at T-cycle granularity inside a tick, so a DIV
/// phase that is not a multiple of 4 still clocks TIMA correctly.
#[test]
fn edge_mid_m_cycle_is_detected() {
    let mut t = Timer::new();
    t.set_div(14);
    t.write(0xFF07, 0x05); // select bit 3 (currently 1; enabling is a rising edge)
    t.tick(); // div 14 -> 18, falling edge at 16 on the 2nd T-cycle
    assert_eq!(t.read(0xFF05), 1);
}

/// With the DIV counter ≡ 0 mod 4 at M-cycle boundaries (every
/// post-boot state is — `model::tests::div_counter_is_m_cycle_aligned`
/// — and DIV writes/STOP reset it to 0 at a boundary), a natural TIMA
/// overflow's falling edge lands on the last T-substep of its M-cycle,
/// and the reload pipeline preserves the substep: the reload + IF
/// commit one M-cycle later also lands on the last T-substep — after
/// the mid-cycle halt-exit sampling point (`TimerTick::late`; gambatte
/// tima/tc*_irq_*, wilbertpol timer_if rounds 5/6).
#[test]
fn natural_reload_commits_in_second_half_of_cycle() {
    let mut t = timer_with(0x06, 0xFE, 0xFE); // bit 5: 64 T period
    for n in 0..32 {
        assert!(!t.tick().late, "no commit before the reload, cycle {n}");
    }
    // Overflow happened during M-cycle 32 (div 124 -> 128, substep 3);
    // the reload + IF commit fires during M-cycle 33 on substep 3.
    let reload = t.tick();
    assert_eq!(reload.iff, 0x04);
    assert!(reload.late, "aligned natural reload commits on substep 3");
}

/// A DIV phase that is not a multiple of 4 at M-cycle boundaries moves
/// the overflow edge — and with it the reload commit — into the first
/// half of the M-cycle, before the mid-cycle sampling point: `late`
/// must report the substep, not "timer is always late" (guards the
/// rule's substep dependence).
#[test]
fn off_alignment_reload_commits_in_first_half_of_cycle() {
    let mut t = timer_with(0x06, 0xFF, 0xFE);
    t.set_div(62); // boundary phase ≡ 2 mod 4
    // Falling edge of bit 5 at div 63 -> 64, substep 1: overflow.
    let over = t.tick();
    assert_eq!(over.iff, 0);
    assert_eq!(t.read(0xFF05), 0x00);
    // Reload + IF commit on substep 1 of this cycle: not late.
    let reload = t.tick();
    assert_eq!(reload.iff, 0x04);
    assert!(!reload.late, "first-half commit is not late");
}

/// A write-induced overflow (DIV/TAC write) arms the 4 T-cycle reload
/// pipeline after the write cycle's four substeps have run, so its
/// reload + IF commit also lands on the last T-substep of the next
/// M-cycle. Pinned for uniformity of the mechanical substep rule; it
/// is unobservable through the halt-wake path (the CPU is mid
/// instruction stream one cycle after its own DIV/TAC write, so the
/// running-CPU end-of-fetch sampling applies — mooneye rapid_toggle's
/// dispatch timing is unaffected).
#[test]
fn write_induced_reload_also_commits_in_second_half() {
    let mut t = timer_with(0x04, 0xFF, 0x42);
    ticks(&mut t, 128); // div = 512, bit 9 high
    t.write(0xFF04, 0); // falling edge -> overflow, pipeline armed
    assert_eq!(t.read(0xFF05), 0x00);
    let reload = t.tick();
    assert_eq!(reload.iff, 0x04);
    assert!(reload.late);
}
