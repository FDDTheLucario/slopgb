//! Per-opcode / per-flag unit tests, with emphasis on the documented quirks.
//!
//! Exhaustive per-opcode correctness is covered by the `SingleStepTests`
//! harness ([`super::harness`]); these tests pin the awkward semantics called
//! out in the SPC700 references so a regression is legible without the full
//! suite.

use super::*;

// -- ADC / SBC / CMP flag semantics ---------------------------------------

#[test]
fn adc_sets_h_v_c() {
    // 0x50 + 0x50 = 0xA0: signed overflow (two positives → negative), N set,
    // no carry, no half-carry.
    let (s, c) = run1(&[0x88, 0x50], |s| s.a = 0x50);
    assert_eq!(s.a, 0xA0);
    assert!(s.psw.n && s.psw.v && !s.psw.c && !s.psw.h && !s.psw.z);
    assert_eq!(c, 2);

    // 0x0F + 0x01 = 0x10: half-carry out of bit 3.
    let (s, _) = run1(&[0x88, 0x01], |s| s.a = 0x0F);
    assert_eq!(s.a, 0x10);
    assert!(s.psw.h && !s.psw.c);

    // 0xFF + 0x01 = 0x00: carry + zero.
    let (s, _) = run1(&[0x88, 0x01], |s| {
        s.a = 0xFF;
    });
    assert_eq!(s.a, 0x00);
    assert!(s.psw.c && s.psw.z && s.psw.h);
}

#[test]
fn adc_honors_carry_in() {
    // 0x01 + 0x01 + C(1) = 0x03.
    let (s, _) = run1(&[0x88, 0x01], |s| {
        s.a = 0x01;
        s.psw.c = true;
    });
    assert_eq!(s.a, 0x03);
}

#[test]
fn sbc_carry_is_inverted_borrow() {
    // 0x50 - 0x10 with C=1 (no borrow-in) = 0x40, C stays set (no borrow-out).
    let (s, _) = run1(&[0xA8, 0x10], |s| {
        s.a = 0x50;
        s.psw.c = true;
    });
    assert_eq!(s.a, 0x40);
    assert!(s.psw.c && !s.psw.n && !s.psw.z);

    // 0x10 - 0x20 with C=1 = 0xF0 with borrow → C cleared, N set.
    let (s, _) = run1(&[0xA8, 0x20], |s| {
        s.a = 0x10;
        s.psw.c = true;
    });
    assert_eq!(s.a, 0xF0);
    assert!(!s.psw.c && s.psw.n);
}

#[test]
fn cmp_touches_only_nzc() {
    // CMP leaves V and H untouched (unlike SBC).
    let (s, _) = run1(&[0x68, 0x50], |s| {
        s.a = 0x50;
        s.psw.v = true;
        s.psw.h = true;
    });
    assert!(s.psw.z && s.psw.c && !s.psw.n);
    assert!(s.psw.v && s.psw.h, "CMP must not disturb V/H");
    assert_eq!(s.a, 0x50, "CMP must not write A");
}

// -- logic / shift / inc / dec --------------------------------------------

#[test]
fn logic_ops() {
    let (s, _) = run1(&[0x28, 0x0F], |s| s.a = 0xF0); // AND A,#0F
    assert_eq!(s.a, 0x00);
    assert!(s.psw.z);
    let (s, _) = run1(&[0x48, 0xFF], |s| s.a = 0x0F); // EOR A,#FF
    assert_eq!(s.a, 0xF0);
    assert!(s.psw.n && !s.psw.z);
}

#[test]
fn shifts_and_rotates() {
    let (s, _) = run1(&[0x1C], |s| s.a = 0x81); // ASL A
    assert_eq!(s.a, 0x02);
    assert!(s.psw.c);
    let (s, _) = run1(&[0x5C], |s| s.a = 0x03); // LSR A
    assert_eq!(s.a, 0x01);
    assert!(s.psw.c);
    let (s, _) = run1(&[0x3C], |s| {
        s.a = 0x80; // ROL A with C=1 → 0x01, C=old bit7=1
        s.psw.c = true;
    });
    assert_eq!(s.a, 0x01);
    assert!(s.psw.c);
    let (s, _) = run1(&[0x7C], |s| {
        s.a = 0x01; // ROR A with C=1 → 0x80, C=old bit0=1
        s.psw.c = true;
    });
    assert_eq!(s.a, 0x80);
    assert!(s.psw.c && s.psw.n);
}

#[test]
fn inc_dec_wraparound() {
    let (s, _) = run1(&[0xBC], |s| s.a = 0xFF); // INC A
    assert_eq!(s.a, 0x00);
    assert!(s.psw.z);
    let (s, _) = run1(&[0x9C], |s| s.a = 0x00); // DEC A
    assert_eq!(s.a, 0xFF);
    assert!(s.psw.n);
}

// -- MOV load/store flag behaviour ----------------------------------------

#[test]
fn mov_load_sets_nz_store_does_not() {
    let (s, _) = run1(&[0xE8, 0x00], |_| {}); // MOV A,#00
    assert!(s.psw.z);
    let (s, _) = run1(&[0xE8, 0x80], |_| {}); // MOV A,#80
    assert!(s.psw.n);
    // MOV dp,A must not touch flags.
    let (s, _) = run1(&[0xC4, 0x20], |s| {
        s.a = 0x37;
        s.psw.z = true;
        s.psw.n = true;
    });
    assert_eq!(s.ram[0x0020], 0x37);
    assert!(s.psw.z && s.psw.n, "MOV store must not alter flags");
}

#[test]
fn mov_sp_x_has_no_flags_but_x_sp_does() {
    let (s, _) = run1(&[0xBD], |s| {
        s.x = 0x00;
        s.psw.z = false;
    });
    assert_eq!(s.sp, 0x00);
    assert!(!s.psw.z, "MOV SP,X must not set Z");
    let (s, _) = run1(&[0x9D], |s| s.sp = 0x00); // MOV X,SP
    assert_eq!(s.x, 0x00);
    assert!(s.psw.z, "MOV X,SP sets Z");
}

#[test]
fn mov_x_autoincrement() {
    // MOV A,(X)+
    let (s, c) = run1(&[0xBF], |s| {
        s.x = 0x10;
        s.ram[0x0010] = 0x99;
    });
    assert_eq!(s.a, 0x99);
    assert_eq!(s.x, 0x11);
    assert!(s.psw.n);
    assert_eq!(c, 4);
    // MOV (X)+,A — no flags.
    let (s, _) = run1(&[0xAF], |s| {
        s.x = 0x10;
        s.a = 0x55;
        s.psw.z = true;
    });
    assert_eq!(s.ram[0x0010], 0x55);
    assert_eq!(s.x, 0x11);
    assert!(s.psw.z, "store form must not touch flags");
}

#[test]
fn direct_page_p_flag_selects_page1() {
    // MOV A,dp with P=1 reads $01xx, not $00xx.
    let (s, _) = run1(&[0xE4, 0x05], |s| {
        s.psw.p = true;
        s.ram[0x0105] = 0x7E;
        s.ram[0x0005] = 0x11;
    });
    assert_eq!(s.a, 0x7E);
}

#[test]
fn mov_dp_dp_operand_order() {
    // FA nn mm : [mm] = [nn] (first byte = source).
    let (s, c) = run1(&[0xFA, 0x10, 0x20], |s| s.ram[0x0010] = 0xC3);
    assert_eq!(s.ram[0x0020], 0xC3);
    assert_eq!(c, 5);
    // 8F nn mm : [mm] = nn (first byte = immediate).
    let (s, _) = run1(&[0x8F, 0xAB, 0x30], |_| {});
    assert_eq!(s.ram[0x0030], 0xAB);
    // OR dp,dp — 09 nn mm : [mm] |= [nn].
    let (s, _) = run1(&[0x09, 0x10, 0x20], |s| {
        s.ram[0x0010] = 0x0F;
        s.ram[0x0020] = 0xF0;
    });
    assert_eq!(s.ram[0x0020], 0xFF);
}

// -- XCN / MUL ------------------------------------------------------------

#[test]
fn xcn_swaps_nibbles() {
    let (s, c) = run1(&[0x9F], |s| s.a = 0x12);
    assert_eq!(s.a, 0x21);
    assert_eq!(c, 5);
}

#[test]
fn mul_ya_high_byte_flags() {
    // 0x10 * 0x10 = 0x0100 → Y=0x01, A=0x00. N,Z from Y.
    let (s, c) = run1(&[0xCF], |s| {
        s.y = 0x10;
        s.a = 0x10;
    });
    assert_eq!(s.y, 0x01);
    assert_eq!(s.a, 0x00);
    assert!(!s.psw.z && !s.psw.n);
    assert_eq!(c, 9);
    // 0xFF * 0xFF = 0xFE01.
    let (s, _) = run1(&[0xCF], |s| {
        s.y = 0xFF;
        s.a = 0xFF;
    });
    assert_eq!(s.y, 0xFE);
    assert_eq!(s.a, 0x01);
    assert!(s.psw.n, "N from Y (0xFE)");
}

// -- DIV YA,X quirk (anomie SPC700 doc / bsnes instructionDivide) ----------

#[test]
fn div_ordinary() {
    // 10 / 3 = 3 rem 1.
    let (s, c) = run1(&[0x9E], |s| {
        s.y = 0x00;
        s.a = 0x0A;
        s.x = 0x03;
    });
    assert_eq!(s.a, 0x03); // quotient
    assert_eq!(s.y, 0x01); // remainder
    assert!(!s.psw.v && !s.psw.h);
    assert_eq!(c, 12);
}

#[test]
fn div_overflow_quirk() {
    // YA = 0x1100 (4352), X = 2. True quotient 2176 > 255 → overflow. The S-SMP
    // produces A=0xF2, Y=0x1C via its non-restoring divider (bsnes formula).
    let (s, _) = run1(&[0x9E], |s| {
        s.y = 0x11;
        s.a = 0x00;
        s.x = 0x02;
    });
    assert_eq!(s.a, 0xF2);
    assert_eq!(s.y, 0x1C);
    assert!(s.psw.v, "V set: quotient does not fit in 8 bits");
    assert!(!s.psw.h, "H = (Y&15)>=(X&15) = 1>=2 = false");
    assert!(s.psw.n, "N from A=0xF2");
}

#[test]
fn div_half_carry_flag() {
    // H = (Y & 15) >= (X & 15). Y=0x08, X=0x05 → 8 >= 5 → H set. No overflow
    // (Y < X? 8 >= 5 so Y>=X → V set too), pick Y<X to isolate: Y=0x03,X=0x1F.
    let (s, _) = run1(&[0x9E], |s| {
        s.y = 0x03;
        s.a = 0x00;
        s.x = 0x1F;
    });
    // (Y&15)=3, (X&15)=0x0F=15 → 3>=15 false → H clear; V: Y(3)>=X(0x1F) false.
    assert!(!s.psw.h && !s.psw.v);
    // Now Y=0x0F, X=0x02 (Y>=X → overflow), (Y&15)=15>=(X&15)=2 → H set.
    let (s, _) = run1(&[0x9E], |s| {
        s.y = 0x0F;
        s.a = 0x00;
        s.x = 0x02;
    });
    assert!(s.psw.h && s.psw.v);
}

#[test]
fn div_by_zero_does_not_panic() {
    // X = 0 routes to the else branch (divisor 256), never a real /0.
    let (s, _) = run1(&[0x9E], |s| {
        s.y = 0x00;
        s.a = 0x08;
        s.x = 0x00;
    });
    // a = 255 - (8 / 256) = 255; y = 0 + (8 % 256) = 8.
    assert_eq!(s.a, 0xFF);
    assert_eq!(s.y, 0x08);
    assert!(s.psw.v && s.psw.h);
}

// -- DAA / DAS (bsnes decimal-adjust) -------------------------------------

#[test]
fn daa_boundaries() {
    // 0x0A with low nibble > 9 → +0x06 = 0x10.
    let (s, c) = run1(&[0xDF], |s| s.a = 0x0A);
    assert_eq!(s.a, 0x10);
    assert_eq!(c, 3);
    // 0x9A → high nibble adjust (+0x60, C set) then low (+0x06) = 0x00, C, Z.
    let (s, _) = run1(&[0xDF], |s| s.a = 0x9A);
    assert_eq!(s.a, 0x00);
    assert!(s.psw.c && s.psw.z);
    // Carry-in forces the high adjust even when A<=0x99.
    let (s, _) = run1(&[0xDF], |s| {
        s.a = 0x10;
        s.psw.c = true;
    });
    assert_eq!(s.a, 0x70);
    assert!(s.psw.c);
}

#[test]
fn das_boundaries() {
    // Borrow (C=0) and half-borrow (H=0) both force subtraction.
    let (s, c) = run1(&[0xBE], |s| {
        s.a = 0x00;
        s.psw.c = false;
        s.psw.h = false;
    });
    assert_eq!(s.a, 0x9A);
    assert!(!s.psw.c);
    assert_eq!(c, 3);
    // Valid BCD (C=1, H=1, A<=0x99): no adjustment.
    let (s, _) = run1(&[0xBE], |s| {
        s.a = 0x15;
        s.psw.c = true;
        s.psw.h = true;
    });
    assert_eq!(s.a, 0x15);
    assert!(s.psw.c);
}

// -- 16-bit word ops ------------------------------------------------------

#[test]
fn addw_high_byte_half_carry() {
    // 0x0F00 + 0x0100 = 0x1000: half-carry from bit 11 (high-byte 0x0F+0x01).
    let (s, c) = run1(&[0x7A, 0x40], |s| {
        s.y = 0x0F;
        s.a = 0x00;
        s.ram[0x0040] = 0x00; // low byte of dp word
        s.ram[0x0041] = 0x01; // high byte
    });
    assert_eq!(s.y, 0x10);
    assert_eq!(s.a, 0x00);
    assert!(s.psw.h, "H = carry out of bit 11");
    assert!(!s.psw.c && !s.psw.z && !s.psw.n);
    assert_eq!(c, 5);
}

#[test]
fn addw_carry_and_zero() {
    // 0xFFFF + 0x0001 = 0x0000 with carry.
    let (s, _) = run1(&[0x7A, 0x40], |s| {
        s.y = 0xFF;
        s.a = 0xFF;
        s.ram[0x0040] = 0x01;
        s.ram[0x0041] = 0x00;
    });
    assert_eq!(s.y, 0x00);
    assert_eq!(s.a, 0x00);
    assert!(s.psw.c && s.psw.z);
}

#[test]
fn subw_basic_and_borrow() {
    // 0x1234 - 0x0111 = 0x1123, no borrow → C set.
    let (s, c) = run1(&[0x9A, 0x40], |s| {
        s.y = 0x12;
        s.a = 0x34;
        s.ram[0x0040] = 0x11;
        s.ram[0x0041] = 0x01;
    });
    assert_eq!(s.y, 0x11);
    assert_eq!(s.a, 0x23);
    assert!(s.psw.c);
    assert_eq!(c, 5);
    // 0x0000 - 0x0001 = 0xFFFF, borrow → C clear, N set.
    let (s, _) = run1(&[0x9A, 0x40], |s| {
        s.y = 0x00;
        s.a = 0x00;
        s.ram[0x0040] = 0x01;
        s.ram[0x0041] = 0x00;
    });
    assert_eq!(s.y, 0xFF);
    assert_eq!(s.a, 0xFF);
    assert!(!s.psw.c && s.psw.n);
}

#[test]
fn cmpw_touches_only_nzc() {
    let (s, c) = run1(&[0x5A, 0x40], |s| {
        s.y = 0x12;
        s.a = 0x34;
        s.ram[0x0040] = 0x34;
        s.ram[0x0041] = 0x12;
        s.psw.v = true;
        s.psw.h = true;
    });
    assert!(s.psw.z && s.psw.c);
    assert!(s.psw.v && s.psw.h, "CMPW must not touch V/H");
    assert_eq!(c, 4);
}

#[test]
fn incw_decw() {
    // INCW: 0x00FF → 0x0100.
    let (s, c) = run1(&[0x3A, 0x40], |s| {
        s.ram[0x0040] = 0xFF;
        s.ram[0x0041] = 0x00;
    });
    assert_eq!(s.ram[0x0040], 0x00);
    assert_eq!(s.ram[0x0041], 0x01);
    assert!(!s.psw.z && !s.psw.n);
    assert_eq!(c, 6);
    // DECW: 0x0000 → 0xFFFF.
    let (s, _) = run1(&[0x1A, 0x40], |s| {
        s.ram[0x0040] = 0x00;
        s.ram[0x0041] = 0x00;
    });
    assert_eq!(s.ram[0x0040], 0xFF);
    assert_eq!(s.ram[0x0041], 0xFF);
    assert!(s.psw.n);
}

#[test]
fn movw_load_store() {
    // MOVW YA,dp — N from bit 15, Z from whole word.
    let (s, c) = run1(&[0xBA, 0x40], |s| {
        s.ram[0x0040] = 0x00;
        s.ram[0x0041] = 0x80;
    });
    assert_eq!(s.a, 0x00);
    assert_eq!(s.y, 0x80);
    assert!(s.psw.n && !s.psw.z);
    assert_eq!(c, 5);
    // MOVW dp,YA — no flags.
    let (s, _) = run1(&[0xDA, 0x40], |s| {
        s.y = 0xAB;
        s.a = 0xCD;
        s.psw.z = true;
    });
    assert_eq!(s.ram[0x0040], 0xCD);
    assert_eq!(s.ram[0x0041], 0xAB);
    assert!(s.psw.z, "MOVW store must not touch flags");
}

// -- branches / jumps / calls ---------------------------------------------

#[test]
fn conditional_branch_taken_costs_two_more() {
    // BEQ +4 taken.
    let (s, c) = run1(&[0xF0, 0x04], |s| s.psw.z = true);
    assert_eq!(s.pc, 0x0202u16.wrapping_add(4));
    assert_eq!(c, 4);
    // BEQ not taken.
    let (s, c) = run1(&[0xF0, 0x04], |s| s.psw.z = false);
    assert_eq!(s.pc, 0x0202);
    assert_eq!(c, 2);
    // Negative offset.
    let (s, _) = run1(&[0xD0, 0xFC], |s| s.psw.z = false); // BNE -4
    assert_eq!(s.pc, 0x0202u16.wrapping_sub(4));
}

#[test]
fn bra_always_taken() {
    let (s, c) = run1(&[0x2F, 0x10], |_| {});
    assert_eq!(s.pc, 0x0202 + 0x10);
    assert_eq!(c, 4);
}

#[test]
fn jmp_abs_and_indexed() {
    let (s, _) = run1(&[0x5F, 0x34, 0x12], |_| {});
    assert_eq!(s.pc, 0x1234);
    // JMP [!abs+X]
    let (s, c) = run1(&[0x1F, 0x00, 0x20], |s| {
        s.x = 0x04;
        s.ram[0x2004] = 0x78;
        s.ram[0x2005] = 0x56;
    });
    assert_eq!(s.pc, 0x5678);
    assert_eq!(c, 6);
}

#[test]
fn call_pushes_return_address() {
    let (s, c) = run1(&[0x3F, 0x00, 0x30], |s| s.sp = 0xEF); // CALL $3000
    assert_eq!(s.pc, 0x3000);
    assert_eq!(s.sp, 0xED);
    // Return address 0x0203 pushed high-then-low.
    assert_eq!(s.ram[0x01EF], 0x02); // high
    assert_eq!(s.ram[0x01EE], 0x03); // low
    assert_eq!(c, 8);
}

#[test]
fn ret_pulls_pc() {
    let (s, c) = run1(&[0x6F], |s| {
        s.sp = 0xED;
        s.ram[0x01EE] = 0x03; // low
        s.ram[0x01EF] = 0x02; // high
    });
    assert_eq!(s.pc, 0x0203);
    assert_eq!(s.sp, 0xEF);
    assert_eq!(c, 5);
}

#[test]
fn tcall_reads_vector_table() {
    // TCALL 0 (0x01) → vector at $FFDE.
    let (s, c) = run1(&[0x01], |s| {
        s.sp = 0xEF;
        s.ram[0xFFDE] = 0x00;
        s.ram[0xFFDF] = 0x40;
    });
    assert_eq!(s.pc, 0x4000);
    assert_eq!(s.sp, 0xED);
    assert_eq!(c, 8);
    // TCALL 15 (0xF1) → vector at $FFC0.
    let (s, _) = run1(&[0xF1], |s| {
        s.sp = 0xEF;
        s.ram[0xFFC0] = 0x11;
        s.ram[0xFFC1] = 0x22;
    });
    assert_eq!(s.pc, 0x2211);
}

#[test]
fn pcall_upper_page() {
    let (s, c) = run1(&[0x4F, 0x50], |s| s.sp = 0xEF); // PCALL $50 → $FF50
    assert_eq!(s.pc, 0xFF50);
    assert_eq!(c, 6);
}

#[test]
fn push_pop_roundtrip() {
    let (s, c) = run1(&[0x2D], |s| {
        s.a = 0x42;
        s.sp = 0xEF;
    });
    assert_eq!(s.ram[0x01EF], 0x42);
    assert_eq!(s.sp, 0xEE);
    assert_eq!(c, 4);
    let (s, c) = run1(&[0xAE], |s| {
        s.sp = 0xEE;
        s.ram[0x01EF] = 0x42;
    });
    assert_eq!(s.a, 0x42);
    assert_eq!(s.sp, 0xEF);
    assert_eq!(c, 4);
}

#[test]
fn cbne_dbnz_affect_no_flags() {
    // CBNE dp,rel: [dp] != A → branch, no flags.
    let (s, c) = run1(&[0x2E, 0x40, 0x05], |s| {
        s.a = 0x10;
        s.ram[0x0040] = 0x99;
        s.psw.z = true;
    });
    assert_eq!(s.pc, 0x0203 + 5);
    assert!(s.psw.z, "CBNE must not set flags");
    assert_eq!(c, 7);
    // Equal → no branch.
    let (s, c) = run1(&[0x2E, 0x40, 0x05], |s| {
        s.a = 0x99;
        s.ram[0x0040] = 0x99;
    });
    assert_eq!(s.pc, 0x0203);
    assert_eq!(c, 5);
    // DBNZ dp,rel: decrement memory, branch if non-zero.
    let (s, c) = run1(&[0x6E, 0x40, 0x05], |s| s.ram[0x0040] = 0x02);
    assert_eq!(s.ram[0x0040], 0x01);
    assert_eq!(s.pc, 0x0203 + 5);
    assert_eq!(c, 7);
    // DBNZ Y,rel down to zero → no branch.
    let (s, c) = run1(&[0xFE, 0x05], |s| s.y = 0x01);
    assert_eq!(s.y, 0x00);
    assert_eq!(s.pc, 0x0202);
    assert_eq!(c, 4);
}

// -- bit ops --------------------------------------------------------------

#[test]
fn set1_clr1_pick_bit_from_opcode() {
    let (s, c) = run1(&[0x02, 0x40], |s| s.ram[0x0040] = 0x00); // SET1 dp.0
    assert_eq!(s.ram[0x0040], 0x01);
    assert_eq!(c, 4);
    let (s, _) = run1(&[0x92, 0x40], |s| s.ram[0x0040] = 0xFF); // CLR1 dp.4
    assert_eq!(s.ram[0x0040], 0xEF);
    let (s, _) = run1(&[0xE2, 0x40], |s| s.ram[0x0040] = 0x00); // SET1 dp.7
    assert_eq!(s.ram[0x0040], 0x80);
}

#[test]
fn bbs_bbc_branch_on_bit() {
    // BBS dp.0,rel — bit set → branch.
    let (s, c) = run1(&[0x03, 0x40, 0x05], |s| s.ram[0x0040] = 0x01);
    assert_eq!(s.pc, 0x0203 + 5);
    assert_eq!(c, 7);
    // BBC dp.7,rel — bit clear → branch.
    let (s, _) = run1(&[0xF3, 0x40, 0x05], |s| s.ram[0x0040] = 0x00);
    assert_eq!(s.pc, 0x0203 + 5);
    // BBS not taken.
    let (s, c) = run1(&[0x03, 0x40, 0x05], |s| s.ram[0x0040] = 0x00);
    assert_eq!(s.pc, 0x0203);
    assert_eq!(c, 5);
}

#[test]
fn tset1_tclr1() {
    // TSET1: mem |= A; N,Z from A - mem_original.
    let (s, c) = run1(&[0x0E, 0x00, 0x20], |s| {
        s.a = 0xF0;
        s.ram[0x2000] = 0x0F;
    });
    assert_eq!(s.ram[0x2000], 0xFF);
    assert!(s.psw.n, "N from 0xF0 - 0x0F = 0xE1");
    assert_eq!(c, 6);
    // TCLR1: mem &= ~A.
    let (s, _) = run1(&[0x4E, 0x00, 0x20], |s| {
        s.a = 0xF0;
        s.ram[0x2000] = 0xFF;
    });
    assert_eq!(s.ram[0x2000], 0x0F);
    // Equal operands → Z.
    let (s, _) = run1(&[0x0E, 0x00, 0x20], |s| {
        s.a = 0x0F;
        s.ram[0x2000] = 0x0F;
    });
    assert!(s.psw.z);
}

#[test]
fn membit_carry_ops_use_13bit_addr() {
    // Operand word = (bit << 13) | addr. addr=0x0005, bit=3 → 0x6005.
    // MOV1 C, mem.bit — C = bit.
    let (s, c) = run1(&[0xAA, 0x05, 0x60], |s| s.ram[0x0005] = 0x08);
    assert!(s.psw.c);
    assert_eq!(c, 4);
    let (s, _) = run1(&[0xAA, 0x05, 0x60], |s| s.ram[0x0005] = 0x00);
    assert!(!s.psw.c);
    // MOV1 mem.bit, C — write C into the bit.
    let (s, c) = run1(&[0xCA, 0x05, 0x60], |s| {
        s.psw.c = true;
        s.ram[0x0005] = 0x00;
    });
    assert_eq!(s.ram[0x0005], 0x08);
    assert_eq!(c, 6);
    // NOT1 — flip the bit.
    let (s, _) = run1(&[0xEA, 0x05, 0x60], |s| s.ram[0x0005] = 0x08);
    assert_eq!(s.ram[0x0005], 0x00);
}

#[test]
fn and1_or1_eor1_combine_carry() {
    // AND1 C, mem.bit (bit=0): C = C & bit.
    let (s, _) = run1(&[0x4A, 0x05, 0x00], |s| {
        s.psw.c = true;
        s.ram[0x0005] = 0x00; // bit0 = 0
    });
    assert!(!s.psw.c);
    // AND1 C, /mem.bit (0x6A): C = C & !bit.
    let (s, _) = run1(&[0x6A, 0x05, 0x00], |s| {
        s.psw.c = true;
        s.ram[0x0005] = 0x00; // !bit = 1
    });
    assert!(s.psw.c);
    // OR1 C, mem.bit: C = C | bit.
    let (s, _) = run1(&[0x0A, 0x05, 0x00], |s| {
        s.psw.c = false;
        s.ram[0x0005] = 0x01;
    });
    assert!(s.psw.c);
    // EOR1 C, mem.bit.
    let (s, _) = run1(&[0x8A, 0x05, 0x00], |s| {
        s.psw.c = true;
        s.ram[0x0005] = 0x01;
    });
    assert!(!s.psw.c);
}

// -- flag / control ops ---------------------------------------------------

#[test]
fn clrv_clears_v_and_h() {
    let (s, c) = run1(&[0xE0], |s| {
        s.psw.v = true;
        s.psw.h = true;
    });
    assert!(!s.psw.v && !s.psw.h, "CLRV clears both V and H");
    assert_eq!(c, 2);
}

#[test]
fn flag_setters() {
    assert!(run1(&[0x80], |_| {}).0.psw.c); // SETC
    assert!(!run1(&[0x60], |s| s.psw.c = true).0.psw.c); // CLRC
    assert!(!run1(&[0xED], |s| s.psw.c = true).0.psw.c); // NOTC
    assert!(run1(&[0x40], |_| {}).0.psw.p); // SETP
    assert!(!run1(&[0x20], |s| s.psw.p = true).0.psw.p); // CLRP
    assert!(run1(&[0xA0], |_| {}).0.psw.i); // EI
    assert!(!run1(&[0xC0], |s| s.psw.i = true).0.psw.i); // DI
}

#[test]
fn brk_and_reti_roundtrip() {
    // BRK: push PC + PSW, jump to [$FFDE], set B, clear I.
    let (s, c) = run1(&[0x0F], |s| {
        s.sp = 0xEF;
        s.psw.i = true;
        s.ram[0xFFDE] = 0x00;
        s.ram[0xFFDF] = 0x50;
    });
    assert_eq!(s.pc, 0x5000);
    assert!(s.psw.b && !s.psw.i);
    assert_eq!(s.sp, 0xEC); // 2 (PC) + 1 (PSW) pushed
    assert_eq!(c, 8);
    // RETI restores PSW then PC.
    let (s, c) = run1(&[0x7F], |s| {
        s.sp = 0xEC;
        s.ram[0x01ED] = 0b0000_0001; // PSW (C set)
        s.ram[0x01EE] = 0x03; // PC low
        s.ram[0x01EF] = 0x02; // PC high
    });
    assert_eq!(s.pc, 0x0203);
    assert!(s.psw.c);
    assert_eq!(s.sp, 0xEF);
    assert_eq!(c, 6);
}

#[test]
fn sleep_and_stop_halt() {
    let (s, _) = run1(&[0xEF], |_| {}); // SLEEP
    assert!(s.stopped);
    let (s, _) = run1(&[0xFF], |_| {}); // STOP
    assert!(s.stopped);
}

// -- indexed / indirect addressing ----------------------------------------

#[test]
fn indexed_indirect_addressing() {
    // MOV A,[dp+X]  (E7): pointer at dp+X.
    let (s, c) = run1(&[0xE7, 0x40], |s| {
        s.x = 0x02;
        s.ram[0x0042] = 0x00; // pointer low
        s.ram[0x0043] = 0x30; // pointer high → $3000
        s.ram[0x3000] = 0x7C;
    });
    assert_eq!(s.a, 0x7C);
    assert_eq!(c, 6);
    // MOV A,[dp]+Y  (F7): pointer at dp, then +Y.
    let (s, c) = run1(&[0xF7, 0x40], |s| {
        s.y = 0x05;
        s.ram[0x0040] = 0x00; // pointer → $3000
        s.ram[0x0041] = 0x30;
        s.ram[0x3005] = 0x9D;
    });
    assert_eq!(s.a, 0x9D);
    assert_eq!(c, 6);
}

#[test]
fn dp_pointer_wraps_within_page() {
    // Pointer at dp=0xFF reads low from $00FF and high from $0000 (page wrap).
    let (s, _) = run1(&[0xF7, 0xFF], |s| {
        s.y = 0x00;
        s.ram[0x00FF] = 0x34; // low
        s.ram[0x0000] = 0x12; // high (wrapped)
        s.ram[0x1234] = 0x5A;
    });
    assert_eq!(s.a, 0x5A);
}
