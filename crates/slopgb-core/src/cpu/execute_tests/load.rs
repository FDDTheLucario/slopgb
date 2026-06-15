//! `execute_tests` — load tests (split for file size).

use super::*;

#[test]
fn nop_is_one_fetch_cycle() {
    let mut c = cpu();
    let mut b = bus(&[0x00]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x00)]);
    assert_eq!(c.regs.pc, PC0 + 1);
    assert_eq!(c.regs.f(), 0);
}

#[test]
fn ld_r_r_moves_value_in_one_cycle() {
    let mut c = cpu();
    c.regs.c = 0x42;
    let mut b = bus(&[0x41]); // LD B,C
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x41)]);
    assert_eq!(c.regs.b, 0x42);
}

#[test]
fn ld_b_b_sets_debug_breakpoint_and_still_loads() {
    let mut c = cpu();
    c.regs.b = 7;
    let mut b = bus(&[0x40]);
    step(&mut c, &mut b);
    assert!(c.debug_breakpoint);
    assert!(c.debug_breakpoint_hit());
    assert_eq!(c.regs.b, 7);
    assert_eq!(b.log, [Read(PC0, 0x40)]);
}

#[test]
fn ld_r_hl_and_ld_hl_r_traces() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    let mut b = bus(&[0x56, 0x73]); // LD D,(HL); LD (HL),E
    b.mem[0xC800] = 0x99;
    c.regs.e = 0x5A;
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x56), Read(0xC800, 0x99)]);
    assert_eq!(c.regs.d, 0x99);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x73), Write(0xC800, 0x5A)]);
    assert_eq!(b.mem[0xC800], 0x5A);
}

#[test]
fn ld_r_imm_and_ld_hl_imm() {
    let mut c = cpu();
    let mut b = bus(&[0x3E, 0xAB, 0x36, 0x77]); // LD A,n; LD (HL),n
    c.regs.set_hl(0xC800);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x3E), Read(PC0 + 1, 0xAB)]);
    assert_eq!(c.regs.a, 0xAB);
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 2, 0x36),
            Read(PC0 + 3, 0x77),
            Write(0xC800, 0x77)
        ]
    );
}

#[test]
fn ld_rp_imm_all_pairs() {
    for (op, check) in [(0x01u8, 0usize), (0x11, 1), (0x21, 2), (0x31, 3)] {
        let mut c = cpu();
        let mut b = bus(&[op, 0xCD, 0xAB]);
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [Read(PC0, op), Read(PC0 + 1, 0xCD), Read(PC0 + 2, 0xAB)]
        );
        let got = match check {
            0 => c.regs.bc(),
            1 => c.regs.de(),
            2 => c.regs.hl(),
            _ => c.regs.sp,
        };
        assert_eq!(got, 0xABCD);
    }
}

#[test]
fn ld_a_indirect_loads_and_stores() {
    // stores
    let mut c = cpu();
    c.regs.a = 0x5C;
    c.regs.set_bc(0xC700);
    c.regs.set_de(0xC701);
    c.regs.set_hl(0xC702);
    let mut b = bus(&[0x02, 0x12, 0x22, 0x32]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x02), Write(0xC700, 0x5C)]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x12), Write(0xC701, 0x5C)]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 2, 0x22), Write(0xC702, 0x5C)]);
    assert_eq!(c.regs.hl(), 0xC703); // HL+
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 3, 0x32), Write(0xC703, 0x5C)]);
    assert_eq!(c.regs.hl(), 0xC702); // HL-

    // loads
    let mut c = cpu();
    c.regs.set_bc(0xC700);
    c.regs.set_de(0xC701);
    c.regs.set_hl(0xC702);
    let mut b = bus(&[0x0A, 0x1A, 0x2A, 0x3A]);
    b.load(0xC700, &[0x11, 0x22, 0x33]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x11);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x22);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x33);
    assert_eq!(c.regs.hl(), 0xC703);
    b.take_log();
    step(&mut c, &mut b);
    // The HL decrement shares the M-cycle with the read (SameBoy v0.12.1
    // ld_a_dhld: an inc/dec-unit read, like the HL+ variant).
    assert_eq!(b.log, [Read(PC0 + 3, 0x3A), ReadInc(0xC703, 0x00)]);
    assert_eq!(c.regs.hl(), 0xC702);
}

#[test]
fn ld_nn_a_and_ld_a_nn() {
    let mut c = cpu();
    c.regs.a = 0x77;
    let mut b = bus(&[0xEA, 0x34, 0xC9, 0xFA, 0x34, 0xC9]);
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0, 0xEA),
            Read(PC0 + 1, 0x34),
            Read(PC0 + 2, 0xC9),
            Write(0xC934, 0x77)
        ]
    );
    c.regs.a = 0;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 3, 0xFA),
            Read(PC0 + 4, 0x34),
            Read(PC0 + 5, 0xC9),
            Read(0xC934, 0x77)
        ]
    );
    assert_eq!(c.regs.a, 0x77);
}

#[test]
fn ldh_imm_and_c_variants() {
    let mut c = cpu();
    c.regs.a = 0x42;
    c.regs.c = 0x81;
    let mut b = bus(&[0xE0, 0x80, 0xF0, 0x80, 0xE2, 0xF2]);
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [Read(PC0, 0xE0), Read(PC0 + 1, 0x80), Write(0xFF80, 0x42)]
    );
    c.regs.a = 0;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [Read(PC0 + 2, 0xF0), Read(PC0 + 3, 0x80), Read(0xFF80, 0x42)]
    );
    assert_eq!(c.regs.a, 0x42);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 4, 0xE2), Write(0xFF81, 0x42)]);
    b.mem[0xFF81] = 0x55;
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 5, 0xF2), Read(0xFF81, 0x55)]);
    assert_eq!(c.regs.a, 0x55);
}

#[test]
fn ld_nn_sp_writes_lo_then_hi() {
    let mut c = cpu();
    c.regs.sp = 0xABCD;
    let mut b = bus(&[0x08, 0x34, 0xC1]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0x08),
            Read(PC0 + 1, 0x34),
            Read(PC0 + 2, 0xC1),
            Write(0xC134, 0xCD),
            Write(0xC135, 0xAB)
        ]
    );
}

#[test]
fn ld_sp_hl_has_internal_cycle() {
    let mut c = cpu();
    c.regs.set_hl(0x1234);
    let mut b = bus(&[0xF9]);
    step(&mut c, &mut b);
    // The internal cycle drives the new SP (= HL) onto the address bus
    // (SameBoy ld_sp_hl: cycle_oam_bug on the HL value).
    assert_eq!(b.log, [Read(PC0, 0xF9), TickAddr(0x1234)]);
    assert_eq!(c.regs.sp, 0x1234);
}

#[test]
fn push_has_internal_cycle_before_writes() {
    let mut c = cpu();
    c.regs.set_bc(0x1234);
    let mut b = bus(&[0xC5]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xC5),
            // The pre-decrement SP rides the address bus (SameBoy push_rr).
            TickAddr(SP0),
            Write(SP0 - 1, 0x12),
            Write(SP0 - 2, 0x34)
        ]
    );
    assert_eq!(c.regs.sp, SP0 - 2);
}

#[test]
fn pop_has_no_internal_cycle() {
    let mut c = cpu();
    let mut b = bus(&[0xD1]); // POP DE
    b.load(SP0, &[0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xD1), ReadInc(SP0, 0x34), ReadInc(SP0 + 1, 0x12)]
    );
    assert_eq!(c.regs.de(), 0x1234);
    assert_eq!(c.regs.sp, SP0 + 2);
}

#[test]
fn pop_af_masks_f_low_nibble() {
    let mut c = cpu();
    let mut b = bus(&[0xF1]);
    b.load(SP0, &[0xFF, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x12);
    assert_eq!(c.regs.f(), 0xF0);
}

#[test]
fn push_af_writes_a_then_f() {
    let mut c = cpu();
    c.regs.a = 0x12;
    c.regs.set_f(0xF0);
    let mut b = bus(&[0xF5]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xF5),
            TickAddr(SP0),
            Write(SP0 - 1, 0x12),
            Write(SP0 - 2, 0xF0)
        ]
    );
}
