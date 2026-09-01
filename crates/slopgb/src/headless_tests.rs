// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

use super::*;
use slopgb_core::Model;
use slopgb_plugin_host::PluginRegistry;
use std::process;

/// Per-process scratch dir (concurrent test runs can't collide).
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slopgb-headless-{tag}-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A minimal ROM-only cart (no battery), just big enough for a valid header.
fn rom_only() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x00; // ROM ONLY
    rom
}

/// A 32 KiB MBC1+RAM+BATTERY cart (8 KiB SRAM), so `gb:save_battery` /
/// `gb:load_battery` have a battery to exercise.
fn battery_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x03; // MBC1+RAM+BATTERY
    rom[0x149] = 0x02; // 8 KiB RAM
    rom
}

/// A 32 KiB MBC3+TIMER+RAM+BATTERY cart (8 KiB SRAM + RTC), for `gb:set_rtc`.
fn rtc_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x10; // MBC3+TIMER+RAM+BATTERY
    rom[0x149] = 0x02; // 8 KiB RAM
    rom
}

/// A blank (no-ROM) [`Machine`], matching what `main` hands the runner: a
/// headless run's `opts.rom` is always `None`, so a script's `gb:load_rom`
/// replaces this.
fn blank_machine() -> Machine {
    Machine {
        session: Session::blank(Model::Dmg),
        syms: SymbolTable::default(),
        boot_rom: None,
        sgb_bios: None,
        plugins_dir: None,
        ram_init: None,
    }
}

/// Rust's own extraction of the ```-fenced block under "## Worked example" in
/// `docs/headless-plan.md`, so the checked-in fixture and the plan can never
/// silently drift.
fn extract_worked_example(md: &str) -> String {
    let mut lines = md.lines();
    for l in lines.by_ref() {
        if l == "## Worked example" {
            break;
        }
    }
    for l in lines.by_ref() {
        if l == "```" {
            break;
        }
    }
    let mut out = String::new();
    for l in lines.by_ref() {
        if l == "```" {
            break;
        }
        out.push_str(l);
        out.push('\n');
    }
    out
}

#[test]
fn worked_example_fixture_matches_the_plan() {
    let fixture = include_str!("headless/worked_example.slp");
    let plan = include_str!("../../../docs/headless-plan.md");
    assert_eq!(fixture, extract_worked_example(plan));
}

#[test]
fn worked_example_parses_against_the_registered_table() {
    let mc = Rc::new(RefCell::new(blank_machine()));
    let registry = PluginRegistry::new();
    let mut interp = build_interp(&mc, &registry);
    if let Err(e) = interp.parse_only(include_str!("headless/worked_example.slp")) {
        panic!("worked_example.slp failed to parse against the registered gb:/const table: {e}");
    }
}

#[test]
fn end_to_end_run_on_a_synthetic_rom() {
    let dir = scratch("e2e");
    let rom_path = dir.join("game.gb");
    fs::write(&rom_path, battery_rom()).unwrap();
    let sav_path = dir.join("out.sav");
    let src = r#"
gb:load_rom(argv[1])
gb:poke(0, 0xA000, 0x42)
check(gb:read(0, 0xA000) == 0x42, "poke round-trip")
gb:save_battery(argv[2])
"#;
    let argv = vec![
        rom_path.to_string_lossy().into_owned(),
        sav_path.to_string_lossy().into_owned(),
    ];
    let registry = PluginRegistry::new();
    let outcome = run_outcome(blank_machine(), src, argv, &registry);
    assert_eq!(outcome, Outcome::Ok { checks: 1 });
    let saved = fs::read(&sav_path).expect("gb:save_battery should have written the file");
    assert_eq!(saved[0], 0x42);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn headless_does_not_auto_restore_the_rom_sav() {
    let dir = scratch("nosav");
    let rom_path = dir.join("game.gb");
    fs::write(&rom_path, battery_rom()).unwrap();
    let sav_path = rom_path.with_extension("sav");
    // A pattern the power-on default (SRAM fills 0xFF) can never produce.
    fs::write(&sav_path, vec![0xAB; 0x2000]).unwrap();
    let registry = PluginRegistry::new();

    let outcome = run_outcome(
        blank_machine(),
        r#"
gb:load_rom(argv[1])
check(gb:read(0, 0xA000) == 0xFF, "power-on SRAM, not the .sav pattern")
"#,
        vec![rom_path.to_string_lossy().into_owned()],
        &registry,
    );
    assert_eq!(outcome, Outcome::Ok { checks: 1 });

    let outcome = run_outcome(
        blank_machine(),
        r#"
gb:load_rom(argv[1])
gb:load_battery(argv[2])
check(gb:read(0, 0xA000) == 0xAB, "explicit gb:load_battery brings the pattern in")
"#,
        vec![
            rom_path.to_string_lossy().into_owned(),
            sav_path.to_string_lossy().into_owned(),
        ],
        &registry,
    );
    assert_eq!(outcome, Outcome::Ok { checks: 1 });
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn exit_codes_cover_every_case() {
    let registry = PluginRegistry::new();

    // A failing `check` -> Failed (exit 1), never Error.
    let outcome = run_outcome(
        blank_machine(),
        r#"check(1 == 2, "nope")"#,
        vec![],
        &registry,
    );
    assert!(matches!(outcome, Outcome::Failed { .. }));
    assert_eq!(exit_code(outcome), 1);

    // A failing `assert` -> Failed (exit 1), never Error.
    let outcome = run_outcome(
        blank_machine(),
        r#"assert(1 == 2, "nope")"#,
        vec![],
        &registry,
    );
    assert!(matches!(outcome, Outcome::Failed { .. }));
    assert_eq!(exit_code(outcome), 1);

    // A parse error (`while` outside any `within`) -> Error (exit 2).
    let outcome = run_outcome(blank_machine(), "while true { }", vec![], &registry);
    assert!(matches!(outcome, Outcome::Error(_)));
    assert_eq!(exit_code(outcome), 2);

    // An unknown symbol -> a runtime error (exit 2), a script bug, not a
    // check failure.
    let outcome = run_outcome(
        blank_machine(),
        r#"gb:read("nonexistent_symbol")"#,
        vec![],
        &registry,
    );
    assert!(matches!(outcome, Outcome::Error(_)));
    assert_eq!(exit_code(outcome), 2);

    // A clean run with zero checks -> Ok (exit 0), silent.
    let outcome = run_outcome(blank_machine(), "let x = 1", vec![], &registry);
    assert_eq!(outcome, Outcome::Ok { checks: 0 });
    assert_eq!(exit_code(outcome), 0);

    // An unreadable script path -> exit 2 (testing `--headless -` stdin is
    // not worth the harness machinery, so it's skipped here).
    let missing = PathBuf::from("/nonexistent/slopgb-headless/case.slp");
    assert_eq!(run(blank_machine(), &missing, vec![], &registry), 2);
}

#[test]
fn failure_summary_omits_the_line_when_no_checks_ran() {
    // An aborting `assert` never increments `checks`, so a run whose only
    // failure is an assert must print nothing (the assert already printed
    // its own line via slopscript).
    assert_eq!(failure_summary(0, 1), None);
    assert_eq!(
        failure_summary(2, 1),
        Some("1 of 2 checks failed".to_string())
    );
}

#[test]
fn set_rtc_round_trips_the_decomposed_registers() {
    let dir = scratch("rtc");
    let rom_path = dir.join("game.gb");
    fs::write(&rom_path, rtc_rom()).unwrap();
    let epoch: i64 = 1_767_225_600; // the plan's worked example, for good measure
    let src = format!("gb:load_rom(argv[1])\ngb:set_rtc({epoch})\n");

    let registry = PluginRegistry::new();
    let mc = Rc::new(RefCell::new(blank_machine()));
    let mut interp = build_interp(&mc, &registry);
    interp.set_argv(vec![rom_path.to_string_lossy().into_owned()]);
    assert_eq!(interp.run(&src), Outcome::Ok { checks: 0 });

    // Expected values computed independently of the implementation.
    let epoch = epoch as u64;
    let s = (epoch % 60) as u8;
    let mi = (epoch / 60 % 60) as u8;
    let h = (epoch / 3600 % 24) as u8;
    let days = epoch / 86400;
    let dl = (days & 0xFF) as u8;
    let mut dh = ((days >> 8) & 1) as u8;
    if days > 511 {
        dh |= 0x80;
    }
    let expected = [s, mi, h, dl, dh];

    let (live, latched) = mc.borrow().session.gb.rtc_state().expect("RTC cart");
    assert_eq!(live, expected);
    assert_eq!(latched, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_and_poke_round_trip_every_shape() {
    let dir = scratch("memshapes");
    let rom_path = dir.join("game.gb");
    fs::write(&rom_path, battery_rom()).unwrap();
    let sym_path = dir.join("game.sym");
    fs::write(&sym_path, "00:A010 wFlag\n00:A020 wTable\n").unwrap();

    let src = r#"
gb:load_rom(argv[1])
gb:load_symbols(argv[2])

gb:poke(0, 0xA000, 0x11)
check(gb:read(0, 0xA000) == 0x11, "bank/addr single byte")

gb:poke(0, 0xA001, [1, 2, 3, 4])
check(gb:read(0, 0xA001, 4) == [1, 2, 3, 4], "bank/addr byte run")

gb:poke("wFlag", 0x99)
check(gb:read("wFlag") == 0x99, "symbol single byte")

gb:poke("wTable", [5, 6, 7])
check(gb:read("wTable", 3) == [5, 6, 7], "symbol byte run")
"#;
    let registry = PluginRegistry::new();
    let outcome = run_outcome(
        blank_machine(),
        src,
        vec![
            rom_path.to_string_lossy().into_owned(),
            sym_path.to_string_lossy().into_owned(),
        ],
        &registry,
    );
    assert_eq!(outcome, Outcome::Ok { checks: 4 });
    let _ = fs::remove_dir_all(&dir);
}

/// `gb:read`/`gb:poke`'s multi-byte shape must not silently read/write past
/// the region the starting address is in — `run_len` used to guard only the
/// `0xFFFF` wrap, so a run starting in ROMX (0x4000-0x7FFF) could carry on
/// into VRAM undetected.
#[test]
fn multi_byte_run_crossing_a_region_boundary_is_a_runtime_error() {
    let dir = scratch("regioncross");
    let rom_path = dir.join("game.gb");
    fs::write(&rom_path, rom_only()).unwrap();
    let registry = PluginRegistry::new();

    // 0x7FFE..=0x8005 starts in ROMX (ends 0x7FFF) and runs into VRAM.
    let outcome = run_outcome(
        blank_machine(),
        r#"
gb:load_rom(argv[1])
gb:read(1, 0x7FFE, 8)
"#,
        vec![rom_path.to_string_lossy().into_owned()],
        &registry,
    );
    assert!(
        matches!(outcome, Outcome::Error(_)),
        "expected a runtime error (exit 2) crossing ROMX into VRAM, got {outcome:?}"
    );

    // The in-region case (same read, sized to stay inside ROMX) still works.
    let outcome = run_outcome(
        blank_machine(),
        r#"
gb:load_rom(argv[1])
let b = gb:read(1, 0x7FFE, 2)
check(b == [0, 0], "in-region multi-byte read still works")
"#,
        vec![rom_path.to_string_lossy().into_owned()],
        &registry,
    );
    assert_eq!(outcome, Outcome::Ok { checks: 1 });
    let _ = fs::remove_dir_all(&dir);
}

/// A ROM-only cart (no battery) parses and runs fine off a headless script
/// that never touches SRAM — confirms `gb:load_rom` alone doesn't require a
/// battery.
#[test]
fn rom_only_cart_runs_with_no_battery_ops() {
    let dir = scratch("romonly");
    let rom_path = dir.join("game.gb");
    fs::write(&rom_path, rom_only()).unwrap();
    let registry = PluginRegistry::new();
    let outcome = run_outcome(
        blank_machine(),
        r#"
gb:load_rom(argv[1])
gb:wait_frames(2)
check(gb:reg(PC) >= 0, "pc reads back")
"#,
        vec![rom_path.to_string_lossy().into_owned()],
        &registry,
    );
    assert_eq!(outcome, Outcome::Ok { checks: 1 });
    let _ = fs::remove_dir_all(&dir);
}
