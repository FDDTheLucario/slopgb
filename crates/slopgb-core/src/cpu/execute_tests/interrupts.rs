//! `execute_tests` — interrupts tests (split for file size).

use super::*;

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
            Read(PC0 + 2, 0x04), // aborted fetch: discarded, PC kept
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
            Read(PC0 + 2, 0xF3), // aborted fetch: DI is discarded
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
        [
            Read(PC0, 0xD9),
            ReadInc(SP0, 0x00),
            ReadInc(SP0 + 1, 0xC1),
            Tick
        ]
    );
    assert!(c.ime);
    assert_eq!(c.regs.pc, 0xC100);
    step(&mut c, &mut b); // immediate dispatch, return address 0xC100
    assert_eq!(
        b.log,
        [
            Read(0xC100, 0x00), // aborted fetch at the return address
            Tick,
            Tick,
            Write(SP0 + 1, 0xC1),
            Write(SP0, 0x00),
            Read(0x0040, 0x00)
        ]
    );
}

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
            Read(0xC123, 0x00), // aborted fetch: discarded, PC kept
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
            Read(0x0212, 0x00), // aborted fetch
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
            Read(0x0212, 0x00), // aborted fetch
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
            Read(0x0212, 0x00), // aborted fetch
            Tick,
            Tick,
            Write(0xFFFF, 0x02),
            Write(0xFFFE, 0x12),
            Read(0x0048, 0x00) // STAT vector
        ]
    );
    assert_eq!(b.mem[0xFF0F], 0x01); // STAT acked, vblank still pending
}

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
    // halted: one discarded prefetch read per idle M-cycle
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x00), Read(PC0 + 1, 0x00)]);
    b.mem[0xFF0F] = 0x04;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0 + 1, 0x00), // prefetch observing IF, aborted
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
    assert_eq!(b.take_log(), [Read(PC0, 0x76), Read(PC0 + 1, 0x04)]);
    b.mem[0xFF0F] = 0x04;
    step(&mut c, &mut b);
    // wakes with no extra delay and just executes the next instruction
    assert_eq!(b.log, [Read(PC0 + 1, 0x04)]);
    assert_eq!(c.regs.b, 1);
    assert_eq!(b.mem[0xFF0F], 0x04); // IF not acked
    assert!(!c.ime);
}

#[test]
fn halt_ime1_dispatch_reuses_the_if_raising_cycle_as_prefetch() {
    // Servicing an interrupt out of HALT takes "exactly same timing as
    // if a long series of NOP instructions were used to wait for the
    // interrupt" (mooneye acceptance/halt_ime1_timing2-GS, verified on
    // DMG/MGB/SGB/SGB2). In the NOP case the M-cycle whose tick raises
    // IF is the aborted prefetch and the vector fetch lands 5 M-cycles
    // later; the halt idle cycle that observes IF must therefore play
    // the same role - no extra fetch cycle in between.
    let mut c = cpu();
    c.ime = true;
    let mut b = bus(&[0x76]); // HALT
    b.mem[0xFFFF] = 0x04; // timer interrupt enabled
    step(&mut c, &mut b); // cycle 0: HALT fetch
    assert_eq!(b.take_log(), [Read(PC0, 0x76)]);
    assert!(c.halted);
    b.raise_if = Some((3, 0x04)); // IF.2 rises during cycle 3
    step(&mut c, &mut b); // cycle 1: idle
    step(&mut c, &mut b); // cycle 2: idle
    assert!(c.halted);
    b.take_log();
    // Cycle 3 sees IF rise -> it is the aborted prefetch; dispatch
    // follows in the same step and the vector fetch is cycle 8 = 3+5.
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 1, 0x00),  // cycle 3: IF rises during this cycle
            Tick,                 // cycle 4
            Tick,                 // cycle 5
            Write(SP0 - 1, 0xC0), // cycle 6
            Write(SP0 - 2, 0x01), // cycle 7: return addr = after HALT
            Read(0x0050, 0x00),   // cycle 8: vector fetch, IF cycle + 5
        ]
    );
    assert!(!c.halted);
    assert!(!c.ime);
    assert_eq!(b.mem[0xFF0F], 0); // acked
    assert_eq!(c.regs.pc, 0x0051);
}

#[test]
fn halt_ime0_wake_fetch_is_the_if_raising_cycle() {
    // IME=0 wake: HALT continues "with exactly same timing as if a long
    // series of NOP instructions were used" (mooneye acceptance/
    // halt_ime0_nointr_timing, verified on all models): the cycle whose
    // tick raises IF would be a NOP fetch in the wait loop, so out of
    // halt it is already the next instruction's opcode fetch.
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // cycle 0: HALT fetch
    b.raise_if = Some((2, 0x04)); // IF.2 rises during cycle 2
    step(&mut c, &mut b); // cycle 1: idle
    assert!(c.halted);
    b.take_log();
    step(&mut c, &mut b); // cycle 2: fetches and executes INC B
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x04)]);
    assert!(!c.halted);
    assert_eq!(c.regs.b, 1);
    assert_eq!(b.mem[0xFF0F], 0x04); // not acked: no dispatch
}

#[test]
fn halt_ime1_late_if_commit_wakes_one_cycle_later() {
    // A timer IF committed on the last T-substep of an M-cycle is missed
    // by that cycle's halt-exit sampling (Bus::pending_halt_wake): the
    // *next* idle prefetch becomes the aborted dispatch prefetch, so the
    // vector fetch lands 6 M-cycles after the commit cycle instead of 5
    // (gambatte tima/tc*_irq_*; wilbertpol acceptance/timer/timer_if
    // rounds 5/6 — dispatch-from-HALT — vs rounds 3/4 — dispatch from a
    // NOP sled, which keeps the end-of-fetch sampling and +5).
    let mut c = cpu();
    c.ime = true;
    let mut b = bus(&[0x76]); // HALT
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // cycle 0: HALT fetch
    assert!(c.halted);
    b.late_if = true;
    b.raise_if = Some((3, 0x04)); // last-substep commit during cycle 3
    step(&mut c, &mut b); // cycle 1: idle
    step(&mut c, &mut b); // cycle 2: idle
    b.take_log();
    step(&mut c, &mut b); // cycle 3: IF commits too late to be seen
    assert!(c.halted, "the commit cycle's wake check misses the IF bit");
    step(&mut c, &mut b); // cycle 4: wake -> aborted prefetch + dispatch
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 1, 0x00),  // cycle 3: still idle
            Read(PC0 + 1, 0x00),  // cycle 4: aborted dispatch prefetch
            Tick,                 // cycle 5
            Tick,                 // cycle 6
            Write(SP0 - 1, 0xC0), // cycle 7
            Write(SP0 - 2, 0x01), // cycle 8: return addr = after HALT
            Read(0x0050, 0x00),   // cycle 9: vector fetch = commit + 6
        ]
    );
    assert!(!c.halted);
    assert_eq!(b.mem[0xFF0F], 0); // acked
}

#[test]
fn halt_ime0_late_if_commit_also_wakes_one_cycle_later() {
    // The same intra-cycle sample feeds the IME=0 resume (SameBoy
    // sm83_cpu.c `GB_cpu_run`: one `interrupt_queue` sample serves both
    // wake paths): a late IF commit keeps the IME=0 halt asleep for one
    // more idle cycle too. Mooneye halt_ime0_nointr_timing stays exact
    // because its wake source is the vblank IF, which is not a late
    // commit (it also anchors its DIV reset with the same one-cycle
    // shift, so even a shifted source would cancel out there).
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // cycle 0: HALT fetch
    b.late_if = true;
    b.raise_if = Some((2, 0x04)); // second-half commit during cycle 2
    step(&mut c, &mut b); // cycle 1: idle
    assert!(c.halted);
    b.take_log();
    step(&mut c, &mut b); // cycle 2: commit invisible to the wake check
    assert!(c.halted, "the commit cycle's wake check misses the IF bit");
    step(&mut c, &mut b); // cycle 3: wakes and executes INC B
    assert_eq!(
        b.take_log(),
        [Read(PC0 + 1, 0x04), Read(PC0 + 1, 0x04)] // cycles 2 and 3
    );
    assert!(!c.halted);
    assert_eq!(c.regs.b, 1);
    assert_eq!(b.mem[0xFF0F], 0x04); // not acked: no dispatch
}

#[test]
fn halt_gates_core_clock_after_the_post_halt_prefetch() {
    // The core clock gate (Bus::set_halted — the OAM DMA controller
    // freezes with it) engages only after the post-HALT prefetch
    // M-cycle, not on HALT execution: the SM83 prefetches the next
    // opcode before sleeping (gbctr halt bug prefetch), and madness/
    // mgb_oam_dma_halt_sprites.s pins the hardware OAM DMA freeze two
    // M-cycles after the HALT opcode fetch.
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // cycle 0: HALT fetch
    assert!(c.halted);
    assert!(b.halt_calls.is_empty(), "no gate during HALT itself");
    step(&mut c, &mut b); // cycle 1: idle prefetch, then the gate engages
    assert_eq!(b.halt_calls, [(2, true)]);
    step(&mut c, &mut b); // cycle 2: idle, gate stays on (idempotent)
    step(&mut c, &mut b); // cycle 3
    assert_eq!(b.halt_calls, [(2, true)]);
    b.raise_if = Some((4, 0x04)); // IF.2 rises during cycle 4
    step(&mut c, &mut b); // cycle 4 observes IF: wake, gate released
    assert!(!c.halted);
    assert_eq!(b.halt_calls, [(2, true), (5, false)]);
    assert_eq!(c.regs.b, 1, "woke into INC B");
}

#[test]
fn halt_with_immediate_wake_never_gates_the_clock() {
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // HALT fetch
    b.raise_if = Some((1, 0x04)); // IF rises during the first prefetch
    step(&mut c, &mut b); // prefetch observes IF: immediate wake
    assert!(!c.halted);
    assert!(b.halt_calls.is_empty(), "clock never stopped");
}

#[test]
fn stop_gates_core_clock_after_the_first_idle_cycle() {
    // Stop mode switches the same core clock gate (and so freezes an
    // in-flight OAM DMA the same way halt mode does).
    let mut c = cpu();
    let mut b = bus(&[0x10, 0x00, 0x04]); // STOP; (skipped); INC B
    step(&mut c, &mut b); // cycle 0: STOP fetch
    assert!(c.stopped);
    assert!(b.halt_calls.is_empty(), "no gate during STOP itself");
    step(&mut c, &mut b); // cycle 1: idle tick, then the gate engages
    assert_eq!(b.halt_calls, [(2, true)]);
    step(&mut c, &mut b); // cycle 2: idle
    assert_eq!(b.halt_calls, [(2, true)]);
    // Joypad wake: the gate is released before execution resumes.
    b.mem[0xFFFF] = 0x10;
    b.mem[0xFF0F] = 0x10;
    step(&mut c, &mut b); // wakes, executes INC B
    assert!(!c.stopped);
    assert_eq!(b.halt_calls, [(2, true), (3, false)]);
    assert_eq!(c.regs.b, 1);
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
            Read(PC0 + 2, 0x00), // aborted fetch after the halt wakes
            Tick,
            Tick,
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x02),
            Read(0x0040, 0x00)
        ]
    );
}

#[test]
fn halted_cpu_consumes_idle_prefetch_cycles() {
    // Each halted M-cycle is a discarded prefetch read of PC (so the
    // cycle that eventually observes IF can double as the real fetch).
    let mut c = cpu();
    c.halted = true;
    let mut b = TestBus::new();
    for _ in 0..5 {
        step(&mut c, &mut b);
    }
    assert_eq!(b.log, [Read(PC0, 0); 5]);
    assert!(c.halted);
}

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
    // The bus is told where the skipped byte lives (it performs that
    // byte's read M-cycle itself) and that no interrupt was pending.
    assert_eq!(b.stop_args, Some((PC0 + 1, false)));
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
    // The bus learns the interrupt was pending: no skipped-byte read
    // M-cycle, and an armed speed switch would happen with no pause
    // (SameBoy stop() gates both on !interrupt_pending).
    assert_eq!(b.stop_args, Some((PC0 + 1, true)));
    // The already-pending interrupt also ends the stop immediately
    // (IME=0, so it is not dispatched).
    step(&mut c, &mut b);
    assert!(!c.stopped);
    assert_eq!(c.regs.b, 1);
}

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

/// The lock state is exposed to harnesses (wilbertpol's mooneye fork ends
/// its tests with 0xED) and is distinct from the LD B,B breakpoint.
#[test]
fn illegal_opcode_reports_debug_undefined_hit() {
    let mut c = cpu();
    let mut b = bus(&[0xED]);
    assert!(!c.debug_undefined_hit());
    step(&mut c, &mut b);
    assert!(c.debug_undefined_hit());
    assert!(!c.debug_breakpoint_hit());
}
