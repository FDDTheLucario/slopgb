//! Shared helpers for the mooneye test-suite integration harness
//! (`tests/mooneye.rs`).
//!
//! # Test protocol
//!
//! A mooneye test ROM signals completion by executing `LD B,B` (opcode 0x40),
//! exposed as [`GameBoy::debug_breakpoint_hit`]. The test passed iff the
//! registers then hold the Fibonacci sequence B=3, C=5, D=8, E=13, H=21, L=34
//! (test-roms-src/README.markdown, "Pass/fail reporting"). Anything else —
//! including 120 emulated seconds without the breakpoint — is a failure.
//!
//! # Model matrix
//!
//! Filename suffixes name the hardware a ROM is expected to pass on
//! (test-roms-src/README.markdown, "Test naming"):
//!
//! - exact revisions: `-dmg0`, `-dmgABC`, `-dmgABCmgb`, `-mgb`, `-sgb`,
//!   `-sgb2`, `-cgb`, `-cgb0`, `-cgbABCDE`
//! - hardware groups, combinable letter-per-group: `G` = dmg+mgb,
//!   `S` = sgb+sgb2, `C` = cgb+agb+ags, `A` = agb+ags
//!
//! Per the architecture contract (docs/ARCHITECTURE.md, src/model.rs) the
//! `C` group is run on [`Model::Cgb`] only and `A` on [`Model::Agb`] only:
//! AGS is not modeled, and `-C` ROMs that depend on AGB-only deviations have
//! dedicated `-A` variants.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Once;

use slopgb_core::{GameBoy, Model, SCREEN_PIXELS, SCREEN_W};

pub mod framecmp;
pub mod png;
mod protocol;
pub use protocol::{FIB, TIMEOUT_TCYCLES};

/// Default DMG shade-to-RGB mapping the harness expects from
/// `Ppu::frame()` on DMG-family models, low 24 bits only (the X byte of
/// XRGB8888 is ignored). Shade 0 (lightest) .. shade 3 (darkest). These are
/// the grey levels of the suite's own reference image
/// `manual-only/sprite_priority-expected.png` (2-bit greyscale PNG).
pub const DMG_SHADE_RGB: [u32; 4] = [0x00FF_FFFF, 0x00AA_AAAA, 0x0055_5555, 0x0000_0000];

/// Expected `manual-only/sprite_priority` frame as one shade class (0..=3)
/// per pixel, row-major 160x144.
///
/// Provenance: decoded offline from the mooneye test suite's own reference
/// image `test-roms-src/manual-only/sprite_priority-expected.png` (160x144,
/// 2-bit greyscale, PNG grey level g maps to DMG shade 3-g). The identical
/// image (RGB-expanded with levels FF/AA/55/00) is shipped by
/// gbdev/GBEmulatorShootout as `testroms/mooneye/manual-only/
/// sprite_priority.png`.
pub const SPRITE_PRIORITY_SHADES: &[u8; SCREEN_PIXELS] =
    include_bytes!("../expected/sprite_priority.bin");

/// Expected `madness/mgb_oam_dma_halt_sprites` frame as one shade class
/// (0..=3) per pixel, row-major 160x144.
///
/// Provenance: decoded offline from the suite's own reference image
/// `test-roms-src/madness/mgb_oam_dma_halt_sprites_expected.png` (160x144,
/// 8-bit greyscale with the three levels 255/176/104; descending brightness
/// maps to DMG shades 0/1/2 — the ROM's BGP $54 draws its checkerboard with
/// shades 0 and 1, and OBP1 $AA maps every sprite color to shade 2).
pub const MGB_OAM_DMA_HALT_SPRITES_SHADES: &[u8; SCREEN_PIXELS] =
    include_bytes!("../expected/mgb_oam_dma_halt_sprites.bin");

/// Locate the newest mooneye test-suite release directory
/// (`<repo>/test-roms/mts-*`). `None` when the ROMs are not checked out —
/// callers print a skip notice instead of failing (unless
/// `SLOPGB_REQUIRE_ROMS=1`, see [`skip_or_fail`]).
pub fn mts_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-roms");
    let mut releases: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("mts-"))
        })
        .collect();
    // Lexicographic sort picks the newest release only because the names are
    // date-prefixed (`mts-YYYYMMDD-...`).
    releases.sort();
    releases.pop()
}

/// Pinned c-sp/game-boy-test-roms release directory name under
/// `<repo>/test-roms/` — the multi-suite aggregation (blargg, gambatte,
/// dmg-acid2, ...). The release zip has no top-level directory;
/// `test-roms/download.sh` extracts it into this directory and verifies the
/// zip's sha256, so bump the script's pin together with this name.
pub const GBTR_DIR: &str = "game-boy-test-roms-v7.0";

/// Locate the pinned game-boy-test-roms collection
/// (`<repo>/test-roms/game-boy-test-roms-v7.0`). `None` when the collection
/// is not checked out — callers print a skip notice instead of failing
/// (unless `SLOPGB_REQUIRE_ROMS=1`, see [`missing_gbtr_outcome`]).
pub fn gbtr_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-roms")
        .join(GBTR_DIR);
    root.is_dir().then_some(root)
}

/// Decide how a missing ROM bundle/directory is handled, given the value of
/// the `SLOPGB_REQUIRE_ROMS` environment variable: `Ok` carries the skip
/// notice to print, `Err` the hard-failure message (when the variable is
/// `1`, as CI sets it, so a checkout that never ran `test-roms/download.sh`
/// cannot come up all-green). The env value is a *parameter* rather than
/// read here so the decision is unit-testable without mutating process
/// environment from parallel test threads. `bundle` names the missing
/// fetchable in the failure message.
fn missing_bundle_outcome(
    require_roms: Option<&str>,
    test: &str,
    missing: &str,
    bundle: &str,
) -> Result<String, String> {
    if require_roms == Some("1") {
        Err(format!(
            "{test}: {missing}, and SLOPGB_REQUIRE_ROMS=1 forbids skipping — \
             run test-roms/download.sh to fetch {bundle}"
        ))
    } else {
        Ok(format!("skipping {test}: {missing}"))
    }
}

/// [`missing_bundle_outcome`] for the mooneye test-suite ROMs.
pub fn missing_roms_outcome(
    require_roms: Option<&str>,
    test: &str,
    missing: &str,
) -> Result<String, String> {
    missing_bundle_outcome(require_roms, test, missing, "the mooneye test ROMs")
}

/// [`missing_bundle_outcome`] for the game-boy-test-roms collection.
pub fn missing_gbtr_outcome(
    require_roms: Option<&str>,
    test: &str,
    missing: &str,
) -> Result<String, String> {
    missing_bundle_outcome(
        require_roms,
        test,
        missing,
        "the game-boy-test-roms collection",
    )
}

/// Print a skip notice for a missing ROM bundle/directory, or panic when
/// `SLOPGB_REQUIRE_ROMS=1` (see [`missing_roms_outcome`]).
pub fn skip_or_fail(test: &str, missing: &str) {
    let require_roms = std::env::var("SLOPGB_REQUIRE_ROMS").ok();
    match missing_roms_outcome(require_roms.as_deref(), test, missing) {
        Ok(notice) => println!("{notice}"),
        Err(msg) => panic!("{msg}"),
    }
}

/// Models a ROM (path relative to the mts root) must pass on.
///
/// An empty vector means "no modeled hardware revision" (e.g. `-cgb0`:
/// `misc/boot_div-cgb0.s` documents "pass: CGB 0 / fail: CGB ABCDE", and we
/// model revision ABCDE as [`Model::Cgb`], so no modeled machine can pass).
pub fn models_for(rel: &Path) -> Vec<Model> {
    let top = rel.iter().next().and_then(|c| c.to_str()).unwrap_or("");
    // madness/ is covered by `run_madness` frame comparison, never by the
    // breakpoint protocol: mgb_oam_dma_halt_sprites halts forever and never
    // executes LD B,B. Defense in depth — checked before suffix parsing so
    // that even a future suffixed madness ROM swept up by `run_group` is
    // skipped loudly instead of timing out for 120 emulated seconds per
    // model.
    if top == "madness" {
        return vec![];
    }
    // Mapper tests probe the cartridge only; they are model-agnostic.
    // One plain and one CGB machine give double-speed-free coverage.
    if top == "emulator-only" {
        return vec![Model::Dmg, Model::Cgb];
    }
    let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if let Some((_, sfx)) = stem.rsplit_once('-') {
        if let Some(models) = suffix_models(sfx) {
            return models;
        }
    }
    match top {
        // misc/ is "extra tests for CGB / AGB hardware" (suite README); all
        // current ROMs there carry suffixes, this is a conservative default.
        "misc" => vec![Model::Cgb, Model::Agb],
        _ => vec![
            Model::Dmg,
            Model::Mgb,
            Model::Sgb,
            Model::Sgb2,
            Model::Cgb,
            Model::Agb,
        ],
    }
}

/// Map one filename suffix (the part after the last `-`) to models, or
/// `None` if it is not a recognized model suffix.
fn suffix_models(sfx: &str) -> Option<Vec<Model>> {
    let models = match sfx {
        "dmg0" => vec![Model::Dmg0],
        "dmgABC" => vec![Model::Dmg],
        "dmgABCmgb" => vec![Model::Dmg, Model::Mgb],
        "mgb" => vec![Model::Mgb],
        "sgb" => vec![Model::Sgb],
        "sgb2" => vec![Model::Sgb2],
        "cgb" | "cgbABCDE" => vec![Model::Cgb],
        // CGB revision 0 is not modeled; the only -cgb0 ROM is documented to
        // fail on the CGB ABCDE revision that Model::Cgb emulates.
        "cgb0" => vec![],
        "agb" => vec![Model::Agb],
        _ => return group_letter_models(sfx),
    };
    Some(models)
}

/// Combined group letters, e.g. `GS` = dmg+mgb+sgb+sgb2.
fn group_letter_models(sfx: &str) -> Option<Vec<Model>> {
    if sfx.is_empty() || !sfx.chars().all(|c| matches!(c, 'G' | 'S' | 'C' | 'A')) {
        return None;
    }
    let mut models = Vec::new();
    for c in sfx.chars() {
        match c {
            'G' => models.extend([Model::Dmg, Model::Mgb]),
            'S' => models.extend([Model::Sgb, Model::Sgb2]),
            'C' => models.push(Model::Cgb),
            'A' => models.push(Model::Agb),
            _ => unreachable!(),
        }
    }
    Some(models)
}

/// Check the post-breakpoint register signature.
pub fn check_fib(b: u8, c: u8, d: u8, e: u8, h: u8, l: u8) -> Result<(), String> {
    if [b, c, d, e, h, l] == FIB {
        Ok(())
    } else {
        Err(format!(
            "regs at breakpoint B={b:02X} C={c:02X} D={d:02X} E={e:02X} H={h:02X} L={l:02X}, \
             want Fibonacci 03/05/08/0D/15/22"
        ))
    }
}

/// Run one ROM image on `model` until the `LD B,B` breakpoint or timeout,
/// then check the Fibonacci signature.
pub fn run_breakpoint_rom(rom: &[u8], model: Model) -> Result<(), String> {
    let mut gb =
        GameBoy::new(model, rom.to_vec()).map_err(|e| format!("cartridge rejected: {e}"))?;
    while !gb.debug_breakpoint_hit() {
        if gb.cycles() > TIMEOUT_TCYCLES {
            return Err(format!(
                "timeout: no LD B,B after {} T-cycles (120 emulated seconds)",
                gb.cycles()
            ));
        }
        gb.step();
    }
    let r = gb.cpu_regs();
    check_fib(r.b, r.c, r.d, r.e, r.h, r.l)
}

/// Collect `.gb`/`.gbc` files under `dir`, sorted for determinism.
/// Non-recursive unless `recursive` (so `acceptance/` does not swallow its
/// per-topic subdirectories, which have their own test functions).
///
/// I/O errors (unreadable directory, failing entry) are propagated rather
/// than swallowed, so a permission problem or interrupted extraction cannot
/// masquerade as an empty — and therefore silently green — group.
pub fn collect_roms(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut paths = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<PathBuf>>>()?;
    paths.sort();
    for p in paths {
        if p.is_dir() {
            if recursive {
                collect_roms(&p, true, out)?;
            }
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x == "gb" || x == "gbc")
        {
            out.push(p);
        }
    }
    Ok(())
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

thread_local! {
    /// True while the current thread is inside [`quiet_catch_unwind`].
    static SUPPRESS_PANIC_OUTPUT: Cell<bool> = const { Cell::new(false) };
}

/// Run `f`, catching panics *without* the default panic hook printing a
/// "thread panicked at ..." report (plus backtrace note) for each one. While
/// core subsystems are still `todo!()`, every PPU-dependent (rom x model)
/// combination panics; hundreds of duplicate hook reports would bury the
/// structured failure list [`run_group`] builds.
///
/// A bare no-op hook would also swallow the *summary* panic that carries that
/// failure list (and any other test's assertion message), so instead the
/// installed hook delegates to the previous one unless the current thread has
/// opted into suppression. The hook is installed exactly once per test binary
/// (`set_hook` is process-global); the suppression flag is per-thread so
/// parallel test threads cannot silence each other.
fn quiet_catch_unwind<R>(
    f: impl FnOnce() -> R,
) -> Result<R, Box<dyn std::any::Any + Send + 'static>> {
    static INSTALL_HOOK: Once = Once::new();
    INSTALL_HOOK.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !SUPPRESS_PANIC_OUTPUT.with(Cell::get) {
                default_hook(info);
            }
        }));
    });
    SUPPRESS_PANIC_OUTPUT.with(|s| s.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    SUPPRESS_PANIC_OUTPUT.with(|s| s.set(false));
    result
}

/// Run every (ROM x model) combination of one directory group through the
/// breakpoint protocol, collecting *all* failures, then panic with a
/// readable `rom [model]: reason` list. Panics inside the core (e.g.
/// unimplemented subsystems) are caught and reported per combination so one
/// broken ROM cannot mask the rest of the group.
pub fn run_group(dir: &str, recursive: bool) {
    let Some(root) = mts_root() else {
        skip_or_fail(dir, "no mooneye ROMs under <repo>/test-roms/mts-*");
        return;
    };
    let group_dir = root.join(dir);
    if !group_dir.is_dir() {
        skip_or_fail(dir, &format!("{} not present", group_dir.display()));
        return;
    }
    let mut roms = Vec::new();
    if let Err(e) = collect_roms(&group_dir, recursive, &mut roms) {
        panic!(
            "{dir}: cannot enumerate ROMs under {}: {e}",
            group_dir.display()
        );
    }
    // Only a *missing* mts root / group directory is an intentional skip; an
    // existing-but-empty group means a corrupt checkout and must fail rather
    // than report "0 combinations passed" as green.
    assert!(
        !roms.is_empty(),
        "{dir} exists but contains no .gb/.gbc ROMs — corrupt checkout?"
    );
    let mut failures: Vec<String> = Vec::new();
    let mut passed = 0usize;
    for rom_path in &roms {
        let rel = rom_path.strip_prefix(&root).unwrap_or(rom_path);
        let models = models_for(rel);
        if models.is_empty() {
            println!(
                "note: {} skipped (no modeled hardware revision)",
                rel.display()
            );
            continue;
        }
        let rom = match std::fs::read(rom_path) {
            Ok(rom) => rom,
            Err(e) => {
                // One entry per suppressed (rom x model) combination, so the
                // "{n} of {total}" denominator still counts the full matrix.
                for model in &models {
                    failures.push(format!("{} [{model:?}]: read failed: {e}", rel.display()));
                }
                continue;
            }
        };
        for model in models {
            match quiet_catch_unwind(|| run_breakpoint_rom(&rom, model)) {
                Ok(Ok(())) => passed += 1,
                Ok(Err(reason)) => {
                    failures.push(format!("{} [{model:?}]: {reason}", rel.display()))
                }
                Err(payload) => failures.push(format!(
                    "{} [{model:?}]: panicked: {}",
                    rel.display(),
                    panic_message(payload.as_ref())
                )),
            }
        }
    }
    if failures.is_empty() {
        println!("{dir}: {passed} rom x model combinations passed");
    } else {
        panic!(
            "{dir}: {} of {} rom x model combinations failed:\n  {}",
            failures.len(),
            passed + failures.len(),
            failures.join("\n  ")
        );
    }
}

fn pixel_coords(index: usize, len: usize) -> String {
    if len == SCREEN_PIXELS {
        format!("({},{})", index % SCREEN_W, index / SCREEN_W)
    } else {
        format!("#{index}")
    }
}

/// Exact-color comparison for DMG-family models: every pixel must equal the
/// expected shade class rendered through [`DMG_SHADE_RGB`] (X byte ignored).
pub fn compare_frame_exact_dmg(frame: &[u32], expected: &[u8]) -> Result<(), String> {
    assert_eq!(frame.len(), expected.len());
    let mut mismatches = 0usize;
    let mut samples = Vec::new();
    for (i, (&px, &class)) in frame.iter().zip(expected).enumerate() {
        let Some(&want) = DMG_SHADE_RGB.get(usize::from(class)) else {
            return Err(format!(
                "{}: invalid shade class {class} in expected data (must be 0..=3) — \
                 corrupt reference asset?",
                pixel_coords(i, frame.len())
            ));
        };
        if px & 0x00FF_FFFF != want {
            mismatches += 1;
            if samples.len() < 8 {
                samples.push(format!(
                    "{}: want shade {} = {want:06X}, got {:06X}",
                    pixel_coords(i, frame.len()),
                    class,
                    px & 0x00FF_FFFF
                ));
            }
        }
    }
    if mismatches == 0 {
        Ok(())
    } else {
        Err(format!(
            "{mismatches} pixel(s) differ from reference image: {}",
            samples.join("; ")
        ))
    }
}

/// Palette-independent structural comparison (used on CGB, where DMG-compat
/// colors come from boot-ROM-assigned palette RAM rather than fixed greys):
///
/// - all pixels of one expected shade class must share one actual color,
/// - different classes must render as different colors,
/// - luminance must strictly decrease with the shade class index (so a
///   priority mix-up that swaps a light-grey sprite with a black one cannot
///   slip through as a mere relabeling).
pub fn compare_frame_structural(frame: &[u32], expected: &[u8]) -> Result<(), String> {
    assert_eq!(frame.len(), expected.len());
    let mut class_color: [Option<u32>; 4] = [None; 4];
    for (i, (&px, &class)) in frame.iter().zip(expected).enumerate() {
        let px = px & 0x00FF_FFFF;
        let Some(slot) = class_color.get_mut(usize::from(class)) else {
            return Err(format!(
                "{}: invalid shade class {class} in expected data (must be 0..=3) — \
                 corrupt reference asset?",
                pixel_coords(i, frame.len())
            ));
        };
        match *slot {
            None => *slot = Some(px),
            Some(c) if c == px => {}
            Some(c) => {
                return Err(format!(
                    "{}: shade class {} rendered both as {c:06X} and {px:06X}",
                    pixel_coords(i, frame.len()),
                    class
                ));
            }
        }
    }
    let lum = |c: u32| ((c >> 16) & 0xFF) + ((c >> 8) & 0xFF) + (c & 0xFF);
    let present: Vec<(usize, u32)> = class_color
        .iter()
        .enumerate()
        .filter_map(|(class, c)| c.map(|c| (class, c)))
        .collect();
    for (a, &(class_a, color_a)) in present.iter().enumerate() {
        for &(class_b, color_b) in &present[a + 1..] {
            if color_a == color_b {
                return Err(format!(
                    "shade classes {class_a} and {class_b} both rendered as {color_a:06X}"
                ));
            }
            if lum(color_a) <= lum(color_b) {
                return Err(format!(
                    "shade class {class_a} ({color_a:06X}) not brighter than \
                     class {class_b} ({color_b:06X})"
                ));
            }
        }
    }
    Ok(())
}

/// Run `rom` on `model` until at least `frames` frames have completed and
/// return a copy of the last frame. Bounded by [`TIMEOUT_TCYCLES`].
pub fn run_for_frames(rom: &[u8], model: Model, frames: u64) -> Result<Vec<u32>, String> {
    let mut gb =
        GameBoy::new(model, rom.to_vec()).map_err(|e| format!("cartridge rejected: {e}"))?;
    while gb.frame_count() < frames {
        if gb.cycles() > TIMEOUT_TCYCLES {
            return Err(format!(
                "timeout: only {} frames after {} T-cycles",
                gb.frame_count(),
                gb.cycles()
            ));
        }
        gb.run_frame();
    }
    Ok(gb.frame().to_vec())
}

/// `manual-only/sprite_priority`: render ~10 frames and compare the frame
/// against the suite's reference image instead of the breakpoint protocol.
pub fn run_sprite_priority() {
    let Some(root) = mts_root() else {
        skip_or_fail(
            "sprite_priority",
            "no mooneye ROMs under <repo>/test-roms/mts-*",
        );
        return;
    };
    let rom_path = root.join("manual-only/sprite_priority.gb");
    if !rom_path.is_file() {
        skip_or_fail(
            "sprite_priority",
            &format!("{} not present", rom_path.display()),
        );
        return;
    }
    let rom = std::fs::read(&rom_path).expect("read sprite_priority.gb");
    let mut failures: Vec<String> = Vec::new();
    for model in [Model::Dmg, Model::Cgb] {
        let result = quiet_catch_unwind(|| {
            let frame = run_for_frames(&rom, model, 10)?;
            if model.is_cgb() {
                compare_frame_structural(&frame, SPRITE_PRIORITY_SHADES)
            } else {
                compare_frame_exact_dmg(&frame, SPRITE_PRIORITY_SHADES)
            }
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => {
                failures.push(format!(
                    "manual-only/sprite_priority.gb [{model:?}]: {reason}"
                ));
            }
            Err(payload) => failures.push(format!(
                "manual-only/sprite_priority.gb [{model:?}]: panicked: {}",
                panic_message(payload.as_ref())
            )),
        }
    }
    if !failures.is_empty() {
        panic!(
            "sprite_priority: {} model(s) failed:\n  {}",
            failures.len(),
            failures.join("\n  ")
        );
    }
}

/// `madness/mgb_oam_dma_halt_sprites`: this ROM never executes `LD B,B` —
/// it halts forever with no interrupt enabled and the pass criterion is the
/// screen the still-running PPU keeps rendering from the HALT-frozen OAM
/// DMA (test-roms-src/madness/mgb_oam_dma_halt_sprites.s: "Verified
/// behaviour: MGB: As described here and visualized by *_expected.png"; the
/// asm documents MGB only, so only [`Model::Mgb`] is run). Render ~10
/// frames and compare against the vendored reference, like
/// `run_sprite_priority`.
pub fn run_madness() {
    let Some(root) = mts_root() else {
        skip_or_fail("madness", "no mooneye ROMs under <repo>/test-roms/mts-*");
        return;
    };
    let rom_path = root.join("madness/mgb_oam_dma_halt_sprites.gb");
    if !rom_path.is_file() {
        skip_or_fail("madness", &format!("{} not present", rom_path.display()));
        return;
    }
    let rom = std::fs::read(&rom_path).expect("read mgb_oam_dma_halt_sprites.gb");
    let result = quiet_catch_unwind(|| {
        let frame = run_for_frames(&rom, Model::Mgb, 10)?;
        compare_frame_exact_dmg(&frame, MGB_OAM_DMA_HALT_SPRITES_SHADES)
    });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => panic!("madness/mgb_oam_dma_halt_sprites.gb [Mgb]: {reason}"),
        Err(payload) => panic!(
            "madness/mgb_oam_dma_halt_sprites.gb [Mgb]: panicked: {}",
            panic_message(payload.as_ref())
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models(path: &str) -> Vec<Model> {
        models_for(Path::new(path))
    }

    // --- model matrix: exact revision suffixes ---

    #[test]
    fn suffix_dmg0() {
        assert_eq!(models("acceptance/boot_div-dmg0.gb"), [Model::Dmg0]);
    }

    #[test]
    fn suffix_dmg_abc() {
        assert_eq!(models("acceptance/boot_regs-dmgABC.gb"), [Model::Dmg]);
    }

    #[test]
    fn suffix_dmg_abc_mgb() {
        assert_eq!(
            models("acceptance/boot_div-dmgABCmgb.gb"),
            [Model::Dmg, Model::Mgb]
        );
        assert_eq!(
            models("acceptance/serial/boot_sclk_align-dmgABCmgb.gb"),
            [Model::Dmg, Model::Mgb]
        );
    }

    #[test]
    fn suffix_mgb_sgb_sgb2() {
        assert_eq!(models("acceptance/boot_regs-mgb.gb"), [Model::Mgb]);
        assert_eq!(models("acceptance/boot_regs-sgb.gb"), [Model::Sgb]);
        assert_eq!(models("acceptance/boot_regs-sgb2.gb"), [Model::Sgb2]);
    }

    #[test]
    fn suffix_cgb_variants() {
        assert_eq!(models("misc/boot_regs-cgb.gb"), [Model::Cgb]);
        assert_eq!(models("misc/boot_div-cgbABCDE.gb"), [Model::Cgb]);
    }

    #[test]
    fn suffix_cgb0_is_skipped() {
        // misc/boot_div-cgb0.s: "pass: CGB 0 / fail: ... CGB ABCDE". We model
        // ABCDE, so this ROM maps to no machine at all.
        assert!(models("misc/boot_div-cgb0.gb").is_empty());
    }

    // --- model matrix: group letters ---

    #[test]
    fn suffix_s_means_both_super_game_boys() {
        assert_eq!(
            models("acceptance/boot_div-S.gb"),
            [Model::Sgb, Model::Sgb2]
        );
        // Trailing digits in the test name must not confuse suffix parsing.
        assert_eq!(
            models("acceptance/boot_div2-S.gb"),
            [Model::Sgb, Model::Sgb2]
        );
    }

    #[test]
    fn suffix_gs_means_dmg_and_sgb_families() {
        assert_eq!(
            models("acceptance/bits/unused_hwio-GS.gb"),
            [Model::Dmg, Model::Mgb, Model::Sgb, Model::Sgb2]
        );
    }

    #[test]
    fn suffix_c_and_a() {
        assert_eq!(models("misc/bits/unused_hwio-C.gb"), [Model::Cgb]);
        assert_eq!(models("misc/boot_regs-A.gb"), [Model::Agb]);
        assert_eq!(models("misc/boot_div-A.gb"), [Model::Agb]);
    }

    // --- model matrix: defaults per directory ---

    #[test]
    fn no_suffix_acceptance_runs_everywhere_but_dmg0() {
        assert_eq!(
            models("acceptance/div_timing.gb"),
            [
                Model::Dmg,
                Model::Mgb,
                Model::Sgb,
                Model::Sgb2,
                Model::Cgb,
                Model::Agb
            ]
        );
        // Underscores are not suffix separators.
        assert_eq!(models("acceptance/timer/tim00_div_trigger.gb").len(), 6);
    }

    #[test]
    fn madness_is_frame_compare_only() {
        // madness/ ROMs are verified by `run_madness` frame comparison and
        // must map to an *empty* breakpoint-protocol matrix: the eternal-HALT
        // ROM never executes LD B,B, so running it via `run_group` would be
        // a 120-emulated-second timeout per model. Guards both the suffixless
        // stem (the shipped ROM) and any future suffixed madness ROM.
        assert!(models("madness/mgb_oam_dma_halt_sprites.gb").is_empty());
        assert!(models("madness/future_rom-mgb.gb").is_empty());
    }

    #[test]
    fn emulator_only_is_model_agnostic() {
        assert_eq!(
            models("emulator-only/mbc1/rom_512kb.gb"),
            [Model::Dmg, Model::Cgb]
        );
        // Even "multicart_rom_8Mb" (no model suffix, despite Mb) stays put.
        assert_eq!(
            models("emulator-only/mbc1/multicart_rom_8Mb.gb"),
            [Model::Dmg, Model::Cgb]
        );
    }

    #[test]
    fn unknown_group_letters_fall_back_to_default() {
        assert_eq!(group_letter_models("X"), None);
        assert_eq!(group_letter_models(""), None);
        assert_eq!(group_letter_models("expected"), None);
    }

    // --- breakpoint protocol register check ---

    #[test]
    fn fib_signature_passes() {
        assert!(check_fib(3, 5, 8, 13, 21, 34).is_ok());
    }

    #[test]
    fn fail_signature_is_reported_with_regs() {
        let err = check_fib(0x42, 0x42, 0x42, 0x42, 0x42, 0x42).unwrap_err();
        assert!(err.contains("B=42"), "{err}");
        assert!(err.contains("Fibonacci"), "{err}");
    }

    // --- vendored frame-compare reference assets ---

    fn shade_histogram(shades: &[u8; SCREEN_PIXELS]) -> [usize; 4] {
        let mut histogram = [0usize; 4];
        for &shade in shades.iter() {
            assert!(shade < 4, "shade class out of range: {shade}");
            histogram[usize::from(shade)] += 1;
        }
        histogram
    }

    #[test]
    fn sprite_priority_reference_histogram() {
        // Locks the asset against corruption: decoded from the suite's
        // sprite_priority-expected.png, the image is 22705 white, 114 light
        // grey, 0 dark grey and 221 black pixels.
        assert_eq!(
            shade_histogram(SPRITE_PRIORITY_SHADES),
            [22705, 114, 0, 221]
        );
    }

    #[test]
    fn mgb_oam_dma_halt_sprites_reference_histogram() {
        // Decoded from the suite's mgb_oam_dma_halt_sprites_expected.png:
        // an even white/light-grey 8x8 checkerboard (11520 + 11520 pixels)
        // plus the 18 dark-grey pixels of one glitch sprite — the '8' glyph
        // (tile $38) at Y=56/X=90 the asm derives from old=$30/next=$40/
        // new=$1A. The sprite's 18 pixels replace 15 light-grey and 3 white
        // checkerboard pixels (its right column, x=88, crosses an 8x8 tile
        // boundary), hence 11517 / 11505.
        assert_eq!(
            shade_histogram(MGB_OAM_DMA_HALT_SPRITES_SHADES),
            [11517, 11505, 18, 0]
        );
    }

    // --- frame comparison helpers (on tiny synthetic frames) ---

    #[test]
    fn exact_compare_accepts_default_palette() {
        let expected = [0u8, 1, 2, 3];
        let frame = [0xFFFF_FFFF, 0xFFAA_AAAA, 0xFF55_5555, 0xFF00_0000];
        assert!(compare_frame_exact_dmg(&frame, &expected).is_ok());
    }

    #[test]
    fn exact_compare_ignores_x_byte_only() {
        let expected = [0u8];
        assert!(compare_frame_exact_dmg(&[0x00FF_FFFF], &expected).is_ok());
        assert!(compare_frame_exact_dmg(&[0x12FF_FFFF], &expected).is_ok());
        let err = compare_frame_exact_dmg(&[0xFFFF_FFFE], &expected).unwrap_err();
        assert!(err.contains("1 pixel(s)"), "{err}");
    }

    #[test]
    fn exact_compare_rejects_invalid_shade_class() {
        // A regenerated/corrupt expected .bin must produce a diagnostic
        // naming the bad pixel, not an index-out-of-bounds panic.
        let err = compare_frame_exact_dmg(&[0x00FF_FFFF], &[4u8]).unwrap_err();
        assert!(err.contains("invalid shade class 4"), "{err}");
        assert!(err.contains("#0"), "{err}");
    }

    #[test]
    fn structural_compare_rejects_invalid_shade_class() {
        let err = compare_frame_structural(&[0x00FF_FFFF], &[7u8]).unwrap_err();
        assert!(err.contains("invalid shade class 7"), "{err}");
        assert!(err.contains("#0"), "{err}");
    }

    #[test]
    fn structural_compare_accepts_any_consistent_ordered_palette() {
        // CGB compat colors: class 0 bright, class 1 mid, class 3 dark.
        let expected = [0u8, 1, 3, 0, 1, 3];
        let frame = [
            0x00E0_F8D0,
            0x0088_C070,
            0x0008_1820,
            0x00E0_F8D0,
            0x0088_C070,
            0x0008_1820,
        ];
        assert!(compare_frame_structural(&frame, &expected).is_ok());
    }

    #[test]
    fn structural_compare_rejects_inconsistent_class_color() {
        let expected = [1u8, 1];
        let err = compare_frame_structural(&[0x0011_2233, 0x0011_2234], &expected).unwrap_err();
        assert!(err.contains("rendered both"), "{err}");
    }

    #[test]
    fn structural_compare_rejects_merged_classes() {
        let expected = [0u8, 3];
        let err = compare_frame_structural(&[0x00AB_CDEF, 0x00AB_CDEF], &expected).unwrap_err();
        assert!(err.contains("both rendered"), "{err}");
    }

    #[test]
    fn structural_compare_rejects_swapped_brightness() {
        // A priority bug that paints the light-grey sprite black and vice
        // versa produces the same partition with inverted luminance — the
        // ordering requirement must catch it.
        let expected = [0u8, 1, 3];
        let frame = [0x00FF_FFFF, 0x0000_0000, 0x00AA_AAAA];
        let err = compare_frame_structural(&frame, &expected).unwrap_err();
        assert!(err.contains("not brighter"), "{err}");
    }

    // --- ROM collection error handling ---

    #[test]
    fn collect_roms_propagates_io_errors() {
        // An unreadable directory must surface as Err, not as a silently
        // empty (and therefore green) group.
        let mut out = Vec::new();
        let missing = Path::new("/nonexistent/slopgb-collect-roms-test");
        assert!(collect_roms(missing, true, &mut out).is_err());
        assert!(out.is_empty());
    }

    #[test]
    fn collect_roms_yields_empty_for_dir_without_roms() {
        // Empty-but-readable is Ok(()) + no ROMs; run_group turns that into
        // an assert failure ("corrupt checkout?") rather than a pass.
        let dir = std::env::temp_dir().join(format!("slopgb-empty-group-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut out = Vec::new();
        collect_roms(&dir, true, &mut out).unwrap();
        assert!(out.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // --- quiet panic capture ---

    #[test]
    fn quiet_catch_unwind_reports_payload_and_resets_suppression() {
        let err = quiet_catch_unwind(|| panic!("boom {}", 42)).unwrap_err();
        assert_eq!(panic_message(err.as_ref()), "boom 42");
        // Non-panicking closures pass their value through, and the
        // suppression flag is cleared so later real panics stay loud.
        assert_eq!(quiet_catch_unwind(|| 7).unwrap(), 7);
        assert!(!SUPPRESS_PANIC_OUTPUT.with(Cell::get));
    }

    // --- SLOPGB_REQUIRE_ROMS skip gate ---
    //
    // The decision takes the env *value* as a parameter: cargo test runs
    // threads in parallel, so set_var/remove_var here would race with other
    // tests reading the process environment.

    #[test]
    fn missing_roms_skip_when_env_unset() {
        let notice = missing_roms_outcome(None, "acceptance", "not present").unwrap();
        assert_eq!(notice, "skipping acceptance: not present");
    }

    #[test]
    fn missing_roms_skip_when_env_not_one() {
        // Only the documented value "1" arms the gate.
        assert!(missing_roms_outcome(Some("0"), "misc", "not present").is_ok());
        assert!(missing_roms_outcome(Some(""), "misc", "not present").is_ok());
    }

    #[test]
    fn missing_roms_fail_when_required() {
        let err = missing_roms_outcome(Some("1"), "acceptance", "not present").unwrap_err();
        // The failure must be actionable: name the gate and the fetch script.
        assert!(err.contains("acceptance: not present"), "{err}");
        assert!(err.contains("SLOPGB_REQUIRE_ROMS=1"), "{err}");
        assert!(err.contains("test-roms/download.sh"), "{err}");
    }

    #[test]
    fn rom_discovery_points_at_mts_release() {
        match mts_root() {
            Some(root) => {
                let name = root.file_name().unwrap().to_str().unwrap();
                assert!(name.starts_with("mts-"), "{name}");
                assert!(root.join("acceptance").is_dir());
            }
            None => println!("note: test-roms/mts-* not present, discovery returned None"),
        }
    }

    #[test]
    fn missing_gbtr_skip_when_env_unset() {
        let notice = missing_gbtr_outcome(None, "blargg", "not present").unwrap();
        assert_eq!(notice, "skipping blargg: not present");
    }

    #[test]
    fn missing_gbtr_skip_when_env_not_one() {
        // Only the documented value "1" arms the gate.
        assert!(missing_gbtr_outcome(Some("0"), "blargg", "not present").is_ok());
        assert!(missing_gbtr_outcome(Some(""), "blargg", "not present").is_ok());
    }

    #[test]
    fn missing_gbtr_fail_when_required() {
        let err = missing_gbtr_outcome(Some("1"), "blargg", "not present").unwrap_err();
        // The failure must be actionable: name the gate and the fetch script.
        assert!(err.contains("blargg: not present"), "{err}");
        assert!(err.contains("SLOPGB_REQUIRE_ROMS=1"), "{err}");
        assert!(err.contains("test-roms/download.sh"), "{err}");
        // And name the right bundle: the collection, not the mooneye ROMs.
        assert!(err.contains("game-boy-test-roms"), "{err}");
    }

    #[test]
    fn rom_discovery_points_at_gbtr_collection() {
        match gbtr_root() {
            Some(root) => {
                let name = root.file_name().unwrap().to_str().unwrap();
                assert_eq!(name, GBTR_DIR);
                // The release zip has no top-level directory; download.sh
                // extracts it into $GBTR_DIR, so the collection's own README
                // sits at the root of the resolved path.
                assert!(root.join("README.md").is_file());
            }
            None => println!("note: test-roms/{GBTR_DIR} not present, discovery returned None"),
        }
    }
}
