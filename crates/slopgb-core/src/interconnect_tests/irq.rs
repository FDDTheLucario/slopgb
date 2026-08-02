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

/// The pure-LYC rise's half-cycle halt law (`Ppu::take_lyc_rise` →
/// `if_late`), CGB single speed. The coincidence fires at the line's dot 4,
/// the second half of that M-cycle, so the halt-exit sampler misses it for
/// one cycle — except on line 153, whose `ly_for_comparison` table matches
/// two dots later (dot 6, the first half) and wakes at once. SameBoy lands
/// the LYC=1, 144 and 153 dispatches on ONE dot; without the mask the
/// ordinary anchors wake a cycle early and their ISR reads FF41 four dots
/// short (gambatte dma/{g,h}dma_cycles_*).
#[test]
fn lyc_rise_second_half_commit_is_halt_late_cgb() {
    for (lyc, late) in [(1u8, true), (144, true), (153, false)] {
        let mut b = ic(Model::Cgb);
        b.ie = 0x02;
        b.write(0xFF45, lyc);
        b.write(0xFF41, 0x40); // LYC STAT source
        b.write(0xFF40, 0x91);
        // Advance whole M-cycles to the coincidence, dropping every other
        // rise so only the LYC edge is left standing.
        let mut fired = false;
        for _ in 0..20_000 {
            b.intf = 0;
            b.tick();
            if b.pending() == 0x02 {
                fired = true;
                break;
            }
        }
        assert!(fired, "lyc {lyc}: coincidence fired");
        assert_eq!(
            b.pending_halt_wake(),
            if late { 0 } else { 0x02 },
            "lyc {lyc}: halt-wake view"
        );
        b.tick();
        assert_eq!(b.pending_halt_wake(), 0x02, "lyc {lyc}: visible next cycle");
    }
}

/// The pure-LYC halt mask is CGB single-speed only: the DMG family emits its
/// line-153 LYC on the dispatch frame already (`stat_irq/reclock.rs`) and a
/// double-speed M-cycle is two dots, so neither carries the half split.
#[test]
fn lyc_rise_halt_mask_is_cgb_single_speed_only() {
    let mut b = ic(Model::Dmg);
    b.ie = 0x02;
    b.write(0xFF45, 1);
    b.write(0xFF41, 0x40);
    b.write(0xFF40, 0x91);
    for _ in 0..20_000 {
        b.intf = 0;
        b.tick();
        if b.pending() == 0x02 {
            break;
        }
    }
    assert_eq!(b.pending_halt_wake(), 0x02, "DMG LYC rise is never masked");
}

/// The timer sync-ahead window measured from the acknowledge: zero machine
/// ticks on the DMG family and one on CGB/AGB (gambatte ackIrq
/// `updateTimaIrq(cc + 2 + isCgb())`, taken from the acknowledge's position
/// two T-cycles into the PC-low push). A reload IF committing in the tick
/// right after the acknowledge therefore survives on DMG and is consumed on
/// CGB; two ticks out it survives everywhere. The TMA reload itself always
/// happens — only the IF bit is consumed.
#[test]
fn dispatch_ack_timer_window_is_zero_dmg_one_cgb() {
    for (model, gap, expect) in [
        (Model::Dmg, 1, 0x04),
        (Model::Sgb, 1, 0x04),
        (Model::Cgb, 1, 0x00),
        (Model::Agb, 1, 0x00),
        (Model::Dmg, 2, 0x04),
        (Model::Cgb, 2, 0x04),
    ] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        // The overflow is armed by tick 4 and the reload + IF commit one
        // tick later, so acking `gap` ticks before that puts the set in the
        // `gap`-th tick after the acknowledge.
        ticks(&mut b, 5 - gap);
        b.ack(2);
        ticks(&mut b, gap);
        assert_eq!(b.read_no_tick(0xFF0F) & 0x04, expect, "{model:?} gap {gap}");
        assert_eq!(
            b.timer.read(0xFF05),
            b.timer.read(0xFF06),
            "{model:?}: reload"
        );
    }
}

/// Serial transfer-complete IF: the same windows as the timer via gambatte's
/// `updateSerial(cc + 3 + isCgb())` — with the completion on the DIV-edge
/// boundary, CGB consumes the set due in the tick after the acknowledge and
/// DMG consumes nothing (serial/start_wait_trigger_int8_read_if_2:
/// dmg08_outE8 vs cgb04c_outE0).
#[test]
fn dispatch_ack_consumes_serial_set_like_gambatte_ackirq() {
    // Completion (8th shift) at div 4096 = machine tick 1024.
    for (model, gap, expect) in [
        (Model::Dmg, 1, 0x08),
        (Model::Cgb, 1, 0x00),
        (Model::Dmg, 2, 0x08),
        (Model::Cgb, 2, 0x08),
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

/// STAT/VBlank rises are consumed only within the two dots left of the
/// acknowledge's own M-cycle at single speed. The vblank rise is a
/// line-anchored event emitted in the *second half* of its M-cycle, so an
/// acknowledge two whole cycles earlier never reaches it (gambatte
/// m2int_m2irq_late_retrigger_1 and
/// irq_precedence/late_m0irq_retrigger_scx1_1 pin the keeps; the consumed
/// cases live on the `*_late_retrigger_ds_2` rows, where the window spans the
/// whole double-speed tick, and on the mode-0 rise's early-dot grid).
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
            // The DMG vblank rise lands in the first 2 dots of the tick after
            // a gap-1 acknowledge, inside the window; the CGB line timeline
            // puts its rise a dot further in, past the window. Gap 2 is a
            // whole cycle further out on both and the IF is kept.
            let expect = if gap == 1 && !model.is_cgb() { 0 } else { 0x01 };
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
