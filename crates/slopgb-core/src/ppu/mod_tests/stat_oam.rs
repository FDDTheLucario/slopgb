//! `mod_tests` — stat tests: OAM + vblank-pulse group (split for file size).

use super::*;

#[test]
fn line0_oam_irq_is_readable_but_dispatch_late() {
    for model in [Model::Dmg, Model::Cgb] {
        let mut p = Ppu::new(model);
        p.write(0xFF41, 0x20); // OAM source only
        p.write(0xFF40, 0x81);
        // Normal line: the pulse commits at dot 0 (CGB: dot 1 — a
        // line-start write still reaches it, see `stat_events_tick`;
        // both land within the same M-cycle) — a second-half commit,
        // so it misses the dispatch sample of its own cycle too (the
        // mealybug m3_* photo handlers pin the anchor).
        run_to(&mut p, 0, 451);
        p.take_stat_late();
        let pulse = p.tick() | if model.is_cgb() { p.tick() } else { 0 };
        assert_eq!(pulse & IF_STAT, IF_STAT, "{model:?} line 1");
        assert!(
            !p.take_stat_late(),
            "{model:?} line-1 pulse dispatches in its own cycle (eager)"
        );
        // Line 0: the IF bit appears in the same M-cycle.
        run_to(&mut p, 0, 0);
        p.take_stat_late();
        assert_eq!(tick_n(&mut p, 4) & IF_STAT, IF_STAT, "{model:?} line 0");
        assert!(
            !p.take_stat_late(),
            "{model:?} line 0 rise dispatches in its own cycle (eager)"
        );
    }
}

#[test]
fn line0_oam_irq_blocked_by_vblank_enable() {
    // With the mode-1 source enable also set, the line-0 OAM rise
    // raises no IRQ at all; the line level still rises, so nothing
    // re-edges later in the OAM window.
    let mut p = dmg();
    p.write(0xFF41, 0x30); // OAM + VBLANK sources
    p.write(0xFF40, 0x81);
    run_to(&mut p, 150, 0);
    run_to(&mut p, 0, 0); // drain vblank-window IRQs
    assert_eq!(
        tick_n(&mut p, 84) & IF_STAT,
        0,
        "line 0 OAM rise is blocked while the vblank enable is set"
    );
    // The next line's pulse (at dot 0) is unaffected.
    let ifs = run_to(&mut p, 0, 455);
    assert_eq!(ifs & IF_STAT, 0, "nothing else fires during line 0");
    assert_eq!(p.tick() & IF_STAT, IF_STAT, "line-1 pulse at (1,0)");
}

#[test]
fn m1_event_blocked_by_oam_enable() {
    // gambatte mstat_irq.h doM1Event: the vblank STAT event at 144:4
    // is suppressed when the (delayed) m2 enable is set — the 144:0
    // OAM pulse is the only STAT IF of the vblank entry.
    let mut p = dmg();
    p.write(0xFF45, 200);
    p.write(0xFF41, 0x30); // OAM + VBLANK sources
    p.write(0xFF40, 0x81);
    run_to(&mut p, 143, 400);
    let ifs = run_to(&mut p, 144, 1);
    assert_eq!(ifs & IF_STAT, IF_STAT, "144:0 OAM pulse fires");
    let ifs = run_to(&mut p, 144, 8);
    assert_eq!(
        ifs & IF_STAT,
        IF_STAT,
        "eager: m1 event fires at 144:4 (not m2-blocked)"
    );
    assert_eq!(ifs & IF_VBLANK, IF_VBLANK, "vblank IF unaffected");
}

#[test]
fn vblank_if_at_144_dot4_and_frame_count_at_dot0() {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    assert_eq!(p.frame_count(), 0);
    let ifs = run_to(&mut p, 144, 0);
    assert_eq!(ifs & 1, 0, "no vblank IF before 144:4");
    assert_eq!(p.frame_count(), 1);
    tick_n(&mut p, 3);
    assert_eq!(p.tick() & 1, 1, "vblank IF at state(144,4)");
    // Exactly one vblank IF per frame.
    let ifs = run_to(&mut p, 144, 3);
    assert_eq!(ifs & 1, 0);
    assert_eq!(p.tick() & 1, 1);
    assert_eq!(p.frame_count(), 2);
}

#[test]
fn oam_irq_pulses_at_line_start() {
    let mut p = dmg();
    p.write(0xFF41, 0x20);
    p.write(0xFF40, 0x81);
    // No mode-2 source on the glitched line. On lines 1-143 the OAM
    // IRQ is an *event* committing at state(line,0) — the LY-increment
    // M-cycle, one M-cycle before the readable mode 2 (SameBoy
    // display.c: "The OAM STAT interrupt occurs 1 T-cycle before STAT
    // actually changes, except on line 0"; the gbmicrotest
    // oam_int_*/int_oam_* grids pin the cycle).
    let ifs = run_to(&mut p, 0, 451);
    assert_eq!(ifs & 2, 0, "no OAM source on the glitch line");
    assert_eq!(p.tick(), 0x02, "OAM IRQ pulse at state(1,0)");
    // The blocking level holds through scan+render: no second edge.
    assert_eq!(run_to(&mut p, 1, 300) & 2, 0);
    run_to(&mut p, 1, 455);
    assert_eq!(p.tick(), 0x02, "next pulse at state(2,0)");
}

#[test]
fn line_start_oam_pulse_is_halt_late() {
    // The dot-0 commit sits in the second half of its M-cycle: the
    // halt-exit sampler misses it for one cycle on every model
    // (gbmicrotest int_oam_* halt rows; wilbertpol intr_2_timing halt
    // rounds land one M-cycle after the IF rows on MGB and CGB alike).
    for model in [Model::Dmg, Model::Cgb] {
        let mut p = Ppu::new(model);
        p.write(0xFF41, 0x20);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 0, 451);
        p.take_stat_halt_late();
        let pulse = p.tick() | if model.is_cgb() { p.tick() } else { 0 };
        assert_eq!(pulse & 2, 2, "{model:?}: pulse at the (1,0) M-cycle");
        assert!(
            p.take_stat_halt_late(),
            "{model:?}: dot-0 pulse is halt-late"
        );
    }
}

#[test]
fn oam_level_blocks_lyc_edge_and_next_pulse() {
    let mut p = dmg();
    p.write(0xFF45, 2);
    p.write(0xFF41, 0x60); // LYC + OAM sources
    p.write(0xFF40, 0x81);
    run_to(&mut p, 1, 455); // drains line 1's own (1,0) pulse
    assert_eq!(p.tick() & 2, 2, "OAM pulse at (2,0)");
    // LYC=2 turns true at (2,4) under the OAM blocking level: no edge
    // (gambatte lycm2int shape). The LYC level then holds to the end
    // of line 2 and overlaps the (3,0) pulse, blocking it too.
    let ifs = run_to(&mut p, 3, 100);
    assert_eq!(ifs & 2, 0, "LYC edge and the (3,0) pulse both blocked");
}

#[test]
fn oam_enable_does_not_block_mode0_events() {
    // With both the OAM and hblank sources enabled, every visible
    // line's mode-0 event still fires: gambatte mstat_irq.h
    // doM0Event is blocked only by a matching delayed LYC, never by
    // the m2 enable (lcdirq_precedence/m0irq_ly44_lcdstat28 expects
    // the m0 IRQ with lcdstat $28), while the per-line m2 pulses
    // vanish (mode2IrqSchedule routes them to the line-0 slot while
    // m0en is set) — so exactly one IF per line, from the m0 event.
    let mut p = dmg();
    p.write(0xFF45, 200);
    p.write(0xFF41, 0x28); // hblank + OAM sources
    p.write(0xFF40, 0x81);
    let ifs = run_to(&mut p, 0, 252);
    assert_eq!(ifs & 2, 2, "glitch-line hblank event");
    run_to(&mut p, 1, 4);
    for line in 1..=10u8 {
        let ifs = run_to(&mut p, line, 250);
        assert_eq!(ifs & 2, 0, "line {line}: no IF before the m0 event");
        let ifs = run_to(&mut p, line + 1, 4);
        assert_eq!(ifs & 2, 2, "line {line}: m0 event fires under m2en");
    }
}

#[test]
fn oam_pulse_at_vblank_entry_dmg() {
    // 144-entry OAM pulse at 144:0, one M-cycle *before* the vblank IF
    // at 144:4, on the DMG family too (wilbertpol intr_2_timing rounds
    // 5-7; gbmicrotest line_144_oam_int_b/c/d). The DMG commit is
    // halt-late, which is what lets `vblank_stat_intr-GS` observe the
    // pulse and the vblank IF in the same halt-wake cycle.
    let mut p = dmg();
    p.write(0xFF41, 0x20);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 143, 455);
    p.take_stat_halt_late();
    p.take_stat_late();
    assert_eq!(p.tick(), 0x02, "OAM pulse at 144:0, before the vblank IF");
    assert!(p.take_stat_halt_late(), "DMG 144:0 pulse is halt-late");
    assert!(p.take_stat_late(), "DMG 144:0 pulse is dispatch-late too");
    tick_n(&mut p, 3);
    assert_eq!(p.tick() & 1, 1, "vblank IF at 144:4");
}

#[test]
fn oam_pulse_at_vblank_entry_cgb_not_halt_late() {
    let mut p = cgb();
    p.write(0xFF41, 0x20);
    p.write(0xFF40, 0x81);
    // Run past line 143's render (the OAM level falls at the visible
    // flip), then assert the vblank-entry pulse at 144:0. Unlike the
    // visible-line pulses, the CGB 144-entry commit is visible to the
    // halt-exit sampler in its own cycle (misc/ppu/vblank_stat_intr-C
    // measures it one cycle apart from the DMG family).
    run_to(&mut p, 143, 300);
    let ifs = run_to(&mut p, 143, 455);
    assert_eq!(ifs & 2, 0, "no OAM edge between the flip and 144:0");
    p.take_stat_halt_late();
    p.take_stat_late();
    assert_eq!(tick_n(&mut p, 2) & 2, 2, "CGB OAM pulse in the 144:0 cycle");
    assert!(!p.take_stat_halt_late(), "CGB 144:0 pulse is not halt-late");
    assert!(
        !p.take_stat_late(),
        "CGB 144:0 pulse dispatches in its own cycle"
    );
    tick_n(&mut p, 2);
    assert_eq!(p.tick() & 1, 1, "vblank IF 4 dots later");
}

#[test]
fn vblank_line_oam_pulses_dot12_dmg_only() {
    let mut p = dmg();
    p.write(0xFF41, 0x20);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 145, 11);
    assert_eq!(
        p.tick() & 2,
        0,
        "eager: dot-12 vblank OAM pulse deferred (no pulse at 145:12)"
    );
    run_to(&mut p, 146, 11);
    assert_eq!(p.tick() & 2, 0, "eager: no pulse at 146:12 either");

    let mut c = cgb();
    c.write(0xFF41, 0x20);
    c.write(0xFF40, 0x81);
    run_to(&mut c, 145, 0);
    let ifs = run_to(&mut c, 153, 450);
    assert_eq!(ifs & 2, 0, "CGB: no vblank-line OAM pulses");
}

/// Port Stage A10 — the vblank-ENTRY OAM (mode-2) STAT pulse on the flag-on
/// [`Ppu::stat_update_tick`] path. The `GB_STAT_update` rising-edge engine
/// mirrors `vis_mode` in vblank (mode 0 at 144:0-3, mode 1 from 144:4), so it
/// never selects the OAM source there and emits no 144:0 pulse — SameBoy raises
/// it as a direct `IF |= 2` poke (`display.c:2160`), not a line rise. These
/// flag-on variants of `oam_pulse_at_vblank_entry_dmg` / `_cgb_not_halt_late`
/// pin the pulse (and its DMG halt/dispatch-late masks, the CGB 144 exemption)
/// against the flag-off engine — the `vblank_stat_intr-GS`/`-C` lift. The DMG
/// 145-153 dot-12 pulses are deferred (measured net-negative flag-on, see
/// `Ppu::stat_update_vblank_oam_pulses`).
#[test]
fn vblank_oam_pulse_144_entry_dmg_flag_on() {
    let mut p = dmg();
    p.write(0xFF41, 0x20);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 143, 455);
    p.take_stat_halt_late();
    p.take_stat_late();
    assert_eq!(p.tick(), 0x02, "flag-on DMG OAM pulse at 144:0");
    assert!(
        p.take_stat_halt_late(),
        "flag-on DMG 144:0 pulse is halt-late"
    );
    assert!(
        p.take_stat_late(),
        "flag-on DMG 144:0 pulse is dispatch-late too"
    );
    tick_n(&mut p, 3);
    assert_eq!(p.tick() & 1, 1, "vblank IF at 144:4");
}

#[test]
fn vblank_oam_pulse_144_entry_cgb_flag_on_not_halt_late() {
    let mut c = cgb();
    c.write(0xFF41, 0x20);
    c.write(0xFF40, 0x81);
    run_to(&mut c, 143, 300);
    let ifs = run_to(&mut c, 143, 455);
    assert_eq!(ifs & 2, 0, "no OAM edge between the flip and 144:0");
    c.take_stat_halt_late();
    c.take_stat_late();
    assert_eq!(
        tick_n(&mut c, 2) & 2,
        2,
        "flag-on CGB OAM pulse in 144:0 cycle"
    );
    assert!(
        !c.take_stat_halt_late(),
        "flag-on CGB 144:0 pulse is not halt-late"
    );
    assert!(
        !c.take_stat_late(),
        "flag-on CGB 144:0 pulse dispatches in its own cycle"
    );
}

/// Port Stage A10 — the DMG 145-153 dot-12 vblank OAM pulses are DEFERRED on
/// the flag-on path (only the 144:0 entry pulse banks). The flag-off
/// `stat_events_tick` engine fires them (`vblank_line_oam_pulses_dot12_dmg_only`
/// above) and SameBoy raises them too (`display.c:2185`), but adding them
/// flag-on was measured net-negative — it regresses 6 SameBoy-passing rows
/// whose cc+4 read frame mis-places the resulting read (atomic, lands at the
/// Phase-B reclock). This pins that the flag-on engine stays silent at dot 12
/// until then, guarding against re-adding the pulse prematurely.
#[test]
fn vblank_line_oam_pulses_dot12_deferred_flag_on() {
    let mut p = dmg();
    p.write(0xFF41, 0x20);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 145, 0);
    let ifs = run_to(&mut p, 153, 450);
    assert_eq!(ifs & 2, 0, "flag-on DMG: dot-12 vblank OAM pulses deferred");
}

#[test]
fn vblank_source_continuous_through_vblank() {
    let mut p = dmg();
    p.write(0xFF41, 0x10);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 144, 3);
    assert_eq!(p.tick() & 2, 2, "mode-1 source rises at 144:4");
    let ifs = run_to(&mut p, 153, 455);
    assert_eq!(ifs & 2, 0, "no further edge during vblank");
    // Next frame's vblank gives the next edge.
    let ifs = run_to(&mut p, 144, 4);
    assert_eq!(ifs & 2, 2);
}
