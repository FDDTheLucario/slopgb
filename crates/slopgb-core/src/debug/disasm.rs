//! Game Boy instruction disassembler in bgb's no$gmb syntax.
//!
//! Pure: `decode(bytes, pc) -> Insn`. Mnemonics are lowercase, hex operands
//! uppercase and zero-padded; the exact spelling (`add b` vs `add a,10`,
//! `ldi (hl),a`, `ld (ff00+44),a`, `jp hl`, `undefined opcode`, …) matches the
//! real bgb captures documented in `docs/bgb-reference/README.md`
//! (§"Disassembler format"). Relative jumps print the absolute target, which is
//! why `pc` is required.

/// One decoded instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insn {
    /// Disassembled text, bgb/no$gmb syntax.
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

/// Disassemble the instruction at `bytes[0]` (located at address `pc`). Missing
/// operand bytes (a truncated `bytes`) read as 0; `len` still reflects the real
/// encoded length so callers can advance correctly.
#[must_use]
pub fn decode(bytes: &[u8], pc: u16) -> Insn {
    let op = bytes.first().copied().unwrap_or(0);
    let b1 = bytes.get(1).copied().unwrap_or(0);
    let b2 = bytes.get(2).copied().unwrap_or(0);
    let imm16 = u16::from(b1) | (u16::from(b2) << 8);

    if op == 0xCB {
        return decode_cb(b1);
    }

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
                1 => (format!("ld ({imm16:04X}),sp"), 3, 5),
                2 => ("stop".into(), 2, 1),
                3 => (format!("jr {}", rel_target(pc, b1)), 2, 3),
                _ => (format!("jr {},{}", CC[y - 4], rel_target(pc, b1)), 2, 2),
            },
            1 if q == 0 => (format!("ld {},{imm16:04X}", RP[p]), 3, 3),
            1 => (format!("add hl,{}", RP[p]), 1, 2),
            2 => {
                let s = match (q, p) {
                    (0, 0) => "ld (bc),a",
                    (0, 1) => "ld (de),a",
                    (0, 2) => "ldi (hl),a",
                    (0, 3) => "ldd (hl),a",
                    (1, 0) => "ld a,(bc)",
                    (1, 1) => "ld a,(de)",
                    (1, 2) => "ldi a,(hl)",
                    _ => "ldd a,(hl)",
                };
                (s.into(), 1, 2)
            }
            3 if q == 0 => (format!("inc {}", RP[p]), 1, 2),
            3 => (format!("dec {}", RP[p]), 1, 2),
            4 => (format!("inc {}", R[y]), 1, 1 + hl_extra(y) * 2),
            5 => (format!("dec {}", R[y]), 1, 1 + hl_extra(y) * 2),
            6 => (format!("ld {},{b1:02X}", R[y]), 2, 2 + hl_extra(y)),
            _ => (ACC[y].into(), 1, 1),
        },
        1 if op == 0x76 => ("halt".into(), 1, 1),
        1 => (
            format!("ld {},{}", R[y], R[z]),
            1,
            1 + hl_extra(y) + hl_extra(z),
        ),
        2 => (format!("{} {}", ALU[y], R[z]), 1, 1 + hl_extra(z)),
        _ => match z {
            0 => match y {
                0..=3 => (format!("ret {}", CC[y]), 1, 2),
                4 => (ldh_imm(b1, false), 2, 3),
                5 => (format!("add sp,{}", signed(b1)), 2, 4),
                6 => (ldh_imm(b1, true), 2, 3),
                _ => (format!("ld hl,sp{}", signed(b1)), 2, 3),
            },
            1 if q == 0 => (format!("pop {}", RP2[p]), 1, 3),
            1 => match p {
                0 => ("ret".into(), 1, 4),
                1 => ("reti".into(), 1, 4),
                2 => ("jp hl".into(), 1, 1),
                _ => ("ld sp,hl".into(), 1, 2),
            },
            2 => match y {
                0..=3 => (format!("jp {},{imm16:04X}", CC[y]), 3, 3),
                4 => ("ld (ff00+c),a".into(), 1, 2),
                5 => (format!("ld ({imm16:04X}),a"), 3, 4),
                6 => ("ld a,(ff00+c)".into(), 1, 2),
                _ => (format!("ld a,({imm16:04X})"), 3, 4),
            },
            3 => match y {
                0 => (format!("jp {imm16:04X}"), 3, 4),
                6 => ("di".into(), 1, 1),
                7 => ("ei".into(), 1, 1),
                _ => undefined(),
            },
            4 => match y {
                0..=3 => (format!("call {},{imm16:04X}", CC[y]), 3, 3),
                _ => undefined(),
            },
            5 if q == 0 => (format!("push {}", RP2[p]), 1, 4),
            5 if p == 0 => (format!("call {imm16:04X}"), 3, 6),
            5 => undefined(),
            6 => (format!("{} a,{b1:02X}", ALU[y]), 2, 2),
            _ => (format!("rst {:02X}", (y as u8) * 8), 1, 4),
        },
    };
    Insn { text, len, cycles }
}

/// CB-prefixed instruction (always 2 bytes). `(hl)` shift/rmw forms cost 4
/// M-cycles, `bit n,(hl)` costs 3; register forms cost 2.
fn decode_cb(op: u8) -> Insn {
    let x = op >> 6;
    let y = ((op >> 3) & 7) as usize;
    let z = (op & 7) as usize;
    let onhl = z == 6;
    let (text, cycles) = match x {
        0 => (format!("{} {}", ROT[y], R[z]), if onhl { 4 } else { 2 }),
        1 => (format!("bit {y},{}", R[z]), if onhl { 3 } else { 2 }),
        2 => (format!("res {y},{}", R[z]), if onhl { 4 } else { 2 }),
        _ => (format!("set {y},{}", R[z]), if onhl { 4 } else { 2 }),
    };
    Insn {
        text,
        len: 2,
        cycles,
    }
}

/// bgb renders every illegal opcode as this literal (len 1, 0 cycles).
fn undefined() -> (String, u8, u8) {
    ("undefined opcode".into(), 1, 0)
}

/// `ld a,(ff00+NN)` (load=true) or `ld (ff00+NN),a` (load=false).
fn ldh_imm(n: u8, load: bool) -> String {
    if load {
        format!("ld a,(ff00+{n:02X})")
    } else {
        format!("ld (ff00+{n:02X}),a")
    }
}

/// Signed 8-bit offset as bgb prints it: sign + 2-hex magnitude (`+05`, `-02`).
/// Widen to i16 first so i8::MIN's magnitude (128) doesn't overflow negation.
fn signed(n: u8) -> String {
    let v = i16::from(n as i8);
    if v < 0 {
        format!("-{:02X}", -v)
    } else {
        format!("+{v:02X}")
    }
}

/// Absolute target of a relative jump at `pc` with displacement byte `disp`:
/// `pc + 2 + (disp as i8)`, as a 4-hex string.
fn rel_target(pc: u16, disp: u8) -> String {
    let target = pc.wrapping_add(2).wrapping_add(disp as i8 as i16 as u16);
    format!("{target:04X}")
}

#[cfg(test)]
#[path = "disasm_tests.rs"]
mod tests;
