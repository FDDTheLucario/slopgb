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
