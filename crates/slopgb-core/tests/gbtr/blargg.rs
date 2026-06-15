//! blargg suite harness (`blargg/` in the c-sp collection): Blargg's
//! hardware test ROMs — 58 ROMs across 9 sub-suites plus the loose
//! `halt_bug.gb` at the suite root.
//!
//! # Pass protocols
//!
//! Blargg's ROMs predate the collection's screenshot convention and report
//! through one of two self-describing channels (verified in the ROM
//! binaries: the older serial framework carries the strings `Passed all
//! tests`/`Failed # `, the newer one the `Run failed tests` menu and the
//! $A000 signature):
//!
//! * **Serial** (`cpu_instrs`, `instr_timing`, `mem_timing`): the ROM
//!   prints its result text by writing the byte to $FF01 and $81 to $FF02.
//!   Pass ⇔ the output contains `Passed` (`Passed all tests` for the
//!   combined multi-test ROMs); any `Failed` is a failure.
//! * **$A000 memory signature** (`mem_timing-2`, `dmg_sound`, `cgb_sound`,
//!   `oam_bug`): the ROM writes the magic bytes DE B0 61 to $A001-$A003
//!   and keeps a status at $A000 — $80 while running, $00 on pass, any
//!   other value is a failure code, with NUL-terminated result text from
//!   $A004 ([`harness::blargg_signature_status`]).
//! * **Timed screenshot** (`halt_bug`, `interrupt_time`): both ROMs carry
//!   the signature runtime, but their headers declare MBC1+RAM with RAM
//!   size *none* ($0147=$02, $0149=$00), so there is no cart RAM for the
//!   signature to land in (verified: the magic never appears). The
//!   collection's own exit condition applies — run the howto's emulated
//!   time, compare the last completed frame against the reference PNG.
//!
//! The howto claims the combined `dmg_sound`/`cgb_sound` ROMs repeat their
//! test list forever (`blargg/game-boy-test-roms-howto.md` footnotes 2 and
//! 3: "Test cases are repeated infinitely, the test never really
//! finishes"), which would force a timed-screenshot fallback. Verified
//! empirically instead (90 emulated seconds, $A000 sampled once per
//! emulated second): both ROMs settle at status $00 right at the howto's
//! 36 s / 37 s exit marks and hold it — the same footnotes note SameBoy
//! "terminates the test correctly", and so does this emulator — so the
//! signature protocol applies to the combined sound ROMs as well.
//!
//! # Model matrix
//!
//! From the cartridge-header CGB flags ($0143) and the howto's
//! device-verification tables: `cpu_instrs`, `instr_timing`, `mem_timing`,
//! `mem_timing-2` and `halt_bug` are flag $80 (CGB-enhanced, DMG
//! compatible) and verified on both DMG-C and CGB → run on Dmg and Cgb.
//! `dmg_sound` is flag $00 → Dmg only. `cgb_sound` is flag $C0 (CGB only)
//! → Cgb only. `oam_bug` runs the signature protocol on Dmg; the bug does
//! not exist on CGB, and the combined ROM's expected CGB screen is
//! documented by `oam_bug-cgb.png` (timed screenshot, 21 s).
//! `interrupt_time` is CGB-only (flag $C0): pass screen on Cgb, plus the
//! howto-documented expected-failure screen on Dmg
//! (`interrupt_time-dmg.png`, checksum 7F8F4AAF, footnote 4), both as
//! timed screenshots after 2 s.
//!
//! # Timeouts
//!
//! Per the howto's exit-condition tables, plus ~30% margin; the singles get
//! their parent suite's combined figure as the bound (they terminate early
//! through their protocol). `cpu_instrs` uses a flat 70 s covering both
//! the 55 s DMG and 31 s CGB figures.

use std::path::Path;

use slopgb_core::{CLOCK_HZ, GameBoy, Model};

use crate::common;
use crate::common::framecmp::CgbColorMap;
use crate::harness::{self, CaseResult, case_key};

/// Timeout headroom over the howto's emulated-seconds figures.
const MARGIN: f64 = 1.3;

/// How one blargg rom×model case decides pass/fail (see module docs).
#[derive(Clone, Debug, PartialEq)]
enum Protocol {
    /// Serial text protocol: pass ⇔ output contains `pass`; `Failed`
    /// terminates as a failure.
    Serial { pass: &'static str, timeout_s: f64 },
    /// $A000 memory-signature protocol: wait for status ≠ $80, pass ⇔ $00.
    Signature { timeout_s: f64 },
    /// Collection screenshot protocol: run `run_s` emulated seconds, then
    /// compare the last completed frame against `png` (a sibling of the
    /// ROM file), rendered with the 5→8 conversion `enc`.
    Screenshot {
        png: &'static str,
        run_s: f64,
        enc: RefEncoding,
    },
}

/// 5→8-bit channel conversion a reference PNG was rendered with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefEncoding {
    /// The collection's common palette, `(x << 3) | (x >> 2)`
    /// (`blargg/game-boy-test-roms-howto.md` §Test Success/Failure) — the
    /// core's native output, compared as-is.
    Common,
    /// Plain `x << 3`. Only `oam_bug-cgb.png` (decoded colors #000000 and
    /// #F8F8F8, where every other blargg reference uses #FFFFFF white)
    /// deviates from the howto's stated conversion; the core's low filler
    /// bits are masked off before comparing ([`shl3_mask`]).
    Shl3,
}

fn serial(pass: &'static str, timeout_s: f64) -> Protocol {
    Protocol::Serial { pass, timeout_s }
}

fn signature(timeout_s: f64) -> Protocol {
    Protocol::Signature { timeout_s }
}

fn screenshot(png: &'static str, run_s: f64) -> Protocol {
    Protocol::Screenshot {
        png,
        run_s,
        enc: RefEncoding::Common,
    }
}

/// Reduce a core XRGB8888 pixel to the [`RefEncoding::Shl3`] color space:
/// the core's 5→8 expansion keeps the 5 significant bits on top, so
/// masking the low 3 filler bits of each channel yields exactly `x << 3`.
fn shl3_mask(px: u32) -> u32 {
    px & 0x00F8_F8F8
}

/// Both DMG-family and CGB legs of a dual-model ROM (header flag $80).
fn both(p: Protocol) -> Vec<(Model, Protocol)> {
    vec![(Model::Dmg, p.clone()), (Model::Cgb, p)]
}

/// Map one collection-relative ROM path to its (model, protocol) cases.
/// Empty for paths outside the documented blargg matrix — the inventory
/// self-check ([`blargg_inventory_matches_disk`]) fails loudly if any
/// on-disk ROM routes to nothing.
fn route(rel: &str) -> Vec<(Model, Protocol)> {
    let Some(sub) = rel.strip_prefix("blargg/") else {
        return Vec::new();
    };
    // The one ROM sitting loose at the suite root (do not miss it). Its
    // header declares MBC1+RAM with RAM size *none* ($0147=$02, $0149=$00),
    // so the $A000 signature its runtime carries has no RAM to land in
    // (verified: the magic never appears) — the collection's timed
    // screenshot is the observable protocol. One reference covers both
    // models (howto: ✅ on DMG-C and every CGB revision at 2 s).
    if sub == "halt_bug.gb" {
        return both(screenshot("halt_bug-dmg-cgb.png", 2.0));
    }
    let Some((dir, rest)) = sub.split_once('/') else {
        return Vec::new();
    };
    if !rest.ends_with(".gb") && !rest.ends_with(".gbc") {
        return Vec::new();
    }
    let is_combined = !rest.contains('/');
    match dir {
        // 55 s DMG / 31 s CGB per the howto → flat 70 s bound for both legs.
        "cpu_instrs" => both(serial(
            if is_combined {
                "Passed all tests"
            } else {
                "Passed"
            },
            70.0,
        )),
        "instr_timing" => both(serial("Passed", 1.0 * MARGIN)),
        "mem_timing" => both(serial(
            if is_combined {
                "Passed all tests"
            } else {
                "Passed"
            },
            3.0 * MARGIN,
        )),
        "mem_timing-2" => both(signature(4.0 * MARGIN)),
        // Header CGB flag $00: DMG-only. Signature protocol for the
        // combined ROM too — see the module docs for the empirical
        // verification against howto footnote 3.
        "dmg_sound" => vec![(Model::Dmg, signature(36.0 * MARGIN))],
        // Header CGB flag $C0: CGB-only; combined as above (footnote 2).
        "cgb_sound" => vec![(Model::Cgb, signature(37.0 * MARGIN))],
        // The OAM corruption bug exists on DMG only; the combined ROM's
        // expected (bug-free) CGB screen is documented by oam_bug-cgb.png —
        // a documented-divergence screenshot case, not an emulator
        // special-case.
        "oam_bug" => {
            let mut cases = vec![(Model::Dmg, signature(21.0 * MARGIN))];
            if is_combined {
                cases.push((
                    Model::Cgb,
                    Protocol::Screenshot {
                        png: "oam_bug-cgb.png",
                        run_s: 21.0,
                        enc: RefEncoding::Shl3,
                    },
                ));
            }
            cases
        }
        // CGB-only ROM (header flag $C0). RAM-less like halt_bug
        // ($0147=$02, $0149=$00) → timed screenshots: the CGB pass screen,
        // plus the howto-documented expected DMG *failure* screen
        // (footnote 4, checksum 7F8F4AAF) as interrupt_time-dmg.png.
        "interrupt_time" => vec![
            (Model::Cgb, screenshot("interrupt_time-cgb.png", 2.0)),
            (Model::Dmg, screenshot("interrupt_time-dmg.png", 2.0)),
        ],
        _ => Vec::new(),
    }
}

/// `needle` occurs anywhere in the raw serial byte stream. The harness
/// polls in batches, so the verdict must wait for the *complete* needle —
/// matching a bare prefix like `Passed` of `Passed all tests` could read a
/// half-transmitted pass line as a failure.
fn serial_contains(haystack: &[u8], needle: &str) -> bool {
    let needle = needle.as_bytes();
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn t_cycles(seconds: f64) -> u64 {
    (seconds * f64::from(CLOCK_HZ)) as u64
}

/// [`harness::expect_frame_png`] for a [`RefEncoding::Shl3`] reference:
/// mask the frame into the reference's color space first, with the same
/// failure-report shape (reference path + ASCII frame for triage).
fn expect_frame_png_shl3(gb: &GameBoy, png_path: &Path) -> Result<(), String> {
    let img = common::png::load_png(png_path)?;
    let masked: Vec<u32> = gb.frame().iter().copied().map(shl3_mask).collect();
    common::framecmp::compare_frame_image(&masked, &img, CgbColorMap::Identity).map_err(|e| {
        format!(
            "{}: {e}\nemulator frame:\n{}",
            png_path.display(),
            common::framecmp::frame_ascii(gb.frame())
        )
    })
}

fn run_case(root: &Path, rel: &str, model: Model, protocol: &Protocol) -> Result<(), String> {
    let rom = std::fs::read(root.join(rel)).map_err(|e| format!("read failed: {e}"))?;
    let mut gb = harness::boot(&rom, model);
    match protocol {
        Protocol::Serial { pass, timeout_s } => {
            let out = harness::run_until_serial(&mut gb, t_cycles(*timeout_s), |out| {
                serial_contains(out, pass) || serial_contains(out, "Failed")
            })?;
            let text = String::from_utf8_lossy(&out);
            if text.contains(pass) {
                Ok(())
            } else {
                Err(format!("serial output: {text:?}"))
            }
        }
        Protocol::Signature { timeout_s } => {
            let deadline = gb.cycles().saturating_add(t_cycles(*timeout_s));
            // Phase 1: wait for the runtime to declare itself running. The
            // magic bytes land in $A001-$A003 *before* the $80 running
            // status reaches $A000, and uninitialized cart RAM reads $FF,
            // so polling straight for "any non-$80 status" would latch
            // that $FF init window as a bogus failure code (observed
            // per-instruction; a once-per-second probe only ever sees
            // $80 → final).
            let budget = deadline.saturating_sub(gb.cycles());
            harness::run_until(&mut gb, budget, |gb| {
                harness::blargg_signature_status(gb) == Some(0x80)
            })
            .map_err(|e| format!("test never started ($A000 never held $80): {e}"))?;
            // Phase 2: wait for the final status within the same budget.
            let budget = deadline.saturating_sub(gb.cycles());
            harness::run_until(&mut gb, budget, |gb| {
                harness::blargg_signature_status(gb).is_some_and(|s| s != 0x80)
            })
            .map_err(|e| format!("still running ($A000=$80): {e}"))?;
            match harness::blargg_signature_status(&gb) {
                Some(0) => Ok(()),
                Some(code) => Err(format!(
                    "status ${code:02X}: {:?}",
                    harness::blargg_signature_text(&gb).trim()
                )),
                // Phase 2's run_until only returns Ok with Some(!=0x80).
                None => unreachable!("signature vanished after settling"),
            }
        }
        Protocol::Screenshot { png, run_s, enc } => {
            // The howto's exit condition: the screen after `run_s` emulated
            // seconds matches the reference. `GameBoy::frame` is the
            // double-buffered *completed* frame (swapped at vblank), so no
            // extra frame advance is needed for a stable image.
            harness::run_for_seconds(&mut gb, *run_s);
            let rom_path = root.join(rel);
            let png_path = rom_path
                .parent()
                .expect("ROM path has a parent directory")
                .join(png);
            match enc {
                RefEncoding::Common => {
                    harness::expect_frame_png(&gb, &png_path, CgbColorMap::Identity)
                }
                RefEncoding::Shl3 => expect_frame_png_shl3(&gb, &png_path),
            }
        }
    }
}

/// Run every rom×model case under `dir` and ratchet against `baseline`.
/// `label` names the sub-suite in skip/failure messages (≠ `dir` only for
/// the loose `halt_bug.gb`, which sits at the suite root and is collected
/// non-recursively so the other sub-suites' directories are not swept up).
fn subsuite(label: &str, dir: &str, recursive: bool, baseline: &[&str]) {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(label, "test-roms/game-boy-test-roms-v7.0 not present");
        return;
    };
    let sub = root.join(dir);
    let mut roms = Vec::new();
    if let Err(e) = common::collect_roms(&sub, recursive, &mut roms) {
        panic!(
            "{label}: cannot enumerate ROMs under {}: {e}",
            sub.display()
        );
    }
    assert!(
        !roms.is_empty(),
        "{label}: {} contains no .gb/.gbc ROMs — corrupt checkout?",
        sub.display()
    );
    // blargg ROMs run for several emulated seconds each — fan the matrix out
    // across cores (results stay in ROM order, identical to sequential).
    let results: Vec<CaseResult> = harness::par_flat_map(&roms, |rom_path| {
        let rel = harness::rel_unix(&root, rom_path);
        route(&rel)
            .into_iter()
            .map(|(model, protocol)| CaseResult {
                key: case_key(&rel, model),
                result: harness::catch_case(|| run_case(&root, &rel, model, &protocol)),
            })
            .collect()
    });
    harness::assert_against_baseline(label, &results, baseline);
}

// Known-failure baselines (harness::assert_against_baseline ratchets:
// shrinking is progress, growing is a regression; empty = sub-suite green).
const BASELINE_CPU_INSTRS: &[&str] = &[];
const BASELINE_INSTR_TIMING: &[&str] = &[];
const BASELINE_MEM_TIMING: &[&str] = &[];
const BASELINE_MEM_TIMING_2: &[&str] = &[];
const BASELINE_DMG_SOUND: &[&str] = &[];
const BASELINE_CGB_SOUND: &[&str] = &[];
// The DMG OAM corruption bug is implemented (`Ppu::oam_bug`). The one
// remaining failure is a defect in the shipped *single*, not in the
// emulation (floor class F per the index in baselines/gambatte.txt —
// never fix in core): with hardware-correct corruption, 7-timing_effect prints
// twenty ~525-byte OAM dumps (19 corruptions swept across the first
// scanline plus one at the start of the second — exactly the set whose
// simulated checksum stream reproduces the ROM's own expected CRC
// $7D792E7C), and its unbounded $A004 text writer (common/shell.s
// `write_text_out`) overflows the 8 KiB cart RAM into the WRAM-resident
// runtime at $C000, destroying the test mid-run — the $A000 signature
// protocol can never complete, on real hardware just the same. The
// combined oam_bug.gb — a multi build with compact per-test reporting,
// and the only artifact the collection's howto verifies on hardware —
// runs the same test-7 logic (same CRC check) and passes.
const BASELINE_OAM_BUG: &[&str] = &["blargg/oam_bug/rom_singles/7-timing_effect.gb [Dmg]"];
const BASELINE_INTERRUPT_TIME: &[&str] = &[];
const BASELINE_HALT_BUG: &[&str] = &[];

#[test]
fn blargg_cpu_instrs() {
    subsuite(
        "blargg/cpu_instrs",
        "blargg/cpu_instrs",
        true,
        BASELINE_CPU_INSTRS,
    );
}

#[test]
fn blargg_instr_timing() {
    subsuite(
        "blargg/instr_timing",
        "blargg/instr_timing",
        true,
        BASELINE_INSTR_TIMING,
    );
}

#[test]
fn blargg_mem_timing() {
    subsuite(
        "blargg/mem_timing",
        "blargg/mem_timing",
        true,
        BASELINE_MEM_TIMING,
    );
}

#[test]
fn blargg_mem_timing_2() {
    subsuite(
        "blargg/mem_timing-2",
        "blargg/mem_timing-2",
        true,
        BASELINE_MEM_TIMING_2,
    );
}

#[test]
fn blargg_dmg_sound() {
    subsuite(
        "blargg/dmg_sound",
        "blargg/dmg_sound",
        true,
        BASELINE_DMG_SOUND,
    );
}

#[test]
fn blargg_cgb_sound() {
    subsuite(
        "blargg/cgb_sound",
        "blargg/cgb_sound",
        true,
        BASELINE_CGB_SOUND,
    );
}

#[test]
fn blargg_oam_bug() {
    subsuite("blargg/oam_bug", "blargg/oam_bug", true, BASELINE_OAM_BUG);
}

#[test]
fn blargg_interrupt_time() {
    subsuite(
        "blargg/interrupt_time",
        "blargg/interrupt_time",
        true,
        BASELINE_INTERRUPT_TIME,
    );
}

#[test]
fn blargg_halt_bug() {
    // Non-recursive: halt_bug.gb is the only ROM sitting loose at the
    // blargg/ root; the sub-suite directories have their own tests.
    subsuite("blargg/halt_bug", "blargg", false, BASELINE_HALT_BUG);
}

/// (claimed, exempted) collection-relative paths of every `.gb`/`.gbc`
/// under `blargg/`. Every blargg ROM produces at least one rom×model case
/// (the suite has no documented never-run ROMs), so `exempted` is empty —
/// [`blargg_inventory_matches_disk`] asserts that and the exact coverage.
pub fn inventory() -> (Vec<String>, Vec<String>) {
    let Some(root) = common::gbtr_root() else {
        return (Vec::new(), Vec::new());
    };
    let dir = root.join("blargg");
    let mut roms = Vec::new();
    common::collect_roms(&dir, true, &mut roms)
        .unwrap_or_else(|e| panic!("blargg: cannot enumerate ROMs under {}: {e}", dir.display()));
    let mut claimed = Vec::new();
    let mut exempted = Vec::new();
    for rom in &roms {
        let rel = harness::rel_unix(&root, rom);
        if route(&rel).is_empty() {
            exempted.push(rel);
        } else {
            claimed.push(rel);
        }
    }
    (claimed, exempted)
}

#[test]
fn blargg_inventory_matches_disk() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "blargg_inventory_matches_disk",
            "test-roms/game-boy-test-roms-v7.0 not present",
        );
        return;
    };
    let (claimed, exempted) = inventory();
    assert!(
        claimed.iter().all(|c| !exempted.contains(c)),
        "claimed ∩ exempted must be empty"
    );
    let mut union: Vec<String> = claimed.iter().chain(&exempted).cloned().collect();
    union.sort();
    let mut disk = Vec::new();
    common::collect_roms(&root.join("blargg"), true, &mut disk).expect("walk blargg/");
    let mut disk: Vec<String> = disk.iter().map(|p| harness::rel_unix(&root, p)).collect();
    disk.sort();
    assert_eq!(union, disk, "claimed ∪ exempted must equal the on-disk set");
    // No blargg ROM is exempt: every one runs through serial, signature or
    // timed-screenshot protocol on at least one model.
    assert!(exempted.is_empty(), "unexpected exemptions: {exempted:?}");
    assert_eq!(claimed.len(), 58, "blargg ships 58 ROMs");
    assert!(
        claimed.iter().any(|c| c == "blargg/halt_bug.gb"),
        "the loose halt_bug.gb at the suite root must be claimed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- routing: protocols ---

    #[test]
    fn cpu_instrs_combined_is_serial_on_both_models() {
        assert_eq!(
            route("blargg/cpu_instrs/cpu_instrs.gb"),
            both(serial("Passed all tests", 70.0))
        );
    }

    #[test]
    fn cpu_instrs_single_is_serial_passed() {
        assert_eq!(
            route("blargg/cpu_instrs/individual/01-special.gb"),
            both(serial("Passed", 70.0))
        );
    }

    #[test]
    fn instr_timing_is_serial() {
        assert_eq!(
            route("blargg/instr_timing/instr_timing.gb"),
            both(serial("Passed", 1.0 * MARGIN))
        );
    }

    #[test]
    fn mem_timing_combined_and_singles_are_serial() {
        assert_eq!(
            route("blargg/mem_timing/mem_timing.gb"),
            both(serial("Passed all tests", 3.0 * MARGIN))
        );
        assert_eq!(
            route("blargg/mem_timing/individual/01-read_timing.gb"),
            both(serial("Passed", 3.0 * MARGIN))
        );
    }

    #[test]
    fn mem_timing_2_is_signature() {
        assert_eq!(
            route("blargg/mem_timing-2/mem_timing.gb"),
            both(signature(4.0 * MARGIN))
        );
        assert_eq!(
            route("blargg/mem_timing-2/rom_singles/03-modify_timing.gb"),
            both(signature(4.0 * MARGIN))
        );
    }

    #[test]
    fn dmg_sound_single_is_signature_on_dmg_only() {
        assert_eq!(
            route("blargg/dmg_sound/rom_singles/01-registers.gb"),
            vec![(Model::Dmg, signature(36.0 * MARGIN))]
        );
    }

    #[test]
    fn dmg_sound_combined_is_signature_like_its_singles() {
        assert_eq!(
            route("blargg/dmg_sound/dmg_sound.gb"),
            vec![(Model::Dmg, signature(36.0 * MARGIN))]
        );
    }

    #[test]
    fn cgb_sound_single_is_signature_on_cgb_only() {
        assert_eq!(
            route("blargg/cgb_sound/rom_singles/12-wave.gb"),
            vec![(Model::Cgb, signature(37.0 * MARGIN))]
        );
    }

    #[test]
    fn cgb_sound_combined_is_signature_like_its_singles() {
        assert_eq!(
            route("blargg/cgb_sound/cgb_sound.gb"),
            vec![(Model::Cgb, signature(37.0 * MARGIN))]
        );
    }

    #[test]
    fn oam_bug_single_is_signature_on_dmg_only() {
        assert_eq!(
            route("blargg/oam_bug/rom_singles/1-lcd_sync.gb"),
            vec![(Model::Dmg, signature(21.0 * MARGIN))]
        );
    }

    #[test]
    fn oam_bug_combined_adds_cgb_screenshot_in_shl3_encoding() {
        assert_eq!(
            route("blargg/oam_bug/oam_bug.gb"),
            vec![
                (Model::Dmg, signature(21.0 * MARGIN)),
                (
                    Model::Cgb,
                    Protocol::Screenshot {
                        png: "oam_bug-cgb.png",
                        run_s: 21.0,
                        enc: RefEncoding::Shl3,
                    }
                ),
            ]
        );
    }

    #[test]
    fn shl3_mask_recovers_the_shl3_encoding_from_core_output() {
        // The core expands 5-bit channels as (x << 3) | (x >> 2); the low
        // three bits are copies of the top bits, so masking them off must
        // recover exactly x << 3 for every channel value.
        for x in 0..32u32 {
            let expanded = (x << 3) | (x >> 2);
            assert_eq!(shl3_mask(expanded * 0x0001_0101), (x << 3) * 0x0001_0101);
        }
        // The X byte is dropped like the comparators do.
        assert_eq!(shl3_mask(0xFFFF_FFFF), 0x00F8_F8F8);
    }

    #[test]
    fn interrupt_time_is_screenshot_per_model() {
        assert_eq!(
            route("blargg/interrupt_time/interrupt_time.gb"),
            vec![
                (Model::Cgb, screenshot("interrupt_time-cgb.png", 2.0)),
                (Model::Dmg, screenshot("interrupt_time-dmg.png", 2.0)),
            ]
        );
    }

    #[test]
    fn halt_bug_is_screenshot_on_both_models() {
        assert_eq!(
            route("blargg/halt_bug.gb"),
            both(screenshot("halt_bug-dmg-cgb.png", 2.0))
        );
    }

    #[test]
    fn unknown_paths_route_to_nothing() {
        assert!(route("blargg/readme.txt").is_empty());
        assert!(route("blargg/unknown_dir/foo.gb").is_empty());
        assert!(route("gambatte/foo.gbc").is_empty());
    }

    // --- serial stream matching ---

    #[test]
    fn serial_contains_finds_needle_anywhere() {
        assert!(serial_contains(
            b"cpu_instrs\n\nPassed all tests\n",
            "Passed all tests"
        ));
        assert!(serial_contains(b"Failed", "Failed"));
        assert!(serial_contains(b"xFailedx", "Failed"));
    }

    #[test]
    fn serial_contains_rejects_partial_and_oversized_needles() {
        // A half-transmitted "Passed all te" must not match yet.
        assert!(!serial_contains(b"Passed all te", "Passed all tests"));
        assert!(!serial_contains(b"", "Passed"));
        assert!(!serial_contains(b"Pass", "Passed"));
    }
}
