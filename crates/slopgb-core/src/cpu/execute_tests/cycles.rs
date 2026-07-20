//! `execute_tests` — cycles tests (split for file size).

use super::*;

#[test]
fn cycle_counts_all_base_opcodes_flags_clear() {
    run_sweep(0x00);
}

#[test]
fn cycle_counts_all_base_opcodes_flags_set() {
    run_sweep(flags::Z | flags::C);
}

#[test]
fn cycle_counts_all_cb_opcodes() {
    for op in 0..=255u8 {
        let expected = if op & 7 != 6 {
            2
        } else if (0x40..=0x7F).contains(&op) {
            3 // BIT n,(HL): no write-back
        } else {
            4
        };
        let mut c = cpu();
        c.regs.set_hl(0xC800);
        let mut b = bus(&[0xCB, op]);
        step(&mut c, &mut b);
        assert_eq!(b.log.len(), expected, "CB {op:#04x}");
        assert_eq!(c.regs.f() & 0x0F, 0, "CB {op:#04x} dirtied F low nibble");
    }
}

// ---- deferred-commit clock: instruction-boundary flush -----------------
//
// `Bus::flush_pending` is the SameBoy `flush_pending_cycles` boundary
// (`sm83_cpu.c:336`). The CPU must call it exactly once per `step`, *after*
// the instruction's last M-cycle, on every exit path, so the next
// instruction begins at a clean cc+0.

#[test]
fn flush_pending_fires_once_per_step_after_the_last_m_cycle() {
    // A 2-M-cycle instruction (LD A,d8): one fetch + one immediate read.
    let mut c = cpu();
    let mut b = bus(&[0x3E, 0x42]);
    step(&mut c, &mut b);
    assert_eq!(b.flush_count, 1, "exactly one boundary flush per step");
    // The boundary arrives after the instruction's final M-cycle.
    assert_eq!(b.flush_at, vec![b.log.len()]);
    assert_eq!(b.log.len(), 2);
}

#[test]
fn flush_pending_fires_once_per_step_across_a_run() {
    // Three NOPs: three steps, three boundary flushes, one per M-cycle.
    let mut c = cpu();
    let mut b = bus(&[0x00, 0x00, 0x00]);
    for _ in 0..3 {
        step(&mut c, &mut b);
    }
    assert_eq!(b.flush_count, 3);
    assert_eq!(b.flush_at, vec![1, 2, 3]);
}

#[test]
fn flush_pending_fires_on_the_locked_early_return() {
    // An illegal opcode hard-locks the CPU; `step` burns one tick and
    // returns early — the boundary flush must still fire.
    let mut c = cpu();
    c.locked = true;
    let mut b = bus(&[0x00]);
    step(&mut c, &mut b);
    assert_eq!(b.flush_count, 1);
    assert_eq!(b.log, vec![Tick]);
}

#[test]
fn flush_pending_fires_on_the_halt_stay_early_return() {
    // Halted with no pending interrupt: `step` issues the discarded
    // prefetch, stays halted, and returns early — flush still fires.
    let mut c = cpu();
    c.halted = true;
    let mut b = bus(&[0x00]);
    step(&mut c, &mut b);
    assert_eq!(b.flush_count, 1);
    // One prefetch M-cycle elapsed before the early return.
    assert_eq!(b.flush_at, vec![1]);
}
