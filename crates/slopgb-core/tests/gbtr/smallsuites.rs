//! Small screenshot suites of the c-sp collection: `bully/`,
//! `strikethrough/`, `turtle-tests/`, `scribbltests/`, `little-things-gb/`,
//! `mbc3-tester/` and `rtc3test/`.
//!
//! All of these are timed-run + frame-compare suites (no breakpoint or
//! serial completion signal); each directory's `game-boy-test-roms-howto.md`
//! gives the emulated run time and the reference screenshot(s). Run times
//! below are the howto figures plus ~30% margin — the result screens are
//! stable once drawn, so extra time is safe. The references use the
//! collection's "common palette" — DMG greys FF/AA/55/00 and straight 5→8
//! CGB expansion — which is exactly the core's output, so every comparison
//! here uses [`CgbColorMap::Identity`].

use std::path::Path;

use slopgb_core::{Button, GameBoy, Model};

use crate::common;
use crate::common::framecmp::CgbColorMap;
use crate::harness::{self, CaseResult, case_key};

/// T-cycles per PPU scanline (Pan Docs "Rendering": 456 dots).
const TCYCLES_PER_LINE: u64 = 456;
/// T-cycles per frame (154 scanlines x 456 dots).
const TCYCLES_PER_FRAME: u64 = 154 * TCYCLES_PER_LINE;

/// Collection directories this module owns (see the inventory test).
const SUITE_DIRS: [&str; 7] = [
    "bully",
    "strikethrough",
    "turtle-tests",
    "scribbltests",
    "little-things-gb",
    "mbc3-tester",
    "rtc3test",
];

// Per-suite case tables (collection-relative paths). The seven `#[test]`
// fns below execute exactly these, and [`inventory`] derives its claimed
// set from the same constants — so the two can never drift apart.

/// BullyGB: one ROM, one shared reference for both models.
const BULLY_ROM: &str = "bully/bully.gb";
const BULLY_PNG: &str = "bully/bully.png";

/// Strikethrough: one ROM, per-model references (suffix via [`png_suffix`]).
const STRIKETHROUGH_ROM: &str = "strikethrough/strikethrough.gb";

/// Turtle Tests: (rom, reference valid on both models).
const TURTLE_CASES: [(&str, &str); 2] = [
    (
        "turtle-tests/window_y_trigger/window_y_trigger.gb",
        "turtle-tests/window_y_trigger/window_y_trigger.png",
    ),
    (
        "turtle-tests/window_y_trigger_wx_offscreen/window_y_trigger_wx_offscreen.gb",
        "turtle-tests/window_y_trigger_wx_offscreen/window_y_trigger_wx_offscreen.png",
    ),
];

/// Scribbltests: (rom, reference on Dmg, reference on Cgb, frames to run).
const SCRIBBL_CASES: [(&str, &str, &str, u64); 5] = [
    (
        "scribbltests/lycscx/lycscx.gb",
        "scribbltests/lycscx/lycscx-cgb-dmg.png",
        "scribbltests/lycscx/lycscx-cgb-dmg.png",
        15,
    ),
    (
        "scribbltests/lycscy/lycscy.gb",
        "scribbltests/lycscy/lycscy-cgb-dmg.png",
        "scribbltests/lycscy/lycscy-cgb-dmg.png",
        15,
    ),
    (
        "scribbltests/palettely/palettely.gb",
        "scribbltests/palettely/palettely-dmg.png",
        "scribbltests/palettely/palettely-cgb.png",
        15,
    ),
    (
        "scribbltests/scxly/scxly.gb",
        "scribbltests/scxly/scxly-dmg.png",
        "scribbltests/scxly/scxly-cgb.png",
        15,
    ),
    (
        "scribbltests/statcount/statcount-auto.gb",
        "scribbltests/statcount/statcount_auto-cgb-dmg.png",
        "scribbltests/statcount/statcount_auto-cgb-dmg.png",
        350,
    ),
];

/// little-things-gb: firstwhite (one shared reference) and the
/// joypad-driven tellinglys (per-model references).
const FIRSTWHITE_ROM: &str = "little-things-gb/firstwhite.gb";
const FIRSTWHITE_PNG: &str = "little-things-gb/firstwhite-dmg-cgb.png";
const TELLINGLYS_ROM: &str = "little-things-gb/tellinglys.gb";

/// MBC3 Bank Tester: one ROM, per-model references.
const MBC3_TESTER_ROM: &str = "mbc3-tester/mbc3-tester.gb";

/// rtc3test: one ROM hosting three menu-selected subtests —
/// (subtest name, menu presses, howto run seconds). Case keys carry a
/// `#<subtest>` discriminator since all three share the one ROM path.
const RTC3TEST_ROM: &str = "rtc3test/rtc3test.gb";
const RTC3TEST_SUBTESTS: [(&str, &[Button], f64); 3] = [
    ("basic-tests", &[Button::A], 13.0),
    ("range-tests", &[Button::Down, Button::A], 8.0),
    (
        "sub-second-writes",
        &[Button::Down, Button::Down, Button::A],
        26.0,
    ),
];

/// Reference-PNG filename suffix for the two models these suites run on.
fn png_suffix(model: Model) -> &'static str {
    match model {
        Model::Dmg => "dmg",
        Model::Cgb => "cgb",
        other => panic!("smallsuites only routes Dmg/Cgb, got {other:?}"),
    }
}

/// Step until at least `tcycles` more T-cycles have elapsed (instruction
/// granularity, like `harness::run_for_seconds` — the overshoot is at most
/// one instruction).
fn run_for_tcycles(gb: &mut GameBoy, tcycles: u64) {
    let target = gb.cycles().saturating_add(tcycles);
    while gb.cycles() < target {
        gb.step();
    }
}

/// One menu tap: press, hold a few frames, release, settle a few frames —
/// these ROMs poll the joypad (or take the interrupt) once per frame, so a
/// multi-frame hold registers reliably.
fn tap(gb: &mut GameBoy, b: Button) {
    gb.press(b);
    harness::run_for_frames(gb, 3);
    gb.release(b);
    harness::run_for_frames(gb, 3);
}

/// Shared case plumbing: read `rom_rel`, boot it on `model`, drive the
/// machine via `drive`, then settle on the next completed frame boundary
/// and compare against `png_rel`.
///
/// `case_rel` is the baseline-key identity — equal to `rom_rel` except for
/// rtc3test, where one ROM hosts three menu-selected subtests and the key
/// carries a `#<subtest>` discriminator.
fn frame_case(
    root: &Path,
    rom_rel: &str,
    case_rel: &str,
    model: Model,
    png_rel: &str,
    drive: impl FnOnce(&mut GameBoy),
) -> CaseResult {
    let key = case_key(case_rel, model);
    // catch_case: a panicking case (core crash) becomes a keyed Err so it
    // cannot abort the rest of the suite matrix.
    let result = harness::catch_case(|| {
        let rom = std::fs::read(root.join(rom_rel)).map_err(|e| format!("read failed: {e}"))?;
        let mut gb = harness::boot(&rom, model);
        drive(&mut gb);
        // Timed runs stop mid-frame; advance to the next completed frame
        // boundary so the comparison sees a frame rendered entirely after
        // the howto's exit condition (`GameBoy::frame` returns the most
        // recently *completed* frame).
        harness::run_for_frames(&mut gb, 1);
        harness::expect_frame_png(&gb, &root.join(png_rel), CgbColorMap::Identity)
    });
    CaseResult { key, result }
}

// ---------------------------------------------------------------- bully --

// Triaged (the screen's only diff vs the all-pass reference is the failure
// message text; decoded from the ROM's ASCII-indexed tilemap):
//
// * [Dmg]: `initram.asm` "Uninitialized RAM not randomized" — the test
//   fails iff every WRAM byte is $00/$FF at power-on, and our WRAM is
//   deterministically zeroed (core design: deterministic, no host
//   entropy). Note the all-pass reference is unreachable for a faithful
//   DMG anyway: the howto documents a *real* DMG-C failing this ROM with
//   "Bad Echo RAM Reads" (bully/game-boy-test-roms-howto.md), a check we
//   pass — so this leg stays a documented divergence whichever way WRAM
//   init goes.
// * [Cgb]: `divtest.asm` "Invalid initial DIV" — the ROM's very first
//   test compares the first DIV read against $1F, a value its own source
//   marks "(TODO: Confirm)". Our hand-off DIV counter is pinned by the
//   hardware-verified mooneye boot_div-cgbABCDE (green); the bully read
//   happens after framework init whose duration couples to LCD phase, so
//   this is a downstream-timing divergence, not a hand-off-state bug.
//   The [Dmg] leg passes the same test ($AD expected).
const BULLY_BASELINE: &[&str] = &["bully/bully.gb [Dmg]", "bully/bully.gb [Cgb]"];

/// BullyGB (`bully/game-boy-test-roms-howto.md`): run 0.5 emulated seconds,
/// compare against the single `bully.png`. Device-specific test cases are
/// "included/skipped automatically" by the ROM itself (BullyGB wiki), so the
/// one reference applies to both DMG and CGB.
///
/// Howto caveat: a *real* DMG (DMG-CPU C) fails with `Bad Echo RAM Reads`
/// while CGB devices pass — so a faithful DMG emulation may legitimately
/// mismatch the reference; the baseline records the observed verdict.
#[test]
fn smallsuites_bully() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "smallsuites_bully",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let mut results = Vec::new();
    for model in [Model::Dmg, Model::Cgb] {
        results.push(frame_case(
            &root,
            BULLY_ROM,
            BULLY_ROM,
            model,
            BULLY_PNG,
            |gb| harness::run_for_seconds(gb, 0.65),
        ));
    }
    harness::assert_against_baseline("smallsuites/bully", &results, BULLY_BASELINE);
}

// -------------------------------------------------------- strikethrough --

// Both legs differ by the same 7 pixels at y=68, x=71-78 (one sprite
// cell): the ROM races an OAM DMA against the mode-2 scan, and the
// dot-serial scan + DMA disconnect (calibrated by the gambatte
// oamdma/late_sp* family) reproduce the strikethrough rows' sprites
// vanishing exactly — the pre-rewrite diff was 53 pixels. The residue is
// the single *glitch* sprite cell hardware still renders out of the
// disconnected bus: its data is undocumented DMA-driver residue (the
// ROM's own source marks the controlling byte "seems to affect the tile
// number of the sprite thats shown" — the same unexplained family as the
// madness MGB freeze glitch), gambatte's rdisabledRam model cannot
// produce it, and reading the in-flight byte instead contradicts the
// dmg08-verified oamdma/late_sp* out0 rows. No documented mechanism to
// emulate; floored.
const STRIKETHROUGH_BASELINE: &[&str] = &[
    "strikethrough/strikethrough.gb [Dmg]",
    "strikethrough/strikethrough.gb [Cgb]",
];

/// Strikethrough (`strikethrough/game-boy-test-roms-howto.md`): run 0.5
/// emulated seconds, compare against the per-model reference.
#[test]
fn smallsuites_strikethrough() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "smallsuites_strikethrough",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let mut results = Vec::new();
    for model in [Model::Dmg, Model::Cgb] {
        results.push(frame_case(
            &root,
            STRIKETHROUGH_ROM,
            STRIKETHROUGH_ROM,
            model,
            &format!("strikethrough/strikethrough-{}.png", png_suffix(model)),
            |gb| harness::run_for_seconds(gb, 0.65),
        ));
    }
    harness::assert_against_baseline(
        "smallsuites/strikethrough",
        &results,
        STRIKETHROUGH_BASELINE,
    );
}

// --------------------------------------------------------- turtle-tests --

const TURTLE_BASELINE: &[&str] = &[];

/// Turtle Tests (`turtle-tests/game-boy-test-roms-howto.md`): around 30
/// frames is sufficient (run 40); one suffix-less reference per ROM, valid
/// on both DMG and CGB ("these tests will probably run on any DMG and CGB").
#[test]
fn smallsuites_turtle_tests() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "smallsuites_turtle_tests",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let mut results = Vec::new();
    for (rom_rel, png_rel) in TURTLE_CASES {
        for model in [Model::Dmg, Model::Cgb] {
            results.push(frame_case(&root, rom_rel, rom_rel, model, png_rel, |gb| {
                harness::run_for_frames(gb, 40)
            }));
        }
    }
    harness::assert_against_baseline("smallsuites/turtle-tests", &results, TURTLE_BASELINE);
}

// --------------------------------------------------------- scribbltests --

// scxly's [Dmg] leg passes since the per-source STAT-event port (the LYC
// IRQ no longer rides the wired-OR line, so the per-line LYC handler runs
// on time). The CGB reference is an off-convention asset — it uses
// green-LCD colors (#0F380F/#98C00F) that no `(c << 3) | (c >> 2)`
// expansion of any RGB555 palette can produce, so the [Cgb] leg can never
// pass under the Identity compare even with correct compat palettes.
const SCRIBBL_BASELINE: &[&str] = &["scribbltests/scxly/scxly.gb [Cgb]"];

/// Scribbltests (`scribbltests/game-boy-test-roms-howto.md`): around 10
/// frames is enough (run 15) except `statcount-auto`, which needs ~270
/// frames (run 350). Verified by the author on MGB and CGB; lycscx, lycscy
/// and statcount-auto share one `-cgb-dmg` reference for both models, while
/// palettely and scxly have per-model references.
///
/// `fairylake`, `winpos` and the plain `statcount` are exempt — see
/// [`inventory`].
#[test]
fn smallsuites_scribbltests() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "smallsuites_scribbltests",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let mut results = Vec::new();
    for (rom_rel, png_dmg, png_cgb, frames) in SCRIBBL_CASES {
        for (model, png_rel) in [(Model::Dmg, png_dmg), (Model::Cgb, png_cgb)] {
            results.push(frame_case(&root, rom_rel, rom_rel, model, png_rel, |gb| {
                harness::run_for_frames(gb, frames)
            }));
        }
    }
    harness::assert_against_baseline("smallsuites/scribbltests", &results, SCRIBBL_BASELINE);
}

// ----------------------------------------------------- little-things-gb --

// Empty: tellinglys matches its per-model references on both models, and
// firstwhite passes on both models via the hardware first-frame-after-LCD-
// enable blanking (Ppu::frame_skip) — without it the ROM's text showed on
// 1 of every 3 frames and the verdict depended on sampling phase.
const LITTLE_THINGS_BASELINE: &[&str] = &[];

/// Joypad schedule for `little-things-gb/tellinglys.gb`:
/// `(button, gap_tcycles_before_press, hold_tcycles)` for all eight buttons.
///
/// The ROM seeds its entropy check from the LY register at each joypad
/// interrupt (tellinglys-readme.md), so the eight press instants must land
/// on varied scanlines — an emulator that effectively presses everything at
/// the same frame position fails as "polling occurs on GB vblank".
fn tellinglys_schedule() -> [(Button, u64, u64); 8] {
    // Prime scanline counts plus prime dot remainders make every gap
    // irregular; combined with the whole-frame paddings below (which are
    // 0 mod the frame period) the cumulative press instants fall on the
    // pairwise distinct scanlines 83, 26, 139, 117, 113, 126, 0 and 45 —
    // verified by `smallsuites_tellinglys_schedule_has_entropy`.
    let gap_lines: [u64; 8] = [83, 97, 113, 131, 149, 167, 181, 199];
    let gap_dots: [u64; 8] = [101, 151, 199, 251, 307, 353, 409, 31];
    // Human-tap pacing: hold each button three frames and pad every gap by
    // ten whole frames (LY-neutral), giving ~220 ms press-to-press —
    // observed: with only ~4 frames press-to-press the ROM dropped presses
    // and never reached its pass screen.
    let pad = 10 * TCYCLES_PER_FRAME;
    let hold = 3 * TCYCLES_PER_FRAME;
    let buttons = [
        Button::A,
        Button::B,
        Button::Select,
        Button::Start,
        Button::Right,
        Button::Left,
        Button::Up,
        Button::Down,
    ];
    let mut sched = [(Button::A, 0u64, 0u64); 8];
    for (i, &button) in buttons.iter().enumerate() {
        sched[i] = (
            button,
            pad + gap_lines[i] * TCYCLES_PER_LINE + gap_dots[i],
            hold,
        );
    }
    sched
}

/// little-things-gb (`little-things-gb/game-boy-test-roms-howto.md`):
///
/// * `firstwhite.gb` — result visible nearly immediately; run 0.5 emulated
///   seconds, one shared `-dmg-cgb` reference for both models (the readme
///   excludes only the Super Game Boy).
/// * `tellinglys.gb` — input-driven: starting at the title screen, press
///   all eight buttons one after another ([`tellinglys_schedule`]), then
///   give it 5 emulated seconds after the last press for the pass screen;
///   per-model references.
#[test]
fn smallsuites_little_things_gb() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "smallsuites_little_things_gb",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let mut results = Vec::new();
    for model in [Model::Dmg, Model::Cgb] {
        results.push(frame_case(
            &root,
            FIRSTWHITE_ROM,
            FIRSTWHITE_ROM,
            model,
            FIRSTWHITE_PNG,
            |gb| harness::run_for_seconds(gb, 0.65),
        ));
        results.push(frame_case(
            &root,
            TELLINGLYS_ROM,
            TELLINGLYS_ROM,
            model,
            &format!("little-things-gb/tellinglys-{}.png", png_suffix(model)),
            |gb| {
                // Let the title screen come up before the first press
                // (readme: "Starting at the title screen, press ...").
                harness::run_for_frames(gb, 60);
                for (button, gap, hold) in tellinglys_schedule() {
                    run_for_tcycles(gb, gap);
                    gb.press(button);
                    run_for_tcycles(gb, hold);
                    gb.release(button);
                }
                // howto: 5 s emulated after the last press is enough.
                harness::run_for_seconds(gb, 6.5);
            },
        ));
    }
    harness::assert_against_baseline(
        "smallsuites/little-things-gb",
        &results,
        LITTLE_THINGS_BASELINE,
    );
}

// ---------------------------------------------------------- mbc3-tester --

// DMG leg passes via the MBC30 8-bit ROM-bank register (the ROM is a 4 MiB
// MBC3-type cart, src/cartridge.rs). After that fix the CGB leg's only
// remaining diff is a defective reference asset: every mismatching pixel
// (3825) is `got #7BFF31 want #7BFF4A`, i.e. the PNG's compat-mode green
// contradicts the suite's own howto, which specifies the background shades
// as "#000000, #0063C6, #7BFF31 and #FFFFFF"
// (mbc3-tester/game-boy-test-roms-howto.md). #7BFF31 is exactly the
// `(c << 3) | (c >> 2)` expansion of the CGB boot ROM's default compat BG
// palette entry $1BEF; do not "fix" this with palette work.
const MBC3_TESTER_BASELINE: &[&str] = &["mbc3-tester/mbc3-tester.gb [Cgb]"];

/// MBC3 Bank Tester (`mbc3-tester/game-boy-test-roms-howto.md`): the ROM
/// loops indefinitely, the result is valid after the first 40 frames (run
/// 60); per-model references — on CGB it runs in CGB compatibility mode.
#[test]
fn smallsuites_mbc3_tester() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "smallsuites_mbc3_tester",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let mut results = Vec::new();
    for model in [Model::Dmg, Model::Cgb] {
        results.push(frame_case(
            &root,
            MBC3_TESTER_ROM,
            MBC3_TESTER_ROM,
            model,
            &format!("mbc3-tester/mbc3-tester-{}.png", png_suffix(model)),
            |gb| harness::run_for_frames(gb, 60),
        ));
    }
    harness::assert_against_baseline("smallsuites/mbc3-tester", &results, MBC3_TESTER_BASELINE);
}

// ------------------------------------------------------------- rtc3test --

const RTC3TEST_BASELINE: &[&str] = &[];

/// rtc3test (`rtc3test/game-boy-test-roms-howto.md`): one ROM, three
/// menu-selected subtests; emulate the button presses, then wait the
/// documented emulated duration:
///
/// | subtest           | presses       | duration |
/// |-------------------|---------------|----------|
/// | basic tests       | A             | 13 s     |
/// | range tests       | down, A       | 8 s      |
/// | sub-second writes | down, down, A | 26 s     |
///
/// "This procedure should be the same on all Game Boy devices"; per-model
/// references exist for DMG and CGB. Case keys carry a `#<subtest>`
/// discriminator since all three share one ROM path.
#[test]
fn smallsuites_rtc3test() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "smallsuites_rtc3test",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let mut results = Vec::new();
    for (name, presses, secs) in RTC3TEST_SUBTESTS {
        for model in [Model::Dmg, Model::Cgb] {
            results.push(frame_case(
                &root,
                RTC3TEST_ROM,
                &format!("{RTC3TEST_ROM}#{name}"),
                model,
                &format!("rtc3test/rtc3test-{name}-{}.png", png_suffix(model)),
                |gb| {
                    // Let the menu draw (~1 emulated second) before pressing.
                    harness::run_for_seconds(gb, 1.3);
                    for &b in presses {
                        tap(gb, b);
                    }
                    harness::run_for_seconds(gb, secs * 1.3);
                },
            ));
        }
    }
    harness::assert_against_baseline("smallsuites/rtc3test", &results, RTC3TEST_BASELINE);
}

// ------------------------------------------------------------ inventory --

/// Every `.gb`/`.gbc` file under [`SUITE_DIRS`], split into ROMs that
/// produce at least one rom×model case (`claimed`) and documented
/// never-run ROMs (`exempted`). The claimed set is derived from the same
/// case-table constants the seven `#[test]` fns execute, so it cannot
/// drift from what actually runs; the rtc3test ROM is claimed once even
/// though its three `#<subtest>` cases share it.
pub fn inventory() -> (Vec<String>, Vec<String>) {
    let mut claimed: Vec<String> = [
        BULLY_ROM,
        STRIKETHROUGH_ROM,
        FIRSTWHITE_ROM,
        TELLINGLYS_ROM,
        MBC3_TESTER_ROM,
        RTC3TEST_ROM,
    ]
    .map(String::from)
    .to_vec();
    claimed.extend(TURTLE_CASES.iter().map(|(rom, _)| (*rom).to_string()));
    claimed.extend(SCRIBBL_CASES.iter().map(|(rom, ..)| (*rom).to_string()));
    claimed.sort();
    claimed.dedup();
    let exempted = [
        // scribbltests howto: "there are no screenshots for failrylake and
        // winpos at the moment". fairylake is additionally "closer to a demo
        // than a proper test ROM" and WIP (its README) — nothing to compare
        // against.
        "scribbltests/fairylake/fairylake.gb",
        // Same missing-screenshot howto note; winpos is an interactive
        // debugging tool (its README: WX/WY modified at runtime via the
        // joypad), with no automatable pass criterion.
        "scribbltests/winpos/winpos.gb",
        // statcount/README.md: the plain statcount ROM is the interactive
        // variant (NOP count selected with Up/Down); only statcount-auto
        // runs unattended and only it has a reference screenshot.
        "scribbltests/statcount/statcount.gb",
    ]
    .map(String::from)
    .to_vec();
    (claimed, exempted)
}

/// Self-check: `claimed` and `exempted` are disjoint and together cover the
/// on-disk `.gb`/`.gbc` set of [`SUITE_DIRS`] exactly (the global Phase B2
/// guard re-asserts this across all suites later).
#[test]
fn smallsuites_inventory_covers_suite_dirs() {
    let (claimed, exempted) = inventory();
    for c in &claimed {
        assert!(!exempted.contains(c), "{c} both claimed and exempted");
    }
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "smallsuites_inventory_covers_suite_dirs",
            "game-boy-test-roms collection not present",
        );
        return;
    };
    let mut on_disk = Vec::new();
    for dir in SUITE_DIRS {
        let mut roms = Vec::new();
        common::collect_roms(&root.join(dir), true, &mut roms)
            .unwrap_or_else(|e| panic!("cannot enumerate {dir}: {e}"));
        for rom in roms {
            let rel = rom.strip_prefix(&root).expect("rom under collection root");
            // Forward-slash keys on every platform (CI runs windows too).
            on_disk.push(
                rel.iter()
                    .map(|c| c.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
    on_disk.sort();
    let mut combined: Vec<String> = claimed.into_iter().chain(exempted).collect();
    combined.sort();
    assert_eq!(
        combined, on_disk,
        "inventory() must cover the suite dirs exactly"
    );
}

// ----------------------------------------------------------- unit tests --

#[test]
fn smallsuites_tellinglys_schedule_has_entropy() {
    let sched = tellinglys_schedule();
    // Every button exactly once (the ROM requires all eight presses).
    let mut buttons: Vec<String> = sched.iter().map(|(b, _, _)| format!("{b:?}")).collect();
    buttons.sort();
    buttons.dedup();
    assert_eq!(
        buttons.len(),
        8,
        "schedule must press all 8 distinct buttons"
    );
    // Press instants, accumulated from a frame boundary (the runner applies
    // the schedule right after `run_for_frames`), must land on pairwise
    // distinct scanlines so LY is a genuine entropy source.
    let mut t = 0u64;
    let mut lys = Vec::new();
    for (_, gap, hold) in sched {
        t += gap;
        lys.push((t % TCYCLES_PER_FRAME) / TCYCLES_PER_LINE);
        t += hold;
    }
    let mut unique = lys.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        8,
        "press instants must hit 8 distinct LY lines, got {lys:?}"
    );
    // Holds must be long enough for the ROM to display the press (a few
    // frames, like a human tap) and gaps must be irregular (all distinct).
    for (_, gap, hold) in sched {
        assert!(hold >= 2 * TCYCLES_PER_FRAME, "hold too short: {hold}");
        assert!(gap >= TCYCLES_PER_LINE, "gap too short: {gap}");
    }
    let mut gaps: Vec<u64> = sched.iter().map(|(_, g, _)| *g).collect();
    gaps.sort_unstable();
    gaps.dedup();
    assert_eq!(gaps.len(), 8, "gaps must be pairwise distinct");
}
