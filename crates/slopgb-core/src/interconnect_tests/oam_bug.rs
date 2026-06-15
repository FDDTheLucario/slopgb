//! `interconnect_tests` — oam_bug tests (split for file size).

use super::*;

#[test]
fn oam_bug_read_in_mode2_corrupts_on_dmg_family_only() {
    for model in [Model::Dmg, Model::Dmg0, Model::Mgb, Model::Sgb, Model::Sgb2] {
        let mut b = ic_lcd_on(model);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        assert_eq!(b.read(0xFE00), 0xFF, "{model:?}: OAM still locked");
        let after = oam_snapshot(&b);
        // Read pattern at row 0x20: glitched word in rows 3 *and* 4,
        // row tail copied from row 3.
        let glitched = before[0x18] | (before[0x20] & before[0x1C]);
        assert_eq!(after[0x20], glitched, "{model:?}");
        assert_eq!(after[0x18], glitched, "{model:?}");
        assert_eq!(after[0x22..0x28], before[0x1A..0x20], "{model:?}");
        assert_eq!(after[..0x18], before[..0x18], "{model:?}: earlier rows");
    }
    for model in [Model::Cgb, Model::Agb] {
        let mut b = ic_lcd_on(model);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        b.read(0xFE00);
        assert_eq!(oam_snapshot(&b), before, "{model:?}: no bug on CGB");
    }
}

#[test]
fn oam_bug_triggers_across_the_whole_fexx_page_only() {
    // The trigger keys on the address byte $FE on the bus: the
    // FEA0-FEFF prohibited area corrupts like OAM proper (blargg
    // oam_bug/8-instr_effect pops from $FEF0), neighbours do not.
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    b.read(0xFEA0);
    assert_ne!(oam_snapshot(&b), before, "prohibited-area read corrupts");
    for addr in [0xFDFF, 0xFF00] {
        let mut b = ic_lcd_on(Model::Dmg);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        b.read(addr);
        assert_eq!(oam_snapshot(&b), before, "read {addr:#06x} is inert");
    }
}

#[test]
fn oam_bug_write_corrupts_with_write_pattern_and_is_dropped() {
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    b.write(0xFE21, 0x77);
    let after = oam_snapshot(&b);
    for i in 0..2 {
        let (a, p0, p2) = (before[0x20 + i], before[0x18 + i], before[0x1C + i]);
        assert_eq!(after[0x20 + i], ((a ^ p2) & (p0 ^ p2)) ^ p2, "byte {i}");
    }
    assert_eq!(after[0x22..0x28], before[0x1A..0x20], "row tail copied");
    assert!(
        !after.contains(&0x77),
        "the blocked CPU write must not land"
    );
}

#[test]
fn oam_bug_internal_cycle_value_corrupts_via_tick_addr() {
    // INC rr's internal cycle carries no memory access; the register
    // value alone triggers the write pattern (blargg oam_bug/2-causes).
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    Bus::tick_addr(&mut b, 0xFE00);
    let after = oam_snapshot(&b);
    for i in 0..2 {
        let (a, p0, p2) = (before[0x20 + i], before[0x18 + i], before[0x1C + i]);
        assert_eq!(after[0x20 + i], ((a ^ p2) & (p0 ^ p2)) ^ p2, "byte {i}");
    }
    assert_eq!(after[0x22..0x28], before[0x1A..0x20]);
    // Out-of-range values are inert (blargg oam_bug/3-non_causes).
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    Bus::tick_addr(&mut b, 0xFDFF);
    Bus::tick_addr(&mut b, 0xFF00);
    assert_eq!(oam_snapshot(&b), before);
}

#[test]
fn oam_bug_increase_read_uses_the_read_increase_pattern() {
    // POP/LD A,(HL+) style reads: special pattern at rows 4..=18
    // (SameBoy v0.12.1 GB_trigger_oam_bug_read_increase).
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    assert_eq!(Bus::read_inc(&mut b, 0xFE05), 0xFF);
    let after = oam_snapshot(&b);
    let mut prev = [0u8; 8];
    prev.copy_from_slice(&before[0x18..0x20]);
    for i in 0..2 {
        let (a, p0, c, d) = (
            before[0x10 + i],
            before[0x18 + i],
            before[0x20 + i],
            before[0x1C + i],
        );
        prev[i] = (p0 & (a | c | d)) | (a & c & d);
    }
    for i in 0..8 {
        assert_eq!(after[0x10 + i], prev[i], "two rows back {i}");
        assert_eq!(after[0x18 + i], prev[i], "preceding row {i}");
        assert_eq!(after[0x20 + i], prev[i], "current row {i}");
    }
}

#[test]
fn oam_bug_suppressed_while_the_core_clock_is_gated() {
    // The halted CPU performs no bus accesses on hardware; the
    // discarded halt prefetch (see cpu::Bus docs) must stay
    // side-effect-free even with PC in $FExx.
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    b.set_cpu_halted(true);
    b.read(0xFE00);
    assert_eq!(oam_snapshot(&b), before, "halted: no corruption");
    b.set_cpu_halted(false);
    park_before_oam_row(&mut b, 0x20);
    b.read(0xFE00);
    assert_ne!(oam_snapshot(&b), before, "running again: corruption");
}

#[test]
fn oam_bug_suppressed_while_oam_dma_copies() {
    // While the DMA engine owns OAM, CPU-side $FExx traffic does not
    // corrupt (the interplay is untested on hardware — SameBoy leaves
    // the same Todo — so the conservative gate wins). The DMA source
    // mirrors the OAM contents so the copy itself is invisible.
    let mut b = ic_lcd_on(Model::Dmg);
    for i in 0..0xA0u16 {
        b.write(0xC000 + i, (i as u8) ^ 0xA5);
    }
    park_before_oam_row(&mut b, 0x10);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    b.write(0xFF46, 0xC0);
    b.tick(); // setup delay
    b.tick(); // first byte copies; the engine owns OAM from here
    b.read(0xFE00); // still inside the scan window (row 0x28)
    assert_eq!(oam_snapshot(&b), before);
}

#[test]
fn oam_bug_inert_outside_the_scan_window() {
    // blargg oam_bug/6-timing_no_bug: accesses bracketing the per-line
    // window, hammering vblank, and with the LCD off are all clean.
    let access_all = |b: &mut Interconnect| {
        let keep = b.peek(0xFE00);
        b.read(0xFE00);
        Bus::tick_addr(b, 0xFE00);
        Bus::read_inc(b, 0xFE00);
        b.write(0xFE00, keep); // may land outside mode 2/3: same value
    };
    // VBlank.
    let mut b = ic_lcd_on(Model::Dmg);
    while b.ppu.mode_bits() != 1 {
        b.tick();
    }
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    access_all(&mut b);
    assert_eq!(oam_snapshot(&b), before, "vblank");
    // Mode 3 (entered fresh, lasts >= 43 M-cycles).
    let mut b = ic_lcd_on(Model::Dmg);
    while b.ppu.mode_bits() != 2 {
        b.tick();
    }
    while b.ppu.mode_bits() != 3 {
        b.tick();
    }
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    access_all(&mut b);
    assert_eq!(oam_snapshot(&b), before, "mode 3");
    // HBlank right after that mode 3.
    while b.ppu.mode_bits() != 0 {
        b.tick();
    }
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    access_all(&mut b);
    assert_eq!(oam_snapshot(&b), before, "hblank");
    // LCD off.
    let mut b = ic_lcd_on(Model::Dmg);
    b.write(0xFF40, 0x00);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    access_all(&mut b);
    assert_eq!(oam_snapshot(&b), before, "LCD off");
}
