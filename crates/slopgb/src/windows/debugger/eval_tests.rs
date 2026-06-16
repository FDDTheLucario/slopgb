use super::*;

fn regs() -> Registers {
    let mut r = Registers::default();
    r.set_bc(0x1234);
    r.set_hl(0x00D0);
    r.pc = 0x0150;
    r.sp = 0xFFFE;
    r
}

#[test]
fn hex_literals_and_arithmetic_wrap_in_u16() {
    let r = regs();
    let read = |_| 0u8;
    // Numbers are hex by default (bgb).
    assert_eq!(eval_expr("FF44", &r, read), Ok(0xFF44));
    assert_eq!(eval_expr("10", &r, read), Ok(0x10));
    // + - * with left-to-right precedence (* binds tighter) and parens.
    assert_eq!(eval_expr("2*3+1", &r, read), Ok(7));
    assert_eq!(eval_expr("(2+3)*4", &r, read), Ok(0x14));
    // Wrapping.
    assert_eq!(eval_expr("0-1", &r, read), Ok(0xFFFF));
}

#[test]
fn registers_take_precedence_over_hex() {
    let r = regs();
    let read = |_| 0u8;
    assert_eq!(
        eval_expr("bc", &r, read),
        Ok(0x1234),
        "bc is the register pair"
    );
    assert_eq!(eval_expr("bc+1", &r, read), Ok(0x1235));
    assert_eq!(
        eval_expr("c", &r, read),
        Ok(0x34),
        "c is register C, not hex"
    );
    assert_eq!(eval_expr("b", &r, read), Ok(0x12));
    assert_eq!(eval_expr("pc", &r, read), Ok(0x0150));
    assert_eq!(eval_expr("SP", &r, read), Ok(0xFFFE), "case-insensitive");
    // A non-register word is hex.
    assert_eq!(eval_expr("ff", &r, read), Ok(0x00FF));
}

#[test]
fn memory_deref_reads_one_byte() {
    let r = regs();
    let read = |a: u16| if a == 0xFF44 { 0x90 } else { 0x00 };
    assert_eq!(eval_expr("[FF44]", &r, read), Ok(0x0090), "LY via deref");
    // Deref of a register + offset.
    let read2 = |a: u16| if a == 0x1235 { 0xAB } else { 0x00 };
    assert_eq!(eval_expr("[bc+1]", &r, read2), Ok(0x00AB));
}

#[test]
fn malformed_input_errors_without_panicking() {
    let r = regs();
    let read = |_| 0u8;
    assert!(eval_expr("", &r, read).is_err());
    assert!(eval_expr("   ", &r, read).is_err());
    assert!(
        eval_expr("xz", &r, read).is_err(),
        "not a register, not hex"
    );
    assert!(eval_expr("1+", &r, read).is_err(), "dangling operator");
    assert!(eval_expr("[FF44", &r, read).is_err(), "unbalanced bracket");
    assert!(eval_expr("(1+2", &r, read).is_err(), "unbalanced paren");
    assert!(eval_expr("1 2", &r, read).is_err(), "trailing tokens");
    assert!(eval_expr("@", &r, read).is_err(), "bad character");
}
