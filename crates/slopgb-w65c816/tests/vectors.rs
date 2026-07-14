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

#[test]
fn ora() {
    run_opcodes(&[
        "09", "05", "15", "0d", "1d", "19", "0f", "1f", "01", "11", "12", "07", "17", "03", "13",
    ]);
}

#[test]
fn and_op() {
    run_opcodes(&[
        "29", "25", "35", "2d", "3d", "39", "2f", "3f", "21", "31", "32", "27", "37", "23", "33",
    ]);
}

#[test]
fn eor() {
    run_opcodes(&[
        "49", "45", "55", "4d", "5d", "59", "4f", "5f", "41", "51", "52", "47", "57", "43", "53",
    ]);
}

#[test]
fn bit_op() {
    run_opcodes(&["89", "24", "34", "2c", "3c"]);
}

#[test]
fn shifts() {
    run_opcodes(&[
        "0a", "06", "16", "0e", "1e", "4a", "46", "56", "4e", "5e", "2a", "26", "36", "2e", "3e",
        "6a", "66", "76", "6e", "7e",
    ]);
}

#[test]
fn inc_dec() {
    run_opcodes(&[
        "1a", "e6", "f6", "ee", "fe", "3a", "c6", "d6", "ce", "de", "e8", "c8", "ca", "88",
    ]);
}

#[test]
fn tsb_trb() {
    run_opcodes(&["04", "0c", "14", "1c"]);
}

#[test]
fn cmp() {
    run_opcodes(&[
        "c9", "c5", "d5", "cd", "dd", "d9", "cf", "df", "c1", "d1", "d2", "c7", "d7", "c3", "d3",
    ]);
}

#[test]
fn cpx_cpy() {
    run_opcodes(&["e0", "e4", "ec", "c0", "c4", "cc"]);
}

#[test]
fn branches() {
    run_opcodes(&["10", "30", "50", "70", "90", "b0", "d0", "f0", "80", "82"]);
}

#[test]
fn jumps_calls() {
    run_opcodes(&["4c", "5c", "6c", "7c", "dc", "20", "fc", "22", "60", "6b"]);
}
