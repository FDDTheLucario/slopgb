//! Unit tests for the gbtr/mooneye harness helpers. Split out of
//! `mod.rs` for file size; compiled as `super::tests` via the `#[path]`.

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
        None => skip_or_fail_gbtr(
            "rom_discovery_points_at_gbtr_collection",
            &format!("test-roms/{GBTR_DIR} not present"),
        ),
    }
}
