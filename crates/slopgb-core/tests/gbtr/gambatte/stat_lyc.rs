//! STAT / LYC / LY line-timing pinned-behavior tests.

use super::*;

/// Mooneye `lcdon_timing-GS` on the deferred reclock. The
/// test reads LY/STAT/OAM/VRAM at fixed cycle counts after an LCD enable. Three
/// glitch/post-glitch frame corrections, all the same shape (the
/// Tier-2 deferred read does not take the leading-edge-only −4 back-date):
/// the glitch mode-3 ENTRY (`stat_irq.rs`, 74→78), the glitch LYC readable
/// compare (`lyc.rs`, drop the last-4-dots line-1 view), and the bare-line
/// mode-0 EXIT (`mode0.rs early_lead` 1→0, the post-glitch line-1 STAT read).
/// Passes DMG + SGB (the -GS models; CGB/AGB fail on hardware per the ROM
/// header). Production (flag-off) byte-identical — all three gated on
/// `tier2_reclock`. Must boot WITH the reclock (the bare-line exit is decided
/// during rendering, not at hand-off, but the construction path matches the
/// real flip).
#[test]
fn tier2_lcdon_timing_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("tier2_lcdon", "game-boy-test-roms collection not present");
        return;
    };
    let rel = "mooneye-test-suite/acceptance/ppu/lcdon_timing-GS.gb";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Sgb] {
        let mut gb = harness::boot_with_reclock(&rom, model);
        harness::run_until_breakpoint(&mut gb, 30_000_000)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
        harness::check_fib(&gb)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
    }
}

/// Mooneye `hblank_ly_scx_timing-GS` on the deferred
/// reclock, the last mooneye gate-blocker. The test reads LY at a fixed delay
/// after a mode-0 STAT IRQ halt-wake and checks the LY-increment latency vs
/// SCX. slopgb's M-cycle-quantized halt-wake collapsed the sub-M-cycle wake
/// phase (SCX pairs that wake 1 clock apart read the same LY); the fix carries
/// the rise's within-M-cycle phase to the first post-wake FF44 read
/// (`Interconnect::halt_ly_phase`, one-shot, so the pre-halt `wait_ly` poll is
/// untouched). Passes DMG + SGB (the -GS models). Production (flag-off)
/// byte-identical (gated on `tier2_reclock`); int_hblank (TIMA) and intr_2
/// (mode-2 wake) are unaffected. Must boot WITH the reclock (the wake is
/// timer/halt-coupled to the C0 +4 frame the flip installs at construction).
#[test]
fn tier2_hblank_ly_scx_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("tier2_hblank", "game-boy-test-roms collection not present");
        return;
    };
    let rel = "mooneye-test-suite/acceptance/ppu/hblank_ly_scx_timing-GS.gb";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Sgb] {
        let mut gb = harness::boot_with_reclock(&rom, model);
        harness::run_until_breakpoint(&mut gb, 30_000_000)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
        harness::check_fib(&gb)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
    }
}

/// The vblank-entry mode-1 STAT re-arm on
/// the deferred reclock. `lycint143_m1irq_2` enables the mode-1 (VBlank) STAT
/// source with LYC=143: hardware services the ly143 LYC-STAT IRQ, then the
/// mode-1 line rises again at line-144 entry (SameBoy `SBLEVEL ly=144 cfl=0`
/// `lyc_line 1->0` then `0->1 mfi=1`, `IF |= 2`), so the post-service read sees
/// STAT bit 1 again (`if=03`, screen `3`). slopgb's `stat_update_tick` held the
/// ly143 LYC match latched across line 144's `ly_for_comparison == -1` gap, so
/// the line never dipped and the mode-1 rise produced no fresh edge (read
/// `if=01`, screen `1`). The fix drops that carried-over LYC match at line 144
/// entry when VBlank is armed (`stat_irq.rs`); the natural dot-4 mode-1 rise is
/// then a real 0→1 edge. Production (flag-off) byte-identical — `stat_update_tick`
/// runs only on the leading-edge / Tier-2 path. SameBoy passes this ROM.
#[test]
fn tier2_m1_vblank_rearm_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m1_rearm",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rel = "gambatte/m1/lycint143_m1irq_2_dmg08_cgb04c_out3.gbc";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Cgb] {
        let mut gb = harness::boot_with_reclock(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "3", model.is_cgb())
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] expected out3 (tier2 flag-on): {e}"));
    }
}

/// The line-0 VBlank carry suppresses the
/// spurious line-0 STAT edge on the deferred reclock. `lycwirq_trigger_ly00_
/// stat50_1` enables LYC (LYC=0) + VBlank with the STAT line held high across
/// the ly153→ly0 wrap; SameBoy never re-sets `mode_for_interrupt` between the
/// line-144 entry (`= 1`) and line 0's OAM step (`= 2`), so its STAT line stays
/// continuously high and raises NO fresh edge on line 0 (`out=E0`). slopgb's
/// `mode_for_interrupt` had read `vis_mode` (mode 0) across line 0 dots 0-3,
/// dropping the line so a line-0 source rise became a spurious edge (`out=E2`).
/// The fix carries the VBlank source (mode 1) there (`stat_irq.rs::update_mode_
/// for_interrupt`), decoupled from the visible FF41 mode-0. Production (flag-off)
/// byte-identical — `mode_for_interrupt` is inert there. DMG-only: CGB reads
/// mode 1 across line-0 dots 0-3 already (byte-identical, a separate residual).
/// SameBoy passes the DMG `out=E0` side.
#[test]
fn tier2_line0_vblank_carry_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_line0_vblank",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rel = "gambatte/lycEnable/lycwirq_trigger_ly00_stat50_1_dmg08_cgb04c_outE0.gbc";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    let model = Model::Dmg;
    let mut gb = harness::boot_with_reclock(&rom, model);
    run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
    check_hex_screen(gb.frame(), "E0", model.is_cgb())
        .unwrap_or_else(|e| panic!("{rel} [{model:?}] expected outE0 (tier2 flag-on): {e}"));
}

/// The CGB LCD-enable glitch-line mode-0 IRQ dispatch
/// reclock (`stat_irq.rs::update_mode_for_interrupt`). The glitch-line mode-0
/// STAT IRQ now keys on `line_render_done` (the dispatch dot, dot 254 = SameBoy
/// cfl=257), NOT on `vis_early` (dot 252) as `vis_mode` does — the bare-line
/// law. `enable_display/frame0_m0irq_count` polls FF0F at a calibrated dot ~252
/// each line expecting the mode-0 STAT bit NOT yet set (the loop runs until the
/// VBlank bit at LY=144 → renders 0x90); keying the engine on the early
/// `vis_early` raised the bit a poll early on the glitch line, so the dot-252
/// poll observed it and the ROM branched to read LY=0 (`out=00`). SameBoy
/// renders 90 (it raises the glitch-line mode-0 at the same cfl=257 as every
/// bare line). `ly0_m0irq_scx1` is the sibling (`outE0`). Production (flag-off)
/// byte-identical — `mode_for_interrupt` is inert there. This pin covers the
/// **CGB** side of both rows; the DMG side splits: `frame0_m0irq_count` DMG
/// stays a baselined floor (a dispatch-COUNT the reclock's cc+0 frame loses —
/// the poll at ~dot252 never sees the rise it must count), while the DMG
/// `ly0_m0irq_scx1_1` READ-frame half ships in `tier2_dmg_m0_coincident_passes`
/// — a verdict-only co-instant read-view mask ([`Ppu::ff0f_dmg_m0_coincident_mask`],
/// [`Ppu::ff0f_stat_peek`]'s file) that clears the bit for a read landing EXACTLY
/// on the flip dot WITHOUT moving the rise, so the `int_hblank_halt` halt-wake
/// grid (which needs the rise at its dispatch dot) is untouched (`int_hblank_halt`
/// + gbmicro 445 green).
#[test]
fn tier2_glitch_m0irq_dispatch_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_glitch_m0irq",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    for (rel, want) in [
        (
            "gambatte/enable_display/frame0_m0irq_count_scx2_1_dmg08_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/enable_display/ly0_m0irq_scx1_1_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
    ] {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), want, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{want} (tier2 flag-on): {e}"));
    }
}

/// The DMG LCD-enable glitch-line mode-0 co-instant
/// FF0F read-view mask ([`Ppu::ff0f_dmg_m0_coincident_mask`]). The third
/// read-frame pass (after the hblank +16 / poweron +20 passes): the DMG face of
/// the read-frame half the `tier2_glitch_m0irq_dispatch_passes` doc parked
/// as "a genuine multi-mechanism atomic ... byte-identical DMG floor". Corrected:
/// `enable_display/ly0_m0irq_scx1_1` polls FF0F (DI, `IE=0`) on the glitch line
/// with the mode-0 STAT armed, reading EXACTLY on the recorded mode-0 flip dot
/// (slopgb `dot253 == flip_dot253`, == SameBoy cfl257). SameBoy's `read_high_memory`
/// orders the CPU read BEFORE the STAT rise at that shared instant → E0 (the bit
/// not yet risen); slopgb's whole-dot frame folds the rise first and commits the
/// set bit → E2. The mask clears the STAT bit for a read AT the flip dot — EXACT,
/// never a window: the `_2` sibling reads dot257 > flip (poll after the rise, E2)
/// and `scx0_2` reads flip+1 (E2), so both are untouched. **Verdict-only** — the
/// rise/dispatch never moves, so the co-located `int_hblank_halt` halt-wake grid
/// (the conflicting-dot atomicity the dispatch doc cited) stays green, the same
/// decoupling as the hblank/poweron passes. `frame0_m0irq_count` DMG (the
/// dispatch-COUNT sibling, poll at dot252 ≠ flip) stays a floor. +1 full-DMG
/// two-bin (0 SameBoy-pass dropped); `tier2` + `!is_cgb` + `glitch_line` + SS
/// scoped → production and CGB byte-identical.
#[test]
fn tier2_dmg_m0_coincident_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("tier2_dmg_m0_coincident", "collection not present");
        return;
    };
    let rel = "gambatte/enable_display/ly0_m0irq_scx1_1_dmg08_cgb04c_outE0.gbc";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    let mut gb = harness::boot_with_reclock(&rom, Model::Dmg);
    run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
    check_hex_screen(gb.frame(), "E0", false)
        .unwrap_or_else(|e| panic!("{rel} [Dmg] expected outE0 (tier2 flag-on): {e}"));
}

/// The LYC-write sub-case: the line-start
/// LYC-carryover hold suppresses a spurious wrap edge on the deferred reclock.
/// `lyc0_late_ff45_enable_3` enables the LYC source (LYC=0) and writes FF45=0
/// late, at the ly0→ly1 wrap. SameBoy re-evaluates `lyc_interrupt_line` only at
/// the line-start `GB_SLEEP` steps that *set* `ly_for_comparison` (state-6 = -1
/// holds, state-7 = N re-latch) — never during the HELD carryover where
/// `ly_for_comparison` still names the previous line — so the late write
/// (landing at `ly1` with `ly_for_comparison == -1`, `SBWRITE ff45 ly=1 lyfc=-1
/// val=0`) raises no fresh LYC edge (`out=E0`). slopgb's per-dot
/// `stat_update_tick` re-latched the carryover `line - 1` (= 0) against the
/// freshly-written LYC=0 → a spurious `ly1 dot0` STAT edge (`out=E2`). The fix
/// holds the latch across the carryover dots 0-2 like the `-1` gap
/// (`stat_irq.rs::stat_update_tick`). Production (flag-off) byte-identical —
/// `stat_update_tick` runs only on the leading-edge / Tier-2 path. DMG-family
/// only: on CGB the LCD-offset rows shift SameBoy's grid so the offset-shifted
/// LYC edge lands on slopgb's carryover dot as a mis-dotted *real* edge (not a
/// spurious one); without a `lcd_offset` port the hold can't tell them apart, so
/// CGB stays a residual. SameBoy passes the DMG `out=E0` side.
#[test]
fn tier2_lyc_carryover_late_ff45_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_lyc_carryover",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rel = "gambatte/lycEnable/lyc0_late_ff45_enable_3_dmg08_cgb04c_outE0.gbc";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    let model = Model::Dmg;
    let mut gb = harness::boot_with_reclock(&rom, model);
    run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
    check_hex_screen(gb.frame(), "E0", model.is_cgb())
        .unwrap_or_else(|e| panic!("{rel} [{model:?}] expected outE0 (tier2 flag-on): {e}"));
}

/// The CGB last-M-cycle LYC-write hold: the line-END complement of
/// [`tier2_lyc_carryover_late_ff45_passes`]'s
/// DMG line-START carryover hold. A late FF45 write in slopgb's leading-edge
/// write frame commits 1 M-cycle earlier than SameBoy, on the current line's
/// last M-cycle (dot >= 452), where the freshly-matching just-written LYC
/// re-latched `lyc_interrupt_line` → a spurious last-dot STAT edge SameBoy never
/// fires (its write lands the NEXT line's `cfl0`, the held carryover / `lyfc=-1`
/// — measured SBWRITE/SBLEVEL). Holding the latch across the last M-cycle
/// (`dot >= 452`, the boundary-write threshold `write_lyc_cgb` uses) carries the
/// just-written LYC into the next line unchanged; an earlier write (e.g.
/// `lyc0_late_ff45_enable_1`'s dot 449) still re-latches/fires. Three CGB rows,
/// three lines (0/6→7/1→2): `lyc0_late_ff45_enable_2` (LYC=0, wrap, outE0),
/// `lyc153_late_ff45_enable_2` (LYC=153 at the vblank wrap, outE0),
/// `m2enable/lyc1_m2irq_late_lyc255_2` (late LYC=255 disabling the match, out0).
/// Single-speed-only (the DS last M-cycle is 2 dots, +1 offset — `dot >= 452`
/// inverts the SameBoy-passing `_ds_1` siblings); CGB-only; LE-Tier2-only;
/// production byte-identical (`reclock.rs::stat_update_tick`).
#[test]
fn tier2_lyc_carryover_late_ff45_cgb_wrap_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_lyc_carryover_cgb",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str); 3] = [
        (
            "gambatte/lycEnable/lyc0_late_ff45_enable_2_dmg08_outE2_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/lycEnable/lyc153_late_ff45_enable_2_dmg08_outE2_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/m2enable/lyc1_m2irq_late_lyc255_2_dmg08_out2_cgb04c_out0.gbc",
            "0",
        ),
    ];
    let model = Model::Cgb;
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
            panic!("{rel} [{model:?}] expected cgb04c out{expect} (tier2 flag-on): {e}")
        });
    }
}

/// CGB lcd-offset, the m3-start palette-RAM window:
/// SameBoy keeps `cgb_palettes_blocked = false` for 3 T-cycles INTO mode 3
/// (`display.c:1867` false → `:1877` true, a 3-cycle SLEEP between), so a deferred
/// palette access landing at the mode-3 entry stays accessible. The lcd-offset
/// shifts the `cgbpal_m3/*_m3start_lcdoffset1_1` access into that window (slopgb
/// `ly1 dot86` vs SameBoy's ~cfl87 lock), where slopgb — locking palette RAM at
/// dot 84 — read FF / dropped the write. The fix extends the `pal_ram_blocked`
/// mode-3 lock by `PAL_M3START_OPEN` on CGB single-speed under Tier-2
/// (`ppu/blocking.rs`), covering both the read (out00) and write (out01) legs.
/// Production (flag-off) byte-identical. Probe (654 CGB baseline rows, flag-on):
/// +2/−0.
#[test]
fn tier2_cgbpal_m3start_lcdoffset1_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_cgbpal_m3start_lcdoffset1",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/cgbpal_m3/cgbpal_read_m3start_lcdoffset1_1_cgb04c_out00.gbc",
            "00",
        ),
        (
            "gambatte/cgbpal_m3/cgbpal_write_m3start_lcdoffset1_1_cgb04c_out01.gbc",
            "01",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// CGB lcd-offset, the dispatch-class HBlank
/// write-trigger: a fresh mode-0 (HBlank) STAT enable written in the
/// line-start hblank carryover (dots 0-3, `vis_mode==0`, the previous line's
/// mode-0 already passed) must raise IF AT the write: the gambatte logic there
/// defers to the scheduled m0irq event, but in the carryover that event points
/// at the next line's mode-0 (beyond the LY increment), so the deferral loses
/// the IRQ before the cc+0 read. The lcd-offset shifts these late enables into
/// that carryover tail (`late_enable_lcdoffset1_1` writes FF41 at `ly dot3`),
/// where SameBoy raises IF at the write — slopgb delivered `if=00` (out0)
/// instead of out2/out3. The fix raises IF for a fresh carryover-tail m0 enable
/// under Tier-2 (`ppu/stat_irq.rs::stat_write_trigger_cgb`, glitch excluded).
/// CGB-only (the DMG trigger is a separate fn → byte-identical). Probe (654 CGB
/// baseline rows, flag-on): +4/−0 (`m0enable/late_enable_lcdoffset1_1`,
/// `m1/ly143_late_m0enable_lcdoffset1_1`, + double-speed `late_enable_ds_1` and
/// `late_enable_ds_lcdoffset1_1` riding the same lever).
#[test]
fn tier2_m0enable_late_lcdoffset_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m0enable_late_lcdoffset",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/m0enable/late_enable_lcdoffset1_1_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/m1/ly143_late_m0enable_lcdoffset1_1_cgb04c_out3.gbc",
            "3",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// CGB lcd-offset, the dispatch-class HBlank
/// write-trigger, DOUBLE-SPEED window. Sibling of
/// [`tier2_m0enable_late_lcdoffset_passes`]: under double speed the deferred cc+0
/// write lands 2 dots earlier, so the carryover fire window halves (`carryover_tail
/// = dot < 2` in DS, `stat_irq.rs::stat_write_trigger_cgb`). The `_ds*_1` enable
/// lands `dot0` (fires, out1/out2/out3) while the `_2` sibling lands `dot2` —
/// where SameBoy's fire is early/cleared by the test, so it must NOT be delivered
/// (out0). A `dot < 4` window over-fired the `_2` enable; halving fixed it plus
/// the base `late_enable_ds_2` and the `lyc143_late_m0enable_lycdisable_ds_1`
/// bonus rows. The `_2` legs are pinned as regression guards. CGB DS only,
/// byte-identical OFF. Probe (3524 CGB rows, flag-on): +3/−0.
#[test]
fn tier2_m0enable_late_ds_lcdoffset_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m0enable_late_ds_lcdoffset",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // The +3 fixed by the DS carryover-window halving.
        (
            "gambatte/m0enable/late_enable_ds_lcdoffset1_2_cgb04c_out0.gbc",
            "0",
        ),
        ("gambatte/m0enable/late_enable_ds_2_cgb04c_out1.gbc", "1"),
        (
            "gambatte/m1/lyc143_late_m0enable_lycdisable_ds_1_cgb04c_out1.gbc",
            "1",
        ),
        // The `_1` siblings that must keep firing (dot0 still inside `< 2`).
        (
            "gambatte/m0enable/late_enable_ds_lcdoffset1_1_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/m1/ly143_late_m0enable_ds_lcdoffset1_1_cgb04c_out3.gbc",
            "3",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// CGB lcd-offset, the dispatch-class VBlank + LYC
/// write-triggers: the lcd-offset shifts these late STAT enables into the
/// line-start dots-0-3 carryover, where the base gambatte logic suppresses them.
/// `m1/m1irq_late_enable_lcdoffset1_1` enables the VBlank source at `ly0 dot3`
/// (the `m1_tail` suppression, `stat_irq.rs`); `lycEnable/late_ff41_enable_lcdoffset1_1`
/// enables the LYC source at `ly7 dot3` with `LYC=ly-1` (the carryover compare —
/// `cmp_cgb` has switched to the new line so `lyc_high` is false). SameBoy fires
/// both at the write; slopgb delivered `if=00` (out0) instead of out2. The fix
/// (Tier-2, `stat_write_trigger_cgb`): drop the `m1_tail` suppression for a fresh
/// VBlank enable, and fire a fresh LYC enable whose LYC matches the PREVIOUS line
/// in the carryover. CGB-only. Probe (654 CGB rows, flag-on): +2/−0, the
/// line-0/lyc pins held.
#[test]
fn tier2_m1_lyc_late_lcdoffset_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m1_lyc_late_lcdoffset",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        "gambatte/m1/m1irq_late_enable_lcdoffset1_1_cgb04c_out2.gbc",
        "gambatte/lycEnable/late_ff41_enable_lcdoffset1_1_cgb04c_out2.gbc",
    ];
    for rel in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "2", true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out2 (tier2 flag-on): {e}"));
    }
}

/// CGB lcd-offset, the lyc-engine dispatch tail.
/// Four clean Tier-2 levers, +4/−0 flag-on, all in the line-153 wrap / line-start
/// `-1` gap the lcd-offset shifts the LYC write/enable into:
/// * `lyc153_late_ff41_enable_lcdoffset1_1` (outE2) — the FF41 LYC enable lands at
///   `ly153 dot11` where `cmp_cgb` wrapped to 0; the held `lyc_interrupt_line`
///   latch (153, dropped at dot 12) is still high, so the fresh enable fires
///   (`stat_irq.rs::stat_write_trigger_cgb` `lyc_wrap_153`).
/// * `lyc153_late_ff45_enable_lcdoffset1_1` (outE2) — the FF45 write (LYC=153)
///   lands at `ly153 dot7` where the gambatte `target` wraps to Some(0); the
///   reclock `ly_for_comparison` is still 153, so it fires (`lyc.rs` `lyc_write_wrap_153`).
/// * `ff45_enable_weirdpoint_lcdoffset1_2` (out0) — the FF45 write lands in the
///   `ly_for_comparison == -1` line-start gap (dot 3), where SameBoy makes no fresh
///   match; the gambatte `target` Some(line) would spuriously fire — suppressed
///   (`lyc.rs` `tier2_minus1_gap`, lines 1-143).
/// * `lyc0_late_ff45_enable_3` (outE0, CGB) — the ly0→ly1 LYC=0 wrap: slopgb's
///   offset-shifted write leaves ly0 unmatched, then re-rises at the ly1 dot-0
///   carryover (`ly_for_comparison=line-1=0`); the line-1 carryover hold
///   (`reclock.rs::stat_update_tick`, CGB line 1 only) drops the spurious re-latch.
///
/// CGB-only; production (flag-off) byte-identical. The named `lycwirq_trigger_ly00_
/// stat50_lcdoffset1_1` (outE0) stays floored: its dispatch now matches SameBoy
/// (spurious ly0/ly1 gone) but the legit LYC=153 IRQ's dispatch dot (slopgb dot6
/// vs SameBoy cfl0) + the offset-shifted FF0F read position are the read-observer
/// read-frame residual. Probe (676 CGB engine-family rows, flag-on): +4/−0.
#[test]
fn tier2_lyc_wrap_lcdoffset_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_lyc_wrap_lcdoffset",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/lycEnable/lyc153_late_ff41_enable_lcdoffset1_1_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/lycEnable/lyc153_late_ff45_enable_lcdoffset1_1_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/lycEnable/ff45_enable_weirdpoint_lcdoffset1_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/lycEnable/lyc0_late_ff45_enable_3_dmg08_cgb04c_outE0.gbc",
            "E0",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// The CGB double-speed line-153 `ly_for_comparison` table (the
/// documented SS placeholder replaced): 153 latches at dot 4 (not 6), holds
/// live through dot 7, the [8,12) window is the `-1` GAP (held for a latched
/// match, no fresh LYC-write re-latch — `lyc153_late_ff45_enable_ds_6`), 0
/// from dot 12 — the unique whole-dot solution to the four
/// `lyc153_m1disable_ds` / `lyc0_m1disable_ds` dip-vs-seamless handoff
/// constraints, with the DS engine view immediate (the two-phase window is
/// sub-dot at 2 dots/M). The dot-4 wake cascades through every
/// LYC=153-anchored DS test: the gdma_cycles read frame, the lcd_offset
/// count-loop first poll, and the late_wy write instants (whose un-matching
/// write now beats the hardware `wy_check` — the trigger-line WY un-latch).
#[test]
fn tier2_ds_line153_lyfc_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_ds_line153",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/lycEnable/lyc153_m1disable_ds_2_cgb04c_outE0.gbc",
            "E0",
        ),
        // GUARDs — the dip legs still fire.
        (
            "gambatte/lycEnable/lyc153_m1disable_ds_1_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/lycEnable/lyc0_m1disable_ds_1_cgb04c_outE2.gbc",
            "E2",
        ),
        // The [8,12) gap takes no fresh LYC-write re-latch.
        (
            "gambatte/lycEnable/lyc153_late_ff45_enable_ds_6_cgb04c_outE0.gbc",
            "E0",
        ),
        (
            "gambatte/ly0/lycint152_lyc153irq_ds_2_cgb04c_outE2.gbc",
            "E2",
        ),
        (
            "gambatte/ly0/lycint152_lyc153irq_ifw_ds_2_cgb04c_outE0.gbc",
            "E0",
        ),
        // The dot-4 wake cascades.
        (
            "gambatte/dma/gdma_cycles_short_scx5_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/lcd_offset/offset1_lyc99int_m0irq_count_scx1_ds_1_cgb04c_out90.gbc",
            "90",
        ),
        ("gambatte/window/late_wy_ds_2_cgb04c_out3.gbc", "3"),
        // The trigger-line WY un-latch (write beats the check at commit
        // dot <= 4). Its `_2` sibling (keeps the trigger, out3) is
        // frame-phase-sensitive at this pin's sample point — covered by the
        // two-bin instead.
        (
            "gambatte/window/arg/late_wy_1toFF_ds_1_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// The held-LYC pre-write-high suppression on the Tier-2
/// carryover-tail m0-enable write fire (`stat_write_trigger_cgb`): a line-144
/// dots-0-3 FF41 write whose OLD value armed LYC with the engine latch still
/// held (the lyfc-gap hold) rewrites an already-HIGH line — no 0→1 edge on
/// hardware. `cmp_cgb` has switched to the new line so the top-of-fn
/// `lyc_high` check misses the held latch.
#[test]
fn tier2_m1_lycdisable_boundary_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m1_lycdisable_boundary",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // The boundary write (line-144 dot ~1-2, old=0x40 held): silent.
        (
            "gambatte/m1/lyc143_late_m0enable_lycdisable_2_dmg08_cgb04c_out1.gbc",
            "1",
        ),
        // GUARDs — the earlier/later legs stay silent for their own reasons
        // (line-143 tail deferral / past the carryover window).
        (
            "gambatte/m1/lyc143_late_m0enable_lycdisable_1_dmg08_cgb04c_out1.gbc",
            "1",
        ),
        (
            "gambatte/m1/lyc143_late_m0enable_lycdisable_3_dmg08_out3_cgb04c_out1.gbc",
            "1",
        ),
        (
            "gambatte/m1/lyc143_late_m0enable_lycdisable_ds_1_cgb04c_out1.gbc",
            "1",
        ),
        (
            "gambatte/m1/lyc143_late_m0enable_lycdisable_ds_2_cgb04c_out1.gbc",
            "1",
        ),
        // GUARD — an old=0x00 carryover-tail enable still fires. (The SS
        // `ly143_late_m0enable_2` sibling is a pre-existing flag-on fail —
        // not pinnable.)
        ("gambatte/m1/ly143_late_m0enable_ds_1_cgb04c_out3.gbc", "3"),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// The co-instant line-0 dot-4 OAM-pulse FF0F read-view
/// mask, LYC-153-anchored: the LYC-153 ISR's IF read lands BEFORE the line-0
/// pulse in SameBoy's frame (dot 3, rise −1) while slopgb's deferred read
/// collapses onto the pulse dot and saw the just-folded bit. CPU-read-first
/// at the shared instant (SameBoy-measured: SBREAD ff0f at the rise fp reads
/// clear). The LYC-152 ISR's same-dot-4 collapse lands 4 dots AFTER the rise
/// on SameBoy and must SEE it — the `self.lyc == 153` anchor guard (built
/// unguarded first: +1/−2 A/B, measured).
#[test]
fn tier2_ly0_pulse_readview_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_ly0_pulse_readview",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_1_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        // GUARD — one M later sees the folded pulse.
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_2_dmg08_cgb04c_out2.gbc",
            "2",
        ),
        // GUARDs — the LYC-152-anchored reads keep seeing it (the anchor
        // guard's measured discriminator).
        ("gambatte/ly0/lycint152_m2irq_2_dmg08_cgb04c_outE2.gbc", "E2"),
        ("gambatte/ly0/lycint152_m2irq_ds_2_cgb04c_outE2.gbc", "E2"),
        ("gambatte/ly0/lycint152_m2irq_1_dmg08_cgb04c_outE0.gbc", "E0"),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// The SHIFTED-frame (post-STOP) co-instant
/// visibility deadline: the lcd_offset count rows' first poll lands ON the
/// mode-0 rise/flip dot in slopgb's whole-dot frame where the hardware event
/// is a half-dot PAST the sample (F1 = L + 1.5, uniform ½-dot
/// margins). Two verdict-only arms, `lcd_shift_dots != 0` scoped (inert
/// un-switched): the FF0F poll masks a same-dot mode-0 rise
/// (`m0irq_count`); the FF41 poll holds mode 3 on the recorded flip's own
/// dot (`m0stat_count`). The error is ONE-SIDED — the `_2` siblings read 2
/// dots past the event and keep their verdicts (guards).
#[test]
fn tier2_lcd_offset_count_deadline_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_lcd_offset_count",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        (
            "gambatte/lcd_offset/offset1_lyc99int_m0irq_count_scx2_ds_1_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/lcd_offset/offset1_lyc99int_m0stat_count_scx2_ds_1_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/lcd_offset/offset3_lyc99int_m0stat_count_scx1_1_cgb04c_out90.gbc",
            "90",
        ),
        // GUARDs — the one-M-later polls keep their verdicts.
        (
            "gambatte/lcd_offset/offset1_lyc99int_m0irq_count_scx2_ds_2_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/lcd_offset/offset1_lyc99int_m0stat_count_scx2_ds_2_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/lcd_offset/offset3_lyc99int_m0stat_count_scx0_2_cgb04c_out90.gbc",
            "90",
        ),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}
