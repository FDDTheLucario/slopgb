//! gambatte suite harness (`gambatte/`, 3524 ROMs).
//!
//! Protocol per `gambatte/game-boy-test-roms-howto.md` and the upstream
//! reference checker `test/testrunner.cpp` of pokemon-speedrunning/
//! gambatte-core @ d819bad196 (the exact revision the howto cites; the
//! original sinamas/gambatte repository has been emptied upstream):
//!
//! * **Run length**: every ROM runs ~15 LCD frames from power-on with no
//!   completion signal — 15 × 70224 = 1 053 360 T-cycles (~252 ms). The
//!   harness runs to that exact dot count, then one further frame: the
//!   testrunner's `while (samplesLeft >= 0)` loop performs `biosLength +
//!   15 + 1` one-frame iterations (post-boot start ⇒ 16), and both its
//!   frame buffer and its audio buffer are evaluated after that final
//!   iteration. `GameBoy::cycles()` counts normal-speed dots (2 per
//!   M-cycle in CGB double speed), so the dot target measures LCD frames
//!   correctly for the `_ds` ROMs that switch speed themselves.
//! * **Models** (ARCHITECTURE.md §CGB revision policy): `dmg08` tags and
//!   `_dmg08.png` references run on [`Model::Dmg`]; `cgb04c` and
//!   `_cgb04c.png` on [`Model::Cgb`] (which models exactly that CGB-CPU-04
//!   / CPU CGB C silicon). The testrunner's AGB leg reuses the CGB
//!   expectations under a literal `FIXME: Actual AGB results` comment and
//!   the only AGB-specific references are the 17 `_gba.png` screenshots —
//!   no verified AGB expectation exists, so no [`Model::Agb`] cases are
//!   produced and `_gba.png` files are ignored.
//! * **Expectations**: `_out<HEX>` compares the top 8 pixel rows against
//!   hex glyphs ([`check_hex_screen`]); `_outaudio0`/`_outaudio1` expect
//!   the final frame's *raw* (pre-resample, pre-high-pass) audio samples
//!   to be all-identical / not ([`check_audio`], the testrunner's
//!   raw-stream sample-equality semantics); otherwise a sibling reference
//!   PNG is compared via
//!   [`harness::expect_frame_png`]. An `x` prefix on a tag or PNG suffix
//!   (`xout…`, `_xdmg08.png`, `_xcgb.png`) marks the expectation
//!   unverified-on-hardware: the testrunner's substring searches can never
//!   match it, so that side is never run (the other side still is).
//! * **Bare PNGs** (`<stem>.png`, 57 files): used as the reference for the
//!   model implied by the ROM extension (`.gb` → Dmg, `.gbc` → Cgb) when
//!   that side has no tagged PNG. 14 ROMs have both `<stem>.png` and
//!   `<stem>_dmg08.png`; the tagged reference wins there and the bare file
//!   goes unused, exactly like in the upstream testrunner (which never
//!   opens bare PNGs at all).
//! * **`_dmg08_cgb_blank`** (2 halt ROMs): no `_out` tag and no PNG — the
//!   name documents that the ROM prints no result on either model (it
//!   halts forever). Run on both models expecting the top tile row — the
//!   print area — to be all white ([`check_blank`]); the rest of the
//!   screen shows boot-ROM VRAM leftovers on hardware.
//!
//! Result keys are `gambatte/<rel> [Model]`; the known-failure baseline
//! lives in `baselines/gambatte.txt` (one key per line, `#` comments).

use std::path::{Path, PathBuf};

use slopgb_core::{CYCLES_PER_FRAME, GameBoy, Model, SCREEN_W};

use crate::common::{self, framecmp, framecmp::CgbColorMap};
use crate::harness::{self, CaseResult};

/// The howto's exit condition: 15 LCD frames = 1 053 360 T-cycles.
const RUN_DOTS: u64 = 15 * CYCLES_PER_FRAME as u64;

/// Known-failure baseline (see `harness::assert_against_baseline`).
const BASELINE_TXT: &str = include_str!("baselines/gambatte.txt");

// ---------------------------------------------------------------------------
// filename-tag parsing (mirrors testrunner.cpp `main` + `evaluateStrTestResults`)
// ---------------------------------------------------------------------------

/// One side's machine-checkable expectation from the filename tags.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StrExpect {
    /// `_out<HEX>`: the screen's top tile row shows these hex digits.
    Hex(String),
    /// `_outaudio<0|1>`: expect silence (`false`) or sound (`true`).
    Audio(bool),
}

/// Decode the text following an `_out` marker, mirroring
/// `evaluateStrTestResults`: a literal `audio0`/`audio1` prefix is an audio
/// expectation; otherwise the maximal leading run of hex digits is the
/// expected screen value (the testrunner's `tileFromChar` stops at the
/// first non-hex character). An empty run means no expectation.
fn decode_out(rest: &str) -> Option<StrExpect> {
    if rest.starts_with("audio0") {
        return Some(StrExpect::Audio(false));
    }
    if rest.starts_with("audio1") {
        return Some(StrExpect::Audio(true));
    }
    let hex: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
    (!hex.is_empty()).then_some(StrExpect::Hex(hex))
}

/// Mirror of the testrunner `main` loop's outstr selection: returns the
/// `(dmg, cgb)` string expectations for a filename stem.
///
/// * `dmg08_cgb04c_out…` → the same expectation on both models;
/// * else `dmg08_out…` → DMG side, plus a CGB side iff `cgb04c_out…` is
///   also present;
/// * else a bare `_out…` → CGB side only (this is how `_cgb04c_out…`-only
///   names are matched; an `x` prefix breaks all of these searches, which
///   is exactly how unverified sides are skipped upstream).
fn str_expectations(stem: &str) -> (Option<StrExpect>, Option<StrExpect>) {
    if let Some(i) = stem.find("dmg08_cgb04c_out") {
        let e = decode_out(&stem[i + "dmg08_cgb04c_out".len()..]);
        return (e.clone(), e);
    }
    if let Some(i) = stem.find("dmg08_out") {
        let dmg = decode_out(&stem[i + "dmg08_out".len()..]);
        let cgb = stem
            .find("cgb04c_out")
            .and_then(|j| decode_out(&stem[j + "cgb04c_out".len()..]));
        return (dmg, cgb);
    }
    if let Some(i) = stem.find("_out") {
        return (None, decode_out(&stem[i + "_out".len()..]));
    }
    (None, None)
}

// ---------------------------------------------------------------------------
// per-ROM case planning (string tags + reference PNGs + blank tag)
// ---------------------------------------------------------------------------

/// The verdict procedure for one (ROM × model) case.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Check {
    Hex(String),
    Audio(bool),
    /// Reference PNG `<stem><suffix>.png` next to the ROM.
    Png(&'static str),
    Blank,
}

/// Reference-PNG suffixes for the (dmg, cgb) sides. `png_exists` is given
/// the suffix (`""` for the bare `<stem>.png`).
///
/// The testrunner first tries `<stem>_dmg08_cgb04c.png` for *both* models,
/// then `_cgb04c.png` / `_dmg08.png` per side; v7.0 ships no
/// `_dmg08_cgb04c.png` (the inventory pin would catch one appearing). The
/// bare `<stem>.png` fallback keyed on the ROM extension is this
/// collection's convention for the 57 untagged screenshots (the upstream
/// runner ignores them; the howto says they "match the test rom's file
/// name").
fn png_refs(
    ext: &str,
    png_exists: impl Fn(&str) -> bool,
) -> (Option<&'static str>, Option<&'static str>) {
    if png_exists("_dmg08_cgb04c") {
        return (Some("_dmg08_cgb04c"), Some("_dmg08_cgb04c"));
    }
    let dmg = if png_exists("_dmg08") {
        Some("_dmg08")
    } else {
        (ext == "gb" && png_exists("")).then_some("")
    };
    let cgb = if png_exists("_cgb04c") {
        Some("_cgb04c")
    } else {
        (ext == "gbc" && png_exists("")).then_some("")
    };
    (dmg, cgb)
}

/// Checks for the (Dmg, Cgb) sides of one ROM; `None` = that side never
/// runs. No ROM in v7.0 carries both a string tag and a PNG for the same
/// side (verified during planning; the testrunner would run both) — the
/// string tag takes precedence here.
fn plan_rom(
    stem: &str,
    ext: &str,
    png_exists: impl Fn(&str) -> bool,
) -> (Option<Check>, Option<Check>) {
    let (str_dmg, str_cgb) = str_expectations(stem);
    let (png_dmg, png_cgb) = png_refs(ext, png_exists);
    // The two halt ROMs named `…_dmg08_cgb_blank` document an all-white
    // screen on both models and ship neither an _out tag nor a PNG.
    let blank = stem.ends_with("dmg08_cgb_blank");
    let side = |s: Option<StrExpect>, p: Option<&'static str>| {
        s.map(|e| match e {
            StrExpect::Hex(h) => Check::Hex(h),
            StrExpect::Audio(a) => Check::Audio(a),
        })
        .or(p.map(Check::Png))
        .or(blank.then_some(Check::Blank))
    };
    (side(str_dmg, png_dmg), side(str_cgb, png_cgb))
}

/// `plan_rom` driven by the filesystem next to `rom_path`.
fn plan_rom_on_disk(rom_path: &Path) -> (Option<Check>, Option<Check>) {
    let stem = rom_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = rom_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let dir = rom_path.parent().unwrap_or(Path::new(""));
    plan_rom(stem, ext, |suffix| {
        dir.join(format!("{stem}{suffix}.png")).is_file()
    })
}

// ---------------------------------------------------------------------------
// hex-screen comparator (mirrors testrunner.cpp `tileFromChar` /
// `tilesAreEqual` / `frameBufferMatchesOut`)
// ---------------------------------------------------------------------------

/// The 0-F hex glyph bitmaps the result value is rendered with, one byte
/// per row, bit 7 = leftmost pixel, 1 = ink (black), 0 = background
/// (white).
///
/// Provenance: transcribed from the `tiles` array in `tileFromChar`,
/// test/testrunner.cpp lines 65-220 of pokemon-speedrunning/gambatte-core
/// @ d819bad196 (`_` = 0xF8F8F8 background, `O` = 0x000000 ink).
const GLYPHS: [[u8; 8]; 16] = [
    [0x00, 0x7F, 0x41, 0x41, 0x41, 0x41, 0x41, 0x7F], // 0
    [0x00, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08], // 1
    [0x00, 0x7F, 0x01, 0x01, 0x7F, 0x40, 0x40, 0x7F], // 2
    [0x00, 0x7F, 0x01, 0x01, 0x3F, 0x01, 0x01, 0x7F], // 3
    [0x00, 0x41, 0x41, 0x41, 0x7F, 0x01, 0x01, 0x01], // 4
    [0x00, 0x7F, 0x40, 0x40, 0x7E, 0x01, 0x01, 0x7E], // 5
    [0x00, 0x7F, 0x40, 0x40, 0x7F, 0x41, 0x41, 0x7F], // 6
    [0x00, 0x7F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10], // 7
    [0x00, 0x3E, 0x41, 0x41, 0x3E, 0x41, 0x41, 0x3E], // 8
    [0x00, 0x7F, 0x41, 0x41, 0x7F, 0x01, 0x01, 0x7F], // 9
    [0x00, 0x08, 0x22, 0x41, 0x7F, 0x41, 0x41, 0x41], // A
    [0x00, 0x7E, 0x41, 0x41, 0x7E, 0x41, 0x41, 0x7E], // B
    [0x00, 0x3E, 0x41, 0x40, 0x40, 0x40, 0x41, 0x3E], // C
    [0x00, 0x7E, 0x41, 0x41, 0x41, 0x41, 0x41, 0x7E], // D
    [0x00, 0x7F, 0x40, 0x40, 0x7F, 0x40, 0x40, 0x7F], // E
    [0x00, 0x7F, 0x40, 0x40, 0x7F, 0x40, 0x40, 0x40], // F
];

/// Re-encode one emulator pixel the way the testrunner sees its own frame
/// buffer, then apply the 0xF8F8F8 mask `tilesAreEqual` compares under.
///
/// CGB frames go through gambatte's CGB-to-RGB conversion first
/// ([`framecmp::gambatte_rgb`], the lut built in `runTestRom`). DMG frames
/// already use the FF/AA/55/00 greys the testrunner's `setDmgPaletteColor`
/// calls configure; the mask drops their low bits (and any X byte), so no
/// separate 24-bit truncation is needed.
fn masked_pixel(px: u32, cgb: bool) -> u32 {
    let v = if cgb { framecmp::gambatte_rgb(px) } else { px };
    v & 0x00F8_F8F8
}

const WHITE: u32 = 0x00F8_F8F8;

/// The masked 8×8 tile at glyph slot `i` (columns `i*8..i*8+8` of the top
/// 8 pixel rows — `frameBufferMatchesOut` walks `framebuf + i * 8`).
fn screen_tile(frame: &[u32], i: usize, cgb: bool) -> [u32; 64] {
    let mut tile = [0u32; 64];
    for y in 0..8 {
        for x in 0..8 {
            tile[y * 8 + x] = masked_pixel(frame[y * SCREEN_W + i * 8 + x], cgb);
        }
    }
    tile
}

fn glyph_pixel(glyph: &[u8; 8], y: usize, x: usize) -> u32 {
    if glyph[y] & (0x80 >> x) != 0 {
        0
    } else {
        WHITE
    }
}

/// `frameBufferMatchesOut`: every digit of `hex` must appear as its glyph
/// in the corresponding top-row tile slot. Failure reports what the screen
/// actually shows (via [`read_hex_screen`]) for triage.
fn check_hex_screen(frame: &[u32], hex: &str, cgb: bool) -> Result<(), String> {
    debug_assert!(hex.len() <= SCREEN_W / 8, "hex value wider than the screen");
    for (i, c) in hex.chars().enumerate() {
        let glyph = &GLYPHS[usize::from(hex_val(c))];
        let tile = screen_tile(frame, i, cgb);
        for y in 0..8 {
            for x in 0..8 {
                if tile[y * 8 + x] != glyph_pixel(glyph, y, x) {
                    return Err(format!(
                        "hex screen mismatch: want \"{hex}\", screen shows \"{}\" \
                         (first diff in digit {i} '{c}' at pixel ({},{y}))",
                        read_hex_screen(frame, cgb),
                        i * 8 + x,
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Numeric value of a hex digit character (caller guarantees validity).
fn hex_val(c: char) -> u8 {
    c.to_digit(16).expect("hex digit") as u8
}

/// Diagnostic only: OCR the top tile row back into hex digits. Unmatched
/// tiles become `?`; trailing all-background tiles are trimmed.
fn read_hex_screen(frame: &[u32], cgb: bool) -> String {
    let mut out = String::new();
    for i in 0..SCREEN_W / 8 {
        let tile = screen_tile(frame, i, cgb);
        let glyph_of = |g: &[u8; 8]| (0..64).all(|p| tile[p] == glyph_pixel(g, p / 8, p % 8));
        match GLYPHS.iter().position(glyph_of) {
            Some(v) => out.push(char::from_digit(v as u32, 16).unwrap().to_ascii_uppercase()),
            None if tile.iter().all(|&p| p == WHITE) => out.push(' '),
            None => out.push('?'),
        }
    }
    out.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// audio + blank comparators
// ---------------------------------------------------------------------------

/// Audio verdict over the final frame's *raw* samples
/// ([`GameBoy::drain_audio_raw`]: the per-dot mixer output, before the
/// box-average resampler and high-pass filter), mirroring the testrunner's
/// `std::count(audiobuf, audiobuf + samples_per_frame, audiobuf[0]) ==
/// samples_per_frame` silence test on gambatte's own raw stream: silence ⇔
/// every sample is bit-identical to the first (an empty capture would be a
/// harness bug, not silence). The filtered [`GameBoy::drain_audio`] stream
/// must not be judged instead: its decaying high-pass tail reads a silent
/// DC level as "sound" (false `_outaudio0` fail) and its filter can
/// flatten genuinely varying output (false `_outaudio1` pass).
fn check_audio(samples: &[(f32, f32)], expect_sound: bool) -> Result<(), String> {
    let Some(&first) = samples.first() else {
        return Err("no audio samples captured in the final frame".into());
    };
    let bits = |(l, r): (f32, f32)| (l.to_bits(), r.to_bits());
    let has_sound = samples.iter().any(|&s| bits(s) != bits(first));
    match (expect_sound, has_sound) {
        (true, false) => Err(format!(
            "expected sound, got constant output over {} samples",
            samples.len()
        )),
        (false, true) => Err(format!(
            "expected silence, got varying output over {} samples",
            samples.len()
        )),
        _ => Ok(()),
    }
}

/// Blank verdict for the `_dmg08_cgb_blank` ROMs (two `halt/` tests that
/// sleep forever): "blank" names the *result*, not the panel — the ROM
/// never reaches its print routine, so the top tile row where every
/// gambatte test renders its hex digits stays empty. The rest of the
/// screen is not white on hardware: these ROMs never touch VRAM, so the
/// LCD keeps showing the boot ROM's leftover logo (see
/// `Interconnect::install_boot_logo_vram`). Under the testrunner's
/// 0xF8F8F8 mask, masked-white identifies exactly DMG shade 0 (0xFFFFFF)
/// and CGB color (31,31,31) on both color maps, so one mask check covers
/// both models.
fn check_blank(frame: &[u32]) -> Result<(), String> {
    let print_area = &frame[..8 * SCREEN_W];
    let non_white = print_area.iter().filter(|&&px| px & WHITE != WHITE).count();
    if non_white == 0 {
        Ok(())
    } else {
        Err(format!(
            "{non_white} non-white pixel(s) in the top tile row, want no printed result"
        ))
    }
}

// ---------------------------------------------------------------------------
// case runner
// ---------------------------------------------------------------------------

/// Advance to an absolute dot (normal-speed T-cycle) position;
/// instruction-granular, so the target is overshot by at most one
/// instruction — irrelevant for these static end-state screens.
fn run_to_dot(gb: &mut GameBoy, dot: u64) {
    while gb.cycles() < dot {
        gb.step();
    }
}

fn run_case(rom: &[u8], model: Model, check: &Check, rom_path: &Path) -> Result<(), String> {
    let mut gb = harness::boot(rom, model);
    // 15 frames per the howto, then the testrunner's final loop iteration
    // (one more frame) whose frame/audio buffers are what gets evaluated.
    run_to_dot(&mut gb, RUN_DOTS);
    match check {
        Check::Audio(expect_sound) => {
            let mut samples = Vec::new();
            gb.drain_audio_raw(&mut samples); // discard frames 1..=15
            samples.clear();
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            gb.drain_audio_raw(&mut samples);
            check_audio(&samples, *expect_sound)
        }
        check => {
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            match check {
                Check::Hex(hex) => check_hex_screen(gb.frame(), hex, model.is_cgb()),
                Check::Png(suffix) => {
                    let stem = rom_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    let png = rom_path
                        .parent()
                        .unwrap_or(Path::new(""))
                        .join(format!("{stem}{suffix}.png"));
                    // DMG references are common-palette greys (identity);
                    // CGB references are rendered through gambatte's
                    // CGB-to-RGB conversion (howto §Screenshot Colors).
                    let map = if model.is_cgb() {
                        CgbColorMap::Gambatte
                    } else {
                        CgbColorMap::Identity
                    };
                    harness::expect_frame_png(&gb, &png, map)
                }
                Check::Blank => check_blank(gb.frame()),
                Check::Audio(_) => unreachable!("handled above"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// suite walk + inventory
// ---------------------------------------------------------------------------

/// Every ROM under `gambatte/`, sorted (collect_roms order).
fn suite_roms(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("gambatte");
    let mut roms = Vec::new();
    common::collect_roms(&dir, true, &mut roms)
        .unwrap_or_else(|e| panic!("cannot enumerate ROMs under {}: {e}", dir.display()));
    assert!(
        !roms.is_empty(),
        "gambatte/ exists but contains no ROMs — corrupt checkout?"
    );
    roms
}

/// The planned (model, check) cases of one ROM.
fn rom_cases(rom_path: &Path) -> Vec<(Model, Check)> {
    let (dmg, cgb) = plan_rom_on_disk(rom_path);
    let mut cases = Vec::new();
    if let Some(check) = dmg {
        cases.push((Model::Dmg, check));
    }
    if let Some(check) = cgb {
        cases.push((Model::Cgb, check));
    }
    cases
}

pub fn inventory() -> (Vec<String>, Vec<String>) {
    let Some(root) = common::gbtr_root() else {
        return (Vec::new(), Vec::new());
    };
    let mut claimed = Vec::new();
    let mut exempted = Vec::new();
    for rom_path in suite_roms(&root) {
        let rel = harness::rel_unix(&root, &rom_path);
        if rom_cases(&rom_path).is_empty() {
            exempted.push(rel);
        } else {
            claimed.push(rel);
        }
    }
    (claimed, exempted)
}

/// The 50 ROMs that produce no machine-checkable case, by category. The
/// howto defines exactly three verdict mechanisms (audio tag, hex tag,
/// reference PNG); these ROMs have none, or only `x`-marked (unverified)
/// ones the upstream testrunner never runs either.
const EXEMPT: &[&str] = &[
    // Fully x-marked PNG references (unverified `_xcgb.png` only).
    "gambatte/bgtiledata/bgtiledata_spx08_ds_1.gbc",
    "gambatte/bgtiledata/bgtiledata_spx08_ds_2.gbc",
    // Root-level dumper / manual-inspection ROMs: they render or store
    // hardware state for a human (or write SRAM) and ship no expectation.
    "gambatte/cgb_bgp_dumper.gbc",
    "gambatte/cgb_objp_dumper.gbc",
    "gambatte/fexx_ffxx_dumper.gbc",
    "gambatte/fexx_read_reset_set_dumper.gbc",
    "gambatte/ioregs_reset_dumper.gbc",
    "gambatte/jpadirq_1.gbc",
    "gambatte/jpadirq_2.gbc",
    // Fully x-marked string tag (`_xout0`, no other expectation).
    "gambatte/m0enable/lycdisable_ff45_ds_2_xout0.gbc",
    // OAM/VRAM dumper ROMs: dump DMA results to the screen for manual
    // comparison; no expectation files exist.
    "gambatte/oamdma/oamdma_src80_oambusy_dumper_1.gbc",
    "gambatte/oamdma/oamdma_srcC0_oambusy_dumper_1.gbc",
    "gambatte/oamdma/oamdmasrc8000_gdmasrcC000_2xgdmalen09_oamdumper_1.gbc",
    "gambatte/oamdma/oamdmasrc8000_gdmasrcC000_2xgdmalen09_vramdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrc0000_gdmalen04_oamdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrc0000_gdmalen04_oamdumper_ds_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrc0000_gdmalen13_oamdumper_ds_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrc0000_gdmalen13_vramdumper_ds_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_2xgdmalen09_oamdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_2xgdmalen09_vramdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen09_oamdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen09_vramdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen13_oamdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen13_oamdumper_2.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen13_oamdumper_ds_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen13_oamdumper_ds_2.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen13_oamdumper_ds_3.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen13_vramdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC000_gdmalen13_vramdumper_ds_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC0F0_gdmalen13_oamdumper_1.gbc",
    "gambatte/oamdma/oamdmasrcC000_gdmasrcC0F0_gdmalen13_vramdumper_1.gbc",
    // Fully x-marked audio tag on both sides (`_dmg08_cgb_xoutaudio1`).
    "gambatte/sound/ch1_duty0_to_duty3_pos3_1_dmg08_cgb_xoutaudio1.gbc",
    // Manual sprite-inspection ROMs: no tag, no reference image.
    "gambatte/sprites/11spritesPrLine_10xposA8.gbc",
    "gambatte/sprites/late_disable_group_image_1.gb",
    "gambatte/sprites/late_disable_group_image_2.gb",
    "gambatte/sprites/late_disable_group_image_3.gb",
    "gambatte/sprites/late_disable_group_image_4.gb",
    "gambatte/sprites/late_disable_group_image_5.gb",
    "gambatte/sprites/late_disable_group_image_6.gb",
    "gambatte/sprites/late_disable_group_image_7.gb",
    "gambatte/sprites/late_disable_group_image_8.gb",
    "gambatte/sprites/late_disable_group_image_9.gb",
    "gambatte/sprites/late_disable_scx5_1.gb",
    "gambatte/sprites/late_disable_scx5_2.gb",
    "gambatte/sprites/late_disable_sp00x18_1.gb",
    "gambatte/sprites/late_disable_sp00x18_2.gb",
    // Fully x-marked string tag (`_dmg08_xout0`, no CGB expectation).
    "gambatte/sprites/sprite_late_enable_spx19_2_dmg08_xout0.gb",
    // More root-level dumpers.
    "gambatte/sram.gbc",
    "gambatte/vram_dumper.gbc",
    "gambatte/wram_dumper.gbc",
];

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[test]
fn gambatte_matrix() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("gambatte", "game-boy-test-roms collection not present");
        return;
    };
    let mut results: Vec<CaseResult> = Vec::new();
    for rom_path in suite_roms(&root) {
        let cases = rom_cases(&rom_path);
        if cases.is_empty() {
            continue; // exempt; pinned by gambatte_inventory_is_exact
        }
        let rel = harness::rel_unix(&root, &rom_path);
        let rom =
            std::fs::read(&rom_path).unwrap_or_else(|e| panic!("read {}: {e}", rom_path.display()));
        for (model, check) in cases {
            let result = harness::catch_case(|| run_case(&rom, model, &check, &rom_path));
            results.push(CaseResult {
                key: harness::case_key(&rel, model),
                result,
            });
        }
    }
    // Routing pin: 5272 cases = 4674 hex + 374 png + 220 audio + 4 blank
    // over 3474 claimed ROMs (see module docs for the per-rule census).
    assert_eq!(results.len(), 5272, "case-matrix drift");
    let passed = results.iter().filter(|c| c.result.is_ok()).count();
    println!("gambatte: {passed}/{} cases pass", results.len());
    harness::assert_against_baseline("gambatte", &results, &harness::parse_baseline(BASELINE_TXT));
}

/// Port Stage S0 — the executable red spec for the kernel pair, the
/// convergence target of S2 (`docs/sameboy-port/PORT-PLAN.md`,
/// `ppu-timing-map.md` §6). Both ROMs reduce to the *same* `ldh a,(FF41)`;
/// SameBoy's cycle-exact frame separates them with no CPU-call-stack
/// discriminator (leading-edge cc+0 sampling + a decoupled `mode_for_interrupt`
/// + the mode-2(−1)/mode-0(+1) anchor swing):
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
/// **Un-ignored at port Stage A6 (2026-06-21):** the spec now runs on the
/// **flag-on** SameBoy cycle-exact path ([`GameBoy::set_leading_edge_reads`] —
/// leading-edge cc+0 reads + the `StatUpdate` engine + the `vis_early`
/// back-date + the A6 halt-late masks). With those four pieces the kernel pair
/// SEPARATES (`m2int`→3 ∧ `m0int`→0) on both models while the canonical mooneye
/// `intr_2_mode0_timing` also holds flag-on (`ppu-subdot-ladder.md` "A6"). This
/// is GREEN as a flag-on acceptance test; production (flag-off) is unchanged —
/// the global default flip + ~7000-row rebaseline is the remaining Phase-B work.
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
            // Run the SameBoy cycle-exact flag-on path (the convergence the
            // whole-dot production model cannot represent); same 16-frame
            // protocol + OCR as `run_case`'s `Check::Hex` arm.
            let mut gb = harness::boot(&rom, model);
            gb.set_leading_edge_reads(true);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (flag-on): {e}")
            });
        }
    }
}

/// Port Stage A9 — the SPRITE-line analog of the kernel pair, on the flag-on
/// path. A sprite-laden line extends mode 3, shifting the visible mode→0
/// boundary; the `vis_early` back-date for sprite/window lines (`lead + 4`, vs
/// bare's `lead + 3`) lands it at SameBoy's frame, so the two equal-`ldh`
/// reads straddle it: `10spritesPrLine_m3stat_1` reads mode 3 (out3) and
/// `_m3stat_2` reads mode 0 (out0) — the same out3/out0 split the kernel pair
/// shows on a bare line. Whole-dot production reads BOTH as mode 3 (the
/// baselined floor); A9 is measured to lift 40 such sprite `m3stat_2` rows
/// flag-on with zero regression (`ppu-subdot-ladder.md` "A9"). Flag-OFF
/// (production) is unchanged.
#[test]
fn sprite_kernel_pair_matches_sameboy_target() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("sprite_kernel_pair", "game-boy-test-roms collection not present");
        return;
    };
    let targets = [
        (
            "gambatte/sprites/10spritesPrLine_m3stat_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/sprites/10spritesPrLine_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
        ),
    ];
    for (rel, expect) in targets {
        let path = root.join(rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for model in [Model::Dmg, Model::Cgb] {
            let mut gb = harness::boot(&rom, model);
            gb.set_leading_edge_reads(true);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (flag-on): {e}")
            });
        }
    }
}

/// Port Stage B (Tier 2) — the kernel pair on the FULL deferred-commit reclock
/// (`set_tier2_reclock`: B1 deferred machine advance + B2 dispatch retime + B3
/// `early_lead`−2), NOT just the Tier-1 leading-edge hybrid the spec above
/// runs. This is the make-or-break thesis result: the deferred reclock makes
/// the two equal `ldh a,(FF41)` reads SEPARATE — `m2int_m3stat_1` reads mode 3
/// (out3) and `m0int_m3stat_2` reads mode 0 (out0) — *while* mooneye
/// `intr_2_mode0_timing` simultaneously passes flag-on
/// (measured, `ppu-subdot-ladder.md` "PHASE B"). That dissolves the A8
/// mutual-exclusion the prior verdict claimed ("m0int=0 forces intr_2 FAIL in
/// the cc+4 frame"): m0int=0 and intr_2 now co-hold. Production (both flags
/// off) is byte-identical; the global default flip + ~7000-row rebaseline + the
/// two residuals (sprite-line dispatch lead + the deferred halt-wake cc+2 mask)
/// are the remaining Phase-B work.
#[test]
fn tier2_kernel_pair_matches_sameboy_target() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("tier2_kernel_pair", "game-boy-test-roms collection not present");
        return;
    };
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
            let mut gb = harness::boot(&rom, model);
            gb.set_tier2_reclock(true);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// Port Stage B5 (L2) — the LONE Phase-B residual cleared: mooneye
/// `intr_2_mode0_timing_sprites` passes on the FULL deferred reclock
/// (`set_tier2_reclock`) for BOTH models. The test resolves each sprite
/// config's mode-3 length to whole M-cycles; our `proj` tracks a finer per-X
/// staircase that the cc+4 read quantizes back into the right buckets
/// (production passes every config), but the cc+0 leading-edge read exposes
/// the sub-M-cycle dispatch phase. The fix (`ppu/render/mode0.rs`) snaps the
/// sprite-line dispatch + `vis_early` to the CPU read grid (dot ≡ 0 mod 4)
/// with `early_lead = 0`, reproducing the cc+4 quantization — verified to pass
/// all 105 configs both models while the bare kernel pair, `intr_2_mode0_timing`
/// and `int_hblank_halt` keep passing (4/4 thesis triad). Production (flag-off)
/// is byte-identical: the snap is gated on `tier2_reclock` and `vis_early` is
/// never set without `leading_edge_reads`. SameBoy passes this ROM
/// (header `pass: DMG..AGS`).
#[test]
fn tier2_intr_2_sprites_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("tier2_intr_2_sprites", "game-boy-test-roms collection not present");
        return;
    };
    let rel = "mooneye-test-suite/acceptance/ppu/intr_2_mode0_timing_sprites.gb";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Cgb] {
        let mut gb = harness::boot(&rom, model);
        gb.set_tier2_reclock(true);
        harness::run_until_breakpoint(&mut gb, 30_000_000)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
        harness::check_fib(&gb)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
    }
}

/// Port Stage B C0 — the deferred-frame DIV/serial re-calibration. The
/// post-boot `div_counter` constants (`model.rs`) are calibrated for the eager
/// tick-then-access frame (register reads sample the timer at cc+4); the
/// deferred-commit reclock samples every read at the M-cycle leading edge
/// (cc+0), one M-cycle earlier, so the boot hand-off DIV phase advances 4 T
/// under `tier2_reclock` (`interconnect/boot.rs`) to land on SameBoy's
/// leading-edge frame. Without it `boot_div-*` reads positioned just after a
/// DIV-byte increment lose 1, and the DIV-derived serial clock fails
/// `boot_sclk_align`. SameBoy passes all of these reading at cc+0 (its
/// `div_counter` rides the same +1-M-cycle timeline). Production (flag-off) is
/// byte-identical — the +4 is gated on `tier2_reclock`, and leading-edge-only
/// leaves DIV at cc+4.
#[test]
fn tier2_boot_div_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("tier2_boot_div", "game-boy-test-roms collection not present");
        return;
    };
    // (rel path under the mooneye-test-suite, model legs the suffix runs on).
    let legs: &[(&str, &[Model])] = &[
        ("acceptance/boot_div-dmgABCmgb.gb", &[Model::Dmg, Model::Mgb]),
        ("acceptance/boot_div-dmg0.gb", &[Model::Dmg0]),
        ("acceptance/boot_div-S.gb", &[Model::Sgb, Model::Sgb2]),
        ("acceptance/boot_div2-S.gb", &[Model::Sgb, Model::Sgb2]),
        ("misc/boot_div-A.gb", &[Model::Agb]),
        ("misc/boot_div-cgbABCDE.gb", &[Model::Cgb]),
        (
            "acceptance/serial/boot_sclk_align-dmgABCmgb.gb",
            &[Model::Dmg, Model::Mgb],
        ),
    ];
    for (rel, models) in legs {
        let path = root.join("mooneye-test-suite").join(rel);
        let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in *models {
            // boot_div's DIV phase is decided at hand-off, so the reclock must
            // be on *before* the post-boot state lands — the runtime toggle is
            // too late (see `harness::boot_with_reclock`).
            let mut gb = harness::boot_with_reclock(&rom, model);
            harness::run_until_breakpoint(&mut gb, 30_000_000)
                .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
            harness::check_fib(&gb)
                .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
        }
    }
}

/// Port Stage B C1 — mooneye `intr_2_mode3_timing` on the deferred reclock.
/// The test counts STAT-read polls from the mode-2 IRQ to the mode-3 read;
/// the CPU-visible 2→3 entry boundary (`mode3_entry_dot`) was back-dated 4
/// dots (80) for the leading-edge-only frame, but the Tier-2 deferred read
/// samples the entry at the trailing frame, so 80 made it see mode 3 one
/// M-cycle early (`test_iter 2` → 1 poll, want 2). Restoring the flag-off 84
/// under `tier2_reclock` passes it both models. Production (flag-off) is
/// byte-identical — the tier2 branch only fires on the reclock path.
#[test]
fn tier2_intr_2_mode3_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr("tier2_intr_2_mode3", "game-boy-test-roms collection not present");
        return;
    };
    let rel = "mooneye-test-suite/acceptance/ppu/intr_2_mode3_timing.gb";
    let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    for model in [Model::Dmg, Model::Cgb] {
        let mut gb = harness::boot(&rom, model);
        gb.set_tier2_reclock(true);
        harness::run_until_breakpoint(&mut gb, 30_000_000)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
        harness::check_fib(&gb)
            .unwrap_or_else(|e| panic!("{rel} [{model:?}] (tier2 flag-on): {e}"));
    }
}

/// Self-verifying inventory: claimed ∩ exempted = ∅ and claimed ∪ exempted
/// covers the on-disk ROM set exactly, with the exemptions pinned to the
/// documented 50-entry list.
#[test]
fn gambatte_inventory_is_exact() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "gambatte_inventory_is_exact",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let (claimed, exempted) = inventory();
    let mut on_disk: Vec<String> = suite_roms(&root)
        .iter()
        .map(|p| harness::rel_unix(&root, p))
        .collect();
    on_disk.sort();
    let mut union: Vec<String> = claimed.iter().chain(&exempted).cloned().collect();
    union.sort();
    assert_eq!(union, on_disk, "claimed ∪ exempted ≠ on-disk ROM set");
    assert!(
        claimed.iter().all(|c| !exempted.contains(c)),
        "claimed ∩ exempted ≠ ∅"
    );
    let mut exempted_sorted = exempted.clone();
    exempted_sorted.sort();
    let mut expected: Vec<&str> = EXEMPT.to_vec();
    expected.sort_unstable();
    assert_eq!(
        exempted_sorted, expected,
        "exempt set drifted from the documented list"
    );
    assert_eq!(claimed.len(), 3474, "claimed-ROM count drift");
    assert_eq!(on_disk.len(), 3524, "gambatte suite ROM count drift");
}

// --- pure-function unit tests --------------------------------------------

/// Table-driven check of the tag parser against real v7.0 stems covering
/// every pattern: both-model hex, split hex, single-side hex, the bare
/// `_out` CGB-only branch, audio on each side, x-marked sides, the `cgb`
/// (not `cgb04c`) decoy tag, `_ds` infixes, long/lowercase hex values and
/// no-tag stems.
#[test]
fn gambatte_str_expectation_parser() {
    use StrExpect::{Audio, Hex};
    let hex = |s: &str| Some(Hex(s.into()));
    #[rustfmt::skip]
    let table: &[(&str, Option<StrExpect>, Option<StrExpect>)] = &[
        // dmg08_cgb04c_out<V>: same value both models
        ("lcdcenable_lyc0irq_1_dmg08_cgb04c_out2", hex("2"), hex("2")),
        ("scx_m3_extend_1_dmg08_cgb04c_out3", hex("3"), hex("3")),
        ("ifandie_ei_halt_sra_dmg08_cgb04c_out0A", hex("0A"), hex("0A")),
        ("lyc153int_m2irq_late_retrigger_2_dmg08_cgb04c_out0", hex("0"), hex("0")),
        ("oamdma_src0000_busyrst0002_dmg08_cgb04c_outFF8DFA9E", hex("FF8DFA9E"), hex("FF8DFA9E")),
        // dmg08_out<V1>_cgb04c_out<V2>: split values
        ("late_m0int_halt_m0stat_scx3_1b_dmg08_out0_cgb04c_out2", hex("0"), hex("2")),
        ("preread_2_dmg08_out3_cgb04c_out0", hex("3"), hex("0")),
        ("tc00_irq_late_retrigger_2_dmg08_outE4_cgb04c_outE0", hex("E4"), hex("E0")),
        ("oamdma_src8000_busypush8001_dmg08_out55761234_cgb04c_out00761234", hex("55761234"), hex("00761234")),
        // dmg08_out<V>: DMG only
        ("start_inc_1_dmg08_outAB", hex("AB"), None),
        ("m2disable_dmg08_cgb_dmg08_out0", hex("0"), None),
        ("oamdma_srcF000_busyreadC000_dmg08_out6_cgb_xoutblank", hex("6"), None),
        ("m2int_wxA6_oambusyread_3_dmg08_out5_cgb_xout1", hex("5"), None),
        // cgb04c_out<V> alone: matched through the bare `_out` branch,
        // CGB only (exactly like the testrunner)
        ("hdma_late_enable_lcdoffset3_1_cgb04c_out1", None, hex("1")),
        ("cgbpal_m3start_ds_1_cgb04c_out1", None, hex("1")),
        ("hdma_vs_m0_scx2_cgb04c_out0183", None, hex("0183")),
        // x-marked side is dead, the other still parses
        ("postread_scx3_2_dmg08_xout1_cgb04c_out0", None, hex("0")),
        ("sprite_late_enable_spx19_2_dmg08_xout0", None, None),
        ("lycdisable_ff45_ds_2_xout0", None, None),
        // audio tags
        ("ch1_duty0_pattern_pos0_dmg08_cgb04c_outaudio0", Some(Audio(false)), Some(Audio(false))),
        ("ch1_duty0_pattern_pos7_dmg08_cgb04c_outaudio1", Some(Audio(true)), Some(Audio(true))),
        ("ch1_init_reset_sweep_counter_timing_11_dmg08_outaudio0_cgb_xoutaudio1lowpitch", Some(Audio(false)), None),
        ("ch1_init_reset_sweep_counter_timing_8_dmg08_outaudio1_cgb_xoutaudio1lowpitch", Some(Audio(true)), None),
        ("ch1_init_reset_sweep_counter_timing_2_dmg08_xoutaudio1lowpitch_cgb04c_outaudio1", None, Some(Audio(true))),
        ("ch1_duty0_to_duty3_pos3_1_dmg08_cgb_xoutaudio1", None, None),
        ("ch1_init_pos_1_dmg08_outaudio0_cgb04c_outaudio1", Some(Audio(false)), Some(Audio(true))),
        ("ch1_init_pos_4_dmg08_outaudio1_cgb04c_outaudio0", Some(Audio(true)), Some(Audio(false))),
        // long + lowercase hex values
        ("disable_display_regs_2_dmg08_cgb04c_out66e06666006666666666", hex("66e06666006666666666"), hex("66e06666006666666666")),
        ("disable_display_regs_3_dmg08_cgb04c_out91e06666006666666666", hex("91e06666006666666666"), hex("91e06666006666666666")),
        // no tags at all
        ("ime_noie_nolcdirq_readstat_dmg08_cgb_blank", None, None),
        ("vram_dumper", None, None),
        ("late_wx_ds_1", None, None),
    ];
    for (stem, dmg, cgb) in table {
        assert_eq!(
            &str_expectations(stem),
            &(dmg.clone(), cgb.clone()),
            "stem: {stem}"
        );
    }
}

#[test]
fn gambatte_png_ref_resolution() {
    // Tagged references win; the bare PNG only backs the extension's model.
    let set = |suffixes: &'static [&'static str]| move |s: &str| suffixes.contains(&s);
    assert_eq!(png_refs("gb", set(&["_dmg08"])), (Some("_dmg08"), None));
    assert_eq!(png_refs("gbc", set(&["_cgb04c"])), (None, Some("_cgb04c")));
    assert_eq!(
        png_refs("gbc", set(&["_dmg08", "_cgb04c"])),
        (Some("_dmg08"), Some("_cgb04c"))
    );
    // Bare fallback by extension (scx_during_m3/old, window/on_screen…).
    assert_eq!(png_refs("gb", set(&[""])), (Some(""), None));
    assert_eq!(png_refs("gbc", set(&[""])), (None, Some("")));
    // dmgpalette_during_m3_1.gb: bare + _dmg08 → tagged wins, bare unused.
    assert_eq!(png_refs("gb", set(&["", "_dmg08"])), (Some("_dmg08"), None));
    // x-marked PNGs are never offered by the caller (they fail is_file
    // lookups for the verified suffixes), so nothing resolves.
    assert_eq!(png_refs("gbc", set(&["_xcgb"])), (None, None));
    // The testrunner's combined reference applies to both sides.
    assert_eq!(
        png_refs("gb", set(&["_dmg08_cgb04c"])),
        (Some("_dmg08_cgb04c"), Some("_dmg08_cgb04c"))
    );
}

#[test]
fn gambatte_plan_combines_tags_pngs_and_blank() {
    let none = |_: &str| false;
    assert_eq!(
        plan_rom("foo_dmg08_out1_cgb04c_outaudio0", "gbc", none),
        (Some(Check::Hex("1".into())), Some(Check::Audio(false)))
    );
    assert_eq!(
        plan_rom("late_wx_ds_1", "gbc", |s: &str| s.is_empty()),
        (None, Some(Check::Png("")))
    );
    assert_eq!(
        plan_rom("ime_noie_nolcdirq_readstat_dmg08_cgb_blank", "gb", none),
        (Some(Check::Blank), Some(Check::Blank))
    );
    assert_eq!(plan_rom("vram_dumper", "gbc", none), (None, None));
}

/// Render `hex` into a synthetic frame through the vendored glyph table
/// with the given ink/background colors.
#[cfg(test)]
fn synthetic_hex_frame(hex: &str, ink: u32, background: u32) -> Vec<u32> {
    let mut frame = vec![background; slopgb_core::SCREEN_PIXELS];
    for (i, c) in hex.chars().enumerate() {
        let glyph = &GLYPHS[usize::from(hex_val(c))];
        for y in 0..8 {
            for x in 0..8 {
                if glyph[y] & (0x80 >> x) != 0 {
                    frame[y * SCREEN_W + i * 8 + x] = ink;
                }
            }
        }
    }
    frame
}

#[test]
fn gambatte_hex_screen_dmg_roundtrip() {
    // DMG palette: FF/AA/55/00 greys, identity mapping + 0xF8F8F8 mask.
    let frame = synthetic_hex_frame("0123456789ABCDEF", 0x0000_0000, 0x00FF_FFFF);
    check_hex_screen(&frame, "0123456789ABCDEF", false).unwrap();
    // Lowercase expectation digits map to the same glyphs.
    check_hex_screen(&frame, "0123456789abcdef", false).unwrap();
    assert_eq!(read_hex_screen(&frame, false), "0123456789ABCDEF");
    // A prefix of the digits is also accepted — extra screen content to
    // the right is never inspected (frameBufferMatchesOut stops at the
    // expectation's end).
    check_hex_screen(&frame, "012", false).unwrap();
    // Wrong digit somewhere → mismatch naming the offending digit.
    let err = check_hex_screen(&frame, "1123", false).unwrap_err();
    assert!(err.contains("want \"1123\""), "{err}");
    assert!(err.contains("screen shows \"0123456789ABCDEF\""), "{err}");
    // A mid-grey pixel inside a glyph cell fails even though it is neither
    // ink nor background (mask keeps 0xA8A8A8 distinct from both).
    let mut grey = frame.clone();
    grey[SCREEN_W + 1] = 0x00AA_AAAA;
    assert!(check_hex_screen(&grey, "0", false).is_err());
}

#[test]
fn gambatte_hex_screen_cgb_uses_gambatte_color_space() {
    // Core CGB pixels: (31,31,31) → 0xFFFFFF, (0,0,0) → 0. Through the
    // testrunner's gbcToRgb32 + 0xF8F8F8 mask they become the tile table's
    // 0xF8F8F8 / 0x000000 exactly.
    let frame = synthetic_hex_frame("9A", 0x0000_0000, 0x00FF_FFFF);
    check_hex_screen(&frame, "9A", true).unwrap();
    assert_eq!(read_hex_screen(&frame, true), "9A");
    // Near-black (0,0,1) (core pixel 0x000008) masks to black in gambatte
    // color space — gbcToRgb32 maps it to 0x000205 — but NOT under the
    // identity (DMG) mapping. Mirrors the testrunner exactly.
    let near_black = synthetic_hex_frame("7", 0x0000_0008, 0x00FF_FFFF);
    check_hex_screen(&near_black, "7", true).unwrap();
    assert!(check_hex_screen(&near_black, "7", false).is_err());
    // Near-white below (31,31,31) is not background in either space.
    let near_white = synthetic_hex_frame("7", 0x0000_0000, 0x00FF_FFF7);
    assert!(check_hex_screen(&near_white, "7", true).is_err());
}

#[test]
fn gambatte_audio_verdicts() {
    let silent = vec![(0.25f32, -0.5f32); 100];
    check_audio(&silent, false).unwrap();
    assert!(check_audio(&silent, true).is_err());
    let mut varying = silent.clone();
    varying[57].0 += 0.01;
    check_audio(&varying, true).unwrap();
    assert!(check_audio(&varying, false).is_err());
    // An empty capture is a harness failure, never a silence verdict.
    assert!(check_audio(&[], false).is_err());
    assert!(check_audio(&[], true).is_err());
}

#[test]
fn gambatte_blank_verdict() {
    // DMG white (FFFFFF) and CGB white (F8F8F8 after the gambatte map,
    // FFFFFF raw) are both masked-white; any other shade in the top tile
    // row (the print area) fails. Pixels below the print area are
    // ignored: hardware shows the boot ROM's leftover VRAM there.
    let white = vec![0x00FF_FFFFu32; slopgb_core::SCREEN_PIXELS];
    check_blank(&white).unwrap();
    let mut speck = white.clone();
    speck[3] = 0x00AA_AAAA;
    let err = check_blank(&speck).unwrap_err();
    assert!(err.contains("1 non-white"), "{err}");
    // Non-white outside the print area is fine (boot logo leftovers).
    let mut logo = white.clone();
    logo[70 * SCREEN_W + 4] = 0x0000_0000;
    check_blank(&logo).unwrap();
}
