//! Unit tests for the PPU core (STAT IRQ events, LYC, access blocking,
//! registers). Split out of `mod.rs` for file size; compiled as
//! `super::tests` via the `#[path]` attribute there.

use super::*;

fn dmg() -> Ppu {
    Ppu::new(Model::Dmg)
}

fn cgb() -> Ppu {
    Ppu::new(Model::Cgb)
}

/// Tick `n` dots, OR-ing the returned IF bits.
fn tick_n(p: &mut Ppu, n: u32) -> u8 {
    let mut ifs = 0;
    for _ in 0..n {
        ifs |= p.tick();
    }
    ifs
}

/// Tick until the PPU sits at (line, dot); returns OR of IF bits seen.
fn run_to(p: &mut Ppu, line: u8, dot: u16) -> u8 {
    let mut ifs = 0;
    let mut guard = 0u32;
    while !(p.line == line && p.dot == dot) {
        ifs |= p.tick();
        guard += 1;
        assert!(guard < 200_000, "run_to({line},{dot}) never reached");
    }
    ifs
}

/// The STAT mode-bit flip edge (`m0_stat_flip`, drained by
/// `take_m0_stat_flip`) fires once per *sprite-extended* line on the
/// mode-3→mode-0 flip dot, and not at all on a bare line (the sprite gate
/// keeps the bare-line DMA/lcd-offset reads off the override — sub-dot
/// event-phase model, INC-DS-1).
#[test]
fn m0_stat_flip_fires_once_on_a_sprite_line() {
    let mut p = cgb();
    // A sprite covering screen lines 0-7 (Y=16), on-screen X — so it is
    // fetched and the flip edge is armed.
    p.oam[0] = 16;
    p.oam[1] = 8;
    p.write(0xFF40, 0x93); // LCD on, OBJ on, BG on
    run_to(&mut p, 1, 0); // steady-state line 1, sprite present
    p.take_m0_stat_flip(); // drain line 0's edge (the interconnect drains every tick)
    let mut fired = 0;
    let mut flip_dot = None;
    for _ in 0..400 {
        p.tick();
        if p.take_m0_stat_flip() {
            fired += 1;
            flip_dot = Some(p.dot);
            assert!(p.line_render_done, "flip edge implies mode-0 entry");
            assert_eq!(p.vis_mode(), 0, "mode bits are 0 at the flip dot");
        }
    }
    assert_eq!(fired, 1, "stat flip fires exactly once on the sprite line");
    assert!(flip_dot.unwrap() > 84, "flip is past the mode-3 start");
}

/// The STAT mode-bit flip edge stays off on a bare line: those DS reads
/// reach FF41 through the DMA-cycle / lcd-offset chains at a different
/// sub-cycle offset, so the override would regress them (INC-DS-1 gate).
#[test]
fn m0_stat_flip_is_off_on_a_bare_line() {
    let mut p = cgb();
    p.write(0xFF40, 0x91); // LCD on, BG on, no sprites
    run_to(&mut p, 1, 0);
    p.take_m0_stat_flip();
    let mut fired = 0;
    for _ in 0..400 {
        p.tick();
        if p.take_m0_stat_flip() {
            fired += 1;
        }
    }
    assert_eq!(fired, 0, "bare-line flip stays off the STAT-mode override");
}

// --- lcdon_timing-GS: read state at 4*(c+2) dots after LCD enable ---

const LCDON_CYCLES: [[u32; 8]; 3] = [
    [0, 17, 60, 110, 130, 174, 224, 244],
    [1, 18, 61, 111, 131, 175, 225, 245],
    [2, 19, 62, 112, 132, 176, 226, 246],
];

fn lcdon_case(lyc: u8, pass: usize, col: usize) -> Ppu {
    let mut p = dmg();
    p.write(0xFF45, lyc);
    p.write(0xFF40, 0x81);
    tick_n(&mut p, 4 * (LCDON_CYCLES[pass][col] + 2));
    p
}

fn check_lcdon_table(lyc: u8, addr: u16, expect: &[[u8; 8]; 3]) {
    for pass in 0..3 {
        for col in 0..8 {
            let p = lcdon_case(lyc, pass, col);
            assert_eq!(
                p.read(addr),
                expect[pass][col],
                "pass {pass} col {col} (cycle {})",
                LCDON_CYCLES[pass][col]
            );
        }
    }
}

#[test]
fn lcdon_ly_table() {
    check_lcdon_table(
        0,
        0xFF44,
        &[
            [0, 0, 0, 0, 1, 1, 1, 2],
            [0, 0, 0, 1, 1, 1, 2, 2],
            [0, 0, 0, 1, 1, 1, 2, 2],
        ],
    );
}

#[test]
fn lcdon_stat_lyc0_table() {
    check_lcdon_table(
        0,
        0xFF41,
        &[
            [0x84, 0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82],
            [0x84, 0x87, 0x84, 0x80, 0x82, 0x80, 0x80, 0x82],
            [0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82, 0x83],
        ],
    );
}

#[test]
fn lcdon_stat_lyc1_table() {
    check_lcdon_table(
        1,
        0xFF41,
        &[
            [0x80, 0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82],
            [0x80, 0x83, 0x80, 0x80, 0x86, 0x84, 0x80, 0x82],
            [0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82, 0x83],
        ],
    );
}

#[test]
fn lcdon_oam_read_table() {
    check_lcdon_table(
        0,
        0xFE00,
        &[
            [0x00, 0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF],
            [0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF, 0xFF],
            [0x00, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0xFF, 0xFF],
        ],
    );
}

#[test]
fn lcdon_vram_read_table() {
    check_lcdon_table(
        0,
        0x8000,
        &[
            [0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00],
            [0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF],
            [0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF],
        ],
    );
}

// --- Line-0 OAM STAT IRQ event semantics ---
//
// The line-0 mode-2 rise differs from every other line's (see the
// `stat_events_tick` comment for the sources): the IF bit is readable
// immediately (gambatte lyc153int_m2irq) but misses the CPU's
// interrupt sample for one M-cycle (SameBoy raises the OAM IRQ "1
// T-cycle before STAT actually changes, except on line 0"; mealybug
// m3_bgp_change compensates "line 0 timing is different by 4
// cycles"), and it is blocked entirely while the mode-1 source enable
// is set (gambatte mstat_irq.h doM2Event `blockedByM1Irq`;
// lcdirq_precedence/m2irq_ly00_lcdstat30).

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
            p.take_stat_late(),
            "{model:?} line-1 pulse is dispatch-late"
        );
        // Line 0: the IF bit appears in the same M-cycle but is
        // flagged late for the dispatch sample.
        run_to(&mut p, 0, 0);
        p.take_stat_late();
        assert_eq!(tick_n(&mut p, 4) & IF_STAT, IF_STAT, "{model:?} line 0");
        assert!(p.take_stat_late(), "{model:?} line 0 rise is late");
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

// --- Per-source STAT IRQ event predicates (gambatte mstat_irq.h /
// --- lyc_irq.cpp port) ---

#[test]
fn lyc_event_fires_despite_hblank_enable() {
    // gambatte lcdirq_precedence/lycirq_ly44_lcdstat48: with the
    // mode-0 source enabled alongside LYC, the LYC event of its line
    // still raises IF — the sources are independent events, not a
    // wired-OR level (LycIrq::doEvent is blocked by the m2 enable
    // only on visible lines, never by m0).
    let mut p = dmg();
    p.write(0xFF45, 68);
    p.write(0xFF41, 0x48); // LYC + mode-0 sources
    p.write(0xFF40, 0x81);
    run_to(&mut p, 67, 400); // past line 67's m0 event
    let ifs = run_to(&mut p, 68, 8);
    assert_eq!(ifs & IF_STAT, IF_STAT, "LYC event fires under m0 enable");
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
    assert_eq!(ifs & IF_STAT, 0, "m1 event blocked by the m2 enable");
    assert_eq!(ifs & IF_VBLANK, IF_VBLANK, "vblank IF unaffected");
}

#[test]
fn cgb_stat_disable_in_event_leadin_still_fires() {
    // gambatte lycEnable/ff41_disable_2 (dmg08_out0_cgb04c_out2): a
    // STAT write committing in the last M-cycle before the LYC event
    // does not reach the event's delayed enable copy on CGB
    // (LycIrq::regChange `time_ - cc > 2`); on DMG it does.
    for (model, expect) in [(Model::Dmg, 0), (Model::Cgb, IF_STAT)] {
        let mut p = Ppu::new(model);
        p.write(0xFF45, 68);
        p.write(0xFF41, 0x48);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 67, 400);
        run_to(&mut p, 68, 0);
        p.write(0xFF41, 0x00); // disable committing at (68,0)
        let ifs = run_to(&mut p, 68, 8);
        assert_eq!(ifs & IF_STAT, expect, "{model:?}");
    }
}

#[test]
fn dmg_ff45_write_in_event_leadin_misses_event() {
    // gambatte lycEnable/lyc153_late_ff45_enable_3 (dmg08_outE0): an
    // FF45 write committing at the line-start M-cycle cannot reach
    // that line's LYC event on DMG either (LycIrq::regChange
    // `time_ - cc > 4 || timeSrc != time_`), and the write trigger
    // sees the old value still matching the held compare ("lyc flag
    // never goes low -> no trigger").
    let mut p = dmg();
    p.write(0xFF45, 152);
    p.write(0xFF41, 0x40);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 152, 300); // past the (152,4) event
    run_to(&mut p, 153, 0);
    p.write(0xFF45, 153); // commits at (153,0)
    let ifs = run_to(&mut p, 153, 8);
    assert_eq!(ifs & IF_STAT, 0, "protected write misses the 153 event");
}

// --- lcdon_write_timing-GS ---

const WRITE_NOPS: [u32; 19] = [
    0, 17, 18, 60, 61, 110, 111, 112, 130, 131, 132, 174, 175, 224, 225, 226, 244, 245, 246,
];

#[test]
fn lcdon_oam_write_table() {
    let expect: [u8; 19] = [
        0x81, 0x81, 0x00, 0x00, 0x81, 0x81, 0x81, 0x00, 0x00, 0x81, 0x00, 0x00, 0x81, 0x81, 0x81,
        0x00, 0x00, 0x81, 0x00,
    ];
    for (i, &nops) in WRITE_NOPS.iter().enumerate() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        tick_n(&mut p, 4 * (nops + 2));
        p.write(0xFE00, 0x81);
        assert_eq!(p.oam[0], expect[i], "nops {nops}");
    }
}

#[test]
fn lcdon_vram_write_table() {
    let expect: [u8; 19] = [
        0x81, 0x81, 0x00, 0x00, 0x81, 0x81, 0x81, 0x81, 0x81, 0x81, 0x00, 0x00, 0x81, 0x81, 0x81,
        0x81, 0x81, 0x81, 0x00,
    ];
    for (i, &nops) in WRITE_NOPS.iter().enumerate() {
        let mut p = dmg();
        p.write(0xFF40, 0x81);
        tick_n(&mut p, 4 * (nops + 2));
        p.write(0x8000, 0x81);
        assert_eq!(p.vram[0], expect[i], "nops {nops}");
    }
}

// --- Line lengths and LY=153 quirk ---

#[test]
fn steady_line_boundaries() {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    tick_n(&mut p, 451);
    assert_eq!(p.read(0xFF44), 0); // glitch line 0 is 452 dots
    p.tick();
    assert_eq!(p.read(0xFF44), 1);
    tick_n(&mut p, 455);
    assert_eq!(p.read(0xFF44), 1); // state(907)
    p.tick();
    assert_eq!(p.read(0xFF44), 2); // state(908)
}

#[test]
fn ly153_reads_zero_from_dot_4() {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 153, 3);
    assert_eq!(p.read(0xFF44), 153);
    p.tick();
    assert_eq!(p.read(0xFF44), 0);
    run_to(&mut p, 0, 0);
    assert_eq!(p.read(0xFF44), 0);
}

#[test]
fn ly153_lyc153_compare_window() {
    let mut p = dmg();
    p.write(0xFF45, 153);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 153, 3);
    assert_eq!(p.read(0xFF41) & 4, 0); // compare invalid dots 0-3
    p.tick();
    assert_eq!(p.read(0xFF41) & 4, 4); // dots 4-7 compare vs 153
    tick_n(&mut p, 3);
    assert_eq!(p.read(0xFF41) & 4, 4);
    p.tick();
    assert_eq!(p.read(0xFF41) & 4, 0); // dots 8-11 invalid
    tick_n(&mut p, 4);
    assert_eq!(p.read(0xFF41) & 4, 0); // dot 12+: compare vs 0
}

#[test]
fn ly153_lyc0_compare_from_dot_12() {
    let mut p = dmg();
    p.write(0xFF45, 0);
    p.write(0xFF41, 0x40);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 153, 11);
    assert_eq!(p.read(0xFF41) & 4, 0);
    assert_eq!(p.tick(), 0x02, "LYC=0 IRQ fires at 153:12");
    assert_eq!(p.read(0xFF41) & 4, 4);
    // The compare stays set through line 0; no further edge.
    assert_eq!(run_to(&mut p, 1, 0) & 2, 0);
}

#[test]
fn lyc_compare_invalid_first_4_dots_of_line() {
    let mut p = dmg();
    p.write(0xFF45, 2);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 2, 0);
    assert_eq!(p.read(0xFF41) & 4, 0);
    tick_n(&mut p, 3);
    assert_eq!(p.read(0xFF41) & 4, 0); // state(2,3)
    p.tick();
    assert_eq!(p.read(0xFF41) & 4, 4); // state(2,4)
}

// --- VBlank / frame ---

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
fn stat_mode_during_vblank() {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 144, 3);
    assert_eq!(p.read(0xFF41) & 3, 0, "144:0-3 still reads mode 0");
    p.tick();
    assert_eq!(p.read(0xFF41) & 3, 1);
    run_to(&mut p, 150, 100);
    assert_eq!(p.read(0xFF41) & 3, 1);
    // OAM and VRAM accessible during vblank (mem_oam).
    p.write(0xFE05, 0x5A);
    assert_eq!(p.read(0xFE05), 0x5A);
    p.write(0x9000, 0xA5);
    assert_eq!(p.read(0x9000), 0xA5);
}

// --- STAT interrupt sources ---

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
fn mode0_irq_at_254_plus_scx_fine() {
    // The IRQ source rises with the visible flip, 2 dots before the
    // pipe end (see render.rs `m0_flip_events`).
    for scx in [0u8, 1, 4, 5, 7, 8, 13] {
        let mut p = dmg();
        p.write(0xFF41, 0x08);
        p.write(0xFF43, scx);
        p.write(0xFF40, 0x81);
        run_to(&mut p, 1, 4); // line start: hblank source dropped
        let v0 = 254 + u16::from(scx & 7);
        let ifs = run_to(&mut p, 1, v0 - 1);
        assert_eq!(ifs & 2, 0, "scx {scx}: no hblank IRQ before {v0}");
        assert_eq!(p.tick(), 0x02, "scx {scx}: hblank IRQ at {v0}");
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
    assert_eq!(p.tick() & 2, 2, "DMG: OAM pulse at 145:12");
    run_to(&mut p, 146, 11);
    assert_eq!(p.tick() & 2, 2, "DMG: OAM pulse at 146:12");

    let mut c = cgb();
    c.write(0xFF41, 0x20);
    c.write(0xFF40, 0x81);
    run_to(&mut c, 145, 0);
    let ifs = run_to(&mut c, 153, 450);
    assert_eq!(ifs & 2, 0, "CGB: no vblank-line OAM pulses");
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

// --- stat_lyc_onoff behaviours ---

#[test]
fn lyc_flag_frozen_while_lcd_off() {
    let mut p = dmg();
    p.write(0xFF41, 0x40);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 144, 10);
    p.write(0xFF45, 0x90); // LY = LYC = 144
    p.tick();
    assert_eq!(p.read(0xFF41), 0xC5); // cmp set, mode 1 (vblank)
    p.write(0xFF40, 0x01); // LCD off
    assert_eq!(p.read(0xFF41), 0xC4, "flag retained");
    assert_eq!(p.write(0xFF45, 0x01), 0, "comparison clock stopped: no IRQ");
    assert_eq!(p.read(0xFF41), 0xC4, "comparison clock stopped");
    assert_eq!(p.write(0xFF40, 0x81), 0); // LCD on: LY=0 vs LYC=1
    assert_eq!(p.read(0xFF41), 0xC0);
}

#[test]
fn lyc_no_edge_when_comparison_unchanged_across_off_on() {
    let mut p = dmg();
    p.write(0xFF41, 0x40);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 144, 10);
    p.write(0xFF45, 0x90);
    p.tick();
    p.write(0xFF40, 0x01);
    p.write(0xFF45, 0x00); // will match LY=0 on enable
    assert_eq!(p.read(0xFF41), 0xC4);
    assert_eq!(p.write(0xFF40, 0x81), 0, "no edge: flag stayed set");
    assert_eq!(p.read(0xFF41), 0xC4);
}

#[test]
fn lyc_irq_on_lcd_enable() {
    let mut p = dmg();
    p.write(0xFF41, 0x40);
    p.write(0xFF45, 0x00);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 144, 10);
    p.write(0xFF40, 0x01); // off with cmp clear (LY=144 vs 0)
    assert_eq!(p.read(0xFF41), 0xC0);
    // On: LY=0 vs LYC=0 -> rising edge.
    assert_eq!(
        p.write(0xFF40, 0x81),
        0x02,
        "stat_lyc_onoff round 4: IRQ in the enabling write's cycle"
    );
    assert_eq!(p.read(0xFF41), 0xC4);
}

#[test]
fn stat_write_bug_dmg_only() {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 1, 300); // real hblank, no sources enabled
    assert_eq!(p.read(0xFF41) & 3, 0);
    assert_eq!(
        p.write(0xFF41, 0x00),
        0x02,
        "DMG STAT write momentarily enables every source"
    );

    let mut c = cgb();
    c.write(0xFF40, 0x81);
    run_to(&mut c, 1, 300);
    assert_eq!(c.write(0xFF41, 0x00), 0, "CGB lacks the STAT write bug");
}

#[test]
fn stat_write_bug_never_fires_from_the_oam_source() {
    // The glitch write enables every source for one cycle, but the m2
    // source is an event, not a level: a write landing mid-scan or
    // mid-render raises nothing (gbmicrotest stat_write_glitch_l0/l1
    // comment tables show E2 only in the hblank/vblank/LYC-match
    // positions and E0 in the mode-2 ones).
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 1, 40); // mode 2 (OAM scan)
    assert_eq!(p.write(0xFF41, 0x00), 0, "no IRQ from the mode-2 position");
    run_to(&mut p, 1, 150); // mode 3 (OAM blocking level still high)
    assert_eq!(p.write(0xFF41, 0x00), 0, "no IRQ from the mode-3 position");
    // A vblank-position write still fires (E2 in the l154 table).
    run_to(&mut p, 145, 100);
    assert_eq!(p.write(0xFF41, 0x00), 0x02, "vblank level fires");
}

// --- LCD off ---

#[test]
fn lcd_off_state() {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 50, 100);
    p.write(0xFF40, 0x01);
    assert_eq!(p.read(0xFF44), 0);
    assert_eq!(p.read(0xFF41) & 3, 0);
    assert!(p.frame().iter().all(|&px| px == 0xFF_FFFF));
    let fc = p.frame_count();
    tick_n(&mut p, 100_000);
    assert_eq!(p.frame_count(), fc, "frame counter frozen while off");
    assert_eq!(p.read(0xFF44), 0);
    // OAM/VRAM freely accessible.
    p.write(0xFE10, 0x12);
    assert_eq!(p.read(0xFE10), 0x12);
}

/// The first frame after the LCD is (re-)enabled is not displayed: the
/// panel stays blank/white for one frame and real output resumes with
/// the following frame (Pan Docs "LCDC.7" warning on mid-frame
/// enabling; SameBoy display.c skips presenting that frame —
/// `GB_FRAMESKIP_LCD_TURNED_ON`; little-things-gb/firstwhite verifies
/// it on hardware).
#[test]
fn first_frame_after_lcd_enable_is_blank() {
    let mut p = dmg();
    p.write(0xFF47, 0xE4); // identity BGP
    // Tile 0 row 0 black; the map is all tile 0, so line 0 renders
    // black across.
    p.vram[0] = 0xFF;
    p.vram[1] = 0xFF;
    p.write(0xFF40, 0x91);
    run_to(&mut p, 144, 0); // first frame boundary after enable
    assert!(
        p.frame().iter().all(|&px| px == 0xFF_FFFF),
        "first frame after LCD enable must be presented blank"
    );
    run_to(&mut p, 0, 0);
    run_to(&mut p, 144, 0); // second frame boundary
    assert_eq!(p.frame()[0], 0x00_0000, "second frame shows content");
}

#[test]
fn frame_count_steady_period() {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 144, 0);
    assert_eq!(p.frame_count(), 1);
    tick_n(&mut p, 70_224);
    assert_eq!(p.frame_count(), 2, "70224 dots per steady frame");
}

// --- DMG OAM corruption bug (Pan Docs "OAM Corruption Bug") ---

/// PPU on a steady visible line with every OAM byte distinct, so any
/// corruption pattern is observable and attributable.
fn oam_bug_ppu(line: u8, dot: u16) -> Ppu {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, line, dot);
    for (i, byte) in p.oam.iter_mut().enumerate() {
        *byte = (i as u8) ^ 0xA5;
    }
    p
}

/// blargg oam_bug/4-scanline_timing + 5-timing_bug pin the corruptible
/// window in M-cycle units: the access covering dots 0-3 of a visible
/// line corrupts the first row and the one covering dots 72-75 the
/// last, while 76-79 (and everything later) is clean. Under
/// tick-then-access the accessing CPU observes state(T) with the cycle
/// covering T-4..T, so rows 8..=0x98 map to T in 4..80.
#[test]
fn oam_bug_row_window_tracks_scan() {
    let mut p = dmg();
    assert_eq!(p.oam_bug_row(), None, "LCD off");
    p.write(0xFF40, 0x81);
    // Glitch line: no OAM scan (lcdon_timing-GS), never vulnerable.
    for _ in 0..GLITCH_LINE_DOTS {
        assert_eq!(p.oam_bug_row(), None, "glitch line dot {}", p.dot);
        p.tick();
    }
    // Steady visible line: rows step every 4 dots through 4..80.
    for line in [1u8, 2, 143] {
        run_to(&mut p, line, 0);
        for dot in 0..456u16 {
            let expect = if (4..80).contains(&dot) {
                Some((dot / 4 * 8) as u8)
            } else {
                None
            };
            assert_eq!(p.oam_bug_row(), expect, "line {line} dot {dot}");
            p.tick();
        }
    }
    // VBlank lines never scan.
    run_to(&mut p, 144, 0);
    for _ in 0..456 {
        assert_eq!(p.oam_bug_row(), None, "vblank dot {}", p.dot);
        p.tick();
    }
}

#[test]
fn oam_bug_write_pattern_formula() {
    // Dot 16 -> row 0x20 (row 4).
    let mut p = oam_bug_ppu(1, 16);
    let before = p.oam;
    p.oam_bug(OamBugKind::Write);
    let row = 0x20;
    for i in 0..2 {
        let (a, b, c) = (before[row + i], before[row - 8 + i], before[row - 4 + i]);
        assert_eq!(p.oam[row + i], ((a ^ c) & (b ^ c)) ^ c, "glitched byte {i}");
    }
    for i in 2..8 {
        assert_eq!(p.oam[row + i], before[row - 8 + i], "copied byte {i}");
    }
    for (i, &byte) in p.oam.iter().enumerate() {
        if !(row..row + 8).contains(&i) {
            assert_eq!(byte, before[i], "byte {i} outside the row untouched");
        }
    }
}

#[test]
fn oam_bug_write_pattern_first_row_references_row_zero() {
    // Dot 4 -> row 8: operands come from row 0, which stays intact.
    let mut p = oam_bug_ppu(1, 4);
    let before = p.oam;
    p.oam_bug(OamBugKind::Write);
    let (a, b, c) = (before[8], before[0], before[4]);
    assert_eq!(p.oam[8], ((a ^ c) & (b ^ c)) ^ c);
    assert_eq!(p.oam[..8], before[..8], "row 0 untouched");
}

#[test]
fn oam_bug_read_pattern_formula() {
    let mut p = oam_bug_ppu(1, 16);
    let before = p.oam;
    p.oam_bug(OamBugKind::Read);
    let row = 0x20;
    for i in 0..2 {
        let (a, b, c) = (before[row + i], before[row - 8 + i], before[row - 4 + i]);
        let glitched = b | (a & c);
        assert_eq!(p.oam[row + i], glitched, "current row byte {i}");
        assert_eq!(p.oam[row - 8 + i], glitched, "preceding row byte {i}");
    }
    for i in 2..8 {
        assert_eq!(p.oam[row + i], before[row - 8 + i], "copied byte {i}");
        assert_eq!(p.oam[row - 8 + i], before[row - 8 + i], "prev tail intact");
    }
}

#[test]
fn oam_bug_read_pattern_on_uniform_oam_is_invisible() {
    // blargg 3-non_causes tolerates read corruption only because
    // b | (a & c) is the identity on uniform data.
    let mut p = oam_bug_ppu(1, 16);
    p.oam = [0x5A; 0xA0];
    p.oam_bug(OamBugKind::Read);
    assert_eq!(p.oam, [0x5A; 0xA0]);
}

#[test]
fn oam_bug_read_increase_pattern_at_row_4_and_up() {
    let mut p = oam_bug_ppu(1, 16);
    let before = p.oam;
    p.oam_bug(OamBugKind::ReadIncrease);
    let row = 0x20;
    // Glitched first word lands in the *preceding* row, then that row
    // (glitched word included) is copied to both the current row and
    // two rows back (SameBoy v0.12.1 GB_trigger_oam_bug_read_increase;
    // the trailing plain read corruption is a no-op after the copy).
    let mut expect_prev = [0u8; 8];
    expect_prev.copy_from_slice(&before[row - 8..row]);
    for i in 0..2 {
        let (a, b, c, d) = (
            before[row - 0x10 + i],
            before[row - 8 + i],
            before[row + i],
            before[row - 4 + i],
        );
        expect_prev[i] = (b & (a | c | d)) | (a & c & d);
    }
    for (i, &expect) in expect_prev.iter().enumerate() {
        assert_eq!(p.oam[row - 0x10 + i], expect, "two rows back {i}");
        assert_eq!(p.oam[row - 8 + i], expect, "preceding row {i}");
        assert_eq!(p.oam[row + i], expect, "current row {i}");
    }
    for (i, &byte) in p.oam.iter().enumerate() {
        if !(row - 0x10..row + 8).contains(&i) {
            assert_eq!(byte, before[i], "byte {i} outside the rows untouched");
        }
    }
}

#[test]
fn oam_bug_read_increase_in_first_rows_is_plain_read() {
    // Rows 1..=3 (and the last row) skip the special pattern: SameBoy
    // v0.12.1 guards 0x20 <= row < 0x98. Dot 8 -> row 0x10.
    let mut p = oam_bug_ppu(1, 8);
    let mut reference = oam_bug_ppu(1, 8);
    p.oam_bug(OamBugKind::ReadIncrease);
    reference.oam_bug(OamBugKind::Read);
    assert_eq!(p.oam, reference.oam);

    // Dot 76 -> row 0x98 (the last row): also plain read only.
    let mut p = oam_bug_ppu(1, 76);
    let mut reference = oam_bug_ppu(1, 76);
    p.oam_bug(OamBugKind::ReadIncrease);
    reference.oam_bug(OamBugKind::Read);
    assert_eq!(p.oam, reference.oam);
}

#[test]
fn oam_bug_outside_window_is_a_no_op() {
    for dot in [0u16, 80, 200, 300] {
        let mut p = oam_bug_ppu(1, dot);
        let before = p.oam;
        p.oam_bug(OamBugKind::Write);
        p.oam_bug(OamBugKind::Read);
        p.oam_bug(OamBugKind::ReadIncrease);
        assert_eq!(p.oam, before, "dot {dot}");
    }
}

// --- CGB-C LY/STAT line timeline (single speed) ---
//
// The CGB line grid differs from the DMG one in a handful of
// CPU-visible windows; each test below cites the hardware oracle in
// its comment. DMG behaviour must stay bit-identical (mooneye-frozen).

/// CGB line 0 dots 0-3 read STAT mode 1 — the vblank persists into
/// line 0; there is no mode-0 gap (wilbertpol ly00_mode1_2-C round 6
/// vs ly00_mode1_0-GS; SameBoy display.c keeps STAT mode for line 0
/// on CGB at the LY write dot; gambatte getStat's mode-1 window ends
/// 3 cycles before line 0's mode 2).
#[test]
fn cgb_line0_reads_mode1_dots_0_3() {
    let mut p = cgb();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 153, 400); // past the glitch frame
    run_to(&mut p, 0, 0);
    assert_eq!(p.read(0xFF41) & 3, 1, "CGB line 0 dot 0 reads mode 1");
    tick_n(&mut p, 3);
    assert_eq!(p.read(0xFF41) & 3, 1, "CGB line 0 dot 3 reads mode 1");
    p.tick();
    assert_eq!(p.read(0xFF41) & 3, 2, "mode 2 from dot 4");

    let mut d = dmg();
    d.write(0xFF40, 0x81);
    run_to(&mut d, 153, 400);
    run_to(&mut d, 0, 0);
    assert_eq!(d.read(0xFF41) & 3, 0, "DMG line 0 dot 0 reads mode 0");
}

/// CGB has no forced-invalid LYC gap at line starts: the comparator
/// holds the previous line's value through dots 0-3 and switches at
/// dot 4 (wilbertpol ly_lyc-C round 7: STAT reads $C4 — mode 0, flag
/// still set for LYC = previous line — at the start of the next
/// line; ly_lyc_144-C round 7 pins the same on the 144→145 edge).
#[test]
fn cgb_lyc_compare_holds_previous_line_through_dot_3() {
    let mut p = cgb();
    p.write(0xFF45, 2);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 3, 0);
    assert_eq!(p.read(0xFF41) & 4, 4, "CGB (3,0): flag holds line 2");
    tick_n(&mut p, 3);
    assert_eq!(p.read(0xFF41) & 4, 4, "CGB (3,3): flag holds line 2");
    p.tick();
    assert_eq!(p.read(0xFF41) & 4, 0, "CGB (3,4): compares line 3");

    let mut d = dmg();
    d.write(0xFF45, 2);
    d.write(0xFF40, 0x81);
    run_to(&mut d, 3, 0);
    assert_eq!(d.read(0xFF41) & 4, 0, "DMG (3,0): invalid window");
}

/// CGB line 153 LYC windows: the comparator sees 152 during dots
/// 0-3, 153 during dots 4-11 (twice as long as DMG — wilbertpol
/// ly_lyc_153-C rounds 7/8 read $C5 one M-cycle later than the -GS
/// build), and 0 from dot 12 (same dot as DMG — ly_lyc_0-C ==
/// ly_lyc_0-GS expectations).
#[test]
fn cgb_ly153_lyc_compare_windows() {
    let mut p = cgb();
    p.write(0xFF45, 153);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 153, 0);
    assert_eq!(p.read(0xFF41) & 4, 0, "dots 0-3 hold the 152 compare");
    tick_n(&mut p, 4);
    assert_eq!(p.read(0xFF41) & 4, 4, "dot 4: 153 compare");
    tick_n(&mut p, 7);
    assert_eq!(p.read(0xFF41) & 4, 4, "dot 11: still 153");
    p.tick();
    assert_eq!(p.read(0xFF41) & 4, 0, "dot 12: 0 compare");

    // LYC=152 stays matched through 153's dots 0-3.
    let mut p = cgb();
    p.write(0xFF45, 152);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 153, 3);
    assert_eq!(p.read(0xFF41) & 4, 4, "dots 0-3 hold the 152 compare");
    p.tick();
    assert_eq!(p.read(0xFF41) & 4, 0, "dot 4: 153 compare");
}

/// CGB-C: LY reads 153 from dot 454 of line 152 — two dots before
/// the line starts — through dot 3, wrapping to 0 at dot 4 like DMG
/// (wilbertpol ly_new_frame-C reads 153 on two consecutive
/// frame-anchored M-cycles — the CGB boot grid sits 2 dots off the
/// M lattice — while age ly-dmgC-cgbBC's enable-anchored ladder
/// sees it exactly once, and three times at 2-dot spacing in double
/// speed; only the early load satisfies all three).
#[test]
fn cgb_ly153_loads_two_dots_early() {
    let mut p = cgb();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 152, 453);
    assert_eq!(p.read(0xFF44), 152);
    p.tick();
    assert_eq!(p.read(0xFF44), 153, "LY=153 from (152,454)");
    run_to(&mut p, 153, 3);
    assert_eq!(p.read(0xFF44), 153);
    p.tick();
    assert_eq!(p.read(0xFF44), 0, "LY=0 from (153,4)");

    let mut d = dmg();
    d.write(0xFF40, 0x81);
    run_to(&mut d, 152, 454);
    assert_eq!(d.read(0xFF44), 152, "DMG keeps LY=152 to the line end");
}

/// CGB VRAM read blocking starts 3 dots later than DMG — a read at
/// state(80) still returns data (gambatte vramReadable `lineCycles <
/// 76 + 3*cgb`; SameBoy oam_search_index-37 `vram_read_blocked =
/// !GB_is_cgb`; age vram-read-cgbBCE).
#[test]
fn cgb_vram_read_open_through_dot_82() {
    let mut p = cgb();
    p.write(0xFF40, 0x81);
    p.write(0x9000, 0x5A);
    run_to(&mut p, 1, 80);
    assert_eq!(p.read(0x9000), 0x5A, "CGB state(80) readable");
    tick_n(&mut p, 3);
    assert_eq!(p.read(0x9000), 0xFF, "CGB state(83) blocked");

    let mut d = dmg();
    d.write(0xFF40, 0x81);
    d.write(0x9000, 0x5A);
    run_to(&mut d, 1, 80);
    assert_eq!(d.read(0x9000), 0xFF, "DMG state(80) blocked");
}

/// CGB OAM write blocking: line-start dots 0-3 block writes on lines
/// whose predecessor was a visible line, and the DMG dots-80-83
/// writable gap does not exist (gambatte oamWritable: blocked from
/// `lineCycles + 3 + cgb >= 456` with the `lineCycles == 76` escape
/// DMG-only; SameBoy sets oam_write_blocked = GB_is_cgb at line
/// start; age oam-write-cgbBCE).
#[test]
fn cgb_oam_write_blocked_at_line_start_and_scan_end() {
    let mut p = cgb();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 2, 0);
    p.write(0xFE00, 0x12);
    assert_eq!(p.oam[0], 0, "CGB (2,0) write blocked");
    run_to(&mut p, 2, 80);
    p.write(0xFE00, 0x34);
    assert_eq!(p.oam[0], 0, "CGB (2,80) write blocked");
    // Line 0's dots 0-3 follow a vblank line: writable (gambatte
    // oamWritable's `ly >= 143` arm — lyCounter still reads 153).
    run_to(&mut p, 0, 0);
    p.write(0xFE00, 0x56);
    assert_eq!(p.oam[0], 0x56, "CGB (0,0) write lands");

    let mut d = dmg();
    d.write(0xFF40, 0x81);
    run_to(&mut d, 2, 0);
    d.write(0xFE00, 0x12);
    assert_eq!(d.oam[0], 0x12, "DMG (2,0) write lands");
    run_to(&mut d, 2, 80);
    d.write(0xFE00, 0x34);
    assert_eq!(d.oam[0], 0x34, "DMG (2,80) write lands");
}

/// CGB single speed: an FF45 write whose comparison raises the STAT
/// line produces its IF bit one M-cycle after the write instead of
/// inside the write cycle (gambatte lycRegChange schedules a oneshot
/// at cc+5 for cgb && !ds; lyc_ff45_trigger_delay_2 carries the
/// dmg08_out0/cgb04c_out2 split).
#[test]
fn cgb_lyc_write_irq_is_one_mcycle_late() {
    let mut p = cgb();
    p.write(0xFF41, 0x40);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 5, 200);
    assert_eq!(p.write(0xFF45, 5), 0, "CGB: no IF in the write cycle");
    assert_eq!(tick_n(&mut p, 4) & IF_STAT, IF_STAT, "IF one cycle later");

    let mut d = dmg();
    d.write(0xFF41, 0x40);
    d.write(0xFF40, 0x81);
    run_to(&mut d, 5, 200);
    assert_eq!(d.write(0xFF45, 5), IF_STAT, "DMG: IF in the write cycle");
}

/// CGB FF45 writes near a line boundary follow gambatte's
/// lycRegChangeTriggersStatIrq: a write committing at the line-start
/// M-cycle cannot stop that line's event (the delayed `lyc_event`
/// copy), a now-matching value written there raises nothing, and a
/// write in the previous line's last M-cycle compares against the
/// upcoming line (wilbertpol ly_lyc_write-C rounds 1-4).
#[test]
fn cgb_lyc_write_line_boundary_windows() {
    // Round-2 shape: killing the match at (N,0) is too late — the
    // dot-4 event still fires from the old LYC.
    let mut p = cgb();
    p.write(0xFF41, 0x40);
    p.write(0xFF45, 2);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 2, 0);
    assert_eq!(p.write(0xFF45, 0xF0), 0);
    assert_eq!(tick_n(&mut p, 4) & IF_STAT, IF_STAT, "event from old LYC");

    // Round-4 shape: making a match at (N,0) raises nothing — the
    // event sampled the old value and the write-trigger compares
    // the upcoming line.
    let mut p = cgb();
    p.write(0xFF41, 0x40);
    p.write(0xFF45, 0xF0);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 3, 0);
    assert_eq!(p.write(0xFF45, 2), 0);
    assert_eq!(tick_n(&mut p, 12) & IF_STAT, 0, "no IRQ this line");

    // Round-1 shape: a kill one M-cycle earlier does reach the event.
    let mut p = cgb();
    p.write(0xFF41, 0x40);
    p.write(0xFF45, 2);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 1, 452);
    assert_eq!(p.write(0xFF45, 0xF0), 0);
    assert_eq!(tick_n(&mut p, 12) & IF_STAT, 0, "event disarmed");
}

/// The CGB vblank STAT-source level extends through line 0 dots 0-3
/// together with the visible mode 1 (gambatte getStat + the
/// lycEnable lyc0_m1disable cgb04c_outE0 rows: an LYC edge under it
/// stays blocked).
#[test]
fn cgb_vblank_level_holds_through_line0_dots_0_3() {
    let mut p = cgb();
    p.write(0xFF41, 0x10);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 153, 400);
    run_to(&mut p, 0, 3);
    assert!(p.stat_line, "level still high at (0,3)");
    p.tick();
    assert!(!p.stat_line, "level drops at (0,4)");

    let mut d = dmg();
    d.write(0xFF41, 0x10);
    d.write(0xFF40, 0x81);
    run_to(&mut d, 153, 400);
    run_to(&mut d, 0, 0);
    assert!(!d.stat_line, "DMG level low from (0,0)");
}
