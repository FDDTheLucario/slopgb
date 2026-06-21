//! `mod_tests` — stat tests (split for file size).

use super::*;

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
        if p.take_m0_stat_flip().is_some() {
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
        if p.take_m0_stat_flip().is_some() {
            fired += 1;
        }
    }
    assert_eq!(fired, 0, "bare-line flip stays off the STAT-mode override");
}

/// S2b: the interrupt-facing mode (`mode_for_interrupt`) diverges from the
/// CPU-visible mode in two one-dot windows on a visible line — the OAM
/// (mode-2) IRQ leads the visible byte by one dot (dot 3), and the mode-0
/// IRQ lags it by one dot (the dot after the visible 3→0 flip). That 2-dot
/// relative swing is what separates the kernel pair (`ppu-timing-map.md` §2).
/// The field is inert at S2b (the STAT engine still fires on the old path);
/// this pins the decoupling directly against the visible mode.
#[test]
fn mode_for_interrupt_swings_two_dots_against_the_visible_mode() {
    let mut p = dmg();
    p.write(0xFF40, 0x91); // LCD + BG on, no sprites
    run_to(&mut p, 1, 0); // start of a steady visible line

    let mut lead_seen = false; // mode-2 IRQ at dot 3 while visible reads 0
    let mut lag_seen = false; // mode-0 IRQ holds 3 one dot past visible 0
    let mut prev_vis = p.vis_mode();
    for _ in 0..300 {
        p.tick();
        if p.line != 1 {
            break;
        }
        let vis = p.vis_mode();
        let mfi = p.mode_for_interrupt();
        match p.dot {
            3 => {
                assert_eq!((vis, mfi), (0, 2), "dot 3: mode-2 lead (IRQ 2, visible 0)");
                lead_seen = true;
            }
            40 => {
                // The OAM source is a line-start pulse, not a sustained level:
                // the visible byte reads 2 but the IRQ mode is NONE through the
                // OAM-search body (display.c:1799) so a later LYC can re-fire.
                assert_eq!(vis, 2, "visible mode 2 in the OAM search");
                assert_eq!(
                    mfi,
                    crate::stat_update::MODE_FOR_INTERRUPT_NONE,
                    "mode-2 body: IRQ source is NONE, not a sustained level"
                );
            }
            100 => assert_eq!((vis, mfi), (3, 3), "steady mode 3 agrees"),
            _ => {}
        }
        // The visible 3→0 flip dot: the IRQ mode must still read 3 (the lag).
        if prev_vis == 3 && vis == 0 {
            assert_eq!(mfi, 3, "mode-0 lag: IRQ still mode 3 on the visible flip dot");
            // One dot later it catches up to 0.
            p.tick();
            assert_eq!(p.vis_mode(), 0);
            assert_eq!(p.mode_for_interrupt(), 0, "IRQ mode catches up to 0 after the lag dot");
            lag_seen = true;
            break;
        }
        prev_vis = vis;
    }
    assert!(lead_seen, "mode-2 lead window observed");
    assert!(lag_seen, "mode-0 lag window observed");
}

/// S2b/S5-refine: the mode-2 lead is suppressed on line 0 — the OAM STAT IRQ
/// does not fire one dot early there (`display.c:1778` "except on line 0"), so
/// at dot 3 the IRQ mode mirrors the visible mode (no `2` lead), unlike lines
/// 1-143. Line 0 instead pulses the OAM source *at* the visible mode→2 edge
/// (dot 4, `display.c:1792`'s unconditional set), then falls to NONE —
/// `ModeTimeline::mode2_irq_offset(0) == 0`.
#[test]
fn mode_for_interrupt_has_no_mode2_lead_on_line_0() {
    let mut p = dmg();
    p.write(0xFF40, 0x91); // LCD + BG on
    run_to(&mut p, 1, 0); // past the LCD-on glitch line 0
    run_to(&mut p, 0, 3); // a steady (frame 2) line 0, at the lead dot
    assert!(!p.glitch_line, "steady line 0, not the enable glitch line");
    assert_eq!(
        (p.vis_mode(), p.mode_for_interrupt()),
        (0, 0),
        "line 0 dot 3: no mode-2 lead — both still the line-start mode-0 carryover \
         (a lead would make the IRQ mode read 2 here, as it does on line 1)"
    );
    // Line 0's OWN OAM pulse: at the visible edge (dot 4), not one dot early.
    run_to(&mut p, 0, 4);
    assert_eq!(
        (p.vis_mode(), p.mode_for_interrupt()),
        (2, 2),
        "line 0 dot 4: OAM pulse at the visible edge (no lead)"
    );
    // Then the source falls to NONE for the OAM-search body (so a later LYC
    // rise can re-fire — STAT blocking), like every other line.
    run_to(&mut p, 0, 5);
    assert_eq!(
        p.mode_for_interrupt(),
        crate::stat_update::MODE_FOR_INTERRUPT_NONE,
        "line 0 dot 5: OAM source falls to NONE after its 1-dot pulse"
    );
    // Contrast: the same dot-3 lead on the next line DOES fire early.
    run_to(&mut p, 1, 3);
    assert_eq!(
        (p.vis_mode(), p.mode_for_interrupt()),
        (0, 2),
        "line 1 dot 3 leads"
    );
}

/// S5-refine: on lines 1-143 the OAM (mode-2) IRQ source is carried across the
/// *whole* line-start window (dots 0-3), not just pulsed at dot 3. SameBoy sets
/// `mode_for_interrupt = 2` at the prior line's end (`display.c:2138`, skipped
/// only for the last visible line `LINES-1`) and re-sets it at the line top
/// (`display.c:1781`), so the source is high continuously from the prior line's
/// HBlank exit through this line's OAM-search start — the "OAM int 1 T-cycle
/// before STAT" lead (`display.c:1778`) seen as a sustained carryover rather
/// than only the one-dot lead the swing test pins at dot 3. The visible byte
/// still reads the mode-0 gap (0) across those dots. Inert field; pins the
/// decoupled model the S5 StatUpdate swap consumes (line 0 is the exception —
/// no prior-line carryover, see the line-0 test).
#[test]
fn mode_for_interrupt_holds_oam_source_across_line_start() {
    let mut p = dmg();
    p.write(0xFF40, 0x91); // LCD + BG on, no sprites
    for dot in 0..4u16 {
        run_to(&mut p, 5, dot); // a steady mid-frame visible line
        assert!(!p.glitch_line, "line 5 is a normal visible line");
        assert_eq!(
            (p.vis_mode(), p.mode_for_interrupt()),
            (0, 2),
            "line 5 dot {dot}: visible mode-0 gap but the OAM IRQ source is carried (2)"
        );
    }
    // Dot 4 (the visible mode→2 edge) drops the source to the OAM-search NONE
    // body so a later LYC rise can re-fire (`display.c:1799`).
    run_to(&mut p, 5, 4);
    assert_eq!(
        (p.vis_mode(), p.mode_for_interrupt()),
        (2, crate::stat_update::MODE_FOR_INTERRUPT_NONE),
        "line 5 dot 4: visible byte reads 2 but the OAM source has fallen to NONE"
    );
}

/// S5-refine: the VBlank interrupt-facing mode (`display.c` 144-152 loop +
/// line-153 tail). Line 144 still reads line 143's HBlank carryover (mode 0)
/// for its first dots, flips to the VBlank source (mode 1) at the vblank-entry
/// step (`display.c:2178` `mode_for_interrupt = 1`, ~dot 4), and every later
/// vblank line (145-153) holds mode 1: there is no mode-2 carryover into vblank
/// (`display.c:2138` skips `LINES-1`) and no `-1` gap. The per-line DMG OAM
/// vblank pulses and the line-144 OAM IF pokes are *direct* `IF |= 2` writes in
/// the STAT engine (`display.c:2160`, `:2185`), not `mode_for_interrupt`
/// transitions, so they are not modelled in this field.
#[test]
fn mode_for_interrupt_vblank_timeline() {
    let mut p = dmg();
    p.write(0xFF40, 0x91);
    for dot in 0..4u16 {
        run_to(&mut p, 144, dot);
        assert_eq!(p.mode_for_interrupt(), 0, "line 144 dot {dot}: HBlank carryover");
    }
    run_to(&mut p, 144, 4);
    assert_eq!(p.mode_for_interrupt(), 1, "line 144 dot 4: VBlank source raised");
    for line in [145u8, 150, 153] {
        run_to(&mut p, line, 80);
        assert_eq!(p.mode_for_interrupt(), 1, "vblank line {line}: holds mode 1");
    }
}

/// S5/A1: SameBoy `ly_for_comparison` (`display.c`) — the *delayed* LY value the
/// LYC==LY interrupt source compares against, distinct from the live FF44. It is
/// `-1` ("no line") at line start, latches to the line number a few dots in, and
/// holds the previous line's value across the first dots of the next line (the
/// LYC-match tail). Pinned for single speed across DMG / CGB-C / AGB; the field
/// is computed for the flag-on StatUpdate path (A4), inert flag-off.
#[test]
fn ly_for_comparison_visible_line_schedule() {
    let mut p = dmg();
    p.write(0xFF40, 0x91);
    // A steady mid-frame line: dots 0-2 hold the previous line's value
    // (display.c held until the dot-3 reset), dot 3 is -1, dots 4+ are N.
    for dot in 0..3u16 {
        run_to(&mut p, 5, dot);
        assert_eq!(p.ly_for_comparison(), 4, "line 5 dot {dot}: prev-line carryover");
    }
    run_to(&mut p, 5, 3);
    assert_eq!(p.ly_for_comparison(), -1, "line 5 dot 3: reset to -1 (display.c:1776)");
    for dot in [4u16, 40, 200] {
        run_to(&mut p, 5, dot);
        assert_eq!(p.ly_for_comparison(), 5, "line 5 dot {dot}: latched to N");
    }
    // Line 1 dot 3 is -1; dots 0-2 carry line 0's value (0).
    run_to(&mut p, 1, 0);
    assert_eq!(p.ly_for_comparison(), 0, "line 1 dot 0: line-0 carryover");
    run_to(&mut p, 1, 3);
    assert_eq!(p.ly_for_comparison(), -1, "line 1 dot 3: -1");
    run_to(&mut p, 1, 4);
    assert_eq!(p.ly_for_comparison(), 1, "line 1 dot 4: latched to 1");
}

/// S5/A1: VBlank `ly_for_comparison` is `-1` for the first four dots of every
/// line (144-152) then latches to the line number (`display.c` 144-152 loop:
/// `ly_for_comparison = -1` at entry, `= current_line` after GB_SLEEP 26+12).
#[test]
fn ly_for_comparison_vblank_schedule() {
    let mut p = dmg();
    p.write(0xFF40, 0x91);
    for line in [144u8, 150] {
        for dot in 0..4u16 {
            run_to(&mut p, line, dot);
            assert_eq!(p.ly_for_comparison(), -1, "vblank line {line} dot {dot}: -1");
        }
        run_to(&mut p, line, 4);
        assert_eq!(p.ly_for_comparison(), i16::from(line), "vblank line {line} dot 4: latched");
    }
}

/// S5/A1: line 153's `ly_for_comparison` runs a model-specific micro-sequence
/// (`display.c` line-153 tail). DMG / CGB-C single speed: `-1` (dots 0-5) ->
/// `153` (6-7) -> `-1` (8-11) -> `0` (12+) — the brief LYC=153 window and the
/// early LYC=0 that fire the once-per-frame line-153 LYC sources. AGB
/// (`model > CGB_C`) shifts the first set two dots earlier and skips the `-1`
/// gap: `-1` (0-3) -> `153` (4-11) -> `0` (12+).
#[test]
fn ly_for_comparison_line_153_schedule() {
    for (mk, label) in [
        (Ppu::new(Model::Dmg), "dmg"),
        (Ppu::new(Model::Cgb), "cgb-c"),
    ] {
        let mut p = mk;
        let _ = label;
        p.write(0xFF40, 0x91);
        let expect = |dot: u16| -> i16 {
            match dot {
                0..=5 => -1,
                6..=7 => 153,
                8..=11 => -1,
                _ => 0,
            }
        };
        for dot in [0u16, 5, 6, 7, 8, 11, 12, 100] {
            run_to(&mut p, 153, dot);
            assert_eq!(p.ly_for_comparison(), expect(dot), "{label} line 153 dot {dot}");
        }
    }
    // AGB shifts earlier, no -1 gap.
    let mut p = Ppu::new(Model::Agb);
    p.write(0xFF40, 0x91);
    let expect = |dot: u16| -> i16 {
        match dot {
            0..=3 => -1,
            4..=11 => 153,
            _ => 0,
        }
    };
    for dot in [0u16, 3, 4, 8, 11, 12, 100] {
        run_to(&mut p, 153, dot);
        assert_eq!(p.ly_for_comparison(), expect(dot), "agb line 153 dot {dot}");
    }
}

/// S2c — cycle-exact mode-3 length, validated as a parallel function.
///
/// SameBoy's bare-line (no sprites, no active window) mode-3 length is
/// `167 + (SCX & 7)` (`display.c:1493`), so the visible STAT mode flips 3→0
/// at dot `MODE2_LENGTH + 167 + (SCX & 7)` = `247 + (SCX & 7)`
/// (`display.c:2091`). The kernel pair renders BG-only, so this closed form
/// is exact for it. [`crate::mode_timeline::ModeTimeline`] is that function;
/// this test pins it to the live PPU's actual SCX sweep and measures the
/// **reclock magnitude** the S2d atomic flip must apply: slopgb's live
/// `m0_flip_events` flip lands a *constant* 7 dots later than the SameBoy
/// boundary (a +4 line-start mode-0-window offset plus a +3 longer mode 3),
/// across every SCX. That fixed delta is exactly what makes the lift a whole
/// re-clock (it moves the rendered pixel-pop dot, hence the mealybug photos)
/// rather than a net-zero sub-dot nudge.
#[test]
fn sameboy_mode3_length_is_seven_dots_short_of_the_live_flip() {
    for scx in 0u8..8 {
        // SameBoy cycle-exact boundary (the parallel function).
        let sameboy = crate::mode_timeline::ModeTimeline::bare(1, scx).visible_mode0_dot();
        assert_eq!(
            sameboy,
            247 + u16::from(scx & 7),
            "scx {scx}: SameBoy bare mode-0 dot = 247 + SCX&7"
        );

        // Live slopgb flip dot for the same bare line.
        let mut p = dmg();
        p.write(0xFF43, scx);
        p.write(0xFF40, 0x91); // LCD + BG on, no sprites/window
        run_to(&mut p, 1, 0);
        let mut prev = p.vis_mode();
        let mut live_flip = None;
        for _ in 0..400 {
            p.tick();
            if p.line != 1 {
                break;
            }
            let v = p.vis_mode();
            if prev == 3 && v == 0 {
                live_flip = Some(p.dot);
                break;
            }
            prev = v;
        }
        let live = live_flip.expect("bare line flips 3→0 within the line");
        assert_eq!(
            i32::from(live) - i32::from(sameboy),
            7,
            "scx {scx}: live flip {live} is a constant 7 dots past SameBoy {sameboy}"
        );
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
