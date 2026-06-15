//! `execute_tests` — control tests (split for file size).

use super::*;

#[test]
fn jp_nn_taken_and_cc_untaken() {
    let mut c = cpu();
    let mut b = bus(&[0xC3, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xC3),
            Read(PC0 + 1, 0x34),
            Read(PC0 + 2, 0x12),
            Tick
        ]
    );
    assert_eq!(c.regs.pc, 0x1234);

    let mut c = cpu(); // Z clear -> JP Z untaken
    let mut b = bus(&[0xCA, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xCA), Read(PC0 + 1, 0x34), Read(PC0 + 2, 0x12)]
    );
    assert_eq!(c.regs.pc, PC0 + 3);
}

#[test]
fn jp_hl_is_one_cycle() {
    let mut c = cpu();
    c.regs.set_hl(0x1234);
    let mut b = bus(&[0xE9]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xE9)]);
    assert_eq!(c.regs.pc, 0x1234);
}

#[test]
fn jr_taken_negative_offset_and_untaken() {
    let mut c = cpu();
    let mut b = bus(&[0x18, 0xFE]); // JR -2: back to itself
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x18), Read(PC0 + 1, 0xFE), Tick]);
    assert_eq!(c.regs.pc, PC0);

    let mut c = cpu(); // Z clear -> JR Z untaken: no internal cycle
    let mut b = bus(&[0x28, 0x05]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x28), Read(PC0 + 1, 0x05)]);
    assert_eq!(c.regs.pc, PC0 + 2);

    let mut c = cpu(); // C clear -> JR NC taken
    let mut b = bus(&[0x30, 0x05]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x30), Read(PC0 + 1, 0x05), Tick]);
    assert_eq!(c.regs.pc, PC0 + 7);
}

#[test]
fn call_nn_exact_event_order() {
    // gbctr: fetch, read lo, read hi, internal, push hi, push lo.
    let mut c = cpu();
    let mut b = bus(&[0xCD, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xCD),
            Read(PC0 + 1, 0x34),
            Read(PC0 + 2, 0x12),
            // Pre-push internal cycle drives SP (SameBoy call_a16).
            TickAddr(SP0),
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x03)
        ]
    );
    assert_eq!(c.regs.pc, 0x1234);
    assert_eq!(c.regs.sp, SP0 - 2);
}

#[test]
fn call_cc_taken_and_untaken() {
    let mut c = cpu(); // Z clear: CALL NZ taken
    let mut b = bus(&[0xC4, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(b.log.len(), 6);
    assert_eq!(c.regs.pc, 0x1234);

    let mut c = cpu(); // CALL Z untaken: 3 cycles, no pushes
    let mut b = bus(&[0xCC, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xCC), Read(PC0 + 1, 0x34), Read(PC0 + 2, 0x12)]
    );
    assert_eq!(c.regs.pc, PC0 + 3);
    assert_eq!(c.regs.sp, SP0);
}

#[test]
fn ret_and_ret_cc_traces() {
    let mut c = cpu();
    let mut b = bus(&[0xC9]);
    b.load(SP0, &[0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xC9),
            ReadInc(SP0, 0x34),
            ReadInc(SP0 + 1, 0x12),
            Tick
        ]
    );
    assert_eq!(c.regs.pc, 0x1234);

    let mut c = cpu(); // RET NZ taken (Z clear): 5 cycles
    let mut b = bus(&[0xC0]);
    b.load(SP0, &[0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xC0),
            Tick,
            ReadInc(SP0, 0x34),
            ReadInc(SP0 + 1, 0x12),
            Tick
        ]
    );
    assert_eq!(c.regs.pc, 0x1234);

    let mut c = cpu(); // RET Z untaken: 2 cycles
    let mut b = bus(&[0xC8]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xC8), Tick]);
    assert_eq!(c.regs.pc, PC0 + 1);
    assert_eq!(c.regs.sp, SP0);
}

#[test]
fn rst_timing_like_call_tail() {
    let mut c = cpu();
    let mut b = bus(&[0xEF]); // RST 28h
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xEF),
            TickAddr(SP0),
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x01)
        ]
    );
    assert_eq!(c.regs.pc, 0x0028);
}

#[test]
fn cb_register_op_is_two_cycles() {
    let mut c = cpu();
    c.regs.c = 0x88;
    let mut b = bus(&[0xCB, 0x11]); // RL C
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xCB), Read(PC0 + 1, 0x11)]);
    assert_eq!(c.regs.c, 0x10);
    assert_eq!(c.regs.f(), flags::C);
}

#[test]
fn cb_rot_kinds_results() {
    // (kind opcode on B, input, carry-in, output, carry-out)
    for (op, input, cin, out, cout) in [
        (0x00u8, 0x85u8, false, 0x0Bu8, true), // RLC
        (0x08, 0x01, false, 0x80, true),       // RRC
        (0x10, 0x80, true, 0x01, true),        // RL
        (0x18, 0x01, true, 0x80, true),        // RR
        (0x20, 0xC0, false, 0x80, true),       // SLA
        (0x28, 0x81, false, 0xC0, true),       // SRA keeps bit 7
        (0x30, 0xA5, true, 0x5A, false),       // SWAP clears C
        (0x38, 0x81, false, 0x40, true),       // SRL
    ] {
        let mut c = cpu();
        c.regs.b = input;
        c.regs.set_f(if cin { flags::C } else { 0 });
        let mut b = bus(&[0xCB, op]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.b, out, "op={op:#04x}");
        assert_eq!(c.regs.f(), fl(out == 0, false, false, cout), "op={op:#04x}");
    }
    // Z set by CB rotates (unlike RLCA-family)
    let mut c = cpu();
    c.regs.b = 0;
    let mut b = bus(&[0xCB, 0x00]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::Z);
}

#[test]
fn cb_hl_read_modify_write_is_four_cycles() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    let mut b = bus(&[0xCB, 0x26]); // SLA (HL)
    b.mem[0xC800] = 0x81;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xCB),
            Read(PC0 + 1, 0x26),
            Read(0xC800, 0x81),
            Write(0xC800, 0x02)
        ]
    );
    assert_eq!(c.regs.f(), flags::C);
}

#[test]
fn bit_hl_is_three_cycles_and_flags() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    c.regs.set_f(flags::C);
    let mut b = bus(&[0xCB, 0x7E]); // BIT 7,(HL)
    b.mem[0xC800] = 0x7F;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xCB), Read(PC0 + 1, 0x7E), Read(0xC800, 0x7F)]
    );
    // bit 7 clear -> Z set; H set; C preserved
    assert_eq!(c.regs.f(), flags::Z | flags::H | flags::C);

    let mut c = cpu();
    c.regs.h = 0x10;
    let mut b = bus(&[0xCB, 0x64]); // BIT 4,H -> set, Z clear
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::H);
}

#[test]
fn res_set_hl_are_four_cycles() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    c.regs.set_f(0xF0);
    let mut b = bus(&[0xCB, 0x86, 0xCB, 0xFE]); // RES 0,(HL); SET 7,(HL)
    b.mem[0xC800] = 0xFF;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0, 0xCB),
            Read(PC0 + 1, 0x86),
            Read(0xC800, 0xFF),
            Write(0xC800, 0xFE)
        ]
    );
    assert_eq!(c.regs.f(), 0xF0); // RES/SET touch no flags
    b.mem[0xC800] = 0x00;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 2, 0xCB),
            Read(PC0 + 3, 0xFE),
            Read(0xC800, 0x00),
            Write(0xC800, 0x80)
        ]
    );
}
