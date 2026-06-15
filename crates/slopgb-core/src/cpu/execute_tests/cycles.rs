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
