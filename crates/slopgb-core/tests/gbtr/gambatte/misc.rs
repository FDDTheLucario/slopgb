//! Speedchange / serial / timer / HDMA / engine misc pinned-behavior tests.

use super::*;

/// The kernel pair: both ROMs reduce to the *same* `ldh a,(FF41)`, and the
/// cycle-exact frame separates them with no CPU-call-stack discriminator
/// (leading-edge cc+0 sampling + a decoupled `mode_for_interrupt` + the
/// mode-2(−1)/mode-0(+1) anchor swing):
///   - `m2int_m3stat_1` → out3 (mode 3) — anchored off a *mode-2* STAT IRQ;
///   - `m0int_m3stat_2` → out0 (mode 0) — anchored off a *mode-0* STAT IRQ.
///
/// Leading-edge cc+0 reads + the `StatUpdate` engine + the `vis_early`
/// back-date + the halt-late masks separate the pair (`m2int`→3 ∧ `m0int`→0)
/// on both models while the mooneye `intr_2_mode0_timing` timing holds.
#[test]
fn kernel_pair_matches_sameboy_target() {
    let Some(root) = common::gbtr_root() else {
        // The collection is required to evaluate this spec; mirror the
        // suite's REQUIRE_ROMS contract rather than silently passing.
        common::skip_or_fail_gbtr("kernel_pair", "game-boy-test-roms collection not present");
        return;
    };
    // (relative ROM path, expected FF41 mode both models)
    let targets = [
        (
            "gambatte/m2int_m3stat/m2int_m3stat_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/m0int_m3stat/m0int_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let path = root.join(rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for model in [Model::Dmg, Model::Cgb] {
            // Same 16-frame protocol + OCR as `run_case`'s `Check::Hex` arm.
            let mut gb = harness::boot(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (flag-on): {e}")
            });
        }
    }
}

/// The IME=1 halt-entry rewind (`halt_entry_rewind_impl`).
///
/// SameBoy's `halt()` (sm83_cpu.c:1043-1047) does not enter HALT when
/// `IE & IF` is already nonzero at the entry view: it clears `halted` and
/// decrements PC, so the dispatched ISR returns *into* the HALT and it
/// re-executes with the IF bit consumed.
///
/// `ifandie_ei_halt_sra` exercises it: `EI; HALT` with `IE & IF` already set,
/// so the entry view must rewind (out0A both models).
#[test]
fn eager_halt_entry_rewind_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_halt_entry_rewind",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rel = "gambatte/halt/ifandie_ei_halt_sra_dmg08_cgb04c_out0A.gbc";
    let path = root.join(rel);
    let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Cgb] {
        let mut gb = harness::boot(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0A", model.is_cgb())
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] expected out0A (eager): {e}"));
    }
}

/// The halt-entry `t0+4` VALUE peek (`Ppu::stat_m0_rise_within`).
///
/// SameBoy's `halt()` samples `IE & IF` *after* the prefetch `cycle_read` walked
/// the machine through the HALT fetch M-cycle (t0+4), so a mode-0 STAT rise
/// landing inside the fetch must arm the rewind. Reconstructing the rise's
/// VALUE at t0+4 rather than advancing the clock keeps machine time honest
/// (advancing would tick the timers 4 T early and break the TIMA-counted
/// `int_hblank_halt` rows). DMG-scoped: see the note in `halt_entry_impl` on
/// the CGB `_3b` skip-path.
#[test]
fn eager_halt_entry_m0_peek_passes_dmg() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_halt_entry_m0_peek",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // The six DMG halt rows the peek covers.
    let rows = [
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx2_3a_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx3_3a_dmg08_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/halt/late_m0int_halt_m0stat_scx3_3b_dmg08_out0_cgb04c_out2.gbc",
            "0",
        ),
        (
            "gambatte/halt/late_m0irq_halt_dec_scx2_2_dmg08_cgb04c_out6.gbc",
            "6",
        ),
        (
            "gambatte/halt/late_m0irq_halt_dec_scx3_2_dmg08_cgb04c_out6.gbc",
            "6",
        ),
        (
            "gambatte/halt/late_m0irq_halt_m0stat_scx3_3b_dmg08_cgb04c_out2.gbc",
            "2",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Dmg);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, false)
            .unwrap_or_else(|e| panic!("{rel} [Dmg] expected out{expect} (eager): {e}"));
    }
}

/// CGB double-speed mode-2→3 entry back-date. The cc+0 FF41 value peek
/// (`leading_edge_sample`) samples the PPU pre-tick, a DS M-cycle (2 dots)
/// before the trailing cc+4 view, so a line-start FF41 read straddling the
/// mode-2→3 boundary must read mode 3. The DS entry back-dates to 80 (as single
/// speed) so the peek lands on mode 3 (`Ppu::mode3_entry_dot`, CGB + DS scoped).
/// The `_1` siblings (want 2) read earlier and stay mode 2.
#[test]
fn eager_ds_mode3_entry_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ds_mode3_entry",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let rows = [
        // Targets:
        (
            "gambatte/m2int_m2stat/m2int_m2stat_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/m2int_m2stat/m2int_scx4_m2stat_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/enable_display/frame0_m3stat_count_ds_2_cgb04c_out90.gbc",
            "90",
        ),
        (
            "gambatte/enable_display/frame1_m3stat_count_ds_2_cgb04c_out90.gbc",
            "90",
        ),
        // Regression guards (the `_1` mode-2 siblings must stay blocked at 2):
        (
            "gambatte/m2int_m2stat/m2int_m2stat_ds_1_cgb04c_out2.gbc",
            "2",
        ),
        (
            "gambatte/m2int_m2stat/m2int_scx4_m2stat_ds_1_cgb04c_out2.gbc",
            "2",
        ),
    ];
    for (rel, expect) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (eager): {e}"));
    }
}

/// The FF0F read-frame peek (`interconnect/bus.rs`, read-frame slice).
///
/// The CGB LYC/STAT engine rise lands beyond the cc+0 FF0F read, so the raw
/// `intf` misses the deterministically-imminent bit; the read ORs in
/// `Ppu::ff0f_stat_peek() & !ff0f_ly0_pulse_mask()` — the same VALUE-at-cc+4
/// shape as the halt-entry peek. Covers the CGB LYC/STAT rows plus their DMG
/// legs.
#[test]
fn eager_ff0f_read_peek_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_ff0f_read_peek",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expected, model)
    let rows = [
        (
            "gambatte/ly0/lycint152_lyc153irq_2_dmg08_cgb04c_outE2.gbc",
            "E2",
            Model::Cgb,
        ),
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_1_dmg08_cgb04c_out0.gbc",
            "0",
            Model::Cgb,
        ),
        (
            "gambatte/lcd_offset/offset1_lyc99int_m0irq_count_scx2_ds_1_cgb04c_out90.gbc",
            "90",
            Model::Cgb,
        ),
        (
            "gambatte/m2int_m0irq/m2int_m0irq_ds_2_cgb04c_out3.gbc",
            "3",
            Model::Cgb,
        ),
        // DMG legs of the LYC family:
        (
            "gambatte/ly0/lycint152_lyc153irq_2_dmg08_cgb04c_outE2.gbc",
            "E2",
            Model::Dmg,
        ),
        (
            "gambatte/lyc153int_m2irq/lyc153int_m2irq_1_dmg08_cgb04c_out0.gbc",
            "0",
            Model::Dmg,
        ),
    ];
    for (rel, expect, model) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, model.is_cgb())
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] expected out{expect} (eager): {e}"));
    }
}

/// The DMG bare-line mode-3 exit backs out only the fine scroll the render
/// added *beyond* what its comparator resolved (`read_laws_exit.rs`, arm 8).
///
/// A mid-mode-3 SCX rewrite commits `eff.scx` at the cc+0 write frame, so the
/// render can over-discard the new fine scroll and flip late. Backing out the
/// whole live `SCX & 7` over-corrects when the hunt latched that same value:
/// `scx_m3_extend_1` writes SCX=5 with `hunt_fine == 5`, so its length is
/// legitimate and the read must still see mode 3 one M-cycle before its `_2`
/// sibling sees mode 0. `late_scx4_2` is the case the back-out exists for —
/// hunt 0 against `eff.scx & 7 == 4` — and is pinned here alongside it.
#[test]
fn eager_dmg_scx_m3_extend_bare_exit_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_scx_m3_extend_bare_exit",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expected, model)
    let rows = [
        (
            "gambatte/scx_during_m3/scx_m3_extend_1_dmg08_cgb04c_out3.gbc",
            "3",
            Model::Dmg,
        ),
        (
            "gambatte/scx_during_m3/scx_m3_extend_2_dmg08_cgb04c_out0.gbc",
            "0",
            Model::Dmg,
        ),
        (
            "gambatte/scx_during_m3/scx_m3_extend_1_dmg08_cgb04c_out3.gbc",
            "3",
            Model::Cgb,
        ),
        (
            "gambatte/scx_during_m3/scx_m3_extend_2_dmg08_cgb04c_out0.gbc",
            "0",
            Model::Cgb,
        ),
        // The siblings the back-out exists for — hunt_fine 0 vs eff.scx&7 4.
        (
            "gambatte/m2int_m3stat/scx/late_scx4_1_dmg08_cgb04c_out3.gbc",
            "3",
            Model::Dmg,
        ),
        (
            "gambatte/m2int_m3stat/scx/late_scx4_2_dmg08_cgb04c_out0.gbc",
            "0",
            Model::Dmg,
        ),
    ];
    for (rel, expect, model) in rows {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, model.is_cgb())
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] expected out{expect}: {e}"));
    }
}

/// The mid-mode-3 SCX BG map column: the DMG pre-output fetch lead
/// (`map_scx_formed`) and the never-matched-hunt column holdback
/// (`render.rs`, at `prefill_pos == 8`).
///
/// `scx_0363c0/_4` pins the lead — its first full tile must miss a write that
/// commits one dot after the CGB cut-off. `scx_0360c0/_2` and `scx_0761c0/_4`
/// pin the holdback: when an SCX write moves `SCX & 7` behind the comparator,
/// the whole first tile is dropped and must not advance the map counter.
/// The two double-speed rows pin the speed gate from both sides:
/// `scx_0360c0/_ds_3` requires the holdback on lines >= 1, and
/// `old/offset_3/_ds_1` requires it off on line 0.
#[test]
fn eager_scx_during_m3_map_column_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_scx_during_m3_map_column",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, reference-png suffix, model)
    let rows = [
        (
            "gambatte/scx_during_m3/scx_0363c0/scx_during_m3_4.gbc",
            "_dmg08",
            Model::Dmg,
        ),
        (
            "gambatte/scx_during_m3/scx_0360c0/scx_during_m3_2.gbc",
            "_dmg08",
            Model::Dmg,
        ),
        (
            "gambatte/scx_during_m3/scx_0360c0/scx_during_m3_2.gbc",
            "_cgb04c",
            Model::Cgb,
        ),
        (
            "gambatte/scx_during_m3/scx_0761c0/scx_during_m3_4.gbc",
            "_dmg08",
            Model::Dmg,
        ),
        (
            // Bare `<stem>.png` (the .gbc-extension fallback the harness uses).
            // The line-0 guard: the holdback must stay off there in double
            // speed, or this row's last tile lands on column 11.
            "gambatte/scx_during_m3/old/offset_3/scx_during_m3_ds_1.gbc",
            "",
            Model::Cgb,
        ),
        (
            // Double speed, lines >= 1: the holdback IS required — this
            // reference demands an even first column and an odd last, which
            // only the held indices produce.
            "gambatte/scx_during_m3/scx_0360c0/scx_during_m3_ds_3.gbc",
            "_cgb04c",
            Model::Cgb,
        ),
        (
            // Double speed on LINE 0, fine scroll non-zero: the holdback is
            // required here too, so the line-0 carve-out cannot be blanket.
            "gambatte/scx_during_m3/scx_0761c0/scx_during_m3_ds_3.gbc",
            "_cgb04c",
            Model::Cgb,
        ),
        (
            // Double speed takes the same map lead 2 as CGB single speed: this
            // row's `$c0` write commits at dot 241 and its dot-242 last-tile
            // read must NOT see it (lead 0 does, and lands column 11).
            "gambatte/scx_during_m3/scx_0060c0/scx_during_m3_ds_2.gbc",
            "_cgb04c",
            Model::Cgb,
        ),
        (
            // The other side of that lead: line 0's STAT dispatch runs 4 dots
            // later than lines 1-143, so its writes move with it and the lead
            // drops back to 0 there — at lead 2 this row's first tile misses
            // its dot-90 commit.
            "gambatte/scx_during_m3/scx_0060c0/scx_during_m3_ds_1.gbc",
            "_cgb04c",
            Model::Cgb,
        ),
        (
            // A double-speed SCX write landing ON the fine-scroll comparator
            // lock takes the POST-lock commit debt (`stage_write_dots`
            // `dot >= hunt_match_dot`): raw dot 92 == the lock, and its
            // dot-98 first-tile read must see it. With `>` it commits a dot
            // late and the tile lands on an odd column.
            "gambatte/scx_during_m3/scx_0360c0/scx_during_m3_ds_6.gbc",
            "_cgb04c",
            Model::Cgb,
        ),
    ];
    for (rel, suffix, model) in rows {
        let path = root.join(rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot(&rom, model);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let png = path
            .parent()
            .unwrap_or(Path::new(""))
            .join(format!("{stem}{suffix}.png"));
        let map = if model.is_cgb() {
            CgbColorMap::Gambatte
        } else {
            CgbColorMap::Identity
        };
        harness::expect_frame_png(&gb, &png, map)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}]: {e}"));
    }
}
