//! `mod_tests` — cgb tests (split for file size).

use super::*;

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
    assert_eq!(
        p.read(0xFF41) & 3,
        2,
        "eager: CGB line 0 dot 0 reads mode 2"
    );
    tick_n(&mut p, 3);
    assert_eq!(
        p.read(0xFF41) & 3,
        2,
        "eager: CGB line 0 dot 3 reads mode 2"
    );
    p.tick();
    assert_eq!(p.read(0xFF41) & 3, 2, "mode 2 from dot 4");

    let mut d = dmg();
    d.write(0xFF40, 0x81);
    run_to(&mut d, 153, 400);
    run_to(&mut d, 0, 0);
    assert_eq!(
        d.read(0xFF41) & 3,
        2,
        "eager: DMG line 0 dot 0 reads mode 2"
    );
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
    assert_eq!(
        p.read(0xFF41) & 4,
        0,
        "eager: (3,0) already compares line 3 (no match)"
    );
    tick_n(&mut p, 3);
    assert_eq!(
        p.read(0xFF41) & 4,
        0,
        "eager: (3,3) already compares line 3 (no match)"
    );
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
    assert_eq!(p.read(0xFF41) & 4, 4, "eager: dots 0-7 compare 153 (match)");
    tick_n(&mut p, 4);
    assert_eq!(p.read(0xFF41) & 4, 4, "dot 4: still 153");
    tick_n(&mut p, 7);
    assert_eq!(
        p.read(0xFF41) & 4,
        0,
        "eager: dot 11 past the window (dropped at dot 8)"
    );
    p.tick();
    assert_eq!(p.read(0xFF41) & 4, 0, "dot 12: 0 compare");

    // LYC=152 stays matched through 153's dots 0-3.
    let mut p = cgb();
    p.write(0xFF45, 152);
    p.write(0xFF40, 0x81);
    run_to(&mut p, 153, 3);
    assert_eq!(
        p.read(0xFF41) & 4,
        0,
        "eager: dots 0-3 already compare 153 (LYC=152 no match)"
    );
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

/// HALFDOT Part-A: the EAGER emergent-flip accessibility release
/// (`Ppu::eager_access_released`) unblocks OAM/VRAM reads + writes at the
/// render's OWN projected flip on the eager clock — where the tier2
/// `vis_early` boolean (LE `early_lead = 3`, 2 dots early) over-releases
/// the `_1` sibling. On an SCX=3 bare line the flip projects to dot 257,
/// so the `_2` read (dot 256, `read_pos_hd` 520 ≥ 2·257+6 = 520) releases
/// while the `_1` read (dot 252, `read_pos_hd` 512 < 520) stays blocked —
/// the gambatte `postread_scx3_1`/`_2` + `postwrite_2_scx3` split. Eager
/// off (production/tier2) keeps both blocked pre-dispatch (byte-identical).
#[test]
fn eager_emergent_flip_releases_accessibility_at_the_projected_exit() {
    let mut p = cgb();
    p.write(0xFF43, 3); // SCX 3 → bare-line mode-3→0 flip projects to dot 257
    p.write(0xFF40, 0x81); // LCD + BG on
    run_to(&mut p, 1, 252); // the `_1` access position (read_pos_hd 512)
    assert_eq!(
        p.projected_flip_dot(),
        257,
        "SCX=3 bare flip projects dot 257"
    );
    assert!(!p.line_render_done, "dispatch has not fired at dot 252");
    assert!(
        p.oam_read_blocked(),
        "eager `_1` OAM read stays blocked pre-flip"
    );
    assert!(
        p.vram_read_blocked(),
        "eager `_1` VRAM read stays blocked pre-flip"
    );
    p.write(0xFE00, 0x11);
    assert_eq!(p.oam[0], 0, "eager `_1` OAM write dropped pre-flip");

    run_to(&mut p, 1, 256); // the `_2` access position (read_pos_hd 520)
    assert!(!p.line_render_done, "dispatch still pending at dot 256");
    assert!(
        !p.oam_read_blocked(),
        "eager `_2` OAM read releases at the emergent flip"
    );
    assert!(
        !p.vram_read_blocked(),
        "eager `_2` VRAM read releases at the emergent flip"
    );
    p.write(0xFE00, 0x22);
    assert_eq!(
        p.oam[0], 0x22,
        "eager `_2` OAM write lands at the emergent flip"
    );
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

/// EAGER off-screen-window (WX=166) exit-arm cluster: the WX=166
/// window activates during HBlank, so slopgb sets `win_active` one M-cycle
/// AFTER the eager cc+0 FF41 read — the window-length arm (arm 1 / D1) misses
/// and the bare arm-8 over-holds mode 3 against the window-elevated projection.
/// `Ppu::eager_offscreen_win_arming` fires the window-length exit for the
/// pre-activation read; `Ppu::eager_offscreen_win_access` releases the
/// stalled-window OAM/VRAM readback. On a CGB SCX=5 WX=166 line the window
/// exit is `259+5` = dot 264 (rphd 528): the dot-260 read (rphd 528 = exit)
/// reads mode 0 where the neutered bare arm-8 over-holds mode 3
/// (`m2int_wxA6_scx5_m3stat` [Cgb] want 0), while dot 259 (rphd 526 < 528)
/// still holds 3 — a real exit, not a blanket force. The stalled-window OAM
/// read releases at the emergent flip (dot 265, rphd 538 ≥ 2·266+6 = 538) —
/// the `m2int_wxA6_{oam,vram}busyread` [Dmg] accessibility class. Eager off
/// (production / tier2) keeps the read blocked — byte-identical.
#[test]
fn eager_offscreen_wx166_window_exit_and_stalled_access() {
    let mut p = cgb();
    p.write(0xFF43, 5); // SCX 5 (as the m2int_wxA6_scx5_m3stat ROM)
    p.write(0xFF4A, 0); // WY 0 → window triggered
    p.write(0xFF4B, 0xA6); // WX 166 (off-screen glitch value)
    p.write(0xFF40, 0xA1); // LCD + BG + window enable

    // Pre-activation (win_active not yet set): the window-length exit (dot 264,
    // rphd 528) drives the read, not the window-elevated bare projection.
    run_to(&mut p, 1, 259);
    assert!(
        !p.render.win_active,
        "wx166 window not yet HBlank-activated at dot 259"
    );
    assert_eq!(
        p.vis_mode_read(),
        3,
        "dot 259 (rphd 526 < exit 528) still holds mode 3"
    );
    run_to(&mut p, 1, 260);
    assert!(!p.render.win_active, "still pre-activation at dot 260");
    assert_eq!(
        p.vis_mode_read(),
        0,
        "dot 260 (rphd 528 = window exit) reads mode 0 via the arming arm"
    );

    // Stalled-window (win_active + win_stalled, renders nothing): the OAM/VRAM
    // readback releases past the emergent flip.
    run_to(&mut p, 1, 264);
    assert!(
        p.render.win_active && p.render.win_stalled,
        "wx166 window active + stalled"
    );
    assert!(
        p.oam_read_blocked(),
        "dot 264 (rphd 536 < 538) OAM still blocked"
    );
    run_to(&mut p, 1, 265);
    assert!(
        !p.oam_read_blocked(),
        "dot 265 (rphd 538 ≥ 2·266+6) OAM readback releases"
    );
    assert!(!p.vram_read_blocked(), "dot 265 VRAM readback releases too");
}
