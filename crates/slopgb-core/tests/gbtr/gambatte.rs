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
        common::skip_or_fail_gbtr(
            "sprite_kernel_pair",
            "game-boy-test-roms collection not present",
        );
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
        common::skip_or_fail_gbtr(
            "tier2_kernel_pair",
            "game-boy-test-roms collection not present",
        );
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
        common::skip_or_fail_gbtr(
            "tier2_intr_2_sprites",
            "game-boy-test-roms collection not present",
        );
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
        common::skip_or_fail_gbtr(
            "tier2_boot_div",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel path under the mooneye-test-suite, model legs the suffix runs on).
    let legs: &[(&str, &[Model])] = &[
        (
            "acceptance/boot_div-dmgABCmgb.gb",
            &[Model::Dmg, Model::Mgb],
        ),
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
        common::skip_or_fail_gbtr(
            "tier2_intr_2_mode3",
            "game-boy-test-roms collection not present",
        );
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

/// Port Stage B C1.2 — mooneye `lcdon_timing-GS` on the deferred reclock. The
/// test reads LY/STAT/OAM/VRAM at fixed cycle counts after an LCD enable. Three
/// glitch/post-glitch frame corrections, all the same shape as C1.1 (the
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

/// Port Stage B C1.3 (S7) — mooneye `hblank_ly_scx_timing-GS` on the deferred
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

/// Port Stage C / S5 (mech 3 root 1) — the vblank-entry mode-1 STAT re-arm on
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

/// Port Stage C / S5 (mech 3 root 2) — the line-0 VBlank carry suppresses the
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

/// Port Stage C / S5 — the CGB LCD-enable glitch-line mode-0 IRQ dispatch
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
/// byte-identical — `mode_for_interrupt` is inert there. **CGB-only**: on DMG
/// this is a genuine multi-mechanism atomic (the same glitch-line rise drives
/// the poll path AND the `int_hblank_halt` halt-wake grid, which want the rise
/// at conflicting dots — SameBoy resolves it sub-T-cycle), so the DMG row stays
/// a baselined floor and DMG is byte-identical (`int_hblank_halt` green). See
/// the source comment + `ppu-subdot-ladder.md` "#11ad".
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

/// Port Stage C / S5 (#11ar — the per-ISR read-POSITION PEEK). The first CLEAN
/// read-position-decoupled C-stage slice: the double-speed OAM-STAT-ISR
/// (`m2int`) FF41 mode read lands +4 dots before SameBoy's cfl, so slopgb's
/// leading-edge read sees mode 3 (`got=3`) where SameBoy — reading 4 dots later,
/// past its bare mode-3 exit — sees mode 0 (`want=0`). The peek
/// (`stat_irq.rs::vis_mode_read`, armed by `interconnect.rs::dispatch_retime`
/// via `Ppu::read_carried`) shifts ONLY that read's VERDICT to `dot + off < SBex`
/// (`off` = the per-source offset, `SBex = 257 + SCX&7 + ds` the SameBoy bare
/// exit) — a transient sample, NOT a machine advance — so the counter-pinned
/// dispatch dot and IF delivery stay put (mooneye flag-on 91/91). SCOPED to the
/// carried read (`read_carried` one-shot), native mode 3 (excludes the m0stat/
/// m2stat/enable reads that probe a different boundary), and a non-window
/// (`!wy_trig_sb`) non-sprite bare line (excludes the co-temporal `late_disable`
/// render-length A/B pair). +6/−0 full-CGB two-bin; production (flag-off)
/// byte-identical. See `ppu-subdot-ladder.md` "#11ar" +
/// `measurements/c2-readpos-peek-built-2026-06-30.md`.
#[test]
fn tier2_m2int_m3stat_ds_readpos_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m2int_m3stat_ds_readpos",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    for rel in [
        "gambatte/m2int_m3stat/m2int_m3stat_ds_2_cgb04c_out0.gbc",
        "gambatte/m2int_m3stat/scx/m2int_scx2_m3stat_ds_2_cgb04c_out0.gbc",
        "gambatte/m2int_m3stat/scx/m2int_scx8_m3stat_ds_2_cgb04c_out0.gbc",
        "gambatte/speedchange/m2int_m3stat_lcdoffds_2_cgb04c_out0.gbc",
    ] {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0", true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out0 (tier2 flag-on): {e}"));
    }
}

/// Port Stage C / S5 (mech 3 root 2 — the LYC-write sub-case) — the line-start
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

/// Port Stage C / S5 (mech 3 root 2 — the CGB last-M-cycle LYC-write hold,
/// #11al) — the line-END complement of [`tier2_lyc_carryover_late_ff45_passes`]'s
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
/// production byte-identical. See `reclock.rs::stat_update_tick` +
/// `ppu-subdot-ladder.md` "#11al".
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

/// Port Stage C / S5 (mech 1 — the read-observer eighth-grid) — the bare-line
/// `m2int_m3stat` reads converge flag-on when the mode-0 dispatch lands at cc2 of
/// its M-cycle (dot ≡ 1 mod 4 ⇔ SCX&7 ∈ {3,7}). A leading-edge FF41 read samples
/// at its M-cycle START and observes the flip at cc+2, so a same-M-cycle read
/// should see mode 0; the cc2 dispatch commits one dot past the start, so
/// `vis_early` is anticipated 1 dot (`early_lead = 1`) to that start
/// (`ppu/render/mode0.rs`). `m2int_scx3_m3stat_2` (DMG+CGB, dispatch 257, read
/// 256) and `m2int_nobg_scx7_m3stat_2` (CGB, dispatch 261, read 260) read mode 0
/// (out0); SameBoy reads mode 0 in the same M-cycle. cc1/cc3/cc4 keep el=0 so the
/// kernel (`m2int@252` dispatch 254 ≡2) and `lcdon` hold; the IRQ dispatch keys
/// on `line_render_done`, not `vis_early`, so it is untouched. Restricted to
/// window-free bare lines (`!wy_latch`), so the `window/late_disable_*` read-
/// collapse A/B pairs (both SameBoy-passing, slopgb renders one digit) are not
/// disturbed. Production (flag-off) byte-identical — `vis_early` never set there.
#[test]
fn tier2_m2int_m3stat_scx3_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_m2int_m3stat",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expect, models) — scx3 both models, nobg_scx7 CGB-only (cgb04c tag).
    let targets: [(&str, &str, &[Model]); 2] = [
        (
            "gambatte/m2int_m3stat/scx/m2int_scx3_m3stat_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Dmg, Model::Cgb],
        ),
        (
            "gambatte/m2int_m3stat/nobg/m2int_nobg_scx7_m3stat_2_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
    ];
    for (rel, expect, models) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in models {
            let mut gb = harness::boot_with_reclock(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// Port Stage C / S5 (mech 1 — the read-observer accessibility coupling) — the
/// OAM/VRAM read-accessibility unblock COINCIDES with the visible mode→0 flip
/// (`vis_early`) on SameBoy, not with the render-done dispatch (`line_render_done`)
/// one dot later. The deferred cc+0 read otherwise sees mode 0 yet OAM/VRAM still
/// locked, rendering "3" where SameBoy reads accessible (out0). The fix releases
/// `oam_read_blocked`/`vram_read_blocked` on `vis_early` under Tier-2
/// (`ppu/blocking.rs`). `vram_m3/postread_scx3_2` (DMG+CGB) and
/// `oam_access/postread_scx3_2` (CGB; the DMG leg is `xout1`-exempt) all read the
/// SCX&7=3 cc2 boundary at the M-cycle the visible flip lands. Production
/// (flag-off) byte-identical — `vis_early` is never set there. The `scx2`/`scx5`
/// siblings (the read lands ON the boundary M-cycle, blocked by the cc+2-MID
/// `m0_access_edge` stamp `vis_early` cannot release) are resolved separately by
/// `tier2_oam_vram_postread_scx2_scx5_passes` (the boundary-coincident release).
#[test]
fn tier2_oam_vram_postread_scx3_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_vram_postread_scx3",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expect, models) — vram_m3 both models, oam_access CGB-only (DMG xout1).
    let targets: [(&str, &str, &[Model]); 2] = [
        (
            "gambatte/vram_m3/postread_scx3_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Dmg, Model::Cgb],
        ),
        (
            "gambatte/oam_access/postread_scx3_2_dmg08_xout1_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
    ];
    for (rel, expect, models) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in models {
            let mut gb = harness::boot_with_reclock(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// Port Stage C / S5 (mech 1 — the read-observer accessibility coupling, WRITE
/// side) — the OAM/VRAM write-unblock at the mode3→0 boundary coincides with the
/// visible mode→0 flip (`vis_early`) on SameBoy, one dot before the render-done
/// dispatch (`line_render_done`). The deferred cc+0 write at the SCX&7=3 boundary's
/// M-cycle (slopgb dot 256 / SameBoy cfl 260) otherwise stays blocked
/// (`!line_render_done`) where SameBoy lands it (`blk=0`). The fix releases
/// `oam_write_blocked`/`vram_write_blocked` on `vis_early` under Tier-2, excluding
/// glitch lines (`write_unblocked_early`) so `lcdon_write_timing-GS` (the
/// line-start dots 80-83 gap) is untouched. `oam_access/postwrite_2_scx3` (out1)
/// and `vramw_m3end/vramw_m3end_scx3_5` (out3), both DMG+CGB. Production (flag-off)
/// byte-identical — `vis_early` is never set there.
#[test]
fn tier2_oam_vram_postwrite_scx3_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_vram_postwrite_scx3",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str, &[Model]); 2] = [
        (
            "gambatte/oam_access/postwrite_2_scx3_dmg08_cgb04c_out1.gbc",
            "1",
            &[Model::Dmg, Model::Cgb],
        ),
        (
            "gambatte/vramw_m3end/vramw_m3end_scx3_5_dmg08_cgb04c_out3.gbc",
            "3",
            &[Model::Dmg, Model::Cgb],
        ),
    ];
    for (rel, expect, models) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in models {
            let mut gb = harness::boot_with_reclock(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// Port Stage C2 (mech 1 — the read-observer accessibility coupling, the
/// BOUNDARY-COINCIDENT release) — the `scx2`/`scx5` siblings the scx3 pin left
/// "floored". Their deferred cc+0 OAM/VRAM read lands on the EXACT dot
/// `line_render_done` fires (the unblock M-cycle), where the production cc+2-MID
/// `m0_access_edge` stamp still reports the second-half unblock as blocked (mode 3)
/// — but SameBoy unblocks AT the boundary (reads accessible, out0). The `_1`
/// sibling reads 4 dots earlier (a different M-cycle, no stamp) and stays blocked,
/// so releasing only the boundary M-cycle's stamp is a clean separation (full-CGB
/// two-bin +4/−0 single speed). The fix pushes the M0Access edge to phase 0 under
/// Tier-2 single speed (`render/mode0.rs` `access_lead`); double speed is excluded
/// (the stamp gates the DS VRAM-WRITE path too — `vramw_m3end_scx5_ds` — the DS
/// read grid is its own S6/S7 reclock). Production (flag-off) byte-identical —
/// `bare_flip`/`tier2_reclock` never release it there.
#[test]
fn tier2_oam_vram_postread_scx2_scx5_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_vram_postread_scx2_scx5",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets: [(&str, &str, &[Model]); 4] = [
        (
            "gambatte/vram_m3/postread_scx2_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
        (
            "gambatte/vram_m3/postread_scx5_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
        (
            "gambatte/oam_access/postread_scx2_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
        (
            "gambatte/oam_access/postread_scx5_2_dmg08_cgb04c_out0.gbc",
            "0",
            &[Model::Cgb],
        ),
    ];
    for (rel, expect, models) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for &model in models {
            let mut gb = harness::boot_with_reclock(&rom, model);
            run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
            check_hex_screen(gb.frame(), expect, model.is_cgb()).unwrap_or_else(|e| {
                panic!("{rel} [{model:?}] expected out{expect} (tier2 flag-on): {e}")
            });
        }
    }
}

/// Port Stage C / S5 (mech 3 — CGB lcd-offset, the line-start OAM-read window) —
/// on CGB single-speed SameBoy keeps `oam_read_blocked = false` for the first few
/// T-cycles of each visible line (`display.c:1805-1810`: the mode-0/HBlank tail
/// runs 2+1 cycles before the mode-2 OAM lock engages at state 7). The lcd-offset
/// shifts the `oam_access/preread_lcdoffset1_1` deferred read into that window
/// (slopgb `ly2 dot2` vs SameBoy `ly2 cfl0 blk=0`), where slopgb — locking OAM
/// from dot 0 — read "3" (blocked) instead of out0 (accessible). The fix releases
/// `oam_read_blocked` for dots `1..CGB_LINESTART_OAM_OPEN` on CGB single-speed
/// under Tier-2 (`ppu/blocking.rs::cgb_linestart_oam_open`). CGB-only, single-
/// speed (the `_ds_` siblings are S6, the DMG base reads in real mode-0 already).
/// Production (flag-off) byte-identical — the window is never open there.
///
/// C2 #11x — the window EXCLUDES dot 0: the BASE `oam_access/preread_2` reads
/// `ly2 dot0` and wants BLOCKED (out3 — SameBoy's mode-2 OAM lock has engaged by
/// then; SBMODE `ly2 cfl0 dc8 vis=2`), while the lcd-offset variant's read is
/// shifted off the line start to `dot2`. Opening dots 0-3 served the offset read
/// but wrongly opened the base's dot-0 read; opening only dots 1-3 separates them
/// (full-CGB two-bin flag-on +1/−0, the lcd-offset pin held). Both rows asserted
/// below.
#[test]
fn tier2_oam_preread_lcdoffset1_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_preread_lcdoffset1",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expect) — the lcd-offset variant reads accessible (dot2, open), the
    // base reads blocked (dot0, excluded from the window).
    let targets: [(&str, &str); 2] = [
        (
            "gambatte/oam_access/preread_lcdoffset1_1_cgb04c_out0.gbc",
            "0",
        ),
        ("gambatte/oam_access/preread_2_dmg08_cgb04c_out3.gbc", "3"),
    ];
    for (rel, expect) in targets {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), expect, true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out{expect} (tier2 flag-on): {e}"));
    }
}

/// Port Stage C2 #11y/#11z — the window visible-mode-3 LENGTH law (the FF41-read
/// half of the atomic reclock). A triggering window's SameBoy mode-3→0 exit is
/// `SBex = 263 + SCX&7` (cfl); the CPU-visible FF41 exit is `SBex − read_offset`.
/// **#11z: the deferred FF41 read samples +4 dots before SameBoy's read (MEASURED:
/// `m2int_wx03_scx5_m3stat_2` slopgb dot264 ↔ SameBoy cfl268=SBex), NOT the +3
/// dispatch frame** — so the exit is `259 + SCX&7`. DECOUPLED from
/// `line_render_done` (the counter-pinned dispatch, config-dependently
/// mis-positioned vs SBex so slopgb over-extends — the `m2int_wx*_m3stat_2` reads
/// see mode 3 where SameBoy reads 0). Applied ONLY to the FF41 register read
/// (`stat_irq.rs::vis_mode_read`, NOT the STAT-line `vis_mode` consumers), CGB
/// normal-trigger ly≥1 windows (`win_active && !win_aborted && wy2!=ly && wy2<=143
/// && wx<0xA0 && !ds`). Line 0 / late-WY / WY-disable windows are EXCLUDED — their
/// reads de-mask an entangled read-frame error (the window length + read-frame
/// co-land in the atomic step; #11y); the normal windows have a correct read-frame,
/// so the length law fixes them cleanly. Full-CGB two-bin flag-on +9/−0 (#11y +7
/// at exit 260, #11z +2 more at 259 — the scx5 `_2` over-extend rows). Production
/// byte-identical OFF (`win_active`/`tier2` never fire there). The DMG legs keep
/// their floor (the offset is CGB-measured; `is_cgb` gate).
#[test]
fn tier2_window_m3stat_length_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_m3stat_length",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // All [Cgb] out0 — the normal-trigger window mode-3 read exits at 259+SCX&7
    // (#11z: SBex 263+SCX&7 − the measured +4 read offset). The scx5 `_2` legs
    // pin the 259 (vs 260) calibration; the scx0 legs read past the exit either way.
    // The wxA5/wxA6 legs pin the #11ac off-screen-window extension (the same
    // 259+SCX&7 exit applies to the off-screen-trigger window; sprite-free).
    let rels = [
        "gambatte/window/m2int_wx00_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wx03_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wx03_scx5_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wx03_scx3_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wx07_scx2_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wxA6_m3stat_3_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wxA5_m3stat_2_dmg08_cgb04c_out0.gbc",
        "gambatte/window/m2int_wxA6_scx5_m3stat_3_dmg08_cgb04c_out0.gbc",
    ];
    for rel in rels {
        let rom = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut gb = harness::boot_with_reclock(&rom, Model::Cgb);
        run_to_dot(&mut gb, RUN_DOTS + u64::from(CYCLES_PER_FRAME));
        check_hex_screen(gb.frame(), "0", true)
            .unwrap_or_else(|e| panic!("{rel} [Cgb] expected out0 (tier2 flag-on): {e}"));
    }
}

/// Port Stage C2 #11af — the window render-level **shadow WY-trigger** (the
/// late-WY half of the #11g window model). SameBoy latches `wy_triggered` from a
/// *continuous* `WY == LY` compare (`display.c` `wy_check`), but slopgb's
/// production `wy_latch` samples only at the three gambatte weMaster dots (line 0
/// dot 2, dots 450/454) — so a *mid-line* late-WY write that SameBoy catches is
/// MISSED by slopgb's discrete sampler, and slopgb renders the line BARE where
/// SameBoy's window triggered and extended mode 3 to `263 + SCX&7` (the POLLED
/// read exit, +0). The shadow [`Ppu::wy_trig_sb`] re-derives SameBoy's decision
/// — sticky `WY == LY` latch + the WX-activation deadline ([`Render::wx_match_dot`]
/// `+ 2`, the wy2-copy phase slack) — purely for the FF41-read law
/// ([`Ppu::vis_mode_read`]), NOT `line_render_done`/the render. Fires ONLY when
/// the trigger latched on THIS line (`trig_line == ly`): the cross-line
/// (`trig_line < ly`) latch is left bare because (a) the line-boundary late-WY
/// writes (`10to0`/`FFto0`) land a line later in the deferred frame so the shadow
/// never latches them, and (b) a `!win_active` cross-line latch means the window
/// was aborted / its WX/LCDC.5 toggled late (`late_wx`/`late_reenable`/
/// `late_enable`) — SameBoy renders THOSE bare. Full-CGB two-bin flag-on **+5/−0**
/// (the `_1` mid-line late-WY rows; the `_2`/`_3` siblings + the toggled-window
/// rows stay bare). Production byte-identical OFF (`tier2`/`is_cgb` gated).
#[test]
fn tier2_window_late_wy_extend_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_late_wy_extend",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    // (rel, expected). The `_1` mid-line late-WY rows now extend mode 3 (out3);
    // the `_2` deadline siblings + the cross-line toggled-window rows stay bare
    // (out0) — the regression guards against an over-aggressive shadow (the +2
    // slack boundary, and the `trig_line == ly` gate that excludes late_wx /
    // late_reenable).
    let targets = [
        // FIXED — the shadow extends the missed mid-line late-WY trigger.
        (
            "gambatte/window/arg/late_wy_10to1_ly1_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx2_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx3_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_wx0f_1_dmg08_cgb04c_out3.gbc",
            "3",
        ),
        // GUARD — the `_2` siblings miss the deadline (+2 slack): stay bare.
        (
            "gambatte/window/arg/late_wy_10to1_ly1_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_scx2_2_dmg08_out3_cgb04c_out0.gbc",
            "0",
        ),
        // GUARD — cross-line toggled-window rows: the shadow must NOT extend.
        ("gambatte/window/late_wx_1_dmg08_cgb04c_out0.gbc", "0"),
        (
            "gambatte/window/late_reenable_scx5_3_dmg08_cgb04c_out0.gbc",
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

/// Port Stage C2 #11ag — the WINDOW family ported to DOUBLE-SPEED: the #11y/#11z
/// length law AND the #11af shadow WY-trigger, with the DS exit/deadline
/// recalibrated. The `vis_mode_read` length law (the `m2int_wx*_m3stat` shorten)
/// and the late-WY shadow extend were both `!ds`-gated; under DS the deferred
/// cc+0 FF41 read lands +1 dot vs SS (the ISR read offset is +3 not +4), so the
/// **length-law exit is `260 + SCX&7`** (`259 + ds`) and the **shadow exit is
/// `264 + SCX&7`** (`263 + ds`). MEASURED: `m2int_wxA6_scx5_m3stat_ds` reads `_1`
/// dot264 / `_2` dot266 → only exit 265 (=260+5) separates them (and does NOT
/// drop the off-screen `_1` SameBoy-pass); `late_wy_FFto2_ly2_scx5_ds_1` reads
/// dot268 → the shadow exit must clear it. The shadow **deadline slack is +4** in
/// DS (the wy2-copy lands the trigdot 2 dots later: `late_wy_FFto2_ly2_ds` `_1`
/// trigdot 101 / `_2` 103 vs wxmatch 97). DS additionally **excludes
/// sprite-laden lines** from BOTH laws (`!ds || n_sprites == 0`) — with sprites
/// the real mode-3 end extends past the bare exit and the DS read frame straddles
/// it (`sprites/space/10spritesPrLine_wx*_m3stat_ds_1` would drop, a SameBoy-pass;
/// that is the #11t DS sprite read-grid, separate). Full-CGB two-bin flag-on
/// **+8/−0** (7 length-law `_2` + the shadow `FFto2_ly2_ds_1`). SS legs
/// byte-identical (the `ds` terms are 0 in single speed); production byte-identical
/// OFF. The `scx5_ds_1` length `_1` rows + `late_wy_*_ds` boundary/disable rows
/// stay atomic (the same SCX-non-linear deadline / deferred-frame walls as #11af).
#[test]
fn tier2_window_ds_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_window_ds",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // FIXED — DS length law (`m2int_wx*_m3stat_ds_2`, want0): shorten at 260+SCX&7.
        (
            "gambatte/window/m2int_wx03_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wx07_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wxDefault_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // FIXED — the off-screen wxA6 DS pair (`_2` shortens; `_1` must stay mode3).
        (
            "gambatte/window/m2int_wxA6_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/window/m2int_wxA6_scx5_m3stat_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // GUARD — the off-screen `_1` (exit 265 separates it from `_2` dot266).
        (
            "gambatte/window/m2int_wxA6_scx5_m3stat_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        // FIXED — DS shadow WY-trigger (`late_wy_FFto2_ly2_ds_1`, want3): extend.
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        // GUARD — the DS `_2` deadline sibling stays bare (slack +4 boundary).
        (
            "gambatte/window/arg/late_wy_FFto2_ly2_ds_2_cgb04c_out0.gbc",
            "0",
        ),
        // GUARD — the DS-sprite exclusion: this `_1` (want3) must NOT be shortened.
        (
            "gambatte/sprites/space/10spritesPrLine_wx0_m3stat_ds_1_cgb04c_out3.gbc",
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

/// Port Stage C / S5 (mech 3 — CGB lcd-offset, the line-start OAM-read window,
/// DOUBLE-SPEED sibling of [`tier2_oam_preread_lcdoffset1_passes`]). Under DS the
/// deferred cc+0 read lands 2 dots earlier in the dot grid, so the accessible
/// `preread_ds*_1` read shifts to `dot0` and the slopgb-side window narrows with
/// it ([`CGB_LINESTART_OAM_OPEN_DS`] = 2). Both the base `preread_ds_1` and the
/// offset `preread_ds_lcdoffset1_1` read accessible (out0); their `_2` siblings
/// read `dot2` and stay blocked (out3 — a lcd-offset RENDER shift slopgb matches
/// via its mode-3 OAM block, NOT the OAM read, so the window must NOT extend to
/// them). The `_2` legs are pinned here as regression guards against a
/// widened window. Probe (3524 CGB rows, flag-on): +2/−0. Byte-identical OFF.
#[test]
fn tier2_oam_preread_ds_lcdoffset1_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_oam_preread_ds_lcdoffset1",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        ("gambatte/oam_access/preread_ds_1_cgb04c_out0.gbc", "0"),
        ("gambatte/oam_access/preread_ds_2_cgb04c_out3.gbc", "3"),
        (
            "gambatte/oam_access/preread_ds_lcdoffset1_1_cgb04c_out0.gbc",
            "0",
        ),
        (
            "gambatte/oam_access/preread_ds_lcdoffset1_2_cgb04c_out3.gbc",
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

/// Port Stage C / S5 (mech 3 — CGB lcd-offset, the m3-start palette-RAM window) —
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

/// Port Stage C / S5 (mech 3 — CGB lcd-offset, the dispatch-class HBlank
/// write-trigger) — a fresh mode-0 (HBlank) STAT enable written in the
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

/// Port Stage C / S5 (mech 3 — CGB lcd-offset, the dispatch-class HBlank
/// write-trigger, DOUBLE-SPEED window). Sibling of
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

/// Port Stage C / S5 (mech 1 read-observer — the DOUBLE-SPEED sprite m3stat
/// read-grid snap). The single-speed sprite read-grid snap (#10 B5) snaps the
/// sprite-line mode-0 dispatch to the CPU read grid (`dot % 4 == 0`); that
/// `snap_ok` term applied in DOUBLE speed too. But the DS sprite-line FF41
/// mode-bit read does not use `vis_early` (which is `!self.ds`-gated, the wrong
/// direction here — these reads want the LAGGING mode 3, not an anticipated 0).
/// It rides the PRODUCTION `stat_mode_edge` override (INC-DS-1 / INC-G3 task 6,
/// `interconnect/memory.rs`: a DS sprite-line m3→m0 flip holds the FF41 mode
/// bits at 3 for the read M-cycle), which is armed by the `m0_stat_flip` stamp
/// that ONLY `m0_flip_events` sets. The mod-4 snap pushed the DS sprite dispatch
/// past the pipe end (`advance_lx` lx=160), where the pipe-end fallback set
/// `m0_src` first and `m0_flip_events` early-returned — so the stamp was never
/// armed and the deferred cc+0 read saw the already-flipped visible mode 0 (digit
/// 0 where SameBoy reads 3). Fix (`render/mode0.rs`): gate the dispatch snap to
/// single speed (`snap_ok = !(tier2 && has_sprites && !ds) || dot % 4 == 0`), so
/// DS sprite lines flip at the natural dot, arm the stamp, and the deferred read
/// straddles the override. SameBoy verified: `..._ds_1` read mode 3, the in-cluster
/// `late_sizechange_*_ds_2` (out3) join the lift; the 3 `late_sizechange_*_ds_1`
/// (out0) are gambatte-reference floors (SameBoy reads mode 3, already baselined in
/// production). CGB DS only; vis_early untouched; byte-identical OFF. Probe (3524
/// CGB rows, flag-on): +87/−3 (net +84), 0 SameBoy-passing rows dropped.
#[test]
fn tier2_sprite_m3stat_ds_passes() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "tier2_sprite_m3stat_ds",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let targets = [
        // The DS sprite m3stat lift (want the lagging mode 3).
        (
            "gambatte/sprites/8spritesPrLine_m3stat_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/sprites/10spritesprline_1xposa7_m3stat_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        (
            "gambatte/sprites/1spritesPrLine_1sprite8pBgPrior_m3stat_ds_1_cgb04c_out3.gbc",
            "3",
        ),
        // The in-cluster A/B winner (size-change `_2`, out3) — joins the lift.
        (
            "gambatte/sprites/late_sizechange_sp00_ds_2_cgb04c_out3.gbc",
            "3",
        ),
        // The `_2` mode-0 sibling is the regression guard (must stay out0).
        (
            "gambatte/sprites/8spritesPrLine_m3stat_ds_2_cgb04c_out0.gbc",
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

/// Port Stage C / S5 (mech 3 — CGB lcd-offset, the dispatch-class VBlank + LYC
/// write-triggers) — the lcd-offset shifts these late STAT enables into the
/// line-start dots-0-3 carryover, where the base gambatte logic suppresses them.
/// `m1/m1irq_late_enable_lcdoffset1_1` enables the VBlank source at `ly0 dot3`
/// (the `m1_tail` suppression, `stat_irq.rs`); `lycEnable/late_ff41_enable_lcdoffset1_1`
/// enables the LYC source at `ly7 dot3` with `LYC=ly-1` (the carryover compare —
/// `cmp_cgb` has switched to the new line so `lyc_high` is false). SameBoy fires
/// both at the write; slopgb delivered `if=00` (out0) instead of out2. The fix
/// (Tier-2, `stat_write_trigger_cgb`): drop the `m1_tail` suppression for a fresh
/// VBlank enable, and fire a fresh LYC enable whose LYC matches the PREVIOUS line
/// in the carryover. CGB-only. Probe (654 CGB rows, flag-on): +2/−0, the #11k/#11l
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

/// Port Stage C / S5 (mech 3 — CGB lcd-offset, the lyc-engine dispatch tail; #11r).
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
/// vs SameBoy cfl0) + the offset-shifted FF0F read position are the mech-1 read-
/// frame residual (C2). Probe (676 CGB engine-family rows, flag-on): +4/−0.
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

// Session-local S5 measurement aid (see the module's doc); `#[ignore]`'d so it
// never runs in the gate.
#[path = "gambatte_flagon_probe.rs"]
mod flagon_probe;
