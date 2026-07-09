//! `mod_tests` — stat tests: LYC / LY comparison group (split for file size).

use super::*;

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
        assert_eq!(
            p.ly_for_comparison(),
            4,
            "line 5 dot {dot}: prev-line carryover"
        );
    }
    run_to(&mut p, 5, 3);
    assert_eq!(
        p.ly_for_comparison(),
        -1,
        "line 5 dot 3: reset to -1 (display.c:1776)"
    );
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
            assert_eq!(
                p.ly_for_comparison(),
                -1,
                "vblank line {line} dot {dot}: -1"
            );
        }
        run_to(&mut p, line, 4);
        assert_eq!(
            p.ly_for_comparison(),
            i16::from(line),
            "vblank line {line} dot 4: latched"
        );
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
            assert_eq!(
                p.ly_for_comparison(),
                expect(dot),
                "{label} line 153 dot {dot}"
            );
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

/// S5/A4: the flag-on path drives the SameBoy `GB_STAT_update` rising-edge
/// engine (`stat_update_tick`) instead of `stat_events_tick`. With only the LYC
/// source enabled (LYC=2), the engine raises exactly one STAT IF per frame — on
/// the rising edge when `ly_for_comparison` reaches 2 — and the held match
/// across the line-2→3 carryover does not re-fire (STAT blocking). The flag-off
/// path must agree for this case where both engines fire the same single LYC
/// interrupt (a net-parity anchor).
#[test]
fn stat_update_engine_fires_lyc_once_per_frame() {
    let count_lyc_if = |flag_on: bool| -> u32 {
        let mut p = dmg();
        p.set_leading_edge_reads(flag_on);
        p.write(0xFF45, 2); // LYC = 2
        p.write(0xFF41, 0x40); // LYC interrupt enable only (no mode sources)
        p.write(0xFF40, 0x91); // LCD on
        run_to(&mut p, 1, 0); // past the LCD-on glitch line
        let mut fired = 0;
        // One full frame (line 1 → line 1 next frame is ~154 lines of dots).
        for _ in 0..(154 * 456) {
            if p.tick() & 0x02 != 0 {
                fired += 1;
            }
        }
        fired
    };
    assert_eq!(
        count_lyc_if(true),
        1,
        "flag-on: one LYC=2 STAT IF per frame"
    );
    assert_eq!(
        count_lyc_if(false),
        1,
        "flag-off: same single LYC=2 STAT IF (parity)"
    );
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

/// Port Stage A11 — an FF41 write that enables a source whose condition is
/// already met fires the STAT IF exactly ONCE on the flag-on path. SameBoy
/// computes the write-fire through `GB_STAT_update` (the rising-edge engine)
/// after `write_stat` (`display.c`); the flag-on path must do the same instead
/// of *also* running the gambatte `stat_write_trigger`, which would double-fire
/// (once on the write, again on the next dot-clocked `stat_update_tick` rising
/// edge re-seeing the new enable). Flag-off keeps the gambatte write-trigger and
/// fires once (its dot-clocked `stat_events_tick` LYC event already passed). The
/// double-fire is read-frame-independent, so this banks standalone.
#[test]
fn ff41_enable_lyc_fires_once_flag_on() {
    let count = |flag_on: bool| -> u32 {
        let mut p = dmg();
        p.set_leading_edge_reads(flag_on);
        p.write(0xFF45, 2); // LYC = 2
        p.write(0xFF40, 0x91);
        run_to(&mut p, 2, 100); // line 2 (LYC matches), mode-3 region
        let w = p.write(0xFF41, 0x40); // enable the LYC source
        let mut fires = u32::from(w & 2 != 0);
        for _ in 0..12 {
            if p.tick() & 2 != 0 {
                fires += 1;
            }
        }
        fires
    };
    assert_eq!(count(false), 1, "flag-off: one IF for the enabling write");
    assert_eq!(
        count(true),
        1,
        "flag-on: one IF (the rising-edge engine, no write-trigger double-fire)"
    );
}

/// Port Stage A12 — an FF45 (LYC) write that creates a match fires the STAT IF
/// exactly ONCE on the flag-on path, the FF45 analogue of A11. `write_lyc_*`
/// fires the gambatte LYC-write trigger; without A12 the dot-clocked
/// `stat_update_tick` then re-sees `lyc_interrupt_line` rise next tick and
/// double-fires (flag-off fires 1, flag-on fired 2). The fix re-derives
/// `lyc_interrupt_line` for the new LYC and re-syncs the `StatUpdate` line when
/// the write-trigger fired, so the next tick sees no fresh rise.
#[test]
fn ff45_match_fires_once_flag_on() {
    let count = |flag_on: bool| -> u32 {
        let mut p = dmg();
        p.set_leading_edge_reads(flag_on);
        p.write(0xFF40, 0x91);
        p.write(0xFF41, 0x40); // enable the LYC source
        run_to(&mut p, 5, 100); // line 5, mode-3 region, no match yet
        let w = p.write(0xFF45, 5); // create the LYC=5 match
        let mut fires = u32::from(w & 2 != 0);
        for _ in 0..12 {
            if p.tick() & 2 != 0 {
                fires += 1;
            }
        }
        fires
    };
    assert_eq!(count(false), 1, "flag-off: one IF for the LYC-match write");
    assert_eq!(
        count(true),
        1,
        "flag-on: one IF (write-trigger fires; A12 blocks the StatUpdate re-fire)"
    );
}

/// Port Stage C / S5 (mech 3 root 2) — `stat_update_tick` HOLDS
/// `lyc_interrupt_line` across the line-start LYC carryover (lines 1-143, dots
/// 0-2, where [`Ppu::ly_for_comparison`] still names the PREVIOUS line). SameBoy
/// re-evaluates `lyc_interrupt_line` only at the `GB_SLEEP` steps that *set*
/// `ly_for_comparison` — state-6 (`= -1`, holds) and state-7 (`= current_line`,
/// re-latch) — and does NOT call `GB_STAT_update` during the held carryover. So
/// a late FF45 write whose new LYC equals that carryover number raises NO fresh
/// LYC edge there (`lyc0_late_ff45_enable_3`, `lycwirq_trigger_ly00_stat50_2`).
/// slopgb's per-dot engine used to re-latch the carryover match → a spurious
/// `ly1 dot0` STAT edge (`got=E2`, want E0). This pins the hold: a freshly-set
/// LYC matching the carryover with the latch low must stay low across dots 0-2
/// and raise no IF (a legitimate LYC=N-1 *tail* is already latched true at line
/// N-1, so the hold preserves it — tested by the steady-state family rows). LE
/// path only.
#[test]
fn lyc_latch_holds_across_line_start_carryover_flag_on() {
    let mut p = dmg();
    p.set_leading_edge_reads(true);
    p.write(0xFF40, 0x91); // LCD on
    p.write(0xFF41, 0x40); // LYC source enabled (OAM/mode sources off)
    // Line 5 dot 0: the carryover `ly_for_comparison` reads 4 (line 4). LYC is
    // still 0 (default), so the latch is low (line 4 never matched).
    run_to(&mut p, 5, 0);
    assert!(
        !p.lyc_interrupt_line,
        "latch low entering line 5 (LYC=0, no match)"
    );
    // Simulate the late write landing at the line-5 start: LYC := 4, the
    // carryover number. With the hold the engine must NOT re-latch it across the
    // carryover dots — no spurious edge. (Without the hold, dots 0-2 re-latch
    // 4==4 → a 0→1 LYC rise → IF bit 1.)
    p.lyc = 4;
    let mut fires = 0u32;
    for _ in 0..6 {
        // dots 1..6 — covers the carryover (0-2), the `-1` gap (3), and the
        // dot-4 re-latch (ly_for_comparison = 5 ≠ LYC 4 → stays low).
        if p.tick() & 2 != 0 {
            fires += 1;
        }
        assert!(
            !p.lyc_interrupt_line,
            "latch held low across line-start carryover at line {} dot {}",
            p.line, p.dot
        );
    }
    assert_eq!(
        fires, 0,
        "no spurious LYC STAT edge during the line-start carryover"
    );
}

/// Write-coherence guard (the FF40 leg of the A11/A12 systematic sweep) — an
/// FF40 LCD-enable that raises the STAT line (LYC source pre-enabled, LY=0=LYC
/// matches on the glitch line) fires the STAT IF exactly ONCE on BOTH paths,
/// with NO fix needed (unlike FF41/FF45). `write_lcdc`'s enable path runs
/// `legacy_level_edge` (the gambatte STAT edge), but the flag-on dot-clocked
/// `stat_update_tick` does NOT double-fire it: the rise lands on the glitch
/// line, where the engine's LYC input is inert (`ly_for_comparison` returns
/// -1, so the LYC latch never re-sees the match) — and the disable path is a
/// non-issue (LCD off → `stat_update_tick`'s `!enabled` early-return holds the
/// engine low). So FF40 needs no A11/A12-style gated re-sync; this pins that
/// (a future change that made the engine fire LYC on the glitch line would
/// re-introduce the double-fire and trip here).
#[test]
fn ff40_enable_lyc_match_fires_once_flag_on() {
    let count = |flag_on: bool| -> u32 {
        let mut p = dmg();
        p.set_leading_edge_reads(flag_on);
        p.write(0xFF45, 0); // LYC = 0 (matches LY=0 on the enable glitch line)
        p.write(0xFF41, 0x40); // enable the LYC source (LCD still off)
        let w = p.write(0xFF40, 0x91); // enable the LCD → STAT line rises
        let mut fires = u32::from(w & 2 != 0);
        for _ in 0..12 {
            if p.tick() & 2 != 0 {
                fires += 1;
            }
        }
        fires
    };
    assert_eq!(
        count(false),
        1,
        "flag-off: one IF for the enabling LCDC write"
    );
    assert_eq!(
        count(true),
        1,
        "flag-on: one IF (no StatUpdate double-fire)"
    );
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
