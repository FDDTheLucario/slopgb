//! GBMicrotest suite harness (`gbmicrotest/`, 513 flat ROMs).
//!
//! Protocol (`gbmicrotest/game-boy-test-roms-howto.md`): a test writes its
//! result to HRAM — $FF80 actual value, $FF81 expected value, $FF82 verdict
//! ($01 pass, $FF fail). Only $FF82 may be trusted ("You should only check
//! `0xFF82` when evaluating test results"); $FF80/$FF81 are reported in
//! failure messages purely for triage. There is no completion signal; the
//! howto's fixed-point semantics apply: run two frames — documented as
//! sufficient for every test except `is_if_set_during_ime0.gb` (~380 ms
//! emulated) — and read $FF82 *at that point* (never latch the first
//! nonzero write: a ROM may store a provisional value before its real
//! verdict). If $FF82 is still zero (HRAM powers up zeroed), continue to a
//! 0.6 emulated-second deadline — covering the outlier with >50% margin —
//! and judge the final value read there.
//!
//! Model routing: the howto pins the verified hardware to a DMG-CPU-08
//! board, i.e. a DMG-CPU B or C SoC — every ROM therefore runs on
//! [`Model::Dmg`] only (docs/ARCHITECTURE.md §CGB revision policy has no
//! gbmicrotest row: the suite never touches a CGB model).

use slopgb_core::{CLOCK_HZ, GameBoy, Model};

use crate::common;
use crate::harness::{self, CaseResult};

/// 0.6 emulated seconds (see module docs for the deadline rationale).
const DEADLINE_TCYCLES: u64 = CLOCK_HZ as u64 * 3 / 5;

/// ROMs that never write a $FF82 verdict: testbenches/visual experiments
/// shipped alongside the pass/fail tests (the howto's protocol section
/// simply does not apply to them). Determined empirically by running all
/// 513 ROMs to the deadline — exactly these 30 left $FF82 = $00 — and
/// cross-checked statically: none of these images contains any
/// $FF82-store instruction (`LDH ($82),A` = E0 82, `LD ($FF82),A` =
/// EA 82 FF, or `LD HL,$FF82` = 21 82 FF), so no fix to the emulator can
/// ever make them report. A 5-emulated-second probe confirmed they stay
/// silent well past the deadline (`temp.gb` alone eventually clobbers
/// $FF82 with a non-verdict code byte $29 at ~1.9 s, far outside the
/// howto's two-frame completion window). Everything else on disk is
/// claimed by the matrix.
const TESTBENCHES: &[&str] = &[
    "000-oam_lock.gb",
    "000-write_to_x8000.gb",
    "001-vram_unlocked.gb",
    "002-vram_locked.gb",
    "004-tima_boot_phase.gb",
    "004-tima_cycle_timer.gb",
    "007-lcd_on_stat.gb",
    "400-dma.gb",
    "500-scx-timing.gb",
    "800-ppu-latch-scx.gb",
    "801-ppu-latch-scy.gb",
    "802-ppu-latch-tileselect.gb",
    "803-ppu-latch-bgdisplay.gb",
    "audio_testbench.gb",
    "cpu_bus_1.gb",
    "flood_vram.gb",
    "lcdon_write_timing.gb",
    "ly_while_lcd_off.gb",
    "minimal.gb",
    "mode2_stat_int_to_oam_unlock.gb",
    "oam_sprite_trashing.gb",
    "poweron.gb",
    "ppu_scx_vs_bgp.gb",
    "ppu_sprite_testbench.gb",
    "ppu_spritex_vs_scx.gb",
    "ppu_win_vs_wx.gb",
    "ppu_wx_early.gb",
    "temp.gb",
    "toggle_lcdc.gb",
    "wave_write_to_0xC003.gb",
];

/// Interpret the HRAM result block once $FF82 is nonzero. Only `status`
/// ($FF82) decides pass/fail per the howto; `actual`/`expected` are
/// diagnostics only (some tests set $FF80 == $FF81 even on failure).
fn verdict(actual: u8, expected: u8, status: u8) -> Result<(), String> {
    match status {
        0x01 => Ok(()),
        0xFF => Err(format!(
            "$FF82=$FF (fail): actual $FF80={actual:#04x}, expected $FF81={expected:#04x}"
        )),
        other => Err(format!(
            "$FF82={other:#04x} (protocol violation, neither $01 pass nor $FF fail): \
             actual $FF80={actual:#04x}, expected $FF81={expected:#04x}"
        )),
    }
}

/// Known-failure baseline (ratchet): the rom×model cases that currently
/// fail. Shrinking this file is progress; growing it is a regression
/// (`harness::assert_against_baseline`).
const BASELINE_TXT: &str = include_str!("baselines/gbmicrotest.txt");

/// Run one gbmicrotest ROM on DMG through the howto's fixed-point
/// semantics (module docs): read $FF82 after two frames; if it is still
/// zero, run to the 0.6 emulated-second deadline and read it once more —
/// the *final* value decides, never the first nonzero write (a ROM whose
/// verdict byte changes between a provisional store and the deadline must
/// be judged by what it settles on). A ROM that never sets $FF82 is a
/// failure here — the documented no-verdict testbenches are exempted
/// before this runs.
fn run_case(rom: &[u8]) -> Result<(), String> {
    let mut gb = harness::boot(rom, Model::Dmg);
    let deadline = gb.cycles().saturating_add(DEADLINE_TCYCLES);
    harness::run_for_frames(&mut gb, 2);
    if gb.peek_no_io(0xFF82) == 0 {
        while gb.cycles() < deadline {
            gb.step();
        }
    }
    match gb.peek_no_io(0xFF82) {
        0 => Err("no $FF82 verdict within 0.6 emulated seconds".into()),
        status => verdict(gb.peek_no_io(0xFF80), gb.peek_no_io(0xFF81), status),
    }
}

/// Run an explicit list of gbmicrotest ROMs on the **flag-on** reclock and
/// report the $FF82 verdict — the fast iteration loop for the DMG engine
/// reclock. `#[ignore]`'d. Usage:
/// `SLOPGB_ROWLIST=/tmp/rows.txt cargo test -p slopgb-core --test gbtr
/// --release -- --ignored gbmicro_flagon_probe --nocapture`. Each row is
/// `gbmicrotest/<name>.gb [Dmg]` (extra columns ignored). `SLOPGB_PROBE_OFF=1`
/// A/Bs against production.
#[test]
#[ignore = "session-local Phase-2 measurement aid; needs SLOPGB_ROWLIST"]
fn gbmicro_flagon_probe() {
    let Ok(list_path) = std::env::var("SLOPGB_ROWLIST") else {
        eprintln!("SLOPGB_ROWLIST unset");
        return;
    };
    let Some(root) = common::gbtr_root() else {
        panic!("gbtr collection not present");
    };
    let off = std::env::var("SLOPGB_PROBE_OFF").is_ok();
    let body = std::fs::read_to_string(&list_path).expect("read rowlist");
    let (mut pass, mut fail, mut skip) = (0u32, 0u32, 0u32);
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rel = line.split_whitespace().next().unwrap_or("");
        if !rel.starts_with("gbmicrotest/") {
            continue;
        }
        let Ok(rom) = std::fs::read(root.join(rel)) else {
            skip += 1;
            continue;
        };
        let mut gb = if off {
            harness::boot(&rom, Model::Dmg)
        } else {
            harness::boot_with_reclock(&rom, Model::Dmg)
        };
        let deadline = gb.cycles().saturating_add(DEADLINE_TCYCLES);
        harness::run_for_frames(&mut gb, 2);
        if gb.peek_no_io(0xFF82) == 0 {
            while gb.cycles() < deadline {
                gb.step();
            }
        }
        let (v, a, e) = (
            gb.peek_no_io(0xFF82),
            gb.peek_no_io(0xFF80),
            gb.peek_no_io(0xFF81),
        );
        if v == 0x01 {
            pass += 1;
        } else {
            fail += 1;
            println!("FAIL {rel} verdict={v:#04X} got={a:#04X} want={e:#04X}");
        }
    }
    println!(
        "gbmicro_flagon_probe[{}] pass={pass} fail={fail} skip={skip}",
        if off { "OFF" } else { "ON" }
    );
}

/// Enumerate the suite directory as collection-relative forward-slash
/// paths, sorted (via `collect_roms`). Panics on a missing/unreadable
/// directory — callers check `gbtr_root()` first.
fn suite_roms(root: &std::path::Path) -> Vec<String> {
    let dir = root.join("gbmicrotest");
    let mut paths = Vec::new();
    common::collect_roms(&dir, false, &mut paths)
        .unwrap_or_else(|e| panic!("cannot enumerate {}: {e}", dir.display()));
    assert!(
        !paths.is_empty(),
        "{} exists but contains no .gb/.gbc ROMs — corrupt checkout?",
        dir.display()
    );
    paths
        .iter()
        .map(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).expect("utf-8 name");
            format!("gbmicrotest/{name}")
        })
        .collect()
}

fn is_testbench(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    TESTBENCHES.contains(&name)
}

/// Inventory hook for the coverage guard: (claimed, exempted)
/// collection-relative paths of every `.gb`/`.gbc` under `gbmicrotest/`.
/// Exempted = the documented no-verdict testbenches ([`TESTBENCHES`]);
/// everything else produces exactly one DMG case.
pub fn inventory() -> (Vec<String>, Vec<String>) {
    let Some(root) = common::gbtr_root() else {
        return (Vec::new(), Vec::new());
    };
    suite_roms(&root)
        .into_iter()
        .partition(|rel| !is_testbench(rel))
}

/// Full matrix: every claimed ROM × DMG through the $FF82 protocol,
/// ratcheted against the known-failure baseline.
#[test]
fn gbmicrotest_dmg_matrix() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "gbmicrotest",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    let mut results: Vec<CaseResult> = Vec::new();
    for rel in suite_roms(&root) {
        if is_testbench(&rel) {
            // Documented no-verdict testbench (see TESTBENCHES) — running it
            // can only ever time out.
            continue;
        }
        let path = root.join(&rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        results.push(CaseResult {
            key: harness::case_key(&rel, Model::Dmg),
            result: harness::catch_case(|| run_case(&rom)),
        });
    }
    harness::assert_against_baseline(
        "gbmicrotest",
        &results,
        &harness::parse_baseline(BASELINE_TXT),
    );
}

/// `int_hblank_halt_scx0-7` on the deferred-commit reclock. The test times the
/// mode-0 STAT IRQ **halt-wake** via TIMA; the deferred halt loop samples
/// `pending_halt_wake` at cc+0, ~2 M-cycles before SameBoy's `GB_cpu_run` DMG
/// mid-cycle sample (`sm83_cpu.c:1621-1628`). The re-derived `if_late` halt
/// mask (2 uniform M-cycles + the `mask{rise cc==4}` second-half term for the
/// deferred `cc = eager+1` rotation; `interconnect/tick.rs`) restores the baked
/// `$FF80` = 62,62,62,63,63,63,63,64 target, so all eight pass **flag-on** on
/// DMG — clearing the residual while the kernel pair and `intr_2_mode0_timing`
/// keep passing (they do not halt-wake on mode 0). Production (flag-off) is
/// byte-identical (the mask is gated on `tier2_reclock`); SameBoy itself passes
/// these (`tools/hramdump.c`).
#[test]
fn tier2_int_hblank_halt_passes_dmg() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_int_hblank",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    for scx in 0..8u8 {
        let rel = format!("gbmicrotest/int_hblank_halt_scx{scx}.gb");
        let rom = std::fs::read(root.join(&rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        // Boot WITH the reclock (construction-time), so the boot-DIV +4 frame
        // the real flip installs is active — the TIMA-counted halt-wake depends
        // on it (the set-after-boot path would skip the +4 and mis-pass at the
        // old mask). The `m0_halt_hold` base was re-derived for this frame.
        let mut gb = harness::boot_with_reclock(&rom, Model::Dmg);
        // Same fixed-point protocol as `run_case` (two frames, then the
        // verdict deadline), on the Tier-2 reclock path.
        let deadline = gb.cycles().saturating_add(DEADLINE_TCYCLES);
        harness::run_for_frames(&mut gb, 2);
        if gb.peek_no_io(0xFF82) == 0 {
            while gb.cycles() < deadline {
                gb.step();
            }
        }
        assert_eq!(
            gb.peek_no_io(0xFF82),
            0x01,
            "{rel} (tier2 flag-on): FF82={:#04X} actual FF80={:#04X} expected FF81={:#04X}",
            gb.peek_no_io(0xFF82),
            gb.peek_no_io(0xFF80),
            gb.peek_no_io(0xFF81),
        );
    }
}

/// The DMG `hblank_int` mode-0 STAT-IF two-latch (DELIVER + SERVICE-CLEAR).
/// The reclock's cc+0 deferred `ldh a,(FF0F)` samples 4 dots before
/// production's cc+4 read of the same load, straddling the counter-pinned
/// mode-0 rise `R = 254 + SCX&7`. The `if_c` legs read `[R-4, R)` and must
/// observe the imminent rise DELIVERED (`ISR CP E2`); the `if_d` legs read
/// `[R, R+4)` where on hardware the mode-0 dispatch clears IF at the read's own
/// cycle so the load returns 0 (`ISR CP 00`) — gated on `intf & ie & STAT`
/// (pending + enabled) to separate the pure poll `hblank_scx2_if_a` (DI + IE=0,
/// wants the bit still set). All 16 pass flag-on. The
/// `if_b`/`nops`/`hblank_scx3` siblings need the counter-pinned dispatch to
/// move (parked). Production (flag-off) byte-identical — the law is
/// `tier2_reclock` + `!is_cgb`-gated; SameBoy passes these on real DMG.
#[test]
fn tier2_dmg_hblank_if_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_hblank_if",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    for scx in 0..8u8 {
        for leg in ["if_c", "if_d"] {
            let rel = format!("gbmicrotest/hblank_int_scx{scx}_{leg}.gb");
            let rom = std::fs::read(root.join(&rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
            // Boot WITH the reclock at construction (the flag-on frame the flip
            // installs) — same protocol as `tier2_int_hblank_halt_passes_dmg`.
            let mut gb = harness::boot_with_reclock(&rom, Model::Dmg);
            let deadline = gb.cycles().saturating_add(DEADLINE_TCYCLES);
            harness::run_for_frames(&mut gb, 2);
            if gb.peek_no_io(0xFF82) == 0 {
                while gb.cycles() < deadline {
                    gb.step();
                }
            }
            assert_eq!(
                gb.peek_no_io(0xFF82),
                0x01,
                "{rel} (tier2 flag-on): FF82={:#04X} actual FF80={:#04X} expected FF81={:#04X}",
                gb.peek_no_io(0xFF82),
                gb.peek_no_io(0xFF80),
                gb.peek_no_io(0xFF81),
            );
        }
    }
}

/// The DMG power-on boot-frame read law (`Ppu::boot_read`): the tier2 deferred
/// read samples the PPU at cc+0, 4 dots before production's cc+4 read of the
/// same `LD A,(nn)`, so the `poweron_*` ROMs (a NOP sled timing a single direct
/// read of STAT/OAM/VRAM/LY on the pristine boot hand-off frame) read the
/// pre-transition value; restoring the read's true (cc+4) verdict — the current
/// (line, dot) advanced 4 dots — fixes all 20 at once. Scoped to the boot frame
/// (`frame_count <= 2` AND no CPU LCD-register write, so a program that
/// reconfigures the PPU reverts to cc+0), `tier2_reclock` + `!is_cgb` +
/// verdict-only → the `+4` boot DIV (`boot_div`) and CGB stay byte-identical.
/// SameBoy passes these on real DMG (they pass flag-OFF too — the whole 20 are
/// reclock flip-blockers).
#[test]
fn tier2_dmg_poweron_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_dmg_poweron",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    const ROWS: [&str; 20] = [
        "poweron_ly_120",
        "poweron_ly_234",
        "poweron_oam_006",
        "poweron_oam_070",
        "poweron_oam_120",
        "poweron_oam_184",
        "poweron_oam_234",
        "poweron_stat_006",
        "poweron_stat_007",
        "poweron_stat_027",
        "poweron_stat_070",
        "poweron_stat_120",
        "poweron_stat_121",
        "poweron_stat_141",
        "poweron_stat_184",
        "poweron_stat_235",
        "poweron_vram_026",
        "poweron_vram_070",
        "poweron_vram_140",
        "poweron_vram_184",
    ];
    for name in ROWS {
        let rel = format!("gbmicrotest/{name}.gb");
        let rom = std::fs::read(root.join(&rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Dmg);
        let deadline = gb.cycles().saturating_add(DEADLINE_TCYCLES);
        harness::run_for_frames(&mut gb, 2);
        if gb.peek_no_io(0xFF82) == 0 {
            while gb.cycles() < deadline {
                gb.step();
            }
        }
        assert_eq!(
            gb.peek_no_io(0xFF82),
            0x01,
            "{rel} (tier2 flag-on): FF82={:#04X} actual FF80={:#04X} expected FF81={:#04X}",
            gb.peek_no_io(0xFF82),
            gb.peek_no_io(0xFF80),
            gb.peek_no_io(0xFF81),
        );
    }
}

/// The eager-clock DMG sprite0 mode-3→0 boundary read
/// (`ppu_sprite0_scx{2,6}_b`). Each `_b` ROM reads STAT with its
/// measurement `ldh a,(FF41)` landing exactly on the bare-line mode-0 flip and
/// wants mode 0 (`$80`); its `_a` sibling reads one M-cycle earlier and wants
/// mode 3 (`$83`) — the pair brackets `flip_dot`. On the eager clock the CPU
/// dispatch moves the read 4 dots (one M-cycle) early but the `+8hd` read-debt
/// keeps its `read_pos_hd` at `2*flip`; production reads mode 0 AT `flip_dot`
/// (the flip is inclusive), so the true CPU-visible boundary is rphd `2*flip`.
/// The bare-exit arm's emergent `2*flip + 2` over-held by 2hd, forcing mode 3
/// (`$83`, wrong). The `- carry` term already lands the CARRIED weld-partners
/// (`gambatte late_scx4_1`/`m2int_m3stat_1`, which read the same rphd 512 wanting
/// mode 3) at `2*flip - 2`, so dropping the `+2` for the POLLED read
/// (`!read_carried`) is the exact discriminator — NOT the uniform read-frame
/// bias a prior sweep (`ARM8BIAS`) mistook for a weld. `eager_value` +
/// `!is_cgb` + polled scoped → production + tier2 byte-identical (this pin fails
/// with the `+2` restored). SameBoy passes these on real DMG.
#[test]
fn eager_dmg_sprite0_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "eager_dmg_sprite0",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    for name in ["ppu_sprite0_scx2_b", "ppu_sprite0_scx6_b"] {
        let rel = format!("gbmicrotest/{name}.gb");
        let rom = std::fs::read(root.join(&rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        // Coherent eager-value C3-flip (`new_with_eager`) — NOT a
        // post-boot toggle. Same fixed-point protocol as `run_case`.
        let mut gb = GameBoy::new_with_eager(Model::Dmg, rom)
            .unwrap_or_else(|e| panic!("cartridge rejected: {e:?}"));
        let deadline = gb.cycles().saturating_add(DEADLINE_TCYCLES);
        harness::run_for_frames(&mut gb, 2);
        if gb.peek_no_io(0xFF82) == 0 {
            while gb.cycles() < deadline {
                gb.step();
            }
        }
        assert_eq!(
            gb.peek_no_io(0xFF82),
            0x01,
            "{rel} (eager): FF82={:#04X} actual FF80={:#04X} expected FF81={:#04X}",
            gb.peek_no_io(0xFF82),
            gb.peek_no_io(0xFF80),
            gb.peek_no_io(0xFF81),
        );
    }
}

/// Self-verifying coverage: claimed ∩ exempted = ∅ and claimed ∪ exempted
/// equals the on-disk ROM set, with the suite size and testbench count
/// pinned so a changed checkout or a typo in [`TESTBENCHES`] fails loudly.
#[test]
fn gbmicrotest_inventory_covers_directory() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "gbmicrotest_inventory",
            &format!("test-roms/{} not present", common::GBTR_DIR),
        );
        return;
    };
    let (claimed, exempted) = inventory();
    for c in &claimed {
        assert!(!exempted.contains(c), "{c} both claimed and exempted");
    }
    let mut union: Vec<String> = claimed.iter().chain(&exempted).cloned().collect();
    union.sort();
    let on_disk = suite_roms(&root);
    assert_eq!(union, on_disk, "inventory does not cover gbmicrotest/");
    // Every documented testbench must actually exist on disk (typo guard),
    // and the partition sizes are pinned to the v7.0 release contents:
    // 513 ROMs = 483 claimed pass/fail tests + 30 no-verdict testbenches.
    assert_eq!(exempted.len(), TESTBENCHES.len(), "missing testbench ROM");
    assert_eq!(TESTBENCHES.len(), 30, "testbench set changed");
    assert_eq!(on_disk.len(), 513, "gbmicrotest ROM count changed");
    assert_eq!(claimed.len(), 483);
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- $FF82 verdict interpretation (howto "Test Success/Failure") ---

    #[test]
    fn verdict_passes_on_01_only() {
        assert!(verdict(0x12, 0x12, 0x01).is_ok());
        // $FF80/$FF81 must not influence the verdict: tests exist that set
        // actual == expected on failure, and vice versa.
        assert!(verdict(0x00, 0xFF, 0x01).is_ok());
        assert!(verdict(0x12, 0x12, 0xFF).is_err());
    }

    #[test]
    fn verdict_failure_carries_actual_and_expected() {
        let err = verdict(0xAB, 0xCD, 0xFF).unwrap_err();
        assert!(err.contains("$FF80=0xab"), "{err}");
        assert!(err.contains("$FF81=0xcd"), "{err}");
    }

    #[test]
    fn verdict_rejects_protocol_violations() {
        let err = verdict(0x00, 0x00, 0x42).unwrap_err();
        assert!(err.contains("protocol violation"), "{err}");
        assert!(err.contains("0x42"), "{err}");
    }

    // --- deadline constant ---

    #[test]
    fn deadline_covers_documented_outlier_with_margin() {
        // is_if_set_during_ime0.gb needs ~380 ms emulated (howto); 0.6 s in
        // T-cycles must exceed that by a healthy margin.
        let outlier = (0.380 * f64::from(CLOCK_HZ)) as u64;
        assert!(DEADLINE_TCYCLES > outlier + outlier / 2);
    }
}
