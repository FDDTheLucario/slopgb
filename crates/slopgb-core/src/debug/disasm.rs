//! Game Boy instruction disassembler in bgb's no$gmb syntax or RGBDS syntax.
//!
//! Pure: `decode(bytes, pc)` (bgb) / `decode_with(bytes, pc, syntax)`. The bgb
//! spelling (`add b` vs `add a,10`, `ldi (hl),a`, `ld (ff00+44),a`, `jp hl`,
//! `undefined opcode`, …) matches the real bgb captures in
//! `docs/bgb-reference/README.md` (§"Disassembler format"). RGBDS uses `$`-hex,
//! `[mem]` brackets, `ld [hli],a`/`ld [hld],a`, `ldh [$ff44],a`, and `db $xx` for
//! illegal opcodes. Relative jumps print the absolute target, so `pc` is required.
//!
//! This decoder is **debug-only** — never called on the emulation/golden path —
//! so its output never affects the gbtr fingerprint or mooneye.

/// Which assembler dialect [`decode_with`] renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Syntax {
    /// bgb / no$gmb: bare hex, `(mem)`, `ldi`/`ldd`, `ld (ff00+n),a`.
    #[default]
    Bgb,
    /// RGBDS: `$`-hex, `[mem]`, `ld [hli],a`, `ldh [$ffnn],a`, `db $xx`.
    Rgbds,
}

/// One decoded instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insn {
    /// Disassembled text in the requested [`Syntax`].
    pub text: String,
    /// Encoded length in bytes (1–3).
    pub len: u8,
    /// M-cycles. Conditional branches report the **not-taken** count, as bgb does.
    pub cycles: u8,
}

const R: [&str; 8] = ["b", "c", "d", "e", "h", "l", "(hl)", "a"];
const RP: [&str; 4] = ["bc", "de", "hl", "sp"];
const RP2: [&str; 4] = ["bc", "de", "hl", "af"];
const CC: [&str; 4] = ["nz", "z", "nc", "c"];
const ALU: [&str; 8] = ["add", "adc", "sub", "sbc", "and", "xor", "or", "cp"];
const ROT: [&str; 8] = ["rlc", "rrc", "rl", "rr", "sla", "sra", "swap", "srl"];
const ACC: [&str; 8] = ["rlca", "rrca", "rla", "rra", "daa", "cpl", "scf", "ccf"];

/// One extra M-cycle when an operand slot is `(hl)` (index 6) vs a register.
const fn hl_extra(slot: usize) -> u8 {
    if slot == 6 { 1 } else { 0 }
}

/// Pick the `rgbds` or `bgb` spelling of a fixed mnemonic.
const fn pick(rg: bool, rgbds: &'static str, bgb: &'static str) -> &'static str {
    if rg { rgbds } else { bgb }
}

/// Disassemble the instruction at `bytes[0]` in bgb syntax (see [`decode_with`]).
#[must_use]
pub fn decode(bytes: &[u8], pc: u16) -> Insn {
    decode_with(bytes, pc, Syntax::Bgb)
}

/// Disassemble the instruction at `bytes[0]` (located at address `pc`) in
/// `syntax`. Missing operand bytes (a truncated `bytes`) read as 0; `len` still
/// reflects the real encoded length so callers can advance correctly.
#[must_use]
pub fn decode_with(bytes: &[u8], pc: u16, syntax: Syntax) -> Insn {
    let op = bytes.first().copied().unwrap_or(0);
    let b1 = bytes.get(1).copied().unwrap_or(0);
    let b2 = bytes.get(2).copied().unwrap_or(0);
    let imm16 = u16::from(b1) | (u16::from(b2) << 8);
    let rg = syntax == Syntax::Rgbds;

    if op == 0xCB {
        return decode_cb(b1, rg);
    }

    // Operand formatters: hex prefix and memory brackets differ by dialect.
    let h16 = |v: u16| {
        if rg {
            format!("${v:04X}")
        } else {
            format!("{v:04X}")
        }
    };
    let h8 = |v: u8| {
        if rg {
            format!("${v:02X}")
        } else {
            format!("{v:02X}")
        }
    };
    let mem = |inner: &str| {
        if rg {
            format!("[{inner}]")
        } else {
            format!("({inner})")
        }
    };
    // The `(hl)` register slot becomes `[hl]` in RGBDS.
    let reg = |i: usize| if rg && i == 6 { "[hl]" } else { R[i] };

    let x = op >> 6;
    let y = ((op >> 3) & 7) as usize;
    let z = (op & 7) as usize;
    let p = y >> 1;
    let q = y & 1;

    // Each arm yields (text, len, cycles).
    let (text, len, cycles): (String, u8, u8) = match x {
        0 => match z {
            0 => match y {
                0 => ("nop".into(), 1, 1),
                1 => (format!("ld {},sp", mem(&h16(imm16))), 3, 5),
                2 => ("stop".into(), 2, 1),
                3 => (format!("jr {}", rel_target(pc, b1, rg)), 2, 3),
                _ => (format!("jr {},{}", CC[y - 4], rel_target(pc, b1, rg)), 2, 2),
            },
            1 if q == 0 => (format!("ld {},{}", RP[p], h16(imm16)), 3, 3),
            1 => (format!("add hl,{}", RP[p]), 1, 2),
            2 => {
                let s = match (q, p) {
                    (0, 0) => pick(rg, "ld [bc],a", "ld (bc),a"),
                    (0, 1) => pick(rg, "ld [de],a", "ld (de),a"),
                    (0, 2) => pick(rg, "ld [hli],a", "ldi (hl),a"),
                    (0, 3) => pick(rg, "ld [hld],a", "ldd (hl),a"),
                    (1, 0) => pick(rg, "ld a,[bc]", "ld a,(bc)"),
                    (1, 1) => pick(rg, "ld a,[de]", "ld a,(de)"),
                    (1, 2) => pick(rg, "ld a,[hli]", "ldi a,(hl)"),
                    _ => pick(rg, "ld a,[hld]", "ldd a,(hl)"),
                };
                (s.into(), 1, 2)
            }
            3 if q == 0 => (format!("inc {}", RP[p]), 1, 2),
            3 => (format!("dec {}", RP[p]), 1, 2),
            4 => (format!("inc {}", reg(y)), 1, 1 + hl_extra(y) * 2),
            5 => (format!("dec {}", reg(y)), 1, 1 + hl_extra(y) * 2),
            6 => (format!("ld {},{}", reg(y), h8(b1)), 2, 2 + hl_extra(y)),
            _ => (ACC[y].into(), 1, 1),
        },
        1 if op == 0x76 => ("halt".into(), 1, 1),
        1 => (
            format!("ld {},{}", reg(y), reg(z)),
            1,
            1 + hl_extra(y) + hl_extra(z),
        ),
        2 => (format!("{} {}", ALU[y], reg(z)), 1, 1 + hl_extra(z)),
        _ => match z {
            0 => match y {
                0..=3 => (format!("ret {}", CC[y]), 1, 2),
                4 => (ldh_imm(b1, false, rg), 2, 3),
                5 => (format!("add sp,{}", signed(b1, rg)), 2, 4),
                6 => (ldh_imm(b1, true, rg), 2, 3),
                _ => (format!("ld hl,sp{}", signed(b1, rg)), 2, 3),
            },
            1 if q == 0 => (format!("pop {}", RP2[p]), 1, 3),
            1 => match p {
                0 => ("ret".into(), 1, 4),
                1 => ("reti".into(), 1, 4),
                2 => ("jp hl".into(), 1, 1),
                _ => ("ld sp,hl".into(), 1, 2),
            },
            2 => match y {
                0..=3 => (format!("jp {},{}", CC[y], h16(imm16)), 3, 3),
                4 => (pick(rg, "ldh [c],a", "ld (ff00+c),a").into(), 1, 2),
                5 => (format!("ld {},a", mem(&h16(imm16))), 3, 4),
                6 => (pick(rg, "ldh a,[c]", "ld a,(ff00+c)").into(), 1, 2),
                _ => (format!("ld a,{}", mem(&h16(imm16))), 3, 4),
            },
            3 => match y {
                0 => (format!("jp {}", h16(imm16)), 3, 4),
                6 => ("di".into(), 1, 1),
                7 => ("ei".into(), 1, 1),
                _ => undefined(op, rg),
            },
            4 => match y {
                0..=3 => (format!("call {},{}", CC[y], h16(imm16)), 3, 3),
                _ => undefined(op, rg),
            },
            5 if q == 0 => (format!("push {}", RP2[p]), 1, 4),
            5 if p == 0 => (format!("call {}", h16(imm16)), 3, 6),
            5 => undefined(op, rg),
            6 => (format!("{} a,{}", ALU[y], h8(b1)), 2, 2),
            _ => (format!("rst {}", h8((y as u8) * 8)), 1, 4),
        },
    };
    Insn { text, len, cycles }
}

/// CB-prefixed instruction (always 2 bytes). `(hl)` shift/rmw forms cost 4
/// M-cycles, `bit n,(hl)` costs 3; register forms cost 2.
fn decode_cb(op: u8, rg: bool) -> Insn {
    let x = op >> 6;
    let y = ((op >> 3) & 7) as usize;
    let z = (op & 7) as usize;
    let onhl = z == 6;
    let reg = if rg && onhl { "[hl]" } else { R[z] };
    let (text, cycles) = match x {
        0 => (format!("{} {}", ROT[y], reg), if onhl { 4 } else { 2 }),
        1 => (format!("bit {y},{reg}"), if onhl { 3 } else { 2 }),
        2 => (format!("res {y},{reg}"), if onhl { 4 } else { 2 }),
        _ => (format!("set {y},{reg}"), if onhl { 4 } else { 2 }),
    };
    Insn {
        text,
        len: 2,
        cycles,
    }
}

/// An illegal opcode: bgb renders the literal `undefined opcode`; RGBDS renders
/// it as the raw byte `db $XX` (len 1, 0 cycles).
fn undefined(op: u8, rg: bool) -> (String, u8, u8) {
    let text = if rg {
        format!("db ${op:02X}")
    } else {
        "undefined opcode".into()
    };
    (text, 1, 0)
}

/// High-RAM load: bgb `ld a,(ff00+NN)` / `ld (ff00+NN),a`; RGBDS `ldh a,[$ffNN]`
/// / `ldh [$ffNN],a` (`load` selects the direction).
fn ldh_imm(n: u8, load: bool, rg: bool) -> String {
    match (rg, load) {
        (false, true) => format!("ld a,(ff00+{n:02X})"),
        (false, false) => format!("ld (ff00+{n:02X}),a"),
        (true, true) => format!("ldh a,[$FF{n:02X}]"),
        (true, false) => format!("ldh [$FF{n:02X}],a"),
    }
}

/// Signed 8-bit offset: sign + 2-hex magnitude (`+05`, `-02`), `$`-prefixed in
/// RGBDS (`+$05`, `-$02`). Widen to i16 first so i8::MIN's magnitude (128) doesn't
/// overflow negation.
fn signed(n: u8, rg: bool) -> String {
    let v = i16::from(n as i8);
    let prefix = if rg { "$" } else { "" };
    if v < 0 {
        format!("-{prefix}{:02X}", -v)
    } else {
        format!("+{prefix}{v:02X}")
    }
}

/// Absolute target of a relative jump at `pc` with displacement byte `disp`:
/// `pc + 2 + (disp as i8)`, as a 4-hex string (`$`-prefixed in RGBDS).
fn rel_target(pc: u16, disp: u8, rg: bool) -> String {
    let target = pc.wrapping_add(2).wrapping_add(disp as i8 as i16 as u16);
    if rg {
        format!("${target:04X}")
    } else {
        format!("{target:04X}")
    }
}

#[cfg(test)]
#[path = "disasm_tests.rs"]
mod tests;
