//! Disassembler tests. Every expected string/len/cycles below is the literal
//! text bgb 1.6.4 produced for that byte sequence — captured in
//! `docs/bgb-reference/disasm-probe-{1,2,3}.png` (built by `gen_disasm_rom.py`).

use super::*;

/// `(bytes, pc, text, len, cycles)` — bgb ground truth. `pc` matters only for
/// the relative jumps; everything else uses 0.
const CASES: &[(&[u8], u16, &str, u8, u8)] = &[
    (&[0x00], 0, "nop", 1, 1),
    (&[0x06, 0x12], 0, "ld b,12", 2, 2),
    (&[0x0E, 0x34], 0, "ld c,34", 2, 2),
    (&[0x3E, 0xFF], 0, "ld a,FF", 2, 2),
    (&[0x21, 0x34, 0x12], 0, "ld hl,1234", 3, 3),
    (&[0x01, 0xBE, 0xBA], 0, "ld bc,BABE", 3, 3),
    (&[0x7E], 0, "ld a,(hl)", 1, 2),
    (&[0x77], 0, "ld (hl),a", 1, 2),
    (&[0x46], 0, "ld b,(hl)", 1, 2),
    (&[0x40], 0, "ld b,b", 1, 1),
    (&[0x04], 0, "inc b", 1, 1),
    (&[0x05], 0, "dec b", 1, 1),
    (&[0x34], 0, "inc (hl)", 1, 3),
    (&[0x09], 0, "add hl,bc", 1, 2),
    (&[0x80], 0, "add b", 1, 1),
    (&[0x86], 0, "add (hl)", 1, 2),
    (&[0xC6, 0x10], 0, "add a,10", 2, 2),
    (&[0x90], 0, "sub b", 1, 1),
    (&[0xA0], 0, "and b", 1, 1),
    (&[0xB8], 0, "cp b", 1, 1),
    (&[0xFE, 0x99], 0, "cp a,99", 2, 2),
    (&[0x18, 0x02], 0x16E, "jr 0172", 2, 3),
    (&[0x20, 0xFC], 0x170, "jr nz,016E", 2, 2),
    (&[0xC3, 0x50, 0x01], 0, "jp 0150", 3, 4),
    (&[0xC2, 0x50, 0x01], 0, "jp nz,0150", 3, 3),
    (&[0xE9], 0, "jp hl", 1, 1),
    (&[0xCD, 0x50, 0x01], 0, "call 0150", 3, 6),
    (&[0xC4, 0x50, 0x01], 0, "call nz,0150", 3, 3),
    (&[0xC9], 0, "ret", 1, 4),
    (&[0xC0], 0, "ret nz", 1, 2),
    (&[0xD9], 0, "reti", 1, 4),
    (&[0xC7], 0, "rst 00", 1, 4),
    (&[0xFF], 0, "rst 38", 1, 4),
    (&[0xE0, 0x44], 0, "ld (ff00+44),a", 2, 3),
    (&[0xF0, 0x44], 0, "ld a,(ff00+44)", 2, 3),
    (&[0xE2], 0, "ld (ff00+c),a", 1, 2),
    (&[0xF2], 0, "ld a,(ff00+c)", 1, 2),
    (&[0xEA, 0x34, 0x12], 0, "ld (1234),a", 3, 4),
    (&[0xFA, 0x34, 0x12], 0, "ld a,(1234)", 3, 4),
    (&[0x08, 0x34, 0x12], 0, "ld (1234),sp", 3, 5),
    (&[0x22], 0, "ldi (hl),a", 1, 2),
    (&[0x2A], 0, "ldi a,(hl)", 1, 2),
    (&[0x32], 0, "ldd (hl),a", 1, 2),
    (&[0xF8, 0x03], 0, "ld hl,sp+03", 2, 3),
    (&[0xF9], 0, "ld sp,hl", 1, 2),
    (&[0xE8, 0x05], 0, "add sp,+05", 2, 4),
    (&[0xC5], 0, "push bc", 1, 4),
    (&[0xF1], 0, "pop af", 1, 3),
    (&[0x07], 0, "rlca", 1, 1),
    (&[0x17], 0, "rla", 1, 1),
    (&[0x27], 0, "daa", 1, 1),
    (&[0x2F], 0, "cpl", 1, 1),
    (&[0x37], 0, "scf", 1, 1),
    (&[0x3F], 0, "ccf", 1, 1),
    (&[0xF3], 0, "di", 1, 1),
    (&[0xFB], 0, "ei", 1, 1),
    (&[0x76], 0, "halt", 1, 1),
    (&[0x10, 0x00], 0, "stop", 2, 1),
    (&[0xCB, 0x7C], 0, "bit 7,h", 2, 2),
    (&[0xCB, 0x16], 0, "rl (hl)", 2, 4),
    (&[0xCB, 0x30], 0, "swap b", 2, 2),
    (&[0xCB, 0x00], 0, "rlc b", 2, 2),
    (&[0xCB, 0x3F], 0, "srl a", 2, 2),
    (&[0xCB, 0xC0], 0, "set 0,b", 2, 2),
    (&[0xCB, 0x86], 0, "res 0,(hl)", 2, 4),
    (&[0xD3], 0, "undefined opcode", 1, 0),
    (&[0xDD], 0, "undefined opcode", 1, 0),
];

#[test]
fn matches_bgb_ground_truth() {
    for &(bytes, pc, text, len, cycles) in CASES {
        let got = decode(bytes, pc);
        assert_eq!(got.text, text, "text for {bytes:02X?}");
        assert_eq!(got.len, len, "len for {bytes:02X?} ({text})");
        assert_eq!(got.cycles, cycles, "cycles for {bytes:02X?} ({text})");
    }
}

/// `(bytes, pc, rgbds_text)` — the RGBDS spelling for representative opcodes.
/// `len`/`cycles` are dialect-independent (checked above), so only text here.
const RGBDS: &[(&[u8], u16, &str)] = &[
    (&[0x00], 0, "nop"),
    (&[0x06, 0x12], 0, "ld b,$12"),
    (&[0x3E, 0xFF], 0, "ld a,$FF"),
    (&[0x21, 0x34, 0x12], 0, "ld hl,$1234"),
    (&[0x7E], 0, "ld a,[hl]"),
    (&[0x77], 0, "ld [hl],a"),
    (&[0x34], 0, "inc [hl]"),
    (&[0x86], 0, "add [hl]"),
    (&[0xC6, 0x10], 0, "add a,$10"),
    (&[0x18, 0x02], 0x16E, "jr $0172"),
    (&[0x20, 0xFC], 0x170, "jr nz,$016E"),
    (&[0xC3, 0x50, 0x01], 0, "jp $0150"),
    (&[0xCD, 0x50, 0x01], 0, "call $0150"),
    (&[0xC7], 0, "rst $00"),
    (&[0xFF], 0, "rst $38"),
    (&[0xE0, 0x44], 0, "ldh [$FF44],a"),
    (&[0xF0, 0x44], 0, "ldh a,[$FF44]"),
    (&[0xE2], 0, "ldh [c],a"),
    (&[0xF2], 0, "ldh a,[c]"),
    (&[0xEA, 0x34, 0x12], 0, "ld [$1234],a"),
    (&[0xFA, 0x34, 0x12], 0, "ld a,[$1234]"),
    (&[0x08, 0x34, 0x12], 0, "ld [$1234],sp"),
    (&[0x22], 0, "ld [hli],a"),
    (&[0x2A], 0, "ld a,[hli]"),
    (&[0x32], 0, "ld [hld],a"),
    (&[0xF8, 0x03], 0, "ld hl,sp+$03"),
    (&[0xE8, 0x05], 0, "add sp,+$05"),
    (&[0xCB, 0x16], 0, "rl [hl]"),
    (&[0xCB, 0x86], 0, "res 0,[hl]"),
    (&[0xCB, 0x7C], 0, "bit 7,h"),
    (&[0xD3], 0, "db $D3"),
    (&[0xDD], 0, "db $DD"),
];

#[test]
fn rgbds_syntax_renders_brackets_dollar_hex_and_ldh() {
    for &(bytes, pc, text) in RGBDS {
        let got = decode_with(bytes, pc, Syntax::Rgbds);
        assert_eq!(got.text, text, "rgbds text for {bytes:02X?}");
        // len/cycles are dialect-independent — must equal the bgb decode.
        let bgb = decode(bytes, pc);
        assert_eq!(got.len, bgb.len, "len parity for {bytes:02X?}");
        assert_eq!(got.cycles, bgb.cycles, "cycles parity for {bytes:02X?}");
    }
}

#[test]
fn rgbds_negative_offsets_and_every_opcode_decodes() {
    assert_eq!(
        decode_with(&[0xF8, 0xFE], 0, Syntax::Rgbds).text,
        "ld hl,sp-$02"
    );
    assert_eq!(
        decode_with(&[0xE8, 0xFB], 0, Syntax::Rgbds).text,
        "add sp,-$05"
    );
    // Every opcode decodes in rgbds without panic; illegal ones become `db $xx`.
    for op in 0u16..=0xFF {
        let op = op as u8;
        let insn = decode_with(&[op, 0, 0], 0, Syntax::Rgbds);
        assert!((1..=3).contains(&insn.len), "len {op:02X}");
    }
}

#[test]
fn negative_signed_offsets() {
    // 0xFE = -2, 0xFB = -5 — sign + 2-hex magnitude, like bgb.
    assert_eq!(decode(&[0xF8, 0xFE], 0).text, "ld hl,sp-02");
    assert_eq!(decode(&[0xE8, 0xFB], 0).text, "add sp,-05");
    // 0x80 = -128: magnitude 0x80 must not overflow the i8 negation.
    assert_eq!(decode(&[0xF8, 0x80], 0).text, "ld hl,sp-80");
}

#[test]
fn relative_jump_target_uses_pc() {
    // pc + 2 + disp, wrapping. Forward and backward.
    assert_eq!(decode(&[0x18, 0x05], 0x0200).text, "jr 0207");
    assert_eq!(decode(&[0x18, 0xFE], 0x0200).text, "jr 0200"); // -2 -> self
    assert_eq!(decode(&[0x18, 0x00], 0xFFFE).text, "jr 0000"); // wraps
}

#[test]
fn truncated_operands_read_as_zero_but_len_is_real() {
    // Only the opcode present: operands default to 0, len still 3.
    let got = decode(&[0xC3], 0);
    assert_eq!(got.text, "jp 0000");
    assert_eq!(got.len, 3);
}

#[test]
fn every_opcode_decodes_without_panic() {
    // Unprefixed: 1..=3 byte length, illegal => "undefined opcode" + 0 cycles.
    const ILLEGAL: [u8; 11] = [
        0xD3, 0xDB, 0xDD, 0xE3, 0xE4, 0xEB, 0xEC, 0xED, 0xF4, 0xFC, 0xFD,
    ];
    for op in 0u16..=0xFF {
        let op = op as u8;
        if op == 0xCB {
            continue;
        }
        let insn = decode(&[op, 0x00, 0x00], 0);
        assert!((1..=3).contains(&insn.len), "len {op:02X}");
        if ILLEGAL.contains(&op) {
            assert_eq!(insn.text, "undefined opcode", "{op:02X}");
            assert_eq!(insn.cycles, 0);
        } else {
            assert_ne!(insn.text, "undefined opcode", "{op:02X} should decode");
            assert!(insn.cycles > 0, "cycles {op:02X}");
        }
    }
    // Every CB opcode decodes to a 2-byte, non-zero-cycle instruction.
    for cb in 0u16..=0xFF {
        let insn = decode(&[0xCB, cb as u8], 0);
        assert_eq!(insn.len, 2, "CB {cb:02X}");
        assert!(insn.cycles >= 2, "CB cycles {cb:02X}");
        assert_ne!(insn.text, "undefined opcode");
    }
}
