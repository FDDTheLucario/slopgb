use super::*;
use slopgb_core::{Model, RamInit};

use crate::windows::options::ModelChoice;
use std::process;

/// A 32 KiB MBC1+RAM+BATTERY cart (8 KiB SRAM) so `save_data`/`flush_save`
/// exercise the battery-persistence path.
fn battery_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x03; // MBC1 + RAM + BATTERY
    rom[0x149] = 0x02; // 8 KiB RAM
    rom
}

/// Per-process scratch dir (concurrent runs can't collide).
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slopgb-{tag}-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

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
    let gb = build_gb(Model::Dmg, rom.clone(), Some(&dmg_boot), false, None).unwrap();
    assert!(gb.boot_active(), "matching boot ROM is executed");
    // Wrong size → falls back to the direct post-boot install (logged).
    let gb = build_gb(
        Model::Dmg,
        rom.clone(),
        Some(&vec![0u8; 0x900]),
        false,
        None,
    )
    .unwrap();
    assert!(!gb.boot_active(), "wrong-size boot ROM ignored");
    // None → no boot ROM (the default golden path).
    let gb = build_gb(Model::Dmg, rom, None, false, None).unwrap();
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
    let mut plain = Session::load(&path, ModelChoice::Dmg, &BootSpec::NONE, None).expect("load");
    assert!(!plain.gb.boot_active());
    plain.reset();
    assert!(
        !plain.gb.boot_active(),
        "no boot ROM → reset stays post-boot"
    );

    // Boot ROM configured: the initial load AND a later reset both run it.
    let mut s =
        Session::load(&path, ModelChoice::Dmg, &BootSpec::cli(Some(&boot)), None).expect("load");
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

    let mut s = Session::load(&path, ModelChoice::Dmg, &BootSpec::NONE, None).expect("load");
    assert_eq!(s.gb.model(), Model::Dmg);
    // Switching to CGB rebuilds the machine.
    assert!(s.set_model(ModelChoice::Cgb));
    assert_eq!(s.gb.model(), Model::Cgb);
    // Re-applying the same model is a no-op.
    assert!(!s.set_model(ModelChoice::Cgb));
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
    let mut s = Session::load(&rom_path, ModelChoice::Dmg, &BootSpec::NONE, None).expect("load");
    for _ in 0..20 {
        s.gb.run_frame();
    }
    let pc = s.gb.cpu_regs().pc;
    let cyc = s.gb.cycles();
    s.save_state_to(&state_path).expect("save state");

    // A fresh same-ROM session restores to the exact saved machine.
    let mut s2 = Session::load(&rom_path, ModelChoice::Dmg, &BootSpec::NONE, None).expect("reload");
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
fn ram_init_runs_before_sav_load_no_data_loss() {
    // A battery cart with an existing .sav loaded under `--ram-init random`:
    // init_ram (seeded garbage) must run BEFORE the .sav restore, so the user's
    // real save survives. If the order were reversed the garbage would clobber
    // the .sav — silent, permanent data loss.
    let dir = scratch("raminit-order");
    let path = dir.join("game.gb");
    fs::write(&path, battery_rom()).unwrap();
    let sav = vec![0x77u8; 0x2000];
    fs::write(path.with_extension("sav"), &sav).unwrap();

    let s = Session::load(
        &path,
        ModelChoice::Dmg,
        &BootSpec::NONE,
        Some(RamInit::Random(0xDEAD_BEEF)),
    )
    .expect("load");
    assert_eq!(
        s.gb.save_data().unwrap(),
        sav,
        "the existing .sav must survive power-on RAM init (init runs first)"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn flush_save_writes_once_then_dedups_until_ram_changes() {
    let dir = scratch("flush-dedup");
    let path = dir.join("game.gb");
    fs::write(&path, battery_rom()).unwrap();
    let sav_path = path.with_extension("sav");

    let mut s = Session::load(&path, ModelChoice::Dmg, &BootSpec::NONE, None).expect("load");
    assert!(!sav_path.exists(), "no save file until the first flush");
    s.flush_save();
    assert!(sav_path.exists(), "first flush writes the battery RAM");

    // Dirty dedup: unchanged RAM must NOT be rewritten. Delete the file; a no-op
    // flush must not recreate it (a regression to always-different rewrites it).
    fs::remove_file(&sav_path).unwrap();
    s.flush_save();
    assert!(
        !sav_path.exists(),
        "unchanged RAM is not rewritten (last_saved dedup)"
    );

    // A RAM change flushes again.
    assert!(s.gb.load_save_data(&vec![0x11u8; 0x2000]));
    s.flush_save();
    assert!(sav_path.exists(), "changed RAM is written");
    assert_eq!(fs::read(&sav_path).unwrap(), vec![0x11u8; 0x2000]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn autosave_flushes_only_after_the_cadence_deadline() {
    let dir = scratch("autosave");
    let path = dir.join("game.gb");
    fs::write(&path, battery_rom()).unwrap();
    let sav_path = path.with_extension("sav");

    let mut s = Session::load(&path, ModelChoice::Dmg, &BootSpec::NONE, None).expect("load");
    assert!(s.gb.load_save_data(&vec![0x33u8; 0x2000]), "make RAM dirty");

    // Before the deadline autosave is a no-op.
    s.next_autosave = u64::MAX;
    s.autosave();
    assert!(
        !sav_path.exists(),
        "before the deadline autosave does nothing"
    );

    // At/after the deadline it flushes and re-arms the next deadline.
    s.next_autosave = s.gb.cycles();
    s.autosave();
    assert!(sav_path.exists(), "at the deadline autosave flushes");
    assert_eq!(
        s.next_autosave,
        s.gb.cycles().saturating_add(AUTOSAVE_CYCLES),
        "autosave re-arms the next cadence deadline"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn quick_load_reanchors_the_autosave_deadline() {
    let dir = scratch("quickload-anchor");
    let path = dir.join("game.gb");
    fs::write(&path, battery_rom()).unwrap();

    let mut s = Session::load(&path, ModelChoice::Dmg, &BootSpec::NONE, None).expect("load");
    s.gb.run_frame();
    s.quick_save();
    let snap_cycles = s.gb.cycles();
    for _ in 0..5 {
        s.gb.run_frame();
    }
    assert!(
        s.gb.cycles() > snap_cycles,
        "time advanced past the snapshot"
    );

    // A stale far-future deadline must be re-anchored by quick_load, else the
    // restored (earlier) machine would suppress autosave until time replays.
    s.next_autosave = u64::MAX;
    assert!(s.quick_load(), "snapshot restored");
    assert_eq!(s.gb.cycles(), snap_cycles, "cycle counter jumped back");
    assert_eq!(
        s.next_autosave,
        snap_cycles.saturating_add(AUTOSAVE_CYCLES),
        "quick_load re-anchors autosave to the restored cycle counter"
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
