use super::*;
use slopgb_core::Model;
use std::process;

#[test]
fn boot_size_ok_matches_model_class() {
    assert!(boot_size_ok(Model::Dmg, 0x100), "DMG accepts 256 B");
    assert!(!boot_size_ok(Model::Dmg, 0x900), "DMG rejects 2304 B");
    assert!(boot_size_ok(Model::Cgb, 0x900), "CGB accepts 2304 B");
    assert!(!boot_size_ok(Model::Cgb, 0x100), "CGB rejects 256 B");
    assert!(!boot_size_ok(Model::Dmg, 123), "junk size rejected");
}

#[test]
fn boot_spec_resolves_by_model_and_falls_back() {
    let dir = std::env::temp_dir().join(format!("slopgb-bootspec-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    let dmg = dir.join("dmg.bin");
    fs::write(&dmg, vec![0u8; 0x100]).unwrap(); // size-valid DMG boot ROM
    let dmg_str = dmg.to_string_lossy().into_owned();
    let fallback = vec![1u8; 0x100];

    let spec = BootSpec {
        enabled: true,
        dmg: &dmg_str,
        gbc: "",
        sgb: "",
        fallback: Some(&fallback),
    };
    // Enabled + a size-valid DMG slot for a DMG model → those bytes.
    assert_eq!(spec.resolve(Model::Dmg).unwrap().len(), 0x100);
    // CGB model, gbc slot empty → the CLI/env fallback.
    assert_eq!(spec.resolve(Model::Cgb), Some(fallback.clone()));
    // Disabled → fallback regardless of the slot paths.
    let off = BootSpec {
        enabled: false,
        ..BootSpec::cli(Some(&fallback))
    };
    assert_eq!(off.resolve(Model::Dmg), Some(fallback.clone()));
    // No fallback + no boot → None (the default golden path).
    assert_eq!(BootSpec::NONE.resolve(Model::Dmg), None);

    // A slot path that exists but is the WRONG SIZE is skipped (logged), then
    // falls through to the fallback — not a hard error, not the bad bytes.
    let wrong = dir.join("wrong.bin");
    fs::write(&wrong, vec![0u8; 0x900]).unwrap(); // 2304 B is wrong for DMG
    let wrong_str = wrong.to_string_lossy().into_owned();
    let bad_size = BootSpec {
        enabled: true,
        dmg: &wrong_str,
        gbc: "",
        sgb: "",
        fallback: Some(&fallback),
    };
    assert_eq!(
        bad_size.resolve(Model::Dmg),
        Some(fallback.clone()),
        "wrong-size Options slot falls back to the CLI/env boot ROM"
    );
    // ...and with no fallback, a wrong-size slot resolves to None (no boot).
    let bad_no_fallback = BootSpec {
        fallback: None,
        ..bad_size
    };
    assert_eq!(bad_no_fallback.resolve(Model::Dmg), None);

    // A slot path that doesn't exist (read error) likewise falls back.
    let missing = BootSpec {
        dmg: "/nonexistent/slopgb/bootrom.bin",
        ..bad_size
    };
    assert_eq!(missing.resolve(Model::Dmg), Some(fallback.clone()));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_gb_executes_a_matching_boot_rom_else_falls_back() {
    let rom = vec![0u8; 0x8000];
    let dmg_boot = vec![0u8; 0x100]; // size-valid (contents irrelevant here)
    // Right size for the model → executes the boot ROM (boot_active).
    let gb = build_gb(Model::Dmg, rom.clone(), Some(&dmg_boot)).unwrap();
    assert!(gb.boot_active(), "matching boot ROM is executed");
    // Wrong size → falls back to the direct post-boot install (logged).
    let gb = build_gb(Model::Dmg, rom.clone(), Some(&vec![0u8; 0x900])).unwrap();
    assert!(!gb.boot_active(), "wrong-size boot ROM ignored");
    // None → no boot ROM (the default golden path).
    let gb = build_gb(Model::Dmg, rom, None).unwrap();
    assert!(!gb.boot_active());
}

#[test]
fn reset_reruns_the_configured_boot_rom() {
    let dir = std::env::temp_dir().join(format!("slopgb-test-reset-boot-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("game.gb");
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x00; // ROM ONLY
    fs::write(&path, &rom).unwrap();
    // A size-valid (all-NOP) DMG boot ROM: it never writes FF50, so `boot_active`
    // stays true — exactly what lets us observe whether the boot ROM is running.
    let boot = vec![0u8; 0x100];

    // No boot configured: a power-cycle replays the post-boot state.
    let mut plain = Session::load(&path, Some(Model::Dmg), &BootSpec::NONE).expect("load");
    assert!(!plain.gb.boot_active());
    plain.reset();
    assert!(
        !plain.gb.boot_active(),
        "no boot ROM → reset stays post-boot"
    );

    // Boot ROM configured: the initial load AND a later reset both run it.
    let mut s = Session::load(&path, Some(Model::Dmg), &BootSpec::cli(Some(&boot))).expect("load");
    assert!(s.gb.boot_active(), "boot ROM runs on the initial load");
    s.gb.run_frame();
    s.reset();
    assert!(
        s.gb.boot_active(),
        "reset re-runs the boot ROM (power-cycle), not the post-boot replay"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn blank_session_has_no_rom_and_no_battery() {
    // The no-ROM startup machine: a valid blank GameBoy, empty title, no battery
    // RAM (so flush_save is a pure no-op — no stray file, no panic), no snapshot.
    let mut s = Session::blank(Model::Dmg);
    assert_eq!(s.title, "");
    assert_eq!(s.gb.model(), Model::Dmg);
    assert!(
        s.gb.save_data().is_none(),
        "blank ROM-only cart has no battery"
    );
    s.flush_save(); // must not panic / write anything
    // A blank machine can still be quick-saved/loaded like any other (used by
    // the State menu); it just starts with no snapshot.
    assert!(!s.quick_load(), "no snapshot until the first quick_save");
}

#[test]
fn cart_info_lines_parse_the_header() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x134..0x13B].copy_from_slice(b"POKEMON"); // title
    rom[0x143] = 0xC0; // CGB only
    rom[0x147] = 0x13; // MBC3+RAM+BATTERY
    rom[0x148] = 0x05; // 32 KiB << 5 = 1 MiB
    rom[0x149] = 0x03; // 32 KiB RAM
    let l = cart_info_lines(&rom);
    assert_eq!(l[0], "title: POKEMON");
    assert!(l[1].contains("13 MBC3"), "{}", l[1]);
    assert_eq!(l[2], "rom:   1024 KiB");
    assert_eq!(l[3], "ram:   32 KiB");
    assert_eq!(l[4], "cgb:   CGB only");
    // A too-small ROM doesn't panic.
    assert_eq!(cart_info_lines(&[0u8; 4]).len(), 1);
}

#[test]
fn set_model_reloads_only_on_change() {
    let dir = std::env::temp_dir().join(format!("slopgb-test-model-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("game.gb");
    let mut rom = vec![0u8; 0x8000]; // 32 KiB ROM-only cart
    rom[0x147] = 0x00; // ROM ONLY
    rom[0x148] = 0x00; // 32 KiB
    fs::write(&path, &rom).unwrap();

    let mut s = Session::load(&path, Some(Model::Dmg), &BootSpec::NONE).expect("load");
    assert_eq!(s.gb.model(), Model::Dmg);
    // Switching to CGB rebuilds the machine.
    assert!(s.set_model(Some(Model::Cgb)));
    assert_eq!(s.gb.model(), Model::Cgb);
    // Re-applying the same model is a no-op.
    assert!(!s.set_model(Some(Model::Cgb)));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn save_state_round_trips_through_a_file() {
    let dir = std::env::temp_dir().join(format!("slopgb-test-state-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    let rom_path = dir.join("game.gb");
    let state_path = dir.join("game.state");
    let mut rom = vec![0u8; 0x8000];
    rom[0x134..0x13B].copy_from_slice(b"STATEST");
    rom[0x147] = 0x00; // ROM ONLY
    fs::write(&rom_path, &rom).unwrap();

    // Run a while, then save to disk.
    let mut s = Session::load(&rom_path, Some(Model::Dmg), &BootSpec::NONE).expect("load");
    for _ in 0..20 {
        s.gb.run_frame();
    }
    let pc = s.gb.cpu_regs().pc;
    let cyc = s.gb.cycles();
    s.save_state_to(&state_path).expect("save state");

    // A fresh same-ROM session restores to the exact saved machine.
    let mut s2 = Session::load(&rom_path, Some(Model::Dmg), &BootSpec::NONE).expect("reload");
    s2.load_state_from(&state_path).expect("load state");
    assert_eq!(s2.gb.cpu_regs().pc, pc);
    assert_eq!(s2.gb.cycles(), cyc);

    // A non-existent path is a non-fatal error (machine intact).
    let before = s2.gb.cycles();
    assert!(s2.load_state_from(&dir.join("nope.state")).is_err());
    assert_eq!(
        s2.gb.cycles(),
        before,
        "failed load leaves the machine intact"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn atomic_write_replaces_existing_file() {
    // Per-process directory so concurrent test runs can't race on it.
    let dir = std::env::temp_dir().join(format!("slopgb-test-sav-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("game.sav");
    write_atomic(&path, b"first").unwrap();
    write_atomic(&path, b"second").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second");
    assert!(!path.with_extension("sav.tmp").exists());
    let _ = fs::remove_dir_all(&dir);
}
