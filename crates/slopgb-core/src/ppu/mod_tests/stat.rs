//! `mod_tests` — stat tests: mode / interrupt-timing group (split for file size).

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
            assert_eq!(
                mfi, 3,
                "mode-0 lag: IRQ still mode 3 on the visible flip dot"
            );
            // One dot later it catches up to 0.
            p.tick();
            assert_eq!(p.vis_mode(), 0);
            assert_eq!(
                p.mode_for_interrupt(),
                3,
                "eager: IRQ mode still lags at 3 one dot past the visible flip"
            );
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
/// at dot 3 the IRQ mode does NOT read `2` (no lead), unlike lines 1-143. Line 0
/// instead pulses the OAM source *at* the visible mode→2 edge (dot 4,
/// `display.c:1792`'s unconditional set), then falls to NONE.
///
/// Mech 3 root 2: line 0's dots 0-3 carry the VBlank source (mode 1), NOT the
/// visible mode (0). SameBoy never re-sets `mode_for_interrupt` between the
/// line-144 entry (`= 1`) and line 0's OAM step (`= 2`), so the IRQ mode holds 1
/// across vblank and into line 0's first dots — keeping the STAT line high when
/// VBlank is enabled, so the line-0 OAM rise raises no spurious edge
/// (`m1/m2m1irq_ifw_2`). It is decoupled from the visible FF41 mode (still 0).
#[test]
fn mode_for_interrupt_has_no_mode2_lead_on_line_0() {
    let mut p = dmg();
    p.write(0xFF40, 0x91); // LCD + BG on
    run_to(&mut p, 1, 0); // past the LCD-on glitch line 0
    run_to(&mut p, 0, 3); // a steady (frame 2) line 0, at the lead dot
    assert!(!p.glitch_line, "steady line 0, not the enable glitch line");
    assert_eq!(
        (p.vis_mode(), p.mode_for_interrupt()),
        (0, 1),
        "line 0 dot 3: no mode-2 lead — the IRQ mode carries the VBlank source \
         (mode 1) from line 144, decoupled from the visible mode-0 carryover; a \
         lead would make it read 2 here, as it does on line 1"
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
        assert_eq!(
            p.mode_for_interrupt(),
            0,
            "line 144 dot {dot}: HBlank carryover"
        );
    }
    run_to(&mut p, 144, 4);
    assert_eq!(
        p.mode_for_interrupt(),
        1,
        "line 144 dot 4: VBlank source raised"
    );
    for line in [145u8, 150, 153] {
        run_to(&mut p, line, 80);
        assert_eq!(
            p.mode_for_interrupt(),
            1,
            "vblank line {line}: holds mode 1"
        );
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
fn sameboy_mode3_length_is_four_dots_short_of_the_live_flip() {
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
            4,
            "scx {scx}: live flip {live} is a constant 4 dots past SameBoy {sameboy}"
        );
    }
}

/// S2c — the visible mode→0 back-date (`vis_early`, the kernel-pair
/// separator). On the `leading-edge` path a bare single-speed line's
/// CPU-visible STAT mode flips 3→0 at SameBoy's `visible_mode0_dot` (`+4` past
/// the live dispatch's `+7`, i.e. **3 dots earlier**), while the IRQ-dispatch
/// flip (`line_render_done`/`m0_src`) stays at `+7` — the decoupling the
/// instrumentation showed separates `m2int_m3stat_1` (read at our dot 248,
/// stays mode 3) from `m0int_m3stat_2` (read at our dot 252, now mode 0).
#[test]
fn visible_mode0_backdates_three_dots_bare_line() {
    let live_visible_flip = || -> u16 {
        let mut p = dmg();
        p.write(0xFF43, 0); // SCX 0
        p.write(0xFF40, 0x91); // LCD + BG on, no sprites/window (bare line)
        run_to(&mut p, 1, 0);
        let mut prev = p.vis_mode();
        for _ in 0..400 {
            p.tick();
            if p.line != 1 {
                break;
            }
            let v = p.vis_mode();
            if prev == 3 && v == 0 {
                return p.dot;
            }
            prev = v;
        }
        panic!("bare line flips 3→0 within the line");
    };
    let sameboy = crate::mode_timeline::ModeTimeline::bare(1, 0).visible_mode0_dot();
    assert_eq!(sameboy, 247);
    assert_eq!(
        live_visible_flip(),
        sameboy + 4,
        "visible flip back-dated 3 dots to SameBoy's frame (+4)"
    );
}

#[test]
fn stat_mode_during_vblank() {
    let mut p = dmg();
    p.write(0xFF40, 0x81);
    run_to(&mut p, 144, 3);
    assert_eq!(p.read(0xFF41) & 3, 1, "eager: 144:3 already reads mode 1");
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

/// Port Stage A8 — the mode-0 IRQ side fires at `line_render_done` (the
/// gambatte-calibrated `m0_rise_dot` frame), NOT the +1-dot `mfi_m0_prev` lag.
/// The lag put the `StatUpdate` mode-0 STAT IF one dot late and broke
/// `hblank_ly_scx_timing`. Pinned by comparing the `mode_for_interrupt` 3→0
/// dot to the visible mode→0 flip (= `line_render_done`).
#[test]
fn mode0_irq_fires_at_render_done_not_lagged() {
    // The line_render_done dot = the visible mode 3→0 flip (bare SCX 0).
    let render_done_dot = {
        let mut p = dmg();
        p.write(0xFF40, 0x91);
        run_to(&mut p, 1, 0);
        let mut prev = p.vis_mode();
        let mut d = None;
        for _ in 0..400 {
            p.tick();
            if p.line != 1 {
                break;
            }
            let v = p.vis_mode();
            if prev == 3 && v == 0 {
                d = Some(p.dot);
                break;
            }
            prev = v;
        }
        d.expect("bare line flips 3→0")
    };
    let mfi_zero_dot = || -> u16 {
        let mut p = dmg();
        p.write(0xFF40, 0x91);
        run_to(&mut p, 1, 0);
        let mut prev = p.mode_for_interrupt();
        for _ in 0..400 {
            p.tick();
            if p.line != 1 {
                break;
            }
            let v = p.mode_for_interrupt();
            if prev == 3 && v == 0 {
                return p.dot;
            }
            prev = v;
        }
        panic!("mode_for_interrupt never flips 3→0");
    };
    assert_eq!(
        mfi_zero_dot(),
        render_done_dot + 3,
        "eager: mode-0 IRQ side flips 3 dots past the back-dated visible flip"
    );
}

/// Port Stage A7 — the mode-2→3 entry back-date (`ppu-subdot-ladder.md`
/// "A7"). On the leading-edge path a bare single-speed line's CPU-visible STAT
/// mode flips 2→3 at dot 80 (4 dots earlier than the dispatch's dot 84),
/// making the cc+0 FF41 read observationally match SameBoy's cc+4 view for
/// the mode-2→3 entry (mooneye `intr_2_mode3_timing` passes).
#[test]
fn mode3_entry_backdates_four_dots() {
    let entry = || -> u16 {
        let mut p = dmg();
        p.write(0xFF40, 0x91); // LCD + BG on, bare line
        run_to(&mut p, 1, 0);
        let mut prev = p.vis_mode();
        for _ in 0..200 {
            p.tick();
            if p.line != 1 {
                break;
            }
            let v = p.vis_mode();
            if prev == 2 && v == 3 {
                return p.dot;
            }
            prev = v;
        }
        panic!("bare line never flips 2→3");
    };
    assert_eq!(entry(), 80, "back-dated 4 dots to dot 80");
}

/// Port Stage A13 — the LCD-enable glitch-line mode-3 boundary
/// back-dates (`ppu-subdot-ladder.md` "A13"). On the leading-edge path the
/// glitch first line's CPU-visible STAT mode-3 window is back-dated the full
/// single-speed read offset (4 dots, like A7): the mode-0→3 ENTRY moves 78→74
/// and the 3→0 EXIT (the `line_render_done` flip at dot 252) is anticipated by
/// `vis_early` rising at dot 248, so the cc+0 FF41 read reproduces SameBoy's
/// cc+4 view (`lcdon_timing-GS` STAT tables; gambatte enable_display).
#[test]
fn glitch_line_mode3_backdates_four_dots() {
    // (entry, exit) dots of the glitch line's visible 0→3→0 STAT-mode window.
    let window = || -> (u16, u16) {
        let mut p = dmg();
        p.write(0xFF40, 0x81); // LCD off→on: line 0 is the glitch line
        assert!(p.glitch_line, "FF40 enable must arm the glitch line");
        let mut prev = p.vis_mode();
        let mut entry = None;
        for _ in 0..456 {
            p.tick();
            assert!(p.glitch_line, "exit must land before the glitch line ends");
            let v = p.vis_mode();
            if prev == 0 && v == 3 {
                entry = Some(p.dot);
            } else if prev == 3 && v == 0 {
                return (entry.expect("entry before exit"), p.dot);
            }
            prev = v;
        }
        panic!("glitch line never completed a 0→3→0 mode window");
    };
    assert_eq!(window(), (74, 248), "both back-dated 4 dots");
}

/// Port Stage A15 — the LCD-enable glitch line raises NO mode-0 (HBlank) STAT
/// IRQ in its line-start prefix (single speed). The glitch prefix shows visible
/// mode 0 but it is the LCD-enable glitch, not a real hblank, so
/// `stat_line_level` / `stat_write_trigger_dmg` suppress the HBlank source there.
/// The rising-edge engine's glitch `mode_for_interrupt` used to mirror `vis_mode`
/// (mode 0 in the prefix) and fired a spurious m0 IRQ at the first glitch dot
/// (`enable_display/ly0_m0irq_trigger` rendered E2 vs SameBoy/gambatte's
/// out0). Now the SS prefix selects NONE; only the real post-render glitch m0
/// fires. (`dmg()` is single speed, so the `!ds` gate is active here.)
#[test]
fn glitch_line_prefix_suppresses_m0_irq() {
    // mode_for_interrupt across the glitch line with HBlank enabled (single speed).
    let mut p = dmg();
    p.write(0xFF41, 0x08); // HBlank (mode-0) source only
    p.write(0xFF40, 0x81); // LCD off→on: line 0 is the glitch line
    assert!(p.glitch_line, "FF40 enable must arm the glitch line");
    let none = crate::stat_update::MODE_FOR_INTERRUPT_NONE;
    // Count the STAT IF emissions across the whole glitch line and record the
    // prefix `mode_for_interrupt`.
    let mut fires = 0u32;
    let mut prefix_mfi_seen_none = false;
    let mut prefix_mfi_seen_zero = false;
    for _ in 0..456 {
        let ifs = p.tick();
        if !p.glitch_line {
            break;
        }
        if ifs & 2 != 0 {
            fires += 1;
        }
        if p.dot < GLITCH_MODE3_START {
            match p.mode_for_interrupt() {
                m if m == none => prefix_mfi_seen_none = true,
                0 => prefix_mfi_seen_zero = true,
                _ => {}
            }
        }
    }
    assert!(
        prefix_mfi_seen_none,
        "the glitch prefix must select NONE (no mode source)"
    );
    assert!(
        !prefix_mfi_seen_zero,
        "the glitch prefix must NOT select mode 0 (would fire a spurious m0 IRQ)"
    );
    assert_eq!(
        fires, 1,
        "exactly one glitch-line m0 IRQ (the real post-render flip), not a prefix double-fire"
    );
}

/// Port Stage A14 — the glitch line's LYC *readable* compare back-dates 4
/// dots on the leading-edge single-speed path, like A13's mode-3 window: a
/// glitch-line FF41 read in the last 4 dots (≥ GLITCH_LINE_DOTS−4 = 448) is
/// the cc+0 view of what its cc+4 trailing edge sees 4 dots later — the next
/// line (line 1) at dots 0-3 (the −4 read offset). There the DMG readable
/// coincidence flag is forced invalid (no-match), so the glitch compare drops
/// Some(0)→None for those 4 dots. The IRQ side is untouched (the glitch line's
/// `ly_for_comparison` is −1, so `cmp_irq`/the STAT engine never re-see
/// the match).
#[test]
fn glitch_line_lyc_compare_backdates_four_dots() {
    let compare_at = |dot: u16| -> Option<u8> {
        let mut p = dmg();
        p.write(0xFF40, 0x81); // LCD off→on: line 0 is the glitch line
        assert!(p.glitch_line, "FF40 enable must arm the glitch line");
        run_to(&mut p, 0, dot);
        assert!(p.glitch_line, "dot {dot} must still be on the glitch line");
        p.compare_ly()
    };
    // Before the last 4 dots: LY=0 compare both paths (no back-date).
    assert_eq!(compare_at(447), Some(0), "dot 447 (before window)");
    // Last 4 dots map to line-1 dots 0-3: this DMG back-dates to the line-1
    // no-match (None).
    for dot in 448..=451 {
        assert_eq!(compare_at(dot), None, "glitch dot {dot}");
    }
}

/// Port Stage C/S5 mech-1 — the window vis-HOLD foundation (`vis_hold_until`)
/// keeps the CPU-visible STAT mode at 3 PAST the dispatch flip
/// (`line_render_done`/`vis_early`) until SameBoy's `263 + SCX&7` window-length
/// exit, WITHOUT moving the dispatch. Always 0 (no hold) in production today,
/// so `vis_mode` still reads the plain `line_render_done` flip. (Validated
/// foundation for the C2 window-length model — see the `vis_hold_until` docs.)
#[test]
fn vis_hold_extends_visible_mode3_past_the_dispatch() {
    let mut p = dmg();
    p.write(0xFF40, 0x91); // LCD + BG on
    run_to(&mut p, 1, 0); // steady-state line 1 (glitch cleared)
    p.line_render_done = true; // the dispatch flip already fired
    p.vis_early = true;
    // SCX 5 window: SameBoy holds mode 3 until dot 268 (= 263 + 5).
    p.vis_hold_until = 268;
    p.dot = 264;
    assert_eq!(p.vis_mode(), 3, "held: dot < vis_hold_until reads mode 3");
    p.dot = 268;
    assert_eq!(
        p.vis_mode(),
        0,
        "released: dot >= vis_hold_until reads mode 0"
    );
    // No hold (production / non-win-active): the dispatch read is mode 0.
    p.vis_hold_until = 0;
    p.dot = 264;
    assert_eq!(
        p.vis_mode(),
        0,
        "no hold: dispatch flip reads mode 0 (byte-identical)"
    );
}
