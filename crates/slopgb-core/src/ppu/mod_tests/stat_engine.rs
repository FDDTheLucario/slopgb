//! `mod_tests` — stat tests: engine / lcd-enable-table / misc group (split for file size).

use super::*;

/// The STAT engine's interrupt line is held low while the LCD is off
/// (SameBoy `GB_STAT_update` early-returns, `display.c:525`), so a re-enable
/// edge-detects from a clean low. A held-high line at the off transition is
/// cleared.
#[test]
fn stat_update_engine_lcd_off_holds_line_low() {
    let mut p = dmg();
    p.write(0xFF45, 3);
    p.write(0xFF41, 0x40); // LYC enable
    p.write(0xFF40, 0x91);
    // Drive to the LYC=3 match so the engine line is high.
    run_to(&mut p, 3, 4);
    assert!(p.stat_update_line(), "engine line high on the LYC=3 match");
    // Turn the LCD off: the next tick resets the engine line low.
    p.write(0xFF40, 0x00);
    p.tick();
    assert!(
        !p.stat_update_line(),
        "engine line forced low with the LCD off"
    );
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
            [0x84, 0x87, 0x84, 0x80, 0x82, 0x80, 0x80, 0x82],
            [0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82, 0x83],
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
            [0x80, 0x83, 0x80, 0x80, 0x86, 0x84, 0x80, 0x82],
            [0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82, 0x83],
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

/// The [`Ppu::stat_update_tick`] halt-commit-mask calibration. The mode-2
/// (OAM) line-start pulse takes the **halt-exit** mask (`stat_halt_late`) but
/// NOT the interrupt-sample mask (`stat_late`) on the cc+0 path: the halt mask
/// delays the canonical mooneye `intr_2_mode0_timing` halt-wake (which then
/// passes), while dropping the sample mask keeps the non-halt `m2int_m3stat_1`
/// `ldh a,(FF41)` dispatch in SameBoy's frame so its read still lands on mode 3.
/// Applying `stat_late` here would re-collapse that pair.
#[test]
fn stat_update_mode2_pulse_halt_mask_only_flag_on() {
    let mut p = dmg();
    p.write(0xFF41, 0x20); // OAM (mode-2) source only — no hblank/lyc
    p.write(0xFF40, 0x91); // LCD + BG on, bare line
    // Sit at the end of a visible line; its successor's dot-0 pulse is the
    // next mode-2 rising edge.
    run_to(&mut p, 2, 455);
    p.take_stat_halt_late();
    p.take_stat_late();
    let ifs = p.tick(); // advances to line 3 dot 0, fires the OAM pulse
    assert_eq!(
        ifs & 2,
        2,
        "mode-2 OAM pulse fires at the visible line start"
    );
    assert!(
        p.take_stat_halt_late(),
        "the mode-2 line-start pulse takes the halt-exit mask"
    );
    assert!(
        !p.take_stat_late(),
        "but NOT the interrupt-sample mask (the leading-edge dispatch is already framed)"
    );
}

/// The mode-0 (HBlank) source rise carries the half-cycle halt law
/// (`m0_rise`), set on its `m0_rise_dot`.
#[test]
fn stat_update_mode0_rise_takes_m0_rise_flag_on() {
    let mut p = dmg();
    p.write(0xFF41, 0x08); // HBlank (mode-0) source only
    p.write(0xFF40, 0x91); // LCD + BG on, bare line
    run_to(&mut p, 2, 0);
    p.take_m0_rise();
    // Drive through the mode-3→0 flip; the rise sets `m0_rise` on its dot.
    let mut saw_rise = false;
    for _ in 0..456 {
        let ifs = p.tick();
        if ifs & 2 != 0 && p.take_m0_rise() {
            saw_rise = true;
            break;
        }
    }
    assert!(
        saw_rise,
        "the mode-0 source rise carries the m0_rise halt mask"
    );
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
