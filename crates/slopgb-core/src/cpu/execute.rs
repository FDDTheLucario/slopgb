//! Instruction decode and execution. CPU work package.
//!
//! Cycle model: every [`Bus::read`]/[`Bus::write`] and every internal
//! [`Bus::tick`] is exactly one M-cycle. Cycle counts and the placement of
//! internal cycles follow the per-instruction tables in *Game Boy: Complete
//! Technical Reference* (gbctr).

use super::{Bus, Cpu, Flags};

/// Execute one instruction (preceded by interrupt dispatch if one is
/// pending and IME is set), or one M-cycle of halt or stop mode.
pub fn step(cpu: &mut Cpu, bus: &mut impl Bus) {
    if cpu.locked {
        // An illegal opcode hard-locks the CPU; it only burns cycles.
        bus.tick();
        return;
    }
    if cpu.halted {
        // Halt mode ends when IE & IF != 0 regardless of IME. Waking adds no
        // extra delay: mooneye acceptance/halt_ime0_nointr_timing states the
        // timing is "exactly the same as if a long series of NOP instructions
        // were used to wait for the interrupt".
        if bus.pending() == 0 {
            bus.tick();
            return;
        }
        cpu.halted = false;
    }
    if cpu.stopped {
        // Stop mode ends on joypad wake (Pan Docs, "Using the STOP
        // Instruction"). The raw P1 input lines are not visible through
        // `Bus`, so wake is modelled as IE & IF != 0, like halt; in stop
        // mode every other interrupt source is frozen, so a newly pending
        // bit can only be the joypad. Not modelled: hardware wakes on the
        // P1 lines even with IE bit 4 clear.
        if bus.pending() == 0 {
            bus.tick();
            return;
        }
        cpu.stopped = false;
    }
    if cpu.ime && bus.pending() != 0 {
        dispatch_interrupt(cpu, bus);
    }
    // EI enables IME only after the instruction *following* EI completes
    // (gbctr; mooneye acceptance/ei_sequence, ei_timing, rapid_di_ei).
    let ei_delay = cpu.ime_pending;
    let opcode = fetch_opcode(cpu, bus);
    execute(cpu, bus, opcode);
    if ei_delay && cpu.ime_pending {
        cpu.ime_pending = false;
        cpu.ime = true;
    }
}

/// Interrupt dispatch. 5 M-cycles total; the 5th is the opcode fetch at the
/// target address, performed by the caller as the start of the next
/// instruction. IME is cleared immediately; a not-yet-committed EI enable is
/// swallowed by the dispatch.
fn dispatch_interrupt(cpu: &mut Cpu, bus: &mut impl Bus) {
    cpu.ime = false;
    cpu.ime_pending = false;
    bus.tick();
    bus.tick();
    let pc = cpu.regs.pc;
    cpu.regs.sp = cpu.regs.sp.wrapping_sub(1);
    bus.write(cpu.regs.sp, (pc >> 8) as u8);
    // IE & IF are re-evaluated *after* the high push: the push itself may
    // have overwritten IE (SP near 0x0000) and cancelled or redirected the
    // dispatch (mooneye acceptance/interrupts/ie_push). The chosen IF bit is
    // acknowledged here, before the low push; on cancellation nothing is
    // acknowledged and the CPU ends up at 0x0000 with IME left disabled.
    let pending = bus.pending();
    let target = if pending == 0 {
        0x0000
    } else {
        let bit = pending.trailing_zeros() as u8;
        bus.ack(bit);
        0x0040 + (u16::from(bit) << 3)
    };
    cpu.regs.sp = cpu.regs.sp.wrapping_sub(1);
    bus.write(cpu.regs.sp, pc as u8);
    cpu.regs.pc = target;
}

/// Opcode fetch. The halt bug (HALT with IME=0 while IE & IF != 0) makes
/// exactly the next opcode fetch skip the PC increment (gbctr).
fn fetch_opcode(cpu: &mut Cpu, bus: &mut impl Bus) -> u8 {
    let opcode = bus.read(cpu.regs.pc);
    if cpu.halt_bug {
        cpu.halt_bug = false;
    } else {
        cpu.regs.pc = cpu.regs.pc.wrapping_add(1);
    }
    opcode
}

fn imm8(cpu: &mut Cpu, bus: &mut impl Bus) -> u8 {
    let v = bus.read(cpu.regs.pc);
    cpu.regs.pc = cpu.regs.pc.wrapping_add(1);
    v
}

fn imm16(cpu: &mut Cpu, bus: &mut impl Bus) -> u16 {
    let lo = imm8(cpu, bus);
    let hi = imm8(cpu, bus);
    u16::from_le_bytes([lo, hi])
}

fn flag(cpu: &Cpu, mask: u8) -> bool {
    cpu.regs.f & mask != 0
}

fn set_flags(cpu: &mut Cpu, z: bool, n: bool, h: bool, c: bool) {
    cpu.regs.f = (u8::from(z) << 7) | (u8::from(n) << 6) | (u8::from(h) << 5) | (u8::from(c) << 4);
}

/// `cc` condition codes: 0=NZ, 1=Z, 2=NC, 3=C.
fn condition(cpu: &Cpu, idx: u8) -> bool {
    match idx {
        0 => !flag(cpu, Flags::Z),
        1 => flag(cpu, Flags::Z),
        2 => !flag(cpu, Flags::C),
        3 => flag(cpu, Flags::C),
        _ => unreachable!(),
    }
}

/// 8-bit operand table: 0=B 1=C 2=D 3=E 4=H 5=L 6=(HL) 7=A. Index 6 costs
/// one memory M-cycle.
fn r8_get(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8) -> u8 {
    match idx {
        0 => cpu.regs.b,
        1 => cpu.regs.c,
        2 => cpu.regs.d,
        3 => cpu.regs.e,
        4 => cpu.regs.h,
        5 => cpu.regs.l,
        6 => bus.read(cpu.regs.hl()),
        7 => cpu.regs.a,
        _ => unreachable!(),
    }
}

fn r8_set(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8, v: u8) {
    match idx {
        0 => cpu.regs.b = v,
        1 => cpu.regs.c = v,
        2 => cpu.regs.d = v,
        3 => cpu.regs.e = v,
        4 => cpu.regs.h = v,
        5 => cpu.regs.l = v,
        6 => bus.write(cpu.regs.hl(), v),
        7 => cpu.regs.a = v,
        _ => unreachable!(),
    }
}

/// 16-bit register pair table used by most 16-bit ops: 0=BC 1=DE 2=HL 3=SP.
fn rp_get(cpu: &Cpu, idx: u8) -> u16 {
    match idx {
        0 => cpu.regs.bc(),
        1 => cpu.regs.de(),
        2 => cpu.regs.hl(),
        3 => cpu.regs.sp,
        _ => unreachable!(),
    }
}

fn rp_set(cpu: &mut Cpu, idx: u8, v: u16) {
    match idx {
        0 => cpu.regs.set_bc(v),
        1 => cpu.regs.set_de(v),
        2 => cpu.regs.set_hl(v),
        3 => cpu.regs.sp = v,
        _ => unreachable!(),
    }
}

/// PUSH/POP register pair table: 0=BC 1=DE 2=HL 3=AF.
fn rp2_get(cpu: &Cpu, idx: u8) -> u16 {
    match idx {
        0 => cpu.regs.bc(),
        1 => cpu.regs.de(),
        2 => cpu.regs.hl(),
        3 => cpu.regs.af(),
        _ => unreachable!(),
    }
}

fn rp2_set(cpu: &mut Cpu, idx: u8, v: u16) {
    match idx {
        0 => cpu.regs.set_bc(v),
        1 => cpu.regs.set_de(v),
        2 => cpu.regs.set_hl(v),
        3 => cpu.regs.set_af(v), // F low nibble forced to zero
        _ => unreachable!(),
    }
}

/// Two write M-cycles: high byte first, then low (gbctr).
fn push16(cpu: &mut Cpu, bus: &mut impl Bus, v: u16) {
    cpu.regs.sp = cpu.regs.sp.wrapping_sub(1);
    bus.write(cpu.regs.sp, (v >> 8) as u8);
    cpu.regs.sp = cpu.regs.sp.wrapping_sub(1);
    bus.write(cpu.regs.sp, v as u8);
}

/// Two read M-cycles: low byte first, then high (gbctr).
fn pop16(cpu: &mut Cpu, bus: &mut impl Bus) -> u16 {
    let lo = bus.read(cpu.regs.sp);
    cpu.regs.sp = cpu.regs.sp.wrapping_add(1);
    let hi = bus.read(cpu.regs.sp);
    cpu.regs.sp = cpu.regs.sp.wrapping_add(1);
    u16::from_le_bytes([lo, hi])
}

fn alu_add(cpu: &mut Cpu, v: u8, carry_in: bool) {
    let a = cpu.regs.a;
    let c = u8::from(carry_in);
    let r = a.wrapping_add(v).wrapping_add(c);
    let h = (a & 0x0F) + (v & 0x0F) + c > 0x0F;
    let cy = u16::from(a) + u16::from(v) + u16::from(c) > 0xFF;
    set_flags(cpu, r == 0, false, h, cy);
    cpu.regs.a = r;
}

fn alu_sub(cpu: &mut Cpu, v: u8, carry_in: bool, store: bool) {
    let a = cpu.regs.a;
    let c = u8::from(carry_in);
    let r = a.wrapping_sub(v).wrapping_sub(c);
    let h = (a & 0x0F) < (v & 0x0F) + c;
    let cy = u16::from(a) < u16::from(v) + u16::from(c);
    set_flags(cpu, r == 0, true, h, cy);
    if store {
        cpu.regs.a = r;
    }
}

/// ALU operation table: 0=ADD 1=ADC 2=SUB 3=SBC 4=AND 5=XOR 6=OR 7=CP.
fn alu(cpu: &mut Cpu, kind: u8, v: u8) {
    match kind {
        0 => alu_add(cpu, v, false),
        1 => {
            let c = flag(cpu, Flags::C);
            alu_add(cpu, v, c);
        }
        2 => alu_sub(cpu, v, false, true),
        3 => {
            let c = flag(cpu, Flags::C);
            alu_sub(cpu, v, c, true);
        }
        4 => {
            cpu.regs.a &= v;
            let z = cpu.regs.a == 0;
            set_flags(cpu, z, false, true, false);
        }
        5 => {
            cpu.regs.a ^= v;
            let z = cpu.regs.a == 0;
            set_flags(cpu, z, false, false, false);
        }
        6 => {
            cpu.regs.a |= v;
            let z = cpu.regs.a == 0;
            set_flags(cpu, z, false, false, false);
        }
        7 => alu_sub(cpu, v, false, false),
        _ => unreachable!(),
    }
}

fn op_inc_r8(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8) {
    let v = r8_get(cpu, bus, idx);
    let r = v.wrapping_add(1);
    let c = flag(cpu, Flags::C);
    set_flags(cpu, r == 0, false, v & 0x0F == 0x0F, c);
    r8_set(cpu, bus, idx, r);
}

fn op_dec_r8(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8) {
    let v = r8_get(cpu, bus, idx);
    let r = v.wrapping_sub(1);
    let c = flag(cpu, Flags::C);
    set_flags(cpu, r == 0, true, v & 0x0F == 0, c);
    r8_set(cpu, bus, idx, r);
}

/// ADD HL,rp: Z preserved, H from bit 11, C from bit 15. One internal cycle.
fn op_add_hl(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8) {
    let hl = cpu.regs.hl();
    let v = rp_get(cpu, idx);
    let (r, carry) = hl.overflowing_add(v);
    let h = (hl & 0x0FFF) + (v & 0x0FFF) > 0x0FFF;
    let z = flag(cpu, Flags::Z);
    set_flags(cpu, z, false, h, carry);
    cpu.regs.set_hl(r);
    bus.tick();
}

/// Shared by ADD SP,e and LD HL,SP+e: reads the offset byte and computes
/// SP+e. Z=N=0; H and C come from the *unsigned* low-byte addition
/// (gbctr; this is what makes negative offsets produce "carry" flags).
fn sp_plus_e(cpu: &mut Cpu, bus: &mut impl Bus) -> u16 {
    let e = imm8(cpu, bus);
    let sp = cpu.regs.sp;
    let h = (sp & 0x000F) + u16::from(e & 0x0F) > 0x000F;
    let c = (sp & 0x00FF) + u16::from(e) > 0x00FF;
    set_flags(cpu, false, false, h, c);
    sp.wrapping_add(e as i8 as u16)
}

/// Decimal adjust, exactly as the SM83 does it (gbctr): in add mode (N=0)
/// the 0x60 correction also sets carry; in subtract mode carry is never
/// cleared and corrections depend only on the incoming H/C flags.
fn op_daa(cpu: &mut Cpu) {
    let n = flag(cpu, Flags::N);
    let h = flag(cpu, Flags::H);
    let mut c = flag(cpu, Flags::C);
    let mut a = cpu.regs.a;
    if n {
        if c {
            a = a.wrapping_sub(0x60);
        }
        if h {
            a = a.wrapping_sub(0x06);
        }
    } else {
        if c || a > 0x99 {
            a = a.wrapping_add(0x60);
            c = true;
        }
        // The 0x60 correction does not change the low nibble, so checking
        // the adjusted value here is equivalent to checking the original.
        if h || a & 0x0F > 0x09 {
            a = a.wrapping_add(0x06);
        }
    }
    set_flags(cpu, a == 0, n, false, c);
    cpu.regs.a = a;
}

fn op_halt(cpu: &mut Cpu, bus: &mut impl Bus) {
    // Halt bug: HALT with IME=0 while IE & IF != 0 does not halt; instead
    // the next opcode fetch fails to increment PC (gbctr). An EI directly
    // before HALT behaves like the IME=1 case instead, because the delayed
    // enable commits while halting (mooneye acceptance/halt_ime0_ei).
    if !cpu.ime && !cpu.ime_pending && bus.pending() != 0 {
        cpu.halt_bug = true;
    } else {
        cpu.halted = true;
    }
}

fn op_stop(cpu: &mut Cpu, bus: &mut impl Bus) {
    // Per the STOP flowchart in Pan Docs ("Using the STOP Instruction"):
    // with no interrupt pending STOP is a 2-byte opcode and the byte after
    // it is skipped; with IE & IF != 0 it stays a 1-byte opcode. Branches
    // not modelled (the joypad input state is not visible through `Bus`):
    // a held button turns STOP into a 1-byte HALT (or a plain 1-byte NOP if
    // an interrupt is also pending), and a pending interrupt with IME=1
    // while a speed switch is armed glitches the CPU non-deterministically.
    if bus.pending() == 0 {
        cpu.regs.pc = cpu.regs.pc.wrapping_add(1);
    }
    // true: the bus performed an armed CGB speed switch and execution
    // continues. false: deep stop; the CPU sleeps like in halt mode until
    // the joypad wakes it (see `step`).
    if !bus.stop() {
        cpu.stopped = true;
    }
}

fn execute(cpu: &mut Cpu, bus: &mut impl Bus, op: u8) {
    match op {
        // --- 0x00..=0x3F ---
        0x00 => {}
        0x10 => op_stop(cpu, bus),
        // LD rp,nn
        0x01 | 0x11 | 0x21 | 0x31 => {
            let v = imm16(cpu, bus);
            rp_set(cpu, (op >> 4) & 3, v);
        }
        // LD (BC)/(DE)/(HL+)/(HL-),A
        0x02 => bus.write(cpu.regs.bc(), cpu.regs.a),
        0x12 => bus.write(cpu.regs.de(), cpu.regs.a),
        0x22 => {
            let hl = cpu.regs.hl();
            bus.write(hl, cpu.regs.a);
            cpu.regs.set_hl(hl.wrapping_add(1));
        }
        0x32 => {
            let hl = cpu.regs.hl();
            bus.write(hl, cpu.regs.a);
            cpu.regs.set_hl(hl.wrapping_sub(1));
        }
        // LD A,(BC)/(DE)/(HL+)/(HL-)
        0x0A => cpu.regs.a = bus.read(cpu.regs.bc()),
        0x1A => cpu.regs.a = bus.read(cpu.regs.de()),
        0x2A => {
            let hl = cpu.regs.hl();
            cpu.regs.a = bus.read(hl);
            cpu.regs.set_hl(hl.wrapping_add(1));
        }
        0x3A => {
            let hl = cpu.regs.hl();
            cpu.regs.a = bus.read(hl);
            cpu.regs.set_hl(hl.wrapping_sub(1));
        }
        // INC/DEC rp: one internal cycle, no flags
        0x03 | 0x13 | 0x23 | 0x33 => {
            let i = (op >> 4) & 3;
            let v = rp_get(cpu, i).wrapping_add(1);
            rp_set(cpu, i, v);
            bus.tick();
        }
        0x0B | 0x1B | 0x2B | 0x3B => {
            let i = (op >> 4) & 3;
            let v = rp_get(cpu, i).wrapping_sub(1);
            rp_set(cpu, i, v);
            bus.tick();
        }
        // INC/DEC r8 (incl. (HL): read + write cycles)
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
            op_inc_r8(cpu, bus, (op >> 3) & 7);
        }
        0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
            op_dec_r8(cpu, bus, (op >> 3) & 7);
        }
        // LD r,n
        0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
            let v = imm8(cpu, bus);
            r8_set(cpu, bus, (op >> 3) & 7, v);
        }
        // Accumulator rotates: Z always 0
        0x07 => {
            let c = cpu.regs.a & 0x80 != 0;
            cpu.regs.a = cpu.regs.a.rotate_left(1);
            set_flags(cpu, false, false, false, c);
        }
        0x0F => {
            let c = cpu.regs.a & 0x01 != 0;
            cpu.regs.a = cpu.regs.a.rotate_right(1);
            set_flags(cpu, false, false, false, c);
        }
        0x17 => {
            let c_in = u8::from(flag(cpu, Flags::C));
            let c = cpu.regs.a & 0x80 != 0;
            cpu.regs.a = cpu.regs.a << 1 | c_in;
            set_flags(cpu, false, false, false, c);
        }
        0x1F => {
            let c_in = u8::from(flag(cpu, Flags::C));
            let c = cpu.regs.a & 0x01 != 0;
            cpu.regs.a = cpu.regs.a >> 1 | c_in << 7;
            set_flags(cpu, false, false, false, c);
        }
        // LD (nn),SP: low byte at nn, high at nn+1
        0x08 => {
            let addr = imm16(cpu, bus);
            bus.write(addr, cpu.regs.sp as u8);
            bus.write(addr.wrapping_add(1), (cpu.regs.sp >> 8) as u8);
        }
        0x09 | 0x19 | 0x29 | 0x39 => op_add_hl(cpu, bus, (op >> 4) & 3),
        // JR e / JR cc,e: internal cycle only when taken
        0x18 => {
            let e = imm8(cpu, bus);
            bus.tick();
            cpu.regs.pc = cpu.regs.pc.wrapping_add(e as i8 as u16);
        }
        0x20 | 0x28 | 0x30 | 0x38 => {
            let e = imm8(cpu, bus);
            if condition(cpu, (op >> 3) & 3) {
                bus.tick();
                cpu.regs.pc = cpu.regs.pc.wrapping_add(e as i8 as u16);
            }
        }
        0x27 => op_daa(cpu),
        0x2F => {
            cpu.regs.a = !cpu.regs.a;
            let z = flag(cpu, Flags::Z);
            let c = flag(cpu, Flags::C);
            set_flags(cpu, z, true, true, c);
        }
        0x37 => {
            let z = flag(cpu, Flags::Z);
            set_flags(cpu, z, false, false, true);
        }
        0x3F => {
            let z = flag(cpu, Flags::Z);
            let c = flag(cpu, Flags::C);
            set_flags(cpu, z, false, false, !c);
        }

        // --- 0x40..=0xBF ---
        0x76 => op_halt(cpu, bus),
        // LD r,r' (LD B,B doubles as the mooneye debug breakpoint)
        0x40..=0x75 | 0x77..=0x7F => {
            if op == 0x40 {
                cpu.debug_breakpoint = true;
            }
            let v = r8_get(cpu, bus, op & 7);
            r8_set(cpu, bus, (op >> 3) & 7, v);
        }
        0x80..=0xBF => {
            let v = r8_get(cpu, bus, op & 7);
            alu(cpu, (op >> 3) & 7, v);
        }

        // --- 0xC0..=0xFF ---
        // RET cc: internal cycle for the condition, then pop + internal
        0xC0 | 0xC8 | 0xD0 | 0xD8 => {
            bus.tick();
            if condition(cpu, (op >> 3) & 3) {
                cpu.regs.pc = pop16(cpu, bus);
                bus.tick();
            }
        }
        0xC9 => {
            cpu.regs.pc = pop16(cpu, bus);
            bus.tick();
        }
        // RETI: like RET but IME is set immediately (no EI-style delay)
        0xD9 => {
            cpu.regs.pc = pop16(cpu, bus);
            bus.tick();
            cpu.ime = true;
        }
        0xC1 | 0xD1 | 0xE1 | 0xF1 => {
            let v = pop16(cpu, bus);
            rp2_set(cpu, (op >> 4) & 3, v);
        }
        // PUSH: internal cycle *before* the writes (POP has none)
        0xC5 | 0xD5 | 0xE5 | 0xF5 => {
            bus.tick();
            let v = rp2_get(cpu, (op >> 4) & 3);
            push16(cpu, bus, v);
        }
        // JP nn / JP cc,nn: internal cycle only when taken
        0xC3 => {
            let nn = imm16(cpu, bus);
            bus.tick();
            cpu.regs.pc = nn;
        }
        0xC2 | 0xCA | 0xD2 | 0xDA => {
            let nn = imm16(cpu, bus);
            if condition(cpu, (op >> 3) & 3) {
                bus.tick();
                cpu.regs.pc = nn;
            }
        }
        0xE9 => cpu.regs.pc = cpu.regs.hl(),
        // CALL nn: fetch, lo, hi, internal, push hi, push lo
        0xCD => {
            let nn = imm16(cpu, bus);
            bus.tick();
            let pc = cpu.regs.pc;
            push16(cpu, bus, pc);
            cpu.regs.pc = nn;
        }
        0xC4 | 0xCC | 0xD4 | 0xDC => {
            let nn = imm16(cpu, bus);
            if condition(cpu, (op >> 3) & 3) {
                bus.tick();
                let pc = cpu.regs.pc;
                push16(cpu, bus, pc);
                cpu.regs.pc = nn;
            }
        }
        // RST: same tail as CALL (internal, push hi, push lo)
        0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
            bus.tick();
            let pc = cpu.regs.pc;
            push16(cpu, bus, pc);
            cpu.regs.pc = u16::from(op & 0x38);
        }
        // ALU A,n
        0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => {
            let v = imm8(cpu, bus);
            alu(cpu, (op >> 3) & 7, v);
        }
        0xE0 => {
            let n = imm8(cpu, bus);
            bus.write(0xFF00 | u16::from(n), cpu.regs.a);
        }
        0xF0 => {
            let n = imm8(cpu, bus);
            cpu.regs.a = bus.read(0xFF00 | u16::from(n));
        }
        0xE2 => bus.write(0xFF00 | u16::from(cpu.regs.c), cpu.regs.a),
        0xF2 => cpu.regs.a = bus.read(0xFF00 | u16::from(cpu.regs.c)),
        0xEA => {
            let addr = imm16(cpu, bus);
            bus.write(addr, cpu.regs.a);
        }
        0xFA => {
            let addr = imm16(cpu, bus);
            cpu.regs.a = bus.read(addr);
        }
        // ADD SP,e: two internal cycles after the offset read
        0xE8 => {
            let r = sp_plus_e(cpu, bus);
            bus.tick();
            bus.tick();
            cpu.regs.sp = r;
        }
        // LD HL,SP+e: one internal cycle after the offset read
        0xF8 => {
            let r = sp_plus_e(cpu, bus);
            bus.tick();
            cpu.regs.set_hl(r);
        }
        0xF9 => {
            cpu.regs.sp = cpu.regs.hl();
            bus.tick();
        }
        // DI takes effect immediately and cancels a pending EI enable
        0xF3 => {
            cpu.ime = false;
            cpu.ime_pending = false;
        }
        0xFB => cpu.ime_pending = true,
        0xCB => {
            let cb = imm8(cpu, bus);
            execute_cb(cpu, bus, cb);
        }
        // Illegal opcodes hard-lock the CPU permanently (gbctr).
        0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD => {
            cpu.locked = true;
        }
    }
}

/// CB-prefixed rotate/shift table: 0=RLC 1=RRC 2=RL 3=RR 4=SLA 5=SRA 6=SWAP
/// 7=SRL. All set Z from the result (unlike the A-register rotates).
fn cb_rot(cpu: &mut Cpu, kind: u8, v: u8) -> u8 {
    let c_in = u8::from(flag(cpu, Flags::C));
    let (r, c) = match kind {
        0 => (v.rotate_left(1), v & 0x80 != 0),
        1 => (v.rotate_right(1), v & 0x01 != 0),
        2 => (v << 1 | c_in, v & 0x80 != 0),
        3 => (v >> 1 | c_in << 7, v & 0x01 != 0),
        4 => (v << 1, v & 0x80 != 0),
        5 => (v >> 1 | (v & 0x80), v & 0x01 != 0),
        6 => (v.rotate_left(4), false),
        7 => (v >> 1, v & 0x01 != 0),
        _ => unreachable!(),
    };
    set_flags(cpu, r == 0, false, false, c);
    r
}

fn execute_cb(cpu: &mut Cpu, bus: &mut impl Bus, op: u8) {
    let idx = op & 7;
    // Decode field y (bits 5..3): the rotate/shift kind in the 0x00..=0x3F
    // range, the bit index for BIT/RES/SET.
    let y = (op >> 3) & 7;
    match op >> 6 {
        // Rotates/shifts: (HL) is read-modify-write
        0 => {
            let v = r8_get(cpu, bus, idx);
            let r = cb_rot(cpu, y, v);
            r8_set(cpu, bus, idx, r);
        }
        // BIT: read only; C preserved, H set
        1 => {
            let v = r8_get(cpu, bus, idx);
            let c = flag(cpu, Flags::C);
            set_flags(cpu, v & 1 << y == 0, false, true, c);
        }
        2 => {
            let v = r8_get(cpu, bus, idx);
            r8_set(cpu, bus, idx, v & !(1 << y));
        }
        3 => {
            let v = r8_get(cpu, bus, idx);
            r8_set(cpu, bus, idx, v | 1 << y);
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Bus, Cpu, Flags, Registers};
    use super::step;
    use Ev::{Read, Tick, Write};

    /// One bus event == one M-cycle. The index in [`TestBus::log`] is the
    /// cycle index, so comparing whole logs asserts both the kind and the
    /// exact cycle position of every access.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Ev {
        Read(u16, u8),
        Write(u16, u8),
        Tick,
    }

    /// 64 KiB flat RAM. IF lives at 0xFF0F and IE at 0xFFFF inside `mem`, so
    /// stack pushes that land on IE behave exactly like on hardware.
    struct TestBus {
        mem: Vec<u8>,
        log: Vec<Ev>,
        stop_result: bool,
        stop_calls: u32,
    }

    impl TestBus {
        fn new() -> Self {
            Self {
                mem: vec![0; 0x10000],
                log: Vec::new(),
                stop_result: false,
                stop_calls: 0,
            }
        }

        fn load(&mut self, addr: u16, bytes: &[u8]) {
            for (i, &b) in bytes.iter().enumerate() {
                self.mem[usize::from(addr) + i] = b;
            }
        }

        fn take_log(&mut self) -> Vec<Ev> {
            std::mem::take(&mut self.log)
        }
    }

    impl Bus for TestBus {
        fn read(&mut self, addr: u16) -> u8 {
            let v = self.mem[usize::from(addr)];
            self.log.push(Read(addr, v));
            v
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.log.push(Write(addr, value));
            self.mem[usize::from(addr)] = value;
        }

        fn tick(&mut self) {
            self.log.push(Tick);
        }

        fn pending(&self) -> u8 {
            self.mem[0xFF0F] & self.mem[0xFFFF] & 0x1F
        }

        fn ack(&mut self, bit: u8) {
            self.mem[0xFF0F] &= !(1 << bit);
        }

        fn stop(&mut self) -> bool {
            self.stop_calls += 1;
            self.stop_result
        }
    }

    const PC0: u16 = 0xC000;
    const SP0: u16 = 0xD000;

    /// Build an F-register value from individual flags.
    fn fl(z: bool, n: bool, h: bool, c: bool) -> u8 {
        (u8::from(z) << 7) | (u8::from(n) << 6) | (u8::from(h) << 5) | (u8::from(c) << 4)
    }

    fn cpu() -> Cpu {
        Cpu {
            regs: Registers {
                pc: PC0,
                sp: SP0,
                ..Registers::default()
            },
            ime: false,
            ime_pending: false,
            halted: false,
            stopped: false,
            halt_bug: false,
            debug_breakpoint: false,
            locked: false,
        }
    }

    fn bus(program: &[u8]) -> TestBus {
        let mut b = TestBus::new();
        b.load(PC0, program);
        b
    }

    // ----- loads -----

    #[test]
    fn nop_is_one_fetch_cycle() {
        let mut c = cpu();
        let mut b = bus(&[0x00]);
        step(&mut c, &mut b);
        assert_eq!(b.log, [Read(PC0, 0x00)]);
        assert_eq!(c.regs.pc, PC0 + 1);
        assert_eq!(c.regs.f, 0);
    }

    #[test]
    fn ld_r_r_moves_value_in_one_cycle() {
        let mut c = cpu();
        c.regs.c = 0x42;
        let mut b = bus(&[0x41]); // LD B,C
        step(&mut c, &mut b);
        assert_eq!(b.log, [Read(PC0, 0x41)]);
        assert_eq!(c.regs.b, 0x42);
    }

    #[test]
    fn ld_b_b_sets_debug_breakpoint_and_still_loads() {
        let mut c = cpu();
        c.regs.b = 7;
        let mut b = bus(&[0x40]);
        step(&mut c, &mut b);
        assert!(c.debug_breakpoint);
        assert!(c.debug_breakpoint_hit());
        assert_eq!(c.regs.b, 7);
        assert_eq!(b.log, [Read(PC0, 0x40)]);
    }

    #[test]
    fn ld_r_hl_and_ld_hl_r_traces() {
        let mut c = cpu();
        c.regs.set_hl(0xC800);
        let mut b = bus(&[0x56, 0x73]); // LD D,(HL); LD (HL),E
        b.mem[0xC800] = 0x99;
        c.regs.e = 0x5A;
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0, 0x56), Read(0xC800, 0x99)]);
        assert_eq!(c.regs.d, 0x99);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0 + 1, 0x73), Write(0xC800, 0x5A)]);
        assert_eq!(b.mem[0xC800], 0x5A);
    }

    #[test]
    fn ld_r_imm_and_ld_hl_imm() {
        let mut c = cpu();
        let mut b = bus(&[0x3E, 0xAB, 0x36, 0x77]); // LD A,n; LD (HL),n
        c.regs.set_hl(0xC800);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0, 0x3E), Read(PC0 + 1, 0xAB)]);
        assert_eq!(c.regs.a, 0xAB);
        step(&mut c, &mut b);
        assert_eq!(
            b.take_log(),
            [
                Read(PC0 + 2, 0x36),
                Read(PC0 + 3, 0x77),
                Write(0xC800, 0x77)
            ]
        );
    }

    #[test]
    fn ld_rp_imm_all_pairs() {
        for (op, check) in [(0x01u8, 0usize), (0x11, 1), (0x21, 2), (0x31, 3)] {
            let mut c = cpu();
            let mut b = bus(&[op, 0xCD, 0xAB]);
            step(&mut c, &mut b);
            assert_eq!(
                b.log,
                [Read(PC0, op), Read(PC0 + 1, 0xCD), Read(PC0 + 2, 0xAB)]
            );
            let got = match check {
                0 => c.regs.bc(),
                1 => c.regs.de(),
                2 => c.regs.hl(),
                _ => c.regs.sp,
            };
            assert_eq!(got, 0xABCD);
        }
    }

    #[test]
    fn ld_a_indirect_loads_and_stores() {
        // stores
        let mut c = cpu();
        c.regs.a = 0x5C;
        c.regs.set_bc(0xC700);
        c.regs.set_de(0xC701);
        c.regs.set_hl(0xC702);
        let mut b = bus(&[0x02, 0x12, 0x22, 0x32]);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0, 0x02), Write(0xC700, 0x5C)]);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0 + 1, 0x12), Write(0xC701, 0x5C)]);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0 + 2, 0x22), Write(0xC702, 0x5C)]);
        assert_eq!(c.regs.hl(), 0xC703); // HL+
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0 + 3, 0x32), Write(0xC703, 0x5C)]);
        assert_eq!(c.regs.hl(), 0xC702); // HL-

        // loads
        let mut c = cpu();
        c.regs.set_bc(0xC700);
        c.regs.set_de(0xC701);
        c.regs.set_hl(0xC702);
        let mut b = bus(&[0x0A, 0x1A, 0x2A, 0x3A]);
        b.load(0xC700, &[0x11, 0x22, 0x33]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x11);
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x22);
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x33);
        assert_eq!(c.regs.hl(), 0xC703);
        b.take_log();
        step(&mut c, &mut b);
        assert_eq!(b.log, [Read(PC0 + 3, 0x3A), Read(0xC703, 0x00)]);
        assert_eq!(c.regs.hl(), 0xC702);
    }

    #[test]
    fn ld_nn_a_and_ld_a_nn() {
        let mut c = cpu();
        c.regs.a = 0x77;
        let mut b = bus(&[0xEA, 0x34, 0xC9, 0xFA, 0x34, 0xC9]);
        step(&mut c, &mut b);
        assert_eq!(
            b.take_log(),
            [
                Read(PC0, 0xEA),
                Read(PC0 + 1, 0x34),
                Read(PC0 + 2, 0xC9),
                Write(0xC934, 0x77)
            ]
        );
        c.regs.a = 0;
        step(&mut c, &mut b);
        assert_eq!(
            b.take_log(),
            [
                Read(PC0 + 3, 0xFA),
                Read(PC0 + 4, 0x34),
                Read(PC0 + 5, 0xC9),
                Read(0xC934, 0x77)
            ]
        );
        assert_eq!(c.regs.a, 0x77);
    }

    #[test]
    fn ldh_imm_and_c_variants() {
        let mut c = cpu();
        c.regs.a = 0x42;
        c.regs.c = 0x81;
        let mut b = bus(&[0xE0, 0x80, 0xF0, 0x80, 0xE2, 0xF2]);
        step(&mut c, &mut b);
        assert_eq!(
            b.take_log(),
            [Read(PC0, 0xE0), Read(PC0 + 1, 0x80), Write(0xFF80, 0x42)]
        );
        c.regs.a = 0;
        step(&mut c, &mut b);
        assert_eq!(
            b.take_log(),
            [Read(PC0 + 2, 0xF0), Read(PC0 + 3, 0x80), Read(0xFF80, 0x42)]
        );
        assert_eq!(c.regs.a, 0x42);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0 + 4, 0xE2), Write(0xFF81, 0x42)]);
        b.mem[0xFF81] = 0x55;
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0 + 5, 0xF2), Read(0xFF81, 0x55)]);
        assert_eq!(c.regs.a, 0x55);
    }

    #[test]
    fn ld_nn_sp_writes_lo_then_hi() {
        let mut c = cpu();
        c.regs.sp = 0xABCD;
        let mut b = bus(&[0x08, 0x34, 0xC1]);
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [
                Read(PC0, 0x08),
                Read(PC0 + 1, 0x34),
                Read(PC0 + 2, 0xC1),
                Write(0xC134, 0xCD),
                Write(0xC135, 0xAB)
            ]
        );
    }

    #[test]
    fn ld_sp_hl_has_internal_cycle() {
        let mut c = cpu();
        c.regs.set_hl(0x1234);
        let mut b = bus(&[0xF9]);
        step(&mut c, &mut b);
        assert_eq!(b.log, [Read(PC0, 0xF9), Tick]);
        assert_eq!(c.regs.sp, 0x1234);
    }

    // ----- push / pop -----

    #[test]
    fn push_has_internal_cycle_before_writes() {
        let mut c = cpu();
        c.regs.set_bc(0x1234);
        let mut b = bus(&[0xC5]);
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [
                Read(PC0, 0xC5),
                Tick,
                Write(SP0 - 1, 0x12),
                Write(SP0 - 2, 0x34)
            ]
        );
        assert_eq!(c.regs.sp, SP0 - 2);
    }

    #[test]
    fn pop_has_no_internal_cycle() {
        let mut c = cpu();
        let mut b = bus(&[0xD1]); // POP DE
        b.load(SP0, &[0x34, 0x12]);
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [Read(PC0, 0xD1), Read(SP0, 0x34), Read(SP0 + 1, 0x12)]
        );
        assert_eq!(c.regs.de(), 0x1234);
        assert_eq!(c.regs.sp, SP0 + 2);
    }

    #[test]
    fn pop_af_masks_f_low_nibble() {
        let mut c = cpu();
        let mut b = bus(&[0xF1]);
        b.load(SP0, &[0xFF, 0x12]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x12);
        assert_eq!(c.regs.f, 0xF0);
    }

    #[test]
    fn push_af_writes_a_then_f() {
        let mut c = cpu();
        c.regs.a = 0x12;
        c.regs.f = 0xF0;
        let mut b = bus(&[0xF5]);
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [
                Read(PC0, 0xF5),
                Tick,
                Write(SP0 - 1, 0x12),
                Write(SP0 - 2, 0xF0)
            ]
        );
    }

    // ----- 8-bit ALU -----

    #[test]
    fn add_and_adc_flags() {
        let mut c = cpu();
        c.regs.a = 0x0F;
        c.regs.b = 0x01;
        let mut b = bus(&[0x80]); // ADD A,B
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x10);
        assert_eq!(c.regs.f, Flags::H);

        // carry + zero: 0xFF + 0x00 + carry-in
        let mut c = cpu();
        c.regs.a = 0xFF;
        c.regs.b = 0x00;
        c.regs.f = Flags::C;
        let mut b = bus(&[0x88]); // ADC A,B
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x00);
        assert_eq!(c.regs.f, Flags::Z | Flags::H | Flags::C);

        // ADC carry contributes to both halves
        let mut c = cpu();
        c.regs.a = 0x80;
        c.regs.b = 0x80;
        let mut b = bus(&[0x80]); // ADD A,B -> 0x00, C
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0);
        assert_eq!(c.regs.f, Flags::Z | Flags::C);
    }

    #[test]
    fn sub_sbc_cp_flags() {
        let mut c = cpu();
        c.regs.a = 0x10;
        c.regs.b = 0x01;
        let mut b = bus(&[0x90]); // SUB B: half borrow
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x0F);
        assert_eq!(c.regs.f, Flags::N | Flags::H);

        let mut c = cpu();
        c.regs.a = 0x00;
        c.regs.b = 0x00;
        c.regs.f = Flags::C;
        let mut b = bus(&[0x98]); // SBC A,B: 0 - 0 - 1
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0xFF);
        assert_eq!(c.regs.f, Flags::N | Flags::H | Flags::C);

        let mut c = cpu();
        c.regs.a = 0x42;
        c.regs.b = 0x42;
        let mut b = bus(&[0xB8]); // CP B: equal, A unchanged
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x42);
        assert_eq!(c.regs.f, Flags::Z | Flags::N);
    }

    #[test]
    fn and_xor_or_flags() {
        let mut c = cpu();
        c.regs.a = 0xF0;
        c.regs.b = 0x0F;
        let mut b = bus(&[0xA0]); // AND B -> 0, H always set
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0);
        assert_eq!(c.regs.f, Flags::Z | Flags::H);

        let mut c = cpu();
        c.regs.a = 0xFF;
        c.regs.b = 0xFF;
        c.regs.f = 0xF0;
        let mut b = bus(&[0xA8]); // XOR B -> 0
        step(&mut c, &mut b);
        assert_eq!(c.regs.f, Flags::Z);

        let mut c = cpu();
        c.regs.a = 0x00;
        c.regs.b = 0x08;
        c.regs.f = 0xF0;
        let mut b = bus(&[0xB0]); // OR B
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x08);
        assert_eq!(c.regs.f, 0);
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

    // ----- INC/DEC -----

    #[test]
    fn inc_dec_r8_flags_preserve_carry() {
        let mut c = cpu();
        c.regs.b = 0x0F;
        c.regs.f = Flags::C;
        let mut b = bus(&[0x04]); // INC B
        step(&mut c, &mut b);
        assert_eq!(c.regs.b, 0x10);
        assert_eq!(c.regs.f, Flags::H | Flags::C);

        let mut c = cpu();
        c.regs.b = 0xFF;
        let mut b = bus(&[0x04]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.b, 0x00);
        assert_eq!(c.regs.f, Flags::Z | Flags::H);

        let mut c = cpu();
        c.regs.b = 0x10;
        c.regs.f = Flags::C;
        let mut b = bus(&[0x05]); // DEC B: borrow from bit 4
        step(&mut c, &mut b);
        assert_eq!(c.regs.b, 0x0F);
        assert_eq!(c.regs.f, Flags::N | Flags::H | Flags::C);

        let mut c = cpu();
        c.regs.b = 0x01;
        let mut b = bus(&[0x05]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.b, 0x00);
        assert_eq!(c.regs.f, Flags::Z | Flags::N);

        let mut c = cpu();
        c.regs.b = 0x00;
        let mut b = bus(&[0x05]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.b, 0xFF);
        assert_eq!(c.regs.f, Flags::N | Flags::H);
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
        assert_eq!(c.regs.f, Flags::H);
    }

    #[test]
    fn inc_dec_rp_trace_and_wrap() {
        let mut c = cpu();
        c.regs.sp = 0xFFFF;
        let mut b = bus(&[0x33, 0x3B]);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0, 0x33), Tick]);
        assert_eq!(c.regs.sp, 0x0000);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0 + 1, 0x3B), Tick]);
        assert_eq!(c.regs.sp, 0xFFFF);
        assert_eq!(c.regs.f, 0); // no flags
    }

    // ----- 16-bit arithmetic -----

    #[test]
    fn add_hl_rp_flags_and_trace() {
        let mut c = cpu();
        c.regs.set_hl(0x0FFF);
        c.regs.set_bc(0x0001);
        c.regs.f = Flags::Z; // Z must be preserved
        let mut b = bus(&[0x09]);
        step(&mut c, &mut b);
        assert_eq!(b.log, [Read(PC0, 0x09), Tick]);
        assert_eq!(c.regs.hl(), 0x1000);
        assert_eq!(c.regs.f, Flags::Z | Flags::H);

        let mut c = cpu();
        c.regs.set_hl(0x8000);
        c.regs.set_de(0x8000);
        let mut b = bus(&[0x19]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.hl(), 0x0000);
        assert_eq!(c.regs.f, Flags::C);

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
        assert_eq!(c.regs.f, Flags::H | Flags::C);

        // negative offset: flags still from unsigned low-byte addition
        let mut c = cpu();
        c.regs.sp = 0xD000;
        let mut b = bus(&[0xE8, 0xFF]); // SP + (-1)
        step(&mut c, &mut b);
        assert_eq!(c.regs.sp, 0xCFFF);
        assert_eq!(c.regs.f, 0); // 0x00 + 0xFF: no half-carry, no carry
    }

    #[test]
    fn ld_hl_sp_e_timing_and_flags() {
        let mut c = cpu();
        c.regs.sp = 0xFFFF;
        let mut b = bus(&[0xF8, 0x01]);
        step(&mut c, &mut b);
        assert_eq!(b.log, [Read(PC0, 0xF8), Read(PC0 + 1, 0x01), Tick]);
        assert_eq!(c.regs.hl(), 0x0000);
        assert_eq!(c.regs.f, Flags::H | Flags::C);
        assert_eq!(c.regs.sp, 0xFFFF); // SP unchanged

        let mut c = cpu();
        c.regs.sp = 0xD002;
        let mut b = bus(&[0xF8, 0xF8]); // SP + (-8)
        step(&mut c, &mut b);
        assert_eq!(c.regs.hl(), 0xCFFA);
        assert_eq!(c.regs.f, 0);
    }

    // ----- accumulator rotates, DAA, misc flags -----

    #[test]
    fn rotate_a_ops_never_set_z() {
        let mut c = cpu();
        c.regs.a = 0x80;
        let mut b = bus(&[0x07]); // RLCA
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x01);
        assert_eq!(c.regs.f, Flags::C);

        let mut c = cpu();
        c.regs.a = 0x00;
        c.regs.f = 0xF0;
        let mut b = bus(&[0x07]); // result 0 but Z stays clear
        step(&mut c, &mut b);
        assert_eq!(c.regs.f, 0);

        let mut c = cpu();
        c.regs.a = 0x01;
        let mut b = bus(&[0x0F]); // RRCA
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x80);
        assert_eq!(c.regs.f, Flags::C);

        let mut c = cpu();
        c.regs.a = 0x80;
        c.regs.f = Flags::C;
        let mut b = bus(&[0x17]); // RLA: carry in to bit 0
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x01);
        assert_eq!(c.regs.f, Flags::C);

        let mut c = cpu();
        c.regs.a = 0x01;
        c.regs.f = Flags::C;
        let mut b = bus(&[0x1F]); // RRA: carry in to bit 7
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x80);
        assert_eq!(c.regs.f, Flags::C);
    }

    /// Independent DAA model for add mode (N=0), written from the algorithm
    /// description in gbctr: accumulate the adjustment, apply it in one
    /// step, carry set by the 0x60 correction. Subtract mode is
    /// deliberately not re-derived here: a flag-based reference would share
    /// `op_daa`'s structure and could not catch a shared misunderstanding.
    /// It is checked against decimal arithmetic instead, in
    /// `daa_after_sub_computes_bcd_difference_for_all_operands`.
    fn daa_add_ref(a: u8, h: bool, c: bool) -> (u8, bool, bool) {
        let mut adjust = 0u8;
        let mut carry = c;
        if h || a & 0x0F > 0x09 {
            adjust += 0x06;
        }
        if c || a > 0x99 {
            adjust += 0x60;
            carry = true;
        }
        let r = a.wrapping_add(adjust);
        (r, r == 0, carry)
    }

    #[test]
    fn daa_matches_reference_for_all_add_mode_inputs() {
        for fbits in 0..4u8 {
            let h = fbits & 1 != 0;
            let cf = fbits & 2 != 0;
            for a in 0..=255u8 {
                let mut c = cpu();
                c.regs.a = a;
                c.regs.f = fl(false, false, h, cf);
                let mut b = bus(&[0x27]);
                step(&mut c, &mut b);
                let (ra, rz, rc) = daa_add_ref(a, h, cf);
                let expect_f = fl(rz, false, false, rc);
                assert_eq!(c.regs.a, ra, "a={a:#04x} h={h} c={cf}");
                assert_eq!(c.regs.f, expect_f, "a={a:#04x} h={h} c={cf}");
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
                    c.regs.f,
                    fl(diff == 0, true, false, borrow),
                    "sub x={x} y={y}"
                );

                // SBC B with carry in: decimal x - y - 1.
                let mut c = cpu();
                c.regs.a = packed(x);
                c.regs.b = packed(y);
                c.regs.f = fl(false, false, false, true);
                let mut b = bus(&[0x98, 0x27]); // SBC B; DAA
                step(&mut c, &mut b);
                step(&mut c, &mut b);
                let diff = (99 + x - y) % 100;
                let borrow = x <= y;
                assert_eq!(c.regs.a, packed(diff), "sbc x={x} y={y}");
                assert_eq!(
                    c.regs.f,
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
        assert_eq!(c.regs.f, 0);

        // 0x90 + 0x90 = 0x180 BCD: result 0x80 with carry
        let mut c = cpu();
        c.regs.a = 0x90;
        c.regs.b = 0x90;
        let mut b = bus(&[0x80, 0x27]);
        step(&mut c, &mut b);
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x80);
        assert_eq!(c.regs.f, Flags::C);

        // 0x20 - 0x13 = 0x07 BCD
        let mut c = cpu();
        c.regs.a = 0x20;
        c.regs.b = 0x13;
        let mut b = bus(&[0x90, 0x27]);
        step(&mut c, &mut b);
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0x07);
        assert_eq!(c.regs.f, Flags::N);
    }

    #[test]
    fn cpl_scf_ccf() {
        let mut c = cpu();
        c.regs.a = 0x35;
        c.regs.f = Flags::Z | Flags::C;
        let mut b = bus(&[0x2F]); // CPL: Z,C preserved; N,H set
        step(&mut c, &mut b);
        assert_eq!(c.regs.a, 0xCA);
        assert_eq!(c.regs.f, Flags::Z | Flags::N | Flags::H | Flags::C);

        let mut c = cpu();
        c.regs.f = Flags::Z | Flags::N | Flags::H;
        let mut b = bus(&[0x37]); // SCF
        step(&mut c, &mut b);
        assert_eq!(c.regs.f, Flags::Z | Flags::C);

        let mut c = cpu();
        c.regs.f = Flags::N | Flags::H | Flags::C;
        let mut b = bus(&[0x3F]); // CCF: complement carry
        step(&mut c, &mut b);
        assert_eq!(c.regs.f, 0);
        let mut b = bus(&[0x3F]);
        c.regs.pc = PC0;
        step(&mut c, &mut b);
        assert_eq!(c.regs.f, Flags::C);
    }

    // ----- jumps / calls / returns -----

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
                Tick,
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
            [Read(PC0, 0xC9), Read(SP0, 0x34), Read(SP0 + 1, 0x12), Tick]
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
                Read(SP0, 0x34),
                Read(SP0 + 1, 0x12),
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
                Tick,
                Write(SP0 - 1, 0xC0),
                Write(SP0 - 2, 0x01)
            ]
        );
        assert_eq!(c.regs.pc, 0x0028);
    }

    // ----- CB-prefixed -----

    #[test]
    fn cb_register_op_is_two_cycles() {
        let mut c = cpu();
        c.regs.c = 0x88;
        let mut b = bus(&[0xCB, 0x11]); // RL C
        step(&mut c, &mut b);
        assert_eq!(b.log, [Read(PC0, 0xCB), Read(PC0 + 1, 0x11)]);
        assert_eq!(c.regs.c, 0x10);
        assert_eq!(c.regs.f, Flags::C);
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
            c.regs.f = if cin { Flags::C } else { 0 };
            let mut b = bus(&[0xCB, op]);
            step(&mut c, &mut b);
            assert_eq!(c.regs.b, out, "op={op:#04x}");
            assert_eq!(c.regs.f, fl(out == 0, false, false, cout), "op={op:#04x}");
        }
        // Z set by CB rotates (unlike RLCA-family)
        let mut c = cpu();
        c.regs.b = 0;
        let mut b = bus(&[0xCB, 0x00]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.f, Flags::Z);
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
        assert_eq!(c.regs.f, Flags::C);
    }

    #[test]
    fn bit_hl_is_three_cycles_and_flags() {
        let mut c = cpu();
        c.regs.set_hl(0xC800);
        c.regs.f = Flags::C;
        let mut b = bus(&[0xCB, 0x7E]); // BIT 7,(HL)
        b.mem[0xC800] = 0x7F;
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [Read(PC0, 0xCB), Read(PC0 + 1, 0x7E), Read(0xC800, 0x7F)]
        );
        // bit 7 clear -> Z set; H set; C preserved
        assert_eq!(c.regs.f, Flags::Z | Flags::H | Flags::C);

        let mut c = cpu();
        c.regs.h = 0x10;
        let mut b = bus(&[0xCB, 0x64]); // BIT 4,H -> set, Z clear
        step(&mut c, &mut b);
        assert_eq!(c.regs.f, Flags::H);
    }

    #[test]
    fn res_set_hl_are_four_cycles() {
        let mut c = cpu();
        c.regs.set_hl(0xC800);
        c.regs.f = 0xF0;
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
        assert_eq!(c.regs.f, 0xF0); // RES/SET touch no flags
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

    // ----- EI / DI / IME sequencing -----

    #[test]
    fn ei_enables_after_following_instruction() {
        // mooneye acceptance/ei_timing: exactly one instruction after EI runs
        // before the interrupt is taken.
        let mut c = cpu();
        let mut b = bus(&[0xFB, 0x04, 0x04]); // EI; INC B; INC B
        b.mem[0xFFFF] = 0x08;
        b.mem[0xFF0F] = 0x08;
        step(&mut c, &mut b); // EI
        assert!(!c.ime);
        step(&mut c, &mut b); // INC B (no dispatch before it)
        assert_eq!(c.regs.b, 1);
        assert!(c.ime);
        b.take_log();
        step(&mut c, &mut b); // dispatch to 0x58
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(SP0 - 1, 0xC0),
                Write(SP0 - 2, 0x02),
                Read(0x0058, 0x00)
            ]
        );
        assert_eq!(c.regs.b, 1); // second INC B did not run
        assert!(!c.ime);
        assert_eq!(b.mem[0xFF0F], 0x00); // serial bit acked
    }

    #[test]
    fn ei_di_leaves_ime_off() {
        // mooneye acceptance/rapid_di_ei: EI directly followed by DI never
        // lets an interrupt through.
        let mut c = cpu();
        let mut b = bus(&[0xFB, 0xF3, 0x00]); // EI; DI; NOP
        b.mem[0xFFFF] = 0x08;
        b.mem[0xFF0F] = 0x08;
        step(&mut c, &mut b);
        step(&mut c, &mut b);
        assert!(!c.ime);
        assert!(!c.ime_pending);
        b.take_log();
        step(&mut c, &mut b); // NOP, no dispatch
        assert_eq!(b.log, [Read(PC0 + 2, 0x00)]);
        assert_eq!(b.mem[0xFF0F], 0x08); // untouched
    }

    #[test]
    fn ei_ei_dispatches_after_second_ei() {
        // mooneye acceptance/ei_sequence: with back-to-back EIs the interrupt
        // is taken right after the *second* EI; the pushed return address is
        // the byte after it.
        let mut c = cpu();
        let mut b = bus(&[0xFB, 0xFB, 0xF3]); // EI; EI; DI
        b.mem[0xFFFF] = 0x08;
        b.mem[0xFF0F] = 0x08;
        step(&mut c, &mut b); // EI #1
        step(&mut c, &mut b); // EI #2; IME commits after it
        assert!(c.ime);
        b.take_log();
        step(&mut c, &mut b); // dispatch; DI never runs
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(SP0 - 1, 0xC0),
                Write(SP0 - 2, 0x02),
                Read(0x0058, 0x00)
            ]
        );
        assert!(!c.ime); // handler runs with IME off
    }

    #[test]
    fn ei_ei_di_without_pending_leaves_ime_off() {
        let mut c = cpu();
        let mut b = bus(&[0xFB, 0xFB, 0xF3]);
        step(&mut c, &mut b);
        step(&mut c, &mut b);
        assert!(c.ime);
        step(&mut c, &mut b); // DI
        assert!(!c.ime);
        assert!(!c.ime_pending);
    }

    #[test]
    fn di_takes_effect_immediately() {
        let mut c = cpu();
        c.ime = true;
        let mut b = bus(&[0xF3, 0x00]);
        step(&mut c, &mut b); // DI
        assert!(!c.ime);
        b.mem[0xFFFF] = 0x01;
        b.mem[0xFF0F] = 0x01;
        b.take_log();
        step(&mut c, &mut b); // NOP, no dispatch
        assert_eq!(b.log, [Read(PC0 + 1, 0x00)]);
    }

    #[test]
    fn reti_sets_ime_immediately() {
        // mooneye acceptance/reti_intr_timing: an interrupt pending when RETI
        // executes is dispatched right at the next instruction boundary.
        let mut c = cpu();
        let mut b = bus(&[0xD9]);
        b.load(SP0, &[0x00, 0xC1]); // return to 0xC100
        b.mem[0xFFFF] = 0x01;
        b.mem[0xFF0F] = 0x01;
        step(&mut c, &mut b);
        assert_eq!(
            b.take_log(),
            [Read(PC0, 0xD9), Read(SP0, 0x00), Read(SP0 + 1, 0xC1), Tick]
        );
        assert!(c.ime);
        assert_eq!(c.regs.pc, 0xC100);
        step(&mut c, &mut b); // immediate dispatch, return address 0xC100
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(SP0 + 1, 0xC1),
                Write(SP0, 0x00),
                Read(0x0040, 0x00)
            ]
        );
    }

    // ----- interrupt dispatch -----

    #[test]
    fn dispatch_trace_priority_and_ack() {
        let mut c = cpu();
        c.ime = true;
        c.regs.pc = 0xC123;
        let mut b = TestBus::new();
        b.mem[0xFFFF] = 0x1F;
        b.mem[0xFF0F] = 0x14; // timer (bit 2) + joypad (bit 4)
        step(&mut c, &mut b);
        // timer wins (lowest bit number = highest priority) -> vector 0x50
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(SP0 - 1, 0xC1),
                Write(SP0 - 2, 0x23),
                Read(0x0050, 0x00)
            ]
        );
        assert_eq!(c.regs.pc, 0x0051); // NOP at vector already executed
        assert!(!c.ime);
        assert_eq!(b.mem[0xFF0F], 0x10); // only the timer bit acked
    }

    #[test]
    fn dispatch_only_when_enabled_in_ie() {
        let mut c = cpu();
        c.ime = true;
        let mut b = bus(&[0x00]);
        b.mem[0xFFFF] = 0x02;
        b.mem[0xFF0F] = 0x01; // pending but masked
        step(&mut c, &mut b);
        assert_eq!(b.log, [Read(PC0, 0x00)]);
    }

    #[test]
    fn ie_push_high_byte_cancels_dispatch() {
        // mooneye acceptance/interrupts/ie_push round 1: the PC-high push
        // overwrites IE and clears the only pending bit; dispatch is
        // cancelled, PC := 0x0000, IF is *not* modified, IME stays off.
        let mut c = cpu();
        c.ime = true;
        c.regs.pc = 0x0212; // high byte 0x02 lands in IE
        c.regs.sp = 0x0000;
        let mut b = TestBus::new();
        b.mem[0xFFFF] = 0x04; // timer enabled
        b.mem[0xFF0F] = 0x04; // timer pending
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(0xFFFF, 0x02), // IE := 0x02, timer no longer enabled
                Write(0xFFFE, 0x12),
                Read(0x0000, 0x00)
            ]
        );
        assert_eq!(c.regs.pc, 0x0001);
        assert_eq!(b.mem[0xFF0F], 0x04); // IF untouched
        assert!(!c.ime); // round 2: IME stays 0 after cancellation
    }

    #[test]
    fn ie_push_low_byte_is_too_late_to_cancel() {
        // ie_push round 3: IE is only clobbered by the PC-low push; the
        // interrupt was already chosen and acked after the high push.
        let mut c = cpu();
        c.ime = true;
        c.regs.pc = 0x0212; // low byte 0x12 clears IE bit 3 - too late
        c.regs.sp = 0x0001;
        let mut b = TestBus::new();
        b.mem[0xFFFF] = 0x08; // serial
        b.mem[0xFF0F] = 0x08;
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(0x0000, 0x02),
                Write(0xFFFF, 0x12),
                Read(0x0058, 0x00)
            ]
        );
        assert_eq!(c.regs.pc, 0x0059);
        assert_eq!(b.mem[0xFF0F], 0x00); // IF cleared: dispatch went through
    }

    #[test]
    fn ie_push_high_byte_redirects_to_remaining_interrupt() {
        // ie_push round 4: the high push rewrites IE keeping a different
        // pending bit enabled; that interrupt is dispatched instead.
        let mut c = cpu();
        c.ime = true;
        c.regs.pc = 0x0212; // IE := 0x02 keeps STAT enabled
        c.regs.sp = 0x0000;
        let mut b = TestBus::new();
        b.mem[0xFFFF] = 0x03; // vblank + stat
        b.mem[0xFF0F] = 0x03;
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(0xFFFF, 0x02),
                Write(0xFFFE, 0x12),
                Read(0x0048, 0x00) // STAT vector
            ]
        );
        assert_eq!(b.mem[0xFF0F], 0x01); // STAT acked, vblank still pending
    }

    // ----- HALT -----

    #[test]
    fn halt_ime1_waits_then_dispatches() {
        let mut c = cpu();
        c.ime = true;
        let mut b = bus(&[0x76]);
        b.mem[0xFFFF] = 0x04;
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0, 0x76)]);
        assert!(c.halted);
        step(&mut c, &mut b);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Tick, Tick]); // halted: one tick per step
        b.mem[0xFF0F] = 0x04;
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(SP0 - 1, 0xC0),
                Write(SP0 - 2, 0x01), // return address = after HALT
                Read(0x0050, 0x00)
            ]
        );
        assert!(!c.halted);
        assert_eq!(b.mem[0xFF0F], 0);
    }

    #[test]
    fn halt_ime0_continues_without_dispatch() {
        let mut c = cpu();
        let mut b = bus(&[0x76, 0x04]); // HALT; INC B
        b.mem[0xFFFF] = 0x04;
        step(&mut c, &mut b);
        assert!(c.halted);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0, 0x76), Tick]);
        b.mem[0xFF0F] = 0x04;
        step(&mut c, &mut b);
        // wakes with no extra delay and just executes the next instruction
        assert_eq!(b.log, [Read(PC0 + 1, 0x04)]);
        assert_eq!(c.regs.b, 1);
        assert_eq!(b.mem[0xFF0F], 0x04); // IF not acked
        assert!(!c.ime);
    }

    #[test]
    fn halt_bug_fetches_next_opcode_twice() {
        // HALT with IME=0 while IE & IF != 0: PC fails to increment for the
        // following opcode fetch (gbctr).
        let mut c = cpu();
        let mut b = bus(&[0x76, 0x3C]); // HALT; INC A
        b.mem[0xFFFF] = 0x04;
        b.mem[0xFF0F] = 0x04;
        step(&mut c, &mut b);
        assert!(!c.halted);
        assert!(c.halt_bug);
        step(&mut c, &mut b);
        assert_eq!(c.regs.pc, PC0 + 1); // PC stuck
        assert_eq!(c.regs.a, 1);
        step(&mut c, &mut b);
        assert_eq!(c.regs.pc, PC0 + 2); // now it advances
        assert_eq!(c.regs.a, 2); // INC A ran twice
        assert_eq!(
            b.log,
            [Read(PC0, 0x76), Read(PC0 + 1, 0x3C), Read(PC0 + 1, 0x3C)]
        );
    }

    #[test]
    fn halt_bug_with_multibyte_instruction_reads_opcode_as_operand() {
        let mut c = cpu();
        let mut b = bus(&[0x76, 0x3E, 0x99]); // HALT; LD A,n
        b.mem[0xFFFF] = 0x01;
        b.mem[0xFF0F] = 0x01;
        step(&mut c, &mut b);
        step(&mut c, &mut b);
        // LD A,n reads its own opcode byte as the operand
        assert_eq!(c.regs.a, 0x3E);
        assert_eq!(c.regs.pc, PC0 + 2);
    }

    #[test]
    fn ei_halt_with_pending_behaves_like_ime1() {
        // mooneye acceptance/halt_ime0_ei: EI directly before HALT means the
        // delayed enable commits while halting; the interrupt dispatches with
        // the return address after the HALT - no halt bug.
        let mut c = cpu();
        let mut b = bus(&[0xFB, 0x76]); // EI; HALT
        b.mem[0xFFFF] = 0x01;
        b.mem[0xFF0F] = 0x01;
        step(&mut c, &mut b); // EI
        step(&mut c, &mut b); // HALT: no halt bug despite pending + IME=0
        assert!(c.halted);
        assert!(!c.halt_bug);
        assert!(c.ime);
        b.take_log();
        step(&mut c, &mut b); // dispatch out of halt
        assert_eq!(
            b.log,
            [
                Tick,
                Tick,
                Write(SP0 - 1, 0xC0),
                Write(SP0 - 2, 0x02),
                Read(0x0040, 0x00)
            ]
        );
    }

    #[test]
    fn halted_cpu_consumes_tick_cycles() {
        let mut c = cpu();
        c.halted = true;
        let mut b = TestBus::new();
        for _ in 0..5 {
            step(&mut c, &mut b);
        }
        assert_eq!(b.log, [Tick, Tick, Tick, Tick, Tick]);
        assert!(c.halted);
    }

    // ----- STOP -----

    #[test]
    fn stop_skips_following_byte_and_sleeps_until_joypad_wake() {
        // Pan Docs STOP flowchart: no interrupt pending -> 2-byte opcode;
        // bus.stop() == false -> deep stop, the CPU burns tick cycles like
        // halt until the joypad interrupt becomes pending.
        let mut c = cpu();
        let mut b = bus(&[0x10, 0x00, 0x04]); // STOP; (skipped); INC B
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0, 0x10)]);
        assert_eq!(b.stop_calls, 1);
        assert_eq!(c.regs.pc, PC0 + 2);
        assert!(c.stopped);
        for _ in 0..3 {
            step(&mut c, &mut b);
        }
        assert_eq!(b.take_log(), [Tick, Tick, Tick]);
        assert_eq!(c.regs.pc, PC0 + 2);
        // Joypad interrupt wakes it; execution resumes after the skipped
        // byte with no extra delay cycles.
        b.mem[0xFFFF] = 0x10;
        b.mem[0xFF0F] = 0x10;
        step(&mut c, &mut b);
        assert!(!c.stopped);
        assert_eq!(b.log, [Read(PC0 + 2, 0x04)]);
        assert_eq!(c.regs.b, 1);
    }

    #[test]
    fn stop_with_speed_switch_continues_normally() {
        // bus.stop() == true: the bus performed the armed CGB speed switch,
        // so the CPU does not enter stop mode and keeps executing.
        let mut c = cpu();
        let mut b = bus(&[0x10, 0x00, 0x04]);
        b.stop_result = true;
        step(&mut c, &mut b);
        assert_eq!(b.stop_calls, 1);
        assert_eq!(c.regs.pc, PC0 + 2);
        assert!(!c.stopped);
        step(&mut c, &mut b);
        assert_eq!(c.regs.b, 1);
    }

    #[test]
    fn stop_with_pending_interrupt_is_one_byte_opcode() {
        // Pan Docs STOP flowchart: with IE & IF != 0, STOP is a 1-byte
        // opcode - the byte after it is executed, not skipped.
        let mut c = cpu();
        let mut b = bus(&[0x10, 0x04]); // STOP; INC B
        b.mem[0xFFFF] = 0x10;
        b.mem[0xFF0F] = 0x10;
        step(&mut c, &mut b);
        assert_eq!(c.regs.pc, PC0 + 1);
        // The already-pending interrupt also ends the stop immediately
        // (IME=0, so it is not dispatched).
        step(&mut c, &mut b);
        assert!(!c.stopped);
        assert_eq!(c.regs.b, 1);
    }

    // ----- illegal opcodes -----

    #[test]
    fn illegal_opcode_locks_cpu_forever() {
        let mut c = cpu();
        let mut b = bus(&[0xD3, 0x04]);
        step(&mut c, &mut b);
        assert_eq!(b.take_log(), [Read(PC0, 0xD3)]);
        assert!(c.locked);
        let regs = c.regs;
        for _ in 0..3 {
            step(&mut c, &mut b);
        }
        assert_eq!(b.log, [Tick, Tick, Tick]);
        assert_eq!(c.regs, regs);
        // not even interrupts get it out
        b.mem[0xFFFF] = 0x01;
        b.mem[0xFF0F] = 0x01;
        c.ime = true;
        b.take_log();
        step(&mut c, &mut b);
        assert_eq!(b.log, [Tick]);
    }

    #[test]
    fn all_illegal_opcodes_lock() {
        for op in [
            0xD3u8, 0xDB, 0xDD, 0xE3, 0xE4, 0xEB, 0xEC, 0xED, 0xF4, 0xFC, 0xFD,
        ] {
            let mut c = cpu();
            let mut b = bus(&[op]);
            step(&mut c, &mut b);
            assert!(c.locked, "opcode {op:#04x} must lock");
        }
    }

    // ----- whole-opcode-space cycle count sweeps -----

    /// Expected M-cycles per base opcode, given the branch outcome of each
    /// condition code. Numbers from the gbctr instruction tables.
    fn base_cycles(op: u8, taken: impl Fn(u8) -> bool) -> usize {
        match op {
            0x01 | 0x11 | 0x21 | 0x31 => 3,
            0x02 | 0x12 | 0x22 | 0x32 | 0x0A | 0x1A | 0x2A | 0x3A => 2,
            0x03 | 0x13 | 0x23 | 0x33 | 0x0B | 0x1B | 0x2B | 0x3B => 2,
            0x34..=0x36 => 3,
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x3E => 2,
            0x08 => 5,
            0x09 | 0x19 | 0x29 | 0x39 => 2,
            0x18 => 3,
            0x20 | 0x28 | 0x30 | 0x38 => {
                if taken((op >> 3) & 3) {
                    3
                } else {
                    2
                }
            }
            0x76 => 1, // HALT (no pending interrupt in the sweep)
            0x40..=0x7F => {
                if (op >> 3) & 7 == 6 || op & 7 == 6 {
                    2
                } else {
                    1
                }
            }
            0x80..=0xBF => {
                if op & 7 == 6 {
                    2
                } else {
                    1
                }
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 => {
                if taken((op >> 3) & 3) {
                    5
                } else {
                    2
                }
            }
            0xC1 | 0xD1 | 0xE1 | 0xF1 => 3,
            0xC2 | 0xCA | 0xD2 | 0xDA => {
                if taken((op >> 3) & 3) {
                    4
                } else {
                    3
                }
            }
            0xC3 => 4,
            0xC4 | 0xCC | 0xD4 | 0xDC => {
                if taken((op >> 3) & 3) {
                    6
                } else {
                    3
                }
            }
            0xC5 | 0xD5 | 0xE5 | 0xF5 => 4,
            0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => 2,
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => 4,
            0xC9 | 0xD9 => 4,
            0xCD => 6,
            0xE0 | 0xF0 => 3,
            0xE2 | 0xF2 => 2,
            0xE8 => 4,
            0xE9 => 1,
            0xEA | 0xFA => 4,
            0xF8 => 3,
            0xF9 => 2,
            // 1-cycle ops, STOP, illegal opcodes (lock after the fetch)
            _ => 1,
        }
    }

    fn run_sweep(flags: u8) {
        let taken = |cc: u8| match cc {
            0 => flags & Flags::Z == 0,
            1 => flags & Flags::Z != 0,
            2 => flags & Flags::C == 0,
            _ => flags & Flags::C != 0,
        };
        for op in 0..=255u8 {
            if op == 0xCB {
                continue; // prefix, swept separately
            }
            let mut c = cpu();
            c.regs.f = flags;
            c.regs.set_hl(0xC800);
            c.regs.set_bc(0xC700);
            c.regs.set_de(0xC701);
            let mut b = bus(&[op, 0x00, 0x00]);
            step(&mut c, &mut b);
            assert_eq!(
                b.log.len(),
                base_cycles(op, taken),
                "opcode {op:#04x} flags {flags:#04x}"
            );
            assert_eq!(c.regs.f & 0x0F, 0, "opcode {op:#04x} dirtied F low nibble");
        }
    }

    #[test]
    fn cycle_counts_all_base_opcodes_flags_clear() {
        run_sweep(0x00);
    }

    #[test]
    fn cycle_counts_all_base_opcodes_flags_set() {
        run_sweep(Flags::Z | Flags::C);
    }

    #[test]
    fn cycle_counts_all_cb_opcodes() {
        for op in 0..=255u8 {
            let expected = if op & 7 != 6 {
                2
            } else if (0x40..=0x7F).contains(&op) {
                3 // BIT n,(HL): no write-back
            } else {
                4
            };
            let mut c = cpu();
            c.regs.set_hl(0xC800);
            let mut b = bus(&[0xCB, op]);
            step(&mut c, &mut b);
            assert_eq!(b.log.len(), expected, "CB {op:#04x}");
            assert_eq!(c.regs.f & 0x0F, 0, "CB {op:#04x} dirtied F low nibble");
        }
    }
}
