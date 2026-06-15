//! `execute_tests` — alu tests (split for file size).

use super::*;

#[test]
fn add_and_adc_flags() {
    let mut c = cpu();
    c.regs.a = 0x0F;
    c.regs.b = 0x01;
    let mut b = bus(&[0x80]); // ADD A,B
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x10);
    assert_eq!(c.regs.f(), flags::H);

    // carry + zero: 0xFF + 0x00 + carry-in
    let mut c = cpu();
    c.regs.a = 0xFF;
    c.regs.b = 0x00;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x88]); // ADC A,B
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x00);
    assert_eq!(c.regs.f(), flags::Z | flags::H | flags::C);

    // ADC carry contributes to both halves
    let mut c = cpu();
    c.regs.a = 0x80;
    c.regs.b = 0x80;
    let mut b = bus(&[0x80]); // ADD A,B -> 0x00, C
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0);
    assert_eq!(c.regs.f(), flags::Z | flags::C);
}

#[test]
fn sub_sbc_cp_flags() {
    let mut c = cpu();
    c.regs.a = 0x10;
    c.regs.b = 0x01;
    let mut b = bus(&[0x90]); // SUB B: half borrow
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x0F);
    assert_eq!(c.regs.f(), flags::N | flags::H);

    let mut c = cpu();
    c.regs.a = 0x00;
    c.regs.b = 0x00;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x98]); // SBC A,B: 0 - 0 - 1
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0xFF);
    assert_eq!(c.regs.f(), flags::N | flags::H | flags::C);

    let mut c = cpu();
    c.regs.a = 0x42;
    c.regs.b = 0x42;
    let mut b = bus(&[0xB8]); // CP B: equal, A unchanged
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x42);
    assert_eq!(c.regs.f(), flags::Z | flags::N);
}

#[test]
fn and_xor_or_flags() {
    let mut c = cpu();
    c.regs.a = 0xF0;
    c.regs.b = 0x0F;
    let mut b = bus(&[0xA0]); // AND B -> 0, H always set
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0);
    assert_eq!(c.regs.f(), flags::Z | flags::H);

    let mut c = cpu();
    c.regs.a = 0xFF;
    c.regs.b = 0xFF;
    c.regs.set_f(0xF0);
    let mut b = bus(&[0xA8]); // XOR B -> 0
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::Z);

    let mut c = cpu();
    c.regs.a = 0x00;
    c.regs.b = 0x08;
    c.regs.set_f(0xF0);
    let mut b = bus(&[0xB0]); // OR B
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x08);
    assert_eq!(c.regs.f(), 0);
}

#[test]
fn alu_hl_and_imm_operand_timing() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    c.regs.a = 1;
    let mut b = bus(&[0x86, 0xC6, 0x05]); // ADD A,(HL); ADD A,5
    b.mem[0xC800] = 2;
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x86), Read(0xC800, 0x02)]);
    assert_eq!(c.regs.a, 3);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0xC6), Read(PC0 + 2, 0x05)]);
    assert_eq!(c.regs.a, 8);
}

#[test]
fn inc_dec_r8_flags_preserve_carry() {
    let mut c = cpu();
    c.regs.b = 0x0F;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x04]); // INC B
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0x10);
    assert_eq!(c.regs.f(), flags::H | flags::C);

    let mut c = cpu();
    c.regs.b = 0xFF;
    let mut b = bus(&[0x04]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0x00);
    assert_eq!(c.regs.f(), flags::Z | flags::H);

    let mut c = cpu();
    c.regs.b = 0x10;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x05]); // DEC B: borrow from bit 4
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0x0F);
    assert_eq!(c.regs.f(), flags::N | flags::H | flags::C);

    let mut c = cpu();
    c.regs.b = 0x01;
    let mut b = bus(&[0x05]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0x00);
    assert_eq!(c.regs.f(), flags::Z | flags::N);

    let mut c = cpu();
    c.regs.b = 0x00;
    let mut b = bus(&[0x05]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0xFF);
    assert_eq!(c.regs.f(), flags::N | flags::H);
}

#[test]
fn inc_hl_is_read_modify_write() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    let mut b = bus(&[0x34]);
    b.mem[0xC800] = 0x0F;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0x34), Read(0xC800, 0x0F), Write(0xC800, 0x10)]
    );
    assert_eq!(c.regs.f(), flags::H);
}

#[test]
fn inc_dec_rp_trace_and_wrap() {
    let mut c = cpu();
    c.regs.sp = 0xFFFF;
    let mut b = bus(&[0x33, 0x3B]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x33), TickAddr(0xFFFF)]);
    assert_eq!(c.regs.sp, 0x0000);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x3B), TickAddr(0x0000)]);
    assert_eq!(c.regs.sp, 0xFFFF);
    assert_eq!(c.regs.f(), 0); // no flags
}

#[test]
fn inc_dec_rp_internal_cycle_drives_pre_op_value() {
    // The *pre*-increment/decrement value rides the address bus: blargg
    // oam_bug/2-causes corrupts on INC DE from $FE00 but 3-non_causes is
    // clean on INC DE from $FDFF and DEC DE from $FF00.
    let mut c = cpu();
    c.regs.set_de(0xFE00);
    let mut b = bus(&[0x13, 0x1B]); // INC DE; DEC DE
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x13), TickAddr(0xFE00)]);
    assert_eq!(c.regs.de(), 0xFE01);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x1B), TickAddr(0xFE01)]);
    assert_eq!(c.regs.de(), 0xFE00);
}

#[test]
fn non_causes_keep_plain_cycles() {
    // blargg oam_bug/3-non_causes: the 16-bit *adder* ops (ADD HL,rr;
    // ADD SP,e; LD HL,SP+e), 8-bit INC/DEC and plain indirect loads do
    // not involve the inc/dec unit — no address-bus value, no
    // increase-read.
    let mut c = cpu();
    c.regs.set_hl(0xFE00);
    c.regs.set_bc(0x0001);
    c.regs.sp = 0xFE00;
    c.regs.set_de(0xC700);
    // ADD HL,BC; ADD SP,1; LD HL,SP+1; INC E; LD A,(DE)
    let mut b = bus(&[0x09, 0xE8, 0x01, 0xF8, 0x01, 0x1C, 0x1A]);
    for _ in 0..5 {
        step(&mut c, &mut b);
    }
    assert!(
        b.log
            .iter()
            .all(|e| !matches!(e, TickAddr(_) | ReadInc(..))),
        "unexpected inc/dec-unit cycle: {:?}",
        b.log
    );
}

#[test]
fn add_hl_rp_flags_and_trace() {
    let mut c = cpu();
    c.regs.set_hl(0x0FFF);
    c.regs.set_bc(0x0001);
    c.regs.set_f(flags::Z); // Z must be preserved
    let mut b = bus(&[0x09]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x09), Tick]);
    assert_eq!(c.regs.hl(), 0x1000);
    assert_eq!(c.regs.f(), flags::Z | flags::H);

    let mut c = cpu();
    c.regs.set_hl(0x8000);
    c.regs.set_de(0x8000);
    let mut b = bus(&[0x19]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.hl(), 0x0000);
    assert_eq!(c.regs.f(), flags::C);

    // ADD HL,HL and ADD HL,SP
    let mut c = cpu();
    c.regs.set_hl(0x1234);
    let mut b = bus(&[0x29]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.hl(), 0x2468);

    let mut c = cpu();
    c.regs.set_hl(0x0001);
    c.regs.sp = 0x00FF;
    let mut b = bus(&[0x39]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.hl(), 0x0100);
}

#[test]
fn add_sp_e_timing_and_unsigned_low_byte_flags() {
    let mut c = cpu();
    c.regs.sp = 0x0FF8;
    let mut b = bus(&[0xE8, 0x08]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xE8), Read(PC0 + 1, 0x08), Tick, Tick]);
    assert_eq!(c.regs.sp, 0x1000);
    assert_eq!(c.regs.f(), flags::H | flags::C);

    // negative offset: flags still from unsigned low-byte addition
    let mut c = cpu();
    c.regs.sp = 0xD000;
    let mut b = bus(&[0xE8, 0xFF]); // SP + (-1)
    step(&mut c, &mut b);
    assert_eq!(c.regs.sp, 0xCFFF);
    assert_eq!(c.regs.f(), 0); // 0x00 + 0xFF: no half-carry, no carry
}

#[test]
fn ld_hl_sp_e_timing_and_flags() {
    let mut c = cpu();
    c.regs.sp = 0xFFFF;
    let mut b = bus(&[0xF8, 0x01]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xF8), Read(PC0 + 1, 0x01), Tick]);
    assert_eq!(c.regs.hl(), 0x0000);
    assert_eq!(c.regs.f(), flags::H | flags::C);
    assert_eq!(c.regs.sp, 0xFFFF); // SP unchanged

    let mut c = cpu();
    c.regs.sp = 0xD002;
    let mut b = bus(&[0xF8, 0xF8]); // SP + (-8)
    step(&mut c, &mut b);
    assert_eq!(c.regs.hl(), 0xCFFA);
    assert_eq!(c.regs.f(), 0);
}

#[test]
fn rotate_a_ops_never_set_z() {
    let mut c = cpu();
    c.regs.a = 0x80;
    let mut b = bus(&[0x07]); // RLCA
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x01);
    assert_eq!(c.regs.f(), flags::C);

    let mut c = cpu();
    c.regs.a = 0x00;
    c.regs.set_f(0xF0);
    let mut b = bus(&[0x07]); // result 0 but Z stays clear
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), 0);

    let mut c = cpu();
    c.regs.a = 0x01;
    let mut b = bus(&[0x0F]); // RRCA
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x80);
    assert_eq!(c.regs.f(), flags::C);

    let mut c = cpu();
    c.regs.a = 0x80;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x17]); // RLA: carry in to bit 0
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x01);
    assert_eq!(c.regs.f(), flags::C);

    let mut c = cpu();
    c.regs.a = 0x01;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x1F]); // RRA: carry in to bit 7
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x80);
    assert_eq!(c.regs.f(), flags::C);
}

#[test]
fn daa_matches_reference_for_all_add_mode_inputs() {
    for fbits in 0..4u8 {
        let h = fbits & 1 != 0;
        let cf = fbits & 2 != 0;
        for a in 0..=255u8 {
            let mut c = cpu();
            c.regs.a = a;
            c.regs.set_f(fl(false, false, h, cf));
            let mut b = bus(&[0x27]);
            step(&mut c, &mut b);
            let (ra, rz, rc) = daa_add_ref(a, h, cf);
            let expect_f = fl(rz, false, false, rc);
            assert_eq!(c.regs.a, ra, "a={a:#04x} h={h} c={cf}");
            assert_eq!(c.regs.f(), expect_f, "a={a:#04x} h={h} c={cf}");
        }
    }
}

/// BCD property oracle for DAA's subtract mode, independent of the
/// flag-correction algorithm: for every valid packed-BCD operand pair,
/// SUB (and SBC with carry-in) followed by DAA must yield the decimal
/// difference modulo 100, with C set exactly on decimal borrow, N kept
/// and H cleared.
#[test]
fn daa_after_sub_computes_bcd_difference_for_all_operands() {
    let packed = |v: u8| (v / 10) << 4 | (v % 10);
    for x in 0..100u8 {
        for y in 0..100u8 {
            // SUB B: decimal x - y.
            let mut c = cpu();
            c.regs.a = packed(x);
            c.regs.b = packed(y);
            let mut b = bus(&[0x90, 0x27]); // SUB B; DAA
            step(&mut c, &mut b);
            step(&mut c, &mut b);
            let diff = (100 + x - y) % 100;
            let borrow = x < y;
            assert_eq!(c.regs.a, packed(diff), "sub x={x} y={y}");
            assert_eq!(
                c.regs.f(),
                fl(diff == 0, true, false, borrow),
                "sub x={x} y={y}"
            );

            // SBC B with carry in: decimal x - y - 1.
            let mut c = cpu();
            c.regs.a = packed(x);
            c.regs.b = packed(y);
            c.regs.set_f(fl(false, false, false, true));
            let mut b = bus(&[0x98, 0x27]); // SBC B; DAA
            step(&mut c, &mut b);
            step(&mut c, &mut b);
            let diff = (99 + x - y) % 100;
            let borrow = x <= y;
            assert_eq!(c.regs.a, packed(diff), "sbc x={x} y={y}");
            assert_eq!(
                c.regs.f(),
                fl(diff == 0, true, false, borrow),
                "sbc x={x} y={y}"
            );
        }
    }
}

#[test]
fn daa_bcd_examples() {
    // 0x15 + 0x27 = 0x42 BCD
    let mut c = cpu();
    c.regs.a = 0x15;
    c.regs.b = 0x27;
    let mut b = bus(&[0x80, 0x27]);
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x42);
    assert_eq!(c.regs.f(), 0);

    // 0x90 + 0x90 = 0x180 BCD: result 0x80 with carry
    let mut c = cpu();
    c.regs.a = 0x90;
    c.regs.b = 0x90;
    let mut b = bus(&[0x80, 0x27]);
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x80);
    assert_eq!(c.regs.f(), flags::C);

    // 0x20 - 0x13 = 0x07 BCD
    let mut c = cpu();
    c.regs.a = 0x20;
    c.regs.b = 0x13;
    let mut b = bus(&[0x90, 0x27]);
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x07);
    assert_eq!(c.regs.f(), flags::N);
}

#[test]
fn cpl_scf_ccf() {
    let mut c = cpu();
    c.regs.a = 0x35;
    c.regs.set_f(flags::Z | flags::C);
    let mut b = bus(&[0x2F]); // CPL: Z,C preserved; N,H set
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0xCA);
    assert_eq!(c.regs.f(), flags::Z | flags::N | flags::H | flags::C);

    let mut c = cpu();
    c.regs.set_f(flags::Z | flags::N | flags::H);
    let mut b = bus(&[0x37]); // SCF
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::Z | flags::C);

    let mut c = cpu();
    c.regs.set_f(flags::N | flags::H | flags::C);
    let mut b = bus(&[0x3F]); // CCF: complement carry
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), 0);
    let mut b = bus(&[0x3F]);
    c.regs.pc = PC0;
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::C);
}
