//! Speedchange / serial / timer / HDMA / engine misc pinned-behavior tests.

use super::*;

/// The executable red spec for the kernel pair. Both ROMs reduce to the
/// *same* `ldh a,(FF41)`; SameBoy's cycle-exact frame separates them with no
/// CPU-call-stack discriminator (leading-edge cc+0 sampling + a decoupled
/// `mode_for_interrupt` + the mode-2(−1)/mode-0(+1) anchor swing):
///   - `m2int_m3stat_1` → out3 (mode 3) — anchored off a *mode-2* STAT IRQ;
///     slopgb's whole-dot model currently reads mode 0 here, so this is the
///     one baselined floor row (`tests/gbtr/baselines/gambatte.txt`).
///   - `m0int_m3stat_2` → out0 (mode 0) — anchored off a *mode-0* STAT IRQ;
///     slopgb already passes this, and the port must keep it passing.
///
/// So the lift is *directional*: make `m2int` read 3 while `m0int` stays 0 —
/// exactly the `O<E ∧ O≥E` "contradiction" that only the decoupled-edge
/// rewrite resolves.
///
/// The spec runs on the production SameBoy cycle-exact eager path (leading-edge
/// cc+0 reads + the `StatUpdate` engine + the `vis_early` back-date + the
/// halt-late masks). With those four pieces the kernel pair SEPARATES
/// (`m2int`→3 ∧ `m0int`→0) on both models while the canonical mooneye
/// `intr_2_mode0_timing` also holds.
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
            // The SameBoy cycle-exact eager path is the production default;
            // same 16-frame protocol + OCR as `run_case`'s `Check::Hex` arm.
            let mut gb = harness::boot(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (flag-on): {e}")
            });
        }
    }
}

/// The IME=1 halt-entry rewind on the EAGER clock (`halt_entry_rewind_impl`).
///
/// SameBoy's `halt()` (sm83_cpu.c:1043-1047) does not enter HALT when
/// `IE & IF` is already nonzero at the entry view: it clears `halted` and
/// decrements PC, so the dispatched ISR returns *into* the HALT and it
/// re-executes with the IF bit consumed. slopgb's production path instead
/// halts and wakes on the first idle check, which pushes the return address to
/// halt+1 — the ISR skips the re-halt and the whole post-wake stream runs one
/// halt round early.
///
/// `ifandie_ei_halt_sra` is the row that exercises it: `EI; HALT` with
/// `IE & IF` already set, so the entry view must rewind. Hardware behaviour,
/// not a clock artifact — hence it fires under `eager`. Production
/// (flag off) keeps the halted+wake shape and stays byte-identical.
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

/// The eager halt-entry `t0+4` VALUE peek (`Ppu::stat_m0_rise_within`).
///
/// SameBoy's `halt()` samples `IE & IF` *after* the prefetch `cycle_read` walked
/// the machine through the HALT fetch M-cycle (t0+4). The deferred clock reaches
/// that view by flushing its parked debt; the eager clock parks none, so its
/// entry sample sits at t0 and a mode-0 STAT rise landing inside the fetch is
/// invisible — the rewind never arms and the post-wake stream runs one halt
/// round early.
///
/// Traced on `late_m0int_halt_m0stat_scx3_3a` [Cgb]: OFF samples ly1 dot 332
/// (w=00, halts, `out0`); tier2 samples dot 260 (w=02 → rewind → re-entry 336,
/// `out0`); eager sampled dot 256 (w=00 → halts early, `out2`). The rise folds
/// at dot 257 — four dots.
///
/// Reconstructing the rise's VALUE at t0+4 rather than advancing the clock keeps
/// machine time honest (advancing would tick the timers 4 T early and break the
/// TIMA-counted `int_hblank_halt` rows). DMG-scoped: see the note in
/// `halt_entry_impl` on the CGB `_3b` skip-path.
#[test]
fn eager_halt_entry_m0_peek_passes_dmg() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_halt_entry_m0_peek",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // The six DMG halt rows the peek recovers — every one a SameBoy-PASS row
    // that was inside the TRUE flip bar.
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

/// CGB DOUBLE-SPEED mode-2→3 ENTRY back-date RE-HOSTED onto the eager clock
/// (L1). The eager cc+0 FF41 value peek (`leading_edge_sample`) samples
/// the PPU pre-tick, a DS M-cycle (2 dots) before the trailing cc+4 view, so a
/// line-start FF41 read straddling the mode-2→3 boundary saw the un-shifted
/// dot-84 entry as mode 2 where SameBoy's cc+4 view reads mode 3. The DS entry
/// back-dates to 80 (as single speed) so the peek lands on mode 3
/// (`Ppu::mode3_entry_dot`, `eager && ds`-scoped). EV CGB two-bin
/// 353 → 348 (clean +5/−0; 4 SameBoy-pass bar + 1 lcd_offset gambatte-want
/// gain). The `_1` siblings (want 2) are the regression guards — they read
/// earlier and must stay mode 2. Tier2's deferred DS frame keeps 84;
/// `eager` off → byte-identical.
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
        // Recovered (SameBoy-pass, was EV-fail):
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

/// The eager FF0F read-frame peek (`interconnect/bus.rs`, read-frame slice).
///
/// The CGB LYC/STAT engine rise lands beyond the eager cc+0 FF0F read, so the
/// raw `intf` misses the deterministically-imminent bit SameBoy's events-first
/// read frame has already folded. The eager read ORs in
/// `Ppu::ff0f_stat_peek() & !ff0f_ly0_pulse_mask()` — the same verdict-only peek
/// the tier2 `read_deferred` path applies, the same VALUE-at-cc+4 shape as the
/// halt-entry peek. Recovers 4 CGB TRUE-bar rows (all SameBoy-PASS) +
/// 2 DMG, zero drops; EV CGB 348→344, EV DMG 85→83.
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
        // DMG legs of the LYC family (also recovered):
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
