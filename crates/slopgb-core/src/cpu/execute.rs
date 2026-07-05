//! Instruction decode and execution. CPU work package.
//!
//! Cycle model: every [`Bus::read`]/[`Bus::write`] and every internal
//! [`Bus::tick`] is exactly one M-cycle. Cycle counts and the placement of
//! internal cycles follow the per-instruction tables in *Game Boy: Complete
//! Technical Reference* (gbctr).

use super::{Bus, Cpu, flags};

/// Execute one instruction (preceded by interrupt dispatch if one is
/// pending and IME is set), one idle M-cycle of halt or stop mode, or a
/// halt wake (the waking cycle plus dispatch and/or the next instruction),
/// then flush the deferred-commit clock at the instruction boundary.
///
/// The flush is placed here, around the whole body, so it fires on *every*
/// exit path (the locked / halt-stay / halt-wake / stop-idle early returns
/// included) with a single call — the SameBoy `flush_pending_cycles`
/// instruction boundary (`sm83_cpu.c:336`). Inert in port Stage S1.
pub fn step(cpu: &mut Cpu, bus: &mut impl Bus) {
    run_step(cpu, bus);
    bus.flush_pending();
}

fn run_step(cpu: &mut Cpu, bus: &mut impl Bus) {
    if cpu.locked {
        // An illegal opcode hard-locks the CPU; it only burns cycles.
        bus.tick();
        return;
    }
    if cpu.halted {
        // halted and halt_bug are mutually exclusive by construction:
        // op_halt arms the bug *instead of* halting, so fetch_opcode below
        // never hits its halt_bug branch and the PC restores on the
        // discarded/aborted paths undo a plain increment.
        debug_assert!(!cpu.halt_bug);
        // Halt mode ends when IE & IF != 0 regardless of IME, and the CPU
        // re-evaluates that condition *within* every M-cycle: an IF bit
        // committed before the cycle's halt-exit sampling point wakes with
        // no delay over a NOP wait loop (mooneye acceptance/
        // halt_ime0_nointr_timing and halt_ime1_timing2-GS both pin this
        // by comparing DIV-measured latencies of the two paths, verified
        // on hardware), while a bit committed after it costs one extra
        // idle cycle (see the wake check below). In a NOP loop the M-cycle
        // whose tick raises IF is an opcode fetch, so the halt idle cycle
        // is modelled as an opcode fetch (bus reads have no side effects;
        // only the M-cycle of time matters) that is rolled back while the
        // wake condition stays false.
        let pc_before = cpu.regs.pc;
        let opcode = fetch_opcode(cpu, bus);
        // The wake check uses the halt-exit sampling point, which sits
        // earlier *within* the M-cycle than the running CPU's end-of-fetch
        // `pending()` view (see `Bus::pending_halt_wake`): an IF bit
        // committed after it — in practice the timer reload's IF — keeps
        // the CPU halted one more cycle. The same sample feeds both the
        // IME=1 dispatch and the IME=0 resume (SameBoy sm83_cpu.c,
        // `GB_cpu_run`: one `interrupt_queue` sample serves both paths).
        if bus.pending_halt_wake_mid() == 0 {
            cpu.regs.pc = pc_before;
            // Staying halted: gate the core clock (and with it the OAM DMA
            // controller) off. The gate engages only now — *after* the
            // first idle prefetch — not on HALT execution: the SM83
            // prefetches the next opcode before sleeping (the same prefetch
            // the halt bug replays, gbctr "halt"), and madness/
            // mgb_oam_dma_halt_sprites.s pins the hardware OAM DMA freeze
            // point exactly two M-cycles after the HALT opcode fetch
            // (`ldh (DMA),a / nop / halt` leaves bytes 0-1 copied and
            // byte 2 mid-access). Repeat calls are idempotent no-ops.
            bus.set_halted(true);
            return;
        }
        // Waking: the core clock restarts. The observing prefetch cycle
        // ticked with the gate still in its previous state (on, unless the
        // wake came at the very first prefetch); exact DMA resume timing
        // within the waking cycle is not pinned by any test ROM.
        bus.set_halted(false);
        cpu.halted = false;
        if cpu.ime {
            // The observing cycle is the aborted prefetch of the dispatch
            // sequence: the vector fetch lands 5 M-cycles after the cycle
            // that saw IF rise, exactly like a dispatch aborting a NOP
            // fetch (acceptance/halt_ime1_timing2-GS rounds 3/4 vs 1/2).
            cpu.regs.pc = pc_before;
            dispatch_interrupt(cpu, bus);
            let vec_pc = cpu.regs.pc;
            let vector_opcode = fetch_opcode(cpu, bus);
            bus.profile_pc(vec_pc);
            bus.check_exec(vec_pc, vector_opcode);
            execute(cpu, bus, vector_opcode);
        } else {
            // IME=0: the observing cycle already was the next instruction's
            // opcode fetch (acceptance/halt_ime0_nointr_timing: "halt +
            // nops 6" measures the same DIV delta as dispatch + jp hl).
            bus.profile_pc(pc_before);
            bus.check_exec(pc_before, opcode);
            execute(cpu, bus, opcode);
        }
        return;
    }
    if cpu.stopped {
        // Stop mode ends on joypad wake (Pan Docs, "Using the STOP
        // Instruction"). The raw P1 input lines are not visible through
        // `Bus`, so wake is modelled as IE & IF != 0, like halt; in stop
        // mode every other interrupt source is frozen, so a newly pending
        // bit can only be the joypad. Not modelled: hardware wakes on the
        // P1 lines even with IE bit 4 clear. Unlike halt, wake is sampled
        // here at the step boundary, one M-cycle coarser than halt's
        // within-cycle re-check: an IF bit raised during the idle cycle
        // below is only observed at the start of the next step, so the
        // resume fetch lands one M-cycle later than the equivalent halt
        // wake would. No test ROM pins stop wake latency.
        if bus.pending() == 0 {
            bus.tick();
            // Stop mode switches the core clock off like halt mode does
            // (same gate, so an in-flight OAM DMA freezes here too); the
            // gate engages after the idle cycle, mirroring the halt path's
            // gate placement (wake sampling differs; see above).
            bus.set_halted(true);
            return;
        }
        bus.set_halted(false);
        cpu.stopped = false;
    }
    // EI enables IME only after the instruction *following* EI completes
    // (gbctr; mooneye acceptance/ei_sequence, ei_timing, rapid_di_ei).
    let ei_delay = cpu.ime_pending;
    // Integration fix: the IE & IF check happens at the *end* of the opcode
    // fetch M-cycle, so an IF bit raised by that very cycle's tick still
    // triggers a dispatch; the fetched opcode is then discarded (PC is not
    // incremented) and re-fetched after the dispatch. This is mooneye-gb's
    // `prefetch_next` model; the pass counters of
    // acceptance/serial/boot_sclk_align-dmgABCmgb and acceptance/intr_timing
    // pin the aborting behavior on hardware.
    let pc_before = cpu.regs.pc;
    let mut opcode = fetch_opcode(cpu, bus);
    // The address of the instruction `execute` will run: `pc_before`, unless an
    // interrupt was dispatched, in which case it is the handler's entry (the
    // re-fetch reads from the vector). Used only by the profiler/exception
    // hooks below (both inert unless the live debugger armed them).
    let mut exec_pc = pc_before;
    if cpu.ime && bus.pending_dispatch() != 0 {
        cpu.regs.pc = pc_before;
        dispatch_interrupt(cpu, bus);
        exec_pc = cpu.regs.pc;
        opcode = fetch_opcode(cpu, bus);
    }
    bus.profile_pc(exec_pc);
    bus.check_exec(exec_pc, opcode);
    execute(cpu, bus, opcode);
    if ei_delay && cpu.ime_pending {
        cpu.ime_pending = false;
        cpu.ime = true;
    }
}

/// Interrupt dispatch: two internal cycles and two pushes here. Counting
/// the aborted opcode fetch that detected the interrupt (see `step`), the
/// whole sequence is 5 M-cycles; the opcode fetch at the target address is
/// performed by the caller as the start of the handler's first instruction.
/// IME is cleared immediately; a not-yet-committed EI enable is swallowed
/// by the dispatch.
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
    // dispatch (mooneye acceptance/interrupts/ie_push). On cancellation
    // (pending == 0) nothing is acknowledged and the CPU ends up at 0x0000
    // with IME left disabled.
    let pending = bus.pending();
    let (target, ack_bit) = if pending == 0 {
        (0x0000, None)
    } else {
        let bit = pending.trailing_zeros() as u8;
        (0x0040 + (u16::from(bit) << 3), Some(bit))
    };
    if bus.dispatch_reclock() {
        // Port Stage B (Tier 2): the IF-ack / vector latch lands AFTER the low
        // push (SameBoy sm83_cpu.c:1690, the M5+2 latch), and the dispatch
        // reclock re-parks pending=2 there so the vector fetch + first handler
        // reads sample 2 dots early ("re-frames every read").
        cpu.regs.sp = cpu.regs.sp.wrapping_sub(1);
        bus.write(cpu.regs.sp, pc as u8);
        bus.dispatch_retime();
        if let Some(bit) = ack_bit {
            bus.ack(bit);
        }
    } else {
        // Eager path (byte-identical): the chosen IF bit is acknowledged
        // before the low push, exactly as before the port.
        if let Some(bit) = ack_bit {
            bus.ack(bit);
        }
        cpu.regs.sp = cpu.regs.sp.wrapping_sub(1);
        bus.write(cpu.regs.sp, pc as u8);
    }
    cpu.regs.pc = target;
}

/// Opcode fetch. The halt bug (HALT with IME=0 while IE & IF != 0) makes
/// exactly the next opcode fetch skip the PC increment (gbctr).
///
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
    cpu.regs.f() & mask != 0
}

fn set_flags(cpu: &mut Cpu, z: bool, n: bool, h: bool, c: bool) {
    cpu.regs
        .set_f((u8::from(z) << 7) | (u8::from(n) << 6) | (u8::from(h) << 5) | (u8::from(c) << 4));
}

/// `cc` condition codes: 0=NZ, 1=Z, 2=NC, 3=C.
fn condition(cpu: &Cpu, idx: u8) -> bool {
    match idx {
        0 => !flag(cpu, flags::Z),
        1 => flag(cpu, flags::Z),
        2 => !flag(cpu, flags::C),
        3 => flag(cpu, flags::C),
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

/// Two read M-cycles: low byte first, then high (gbctr). SP increments
/// share the read M-cycles (16-bit inc/dec unit), so reads from
/// $FE00-$FEFF trigger the OAM bug's "read during increase" pattern
/// (blargg oam_bug/2-causes test 8: POP with SP=$FDFF corrupts via its
/// second read; 8-instr_effect test 3 pins the pattern).
fn pop16(cpu: &mut Cpu, bus: &mut impl Bus) -> u16 {
    let lo = bus.read_inc(cpu.regs.sp);
    cpu.regs.sp = cpu.regs.sp.wrapping_add(1);
    let hi = bus.read_inc(cpu.regs.sp);
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
            let c = flag(cpu, flags::C);
            alu_add(cpu, v, c);
        }
        2 => alu_sub(cpu, v, false, true),
        3 => {
            let c = flag(cpu, flags::C);
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
    let c = flag(cpu, flags::C);
    set_flags(cpu, r == 0, false, v & 0x0F == 0x0F, c);
    r8_set(cpu, bus, idx, r);
}

fn op_dec_r8(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8) {
    let v = r8_get(cpu, bus, idx);
    let r = v.wrapping_sub(1);
    let c = flag(cpu, flags::C);
    set_flags(cpu, r == 0, true, v & 0x0F == 0, c);
    r8_set(cpu, bus, idx, r);
}

/// ADD HL,rp: Z preserved, H from bit 11, C from bit 15. One internal cycle.
fn op_add_hl(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8) {
    let hl = cpu.regs.hl();
    let v = rp_get(cpu, idx);
    let (r, carry) = hl.overflowing_add(v);
    let h = (hl & 0x0FFF) + (v & 0x0FFF) > 0x0FFF;
    let z = flag(cpu, flags::Z);
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
    let n = flag(cpu, flags::N);
    let h = flag(cpu, flags::H);
    let mut c = flag(cpu, flags::C);
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
    if !cpu.ime && !cpu.ime_pending {
        if bus.pending_halt_entry() != 0 {
            cpu.halt_bug = true;
        } else {
            cpu.halted = true;
        }
        return;
    }
    // #11bf — IME=1 (or EI-pending) with IE & IF already nonzero at the
    // entry view: the halt is NOT entered and PC rewinds to the HALT
    // itself, so the dispatched ISR returns INTO the halt and it
    // re-executes with the IF bit consumed (SameBoy halt()
    // sm83_cpu.c:1043-1047: `halted = false; pc--`). The prior
    // halted+first-check-wake path pushed halt+1 — the ISR skipped the
    // re-halt and the whole post-wake stream ran one halt round early
    // (`late_m0int_halt_m0stat_*` dual-traced). Tier2-gated inside
    // `halt_entry_rewind` (production keeps the halted+wake shape).
    if bus.halt_entry_rewind() {
        cpu.regs.pc = cpu.regs.pc.wrapping_sub(1);
        return;
    }
    cpu.halted = true;
}

fn op_stop(cpu: &mut Cpu, bus: &mut impl Bus) {
    // Per the STOP flowchart in Pan Docs ("Using the STOP Instruction"):
    // with no interrupt pending STOP is a 2-byte opcode and the byte after
    // it is skipped — at the cost of a real read M-cycle, performed inside
    // `Bus::stop` (SameBoy sm83_cpu.c stop(): `cycle_read(gb, gb->pc++)`
    // gated on no pending interrupt); with IE & IF != 0 it stays a 1-byte
    // opcode with no read. Branches not modelled (the joypad input state
    // is not visible through `Bus`): a held button turns STOP into a
    // 1-byte HALT (or a plain 1-byte NOP if an interrupt is also pending),
    // and a pending interrupt with IME=1 while a speed switch is armed
    // glitches the CPU non-deterministically.
    let pending = bus.pending() != 0;
    let skipped = cpu.regs.pc;
    if !pending {
        cpu.regs.pc = cpu.regs.pc.wrapping_add(1);
    }
    // true: the bus performed an armed CGB speed switch (including the
    // CPU pause while the rest of the machine runs — see `Bus::stop`) and
    // execution continues. false: deep stop; the CPU sleeps like in halt
    // mode until the joypad wakes it (see `step`).
    if !bus.stop(skipped, pending) {
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
        // The 16-bit inc/dec unit adjusts HL in the same M-cycle as the
        // read, which selects the OAM bug's "read during increase"
        // pattern when HL is in $FE00-$FEFF (SameBoy v0.12.1 ld_a_dhli/
        // ld_a_dhld; blargg oam_bug/8-instr_effect test 5). The HL+/-
        // *store* variants above stay plain writes: the write-pattern
        // corruption already covers them and no distinct write+increase
        // pattern is documented.
        0x2A => {
            let hl = cpu.regs.hl();
            cpu.regs.a = bus.read_inc(hl);
            cpu.regs.set_hl(hl.wrapping_add(1));
        }
        0x3A => {
            let hl = cpu.regs.hl();
            cpu.regs.a = bus.read_inc(hl);
            cpu.regs.set_hl(hl.wrapping_sub(1));
        }
        // INC/DEC rp: one internal cycle, no flags. The *pre*-op value
        // rides the address bus during that cycle — blargg oam_bug/
        // 2-causes corrupts on INC DE from $FE00, 3-non_causes is clean
        // from $FDFF (and on DEC DE from $FF00).
        0x03 | 0x13 | 0x23 | 0x33 => {
            let i = (op >> 4) & 3;
            let v = rp_get(cpu, i);
            rp_set(cpu, i, v.wrapping_add(1));
            bus.tick_addr(v);
        }
        0x0B | 0x1B | 0x2B | 0x3B => {
            let i = (op >> 4) & 3;
            let v = rp_get(cpu, i);
            rp_set(cpu, i, v.wrapping_sub(1));
            bus.tick_addr(v);
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
            let c_in = u8::from(flag(cpu, flags::C));
            let c = cpu.regs.a & 0x80 != 0;
            cpu.regs.a = cpu.regs.a << 1 | c_in;
            set_flags(cpu, false, false, false, c);
        }
        0x1F => {
            let c_in = u8::from(flag(cpu, flags::C));
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
            let z = flag(cpu, flags::Z);
            let c = flag(cpu, flags::C);
            set_flags(cpu, z, true, true, c);
        }
        0x37 => {
            let z = flag(cpu, flags::Z);
            set_flags(cpu, z, false, false, true);
        }
        0x3F => {
            let z = flag(cpu, flags::Z);
            let c = flag(cpu, flags::C);
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
        // PUSH: internal cycle *before* the writes (POP has none). The
        // not-yet-decremented SP rides the address bus during it — the
        // 16-bit dec unit is preparing the push (SameBoy push_rr; blargg
        // oam_bug/2-causes test 9 corrupts on PUSH with SP=$FE00).
        0xC5 | 0xD5 | 0xE5 | 0xF5 => {
            bus.tick_addr(cpu.regs.sp);
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
        // CALL nn: fetch, lo, hi, internal, push hi, push lo. Like PUSH,
        // the internal cycle drives SP (SameBoy call_a16/call_cc_a16).
        0xCD => {
            let nn = imm16(cpu, bus);
            bus.tick_addr(cpu.regs.sp);
            let pc = cpu.regs.pc;
            push16(cpu, bus, pc);
            cpu.regs.pc = nn;
        }
        0xC4 | 0xCC | 0xD4 | 0xDC => {
            let nn = imm16(cpu, bus);
            if condition(cpu, (op >> 3) & 3) {
                bus.tick_addr(cpu.regs.sp);
                let pc = cpu.regs.pc;
                push16(cpu, bus, pc);
                cpu.regs.pc = nn;
            }
        }
        // RST: same tail as CALL (internal, push hi, push lo)
        0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
            bus.tick_addr(cpu.regs.sp);
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
        // LD SP,HL: the internal cycle drives the transferred value
        // (SameBoy ld_sp_hl passes the new SP to its OAM bug check).
        0xF9 => {
            cpu.regs.sp = cpu.regs.hl();
            bus.tick_addr(cpu.regs.sp);
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
    let c_in = u8::from(flag(cpu, flags::C));
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
            let c = flag(cpu, flags::C);
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
#[path = "execute_tests.rs"]
mod tests;
