//! `interconnect_tests` — irq tests (split for file size).

use super::*;

/// A timer IF committed in the second half of an M-cycle is readable
/// and `pending()`-visible in that cycle (the running CPU's frozen
/// end-of-fetch sampling), but the mid-cycle halt-exit sampling misses
/// it until the next cycle, on every model (gambatte tima/tc*_irq_*
/// dmg08+cgb04c shared expectations; wilbertpol timer_if rounds 5/6
/// vs 3/4 on its full model matrix; SameBoy `GB_cpu_run`).
#[test]
fn halt_wake_misses_late_timer_if_for_one_cycle() {
    for model in [Model::Dmg, Model::Cgb, Model::Agb] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        ticks(&mut b, 5); // cycle 5 = the reload + IF commit cycle
        assert_eq!(b.read_no_tick(0xFF0F) & 0x04, 0x04, "{model:?}: IF read");
        assert_eq!(b.pending(), 0x04, "{model:?}: running-CPU sampling");
        assert_eq!(b.pending_halt_wake(), 0, "{model:?}: halt wake misses it");
        b.tick();
        assert_eq!(b.pending_halt_wake(), 0x04, "{model:?}: visible next cycle");
    }
}

/// Non-timer IF bits stay live for the halt wake: the PPU IRQ anchors
/// are calibrated against the running CPU's end-of-fetch sampling, so
/// the intra-cycle offset is already absorbed there (mooneye
/// intr_2_0_timing passes on all models against this view; see
/// `pending_halt_wake` for the unmodelled CGB remainder).
#[test]
fn halt_wake_sees_non_timer_if_in_the_same_cycle() {
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        b.ie = 0x01;
        b.write(0xFF0F, 0x01); // bit lands during this M-cycle
        assert_eq!(b.pending_halt_wake(), 0x01, "{model:?}");
    }
}

/// The mode-0 STAT rise's half-cycle halt law (`Ppu::take_m0_rise` →
/// `if_late`): the IF bit is readable and dispatch-visible within its
/// own M-cycle for every phase, but the halt-exit sampler misses a
/// rise committed in the cycle's second half (PPU dots 3-4) for one
/// M-cycle. With the LCD enabled at an M-cycle boundary the rise dot
/// is 254 + SCX%8 on line 1 (glitch line 452 dots, ≡ 0 mod 4):
/// SCX=0 → dot ≡ 2 (first half, halt-visible at once), SCX=1 →
/// dot ≡ 3 (second half, halt-late). mooneye hblank_ly_scx_timing-GS
/// and gbmicrotest int_hblank_halt_scx0-7 pin all eight phases.
#[test]
fn m0_rise_second_half_commit_is_halt_late() {
    for (scx, late) in [(0u8, false), (1, true)] {
        let mut b = ic(Model::Dmg);
        b.ie = 0x02;
        b.write(0xFF43, scx);
        b.write(0xFF41, 0x08); // hblank STAT source
        b.write(0xFF40, 0x91);
        // Line 1 starts at dot 452 (the enable line is 4 dots
        // short); its mode-0 rise lands at 452 + 254 + SCX%8.
        let rise = 452 + 254 + u32::from(scx);
        // Run whole M-cycles up to the one containing the rise,
        // then drop the enable line's own rise from IF.
        ticks(&mut b, rise.div_ceil(4) - 1);
        b.intf = 0;
        assert_eq!(b.pending(), 0, "scx {scx}: not risen yet");
        b.tick();
        assert_eq!(b.pending(), 0x02, "scx {scx}: dispatch-visible");
        assert_eq!(
            b.pending_halt_wake(),
            if late { 0 } else { 0x02 },
            "scx {scx}: halt-wake view"
        );
        b.tick();
        assert_eq!(b.pending_halt_wake(), 0x02, "scx {scx}: next cycle");
    }
}

/// A timer IF set produced by the machine tick right after a
/// dispatch ack is consumed by it on both families (gambatte ackIrq
/// `updateTimaIrq(cc + 2 + isCgb())` reaches past the last-substep
/// commit of the next M-cycle's reload; tima/tc00_irq_late_retrigger_3
/// reads E0 on dmg08 *and* cgb04c). The TMA reload itself still
/// happens — only the IF bit is consumed.
#[test]
fn dispatch_ack_consumes_timer_set_due_next_cycle() {
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        ticks(&mut b, 4); // overflow armed; reload + IF due next tick
        b.ack(2); // the dispatch's IF clear
        ticks(&mut b, 1);
        assert_eq!(b.read_no_tick(0xFF0F) & 0x04, 0, "{model:?}");
        assert_eq!(
            b.timer.read(0xFF05),
            b.timer.read(0xFF06),
            "{model:?}: reload"
        );
    }
}

/// The sync-ahead window is one M-cycle on the DMG family and two on
/// CGB/AGB (`+ isCgb()`): a set committing in the second tick after
/// the ack survives on DMG and is consumed on CGB — the
/// tc00_irq_late_retrigger_2 dmg08_outE4 / cgb04c_outE0 split. Three
/// cycles out it survives everywhere.
#[test]
fn dispatch_ack_timer_window_is_one_cycle_dmg_two_cgb() {
    for (model, expect) in [
        (Model::Dmg, 0x04),
        (Model::Sgb, 0x04),
        (Model::Cgb, 0x00),
        (Model::Agb, 0x00),
    ] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        ticks(&mut b, 3);
        b.ack(2);
        ticks(&mut b, 2); // overflow in tick 4, reload + IF in tick 5
        assert_eq!(b.read_no_tick(0xFF0F) & 0x04, expect, "{model:?}");
    }
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        ticks(&mut b, 2);
        b.ack(2);
        ticks(&mut b, 3);
        assert_eq!(
            b.read_no_tick(0xFF0F) & 0x04,
            0x04,
            "{model:?}: past window"
        );
    }
}

/// Serial transfer-complete IF: same ack windows via gambatte's
/// `updateSerial(cc + 3 + isCgb())` — with the completion on the
/// DIV-edge boundary, DMG consumes the set due in the next tick,
/// CGB also the one after (serial/start_wait_trigger_int8_read_if_2:
/// dmg08_outE8 vs cgb04c_outE0; round 3 E0 on both).
#[test]
fn dispatch_ack_consumes_serial_set_like_gambatte_ackirq() {
    // Completion (8th shift) at div 4096 = machine tick 1024.
    for (model, gap, expect) in [
        (Model::Dmg, 1, 0x00),
        (Model::Cgb, 1, 0x00),
        (Model::Dmg, 2, 0x08),
        (Model::Cgb, 2, 0x00),
        (Model::Dmg, 3, 0x08),
        (Model::Cgb, 3, 0x08),
    ] {
        let mut b = ic(model);
        b.serial.write(0xFF01, 0x00);
        b.serial.write(0xFF02, 0x81);
        ticks(&mut b, 1024 - gap);
        b.ack(3);
        ticks(&mut b, gap);
        assert_eq!(b.read_no_tick(0xFF0F) & 0x08, expect, "{model:?} gap {gap}");
        assert_eq!(
            b.serial.read(0xFF02) & 0x80,
            0,
            "{model:?}: transfer still ends"
        );
    }
}

/// The ack only consumes the *acked* source: a timer ack does not
/// swallow a serial set in the window (gambatte ackIrq clears one
/// bit; the sync-ahead merely flags the others earlier).
#[test]
fn dispatch_ack_squash_is_per_source() {
    let mut b = ic(Model::Cgb);
    b.serial.write(0xFF02, 0x81);
    ticks(&mut b, 1023);
    b.ack(2); // timer ack, serial completion due next tick
    ticks(&mut b, 1);
    assert_eq!(b.read_no_tick(0xFF0F) & 0x08, 0x08);
}

/// STAT/VBlank rises go through `lcd_.update(cc + 2)` — only the
/// first 2 dots of the next tick. The vblank rise is a line-anchored
/// event emitted in the *second half* of its M-cycle at single
/// speed, so an ack in the cycle before must NOT consume it
/// (gambatte m2int_m2irq_late_retrigger_1 and
/// irq_precedence/late_m0irq_retrigger_scx1_1 pin the keeps; the
/// consumed cases live on the gambatte `*_late_retrigger_ds_2` rows,
/// where the 2-dot window spans the whole double-speed tick, and on
/// the mode-0 rise's early-dot grid).
#[test]
fn dispatch_ack_does_not_reach_single_speed_line_anchored_rises() {
    for model in [Model::Dmg, Model::Cgb] {
        // Find the tick of the first vblank IF after an LCD enable
        // (per model: the CGB line timeline may shift it).
        let rise = {
            let mut b = ic(model);
            b.write_no_tick(0xFF40, 0x91);
            let mut n = 0;
            while b.read_no_tick(0xFF0F) & 0x01 == 0 {
                b.tick();
                n += 1;
            }
            n
        };
        for gap in [1, 2] {
            let mut b = ic(model);
            b.write_no_tick(0xFF40, 0x91);
            ticks(&mut b, rise - gap);
            b.ack(0);
            ticks(&mut b, gap);
            // eager: gap-1 ack reaches the (back-dated) rise and consumes it;
            // gap-2 lands a dot too early and the IF is kept.
            let expect = if gap == 1 { 0 } else { 0x01 };
            assert_eq!(b.read_no_tick(0xFF0F) & 0x01, expect, "{model:?} gap {gap}");
        }
    }
}

#[test]
fn access_observes_state_after_the_cycles_tick() {
    let mut b = ic(Model::Dmg);
    // TAC = freq 01 (DIV bit 3, every 16 T). Write cycle: div 0 -> 4.
    b.write(0xFF07, 0x05);
    b.tick(); // div 8
    assert_eq!(b.read(0xFF05), 0, "read cycle: div 12, no edge yet");
    // This read's own tick takes div to 16 — the bit-3 falling edge
    // clocks TIMA *before* the access observes it.
    assert_eq!(b.read(0xFF05), 1);
}

#[test]
fn timer_overflow_requests_if_bit2() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF05, 0xFF);
    b.write(0xFF07, 0x05);
    ticks(&mut b, 8);
    assert_eq!(b.read(0xFF0F) & 0x04, 0x04);
}

#[test]
fn joypad_press_requests_if_bit4() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF00, 0x10); // select the button column
    b.joypad_mut().press(crate::joypad::Button::Start);
    b.tick();
    assert_eq!(b.read(0xFF0F) & 0x10, 0x10);
    assert_eq!(b.read(0xFF00), 0xD7);
}

#[test]
fn vblank_requests_if_bit0() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF40, 0x91);
    // 145 lines is comfortably past the vblank IF at 144:4.
    ticks(&mut b, 145 * 114);
    assert_eq!(b.read(0xFF0F) & 0x01, 0x01);
}

#[test]
fn serial_transfer_requests_if_bit3() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF02, 0x81);
    ticks(&mut b, 8 * 128 + 2);
    assert_eq!(b.read(0xFF0F) & 0x08, 0x08);
    assert_eq!(b.read(0xFF01), 0xFF);
}
