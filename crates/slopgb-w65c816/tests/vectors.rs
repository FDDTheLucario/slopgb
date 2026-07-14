//! SingleStepTests 65816 vector conformance, grouped by opcode family. Each test
//! runs all 10000 cases per opcode in both emulation and native mode. Vectors
//! are gitignored; absent, the tests skip (set `SLOPGB_REQUIRE_ROMS` to fail
//! instead). See `test-roms/download-65816-tests.sh`.

mod support;

use support::runner::{run_opcode, run_opcodes};

#[test]
fn nop() {
    run_opcode("ea");
}

#[test]
fn lda() {
    run_opcodes(&[
        "a9", "a5", "b5", "ad", "bd", "b9", "af", "bf", "a1", "b1", "b2", "a7", "b7", "a3", "b3",
    ]);
}

#[test]
fn ldx_ldy() {
    run_opcodes(&["a2", "a6", "b6", "ae", "be", "a0", "a4", "b4", "ac", "bc"]);
}

#[test]
fn sta() {
    run_opcodes(&[
        "85", "95", "8d", "9d", "99", "8f", "9f", "81", "91", "92", "87", "97", "83", "93",
    ]);
}

#[test]
fn stx_sty_stz() {
    run_opcodes(&["86", "96", "8e", "84", "94", "8c", "64", "74", "9c", "9e"]);
}

#[test]
fn transfers() {
    run_opcodes(&[
        "aa", "a8", "ba", "8a", "9a", "9b", "98", "bb", "5b", "7b", "1b", "3b",
    ]);
}

#[test]
fn xba() {
    run_opcode("eb");
}

#[test]
fn stack() {
    run_opcodes(&[
        "48", "da", "5a", "08", "8b", "4b", "0b", "68", "fa", "7a", "28", "ab", "2b", "f4", "d4",
        "62",
    ]);
}

#[test]
fn flags_mode() {
    run_opcodes(&[
        "18", "38", "58", "78", "b8", "d8", "f8", "c2", "e2", "fb", "42",
    ]);
}
