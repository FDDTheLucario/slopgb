//! Mode/width rules pinned to the datasheet: reset state, XCE, REP/SEP.

use super::*;

#[test]
fn reset_is_emulation_8bit() {
    let r = Regs::at_reset();
    assert!(r.e, "reset enters emulation mode");
    assert!(!r.acc16(), "8-bit accumulator at reset");
    assert!(!r.idx16(), "8-bit index at reset");
    assert_eq!(r.d, 0, "direct page 0");
    assert_eq!((r.pbr, r.dbr), (0, 0), "banks 0");
    assert_eq!(r.s & 0xFF00, 0x0100, "stack in page 1");
    assert_ne!(r.p & flag::I, 0, "IRQs masked");
    assert_eq!(r.p & flag::D, 0, "decimal off");
}

#[test]
fn clc_xce_enters_native_then_sec_xce_returns() {
    let mut r = Regs::at_reset(); // emulation, C=0
    // CLC; XCE  →  native (E := old C = 0), and C := old E = 1.
    r.rep(flag::C);
    r.xce();
    assert!(!r.e, "entered native mode");
    assert_ne!(r.p & flag::C, 0, "carry holds the old emulation bit");

    // Native lets M/X clear to 16-bit.
    r.rep(flag::M | flag::X);
    assert!(r.acc16() && r.idx16(), "16-bit A/X/Y in native");

    // SEC; XCE  →  back to emulation, forcing 8-bit + page-1 stack.
    r.x = 0x1234;
    r.y = 0xABCD;
    r.p |= flag::C;
    r.xce();
    assert!(r.e, "back in emulation");
    assert!(!r.acc16() && !r.idx16(), "forced 8-bit");
    assert_eq!((r.x, r.y), (0x0034, 0x00CD), "index high bytes cleared");
    assert_eq!(r.s & 0xFF00, 0x0100, "stack re-pinned to page 1");
}

#[test]
fn rep_sep_toggle_width_in_native_only() {
    let mut r = Regs::at_reset();
    r.rep(flag::C);
    r.xce(); // native

    r.sep(flag::M | flag::X);
    assert!(!r.acc16() && !r.idx16(), "SEP → 8-bit");
    r.rep(flag::M | flag::X);
    assert!(r.acc16() && r.idx16(), "REP → 16-bit");

    // In emulation REP cannot widen M/X.
    r.p |= flag::C;
    r.xce(); // emulation
    r.rep(flag::M | flag::X);
    assert!(
        !r.acc16() && !r.idx16(),
        "emulation stays 8-bit through REP"
    );
}

#[test]
fn sep_x_truncates_index_high() {
    let mut r = Regs::at_reset();
    r.rep(flag::C);
    r.xce();
    r.rep(flag::X); // 16-bit index
    r.x = 0x12FF;
    r.y = 0x34AB;
    r.sep(flag::X); // back to 8-bit index
    assert_eq!((r.x, r.y), (0x00FF, 0x00AB), "SEP X drops index high bytes");
}
