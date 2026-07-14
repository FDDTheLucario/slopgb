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

/// A 32 KiB ROM-only cart flagged SGB-enhanced (header `$0146 = 0x03`).
fn sgb_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03; // SGB flag
    rom[0x14B] = 0x33; // old licensee code (SGB requires it)
    rom
}

/// Drive a 16-byte SGB command packet through the joypad (`FF00` pulses), per
/// the SGB command protocol (Pan Docs "SGB Command Packet").
fn send_sgb_packet(gb: &mut GameBoy, data: &[u8; 16]) {
    gb.debug_write(0xFF00, 0x30);
    gb.debug_write(0xFF00, 0x00);
    gb.debug_write(0xFF00, 0x30);
    for &byte in data {
        for bit in 0..8 {
            gb.debug_write(0xFF00, if byte >> bit & 1 != 0 { 0x10 } else { 0x20 });
            gb.debug_write(0xFF00, 0x30);
        }
    }
    gb.debug_write(0xFF00, 0x20);
    gb.debug_write(0xFF00, 0x30);
}

/// Peak amplitude of drained stereo PCM.
fn peak(out: &[(f32, f32)]) -> f32 {
    out.iter()
        .fold(0.0f32, |m, &(l, r)| m.max(l.abs()).max(r.abs()))
}

/// A bare SGB SOUND ($08) packet, effect A = note 0x40 (trigger defaults on).
fn sound_packet() -> [u8; 16] {
    let mut packet = [0u8; 16];
    packet[0] = 0x08 * 8 + 1; // command $08, length 1
    packet[1] = 0x40;
    packet
}

/// Run `frames` and collect every drained sample's peak.
fn play_and_peak(gb: &mut GameBoy, frames: u32) -> f32 {
    let mut out = Vec::new();
    for _ in 0..frames {
        gb.run_frame();
        gb.drain_audio(&mut out);
    }
    peak(&out)
}

/// Build the two SGB coprocessor plugin crates for `wasm32` and drop them into
/// `dir` as `spc700.wasm` + `w65c816.wasm` (the names [`Session`] loads). `false`
/// if the wasm target / build is unavailable (the caller then skips). Shares the
/// per-plugin temp target dir with the `slopgb-sgb-coprocessor` tests, so the
/// wasm build is cached across both suites.
fn build_sgb_plugins(dir: &Path) -> bool {
    for (pkg, stem, out) in [
        (
            "slopgb-spc700-plugin",
            "slopgb_spc700_plugin",
            "spc700.wasm",
        ),
        (
            "slopgb-w65c816-plugin",
            "slopgb_w65c816_plugin",
            "w65c816.wasm",
        ),
    ] {
        let manifest = format!("{}/../{pkg}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
        let target = std::env::temp_dir().join(format!("slopgb-sgb-cop-{stem}"));
        let ok = process::Command::new(env!("CARGO"))
            .args([
                "build",
                "--release",
                "--target",
                "wasm32-unknown-unknown",
                "--manifest-path",
                &manifest,
            ])
            .env("CARGO_TARGET_DIR", &target)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return false;
        }
        let wasm = target.join(format!("wasm32-unknown-unknown/release/{stem}.wasm"));
        if fs::copy(&wasm, dir.join(out)).is_err() {
            return false;
        }
    }
    true
}

#[test]
fn sgb_coprocessor_toggle_swaps_the_audio_backend() {
    let dir = scratch("sgb-coprocessor");
    let path = dir.join("game.gb");
    fs::write(&path, sgb_rom()).unwrap();

    // Default (built-in HLE APU): a bare SOUND command with no game driver / BIOS
    // makes no real audio (the default sound bank isn't present) — the golden-safe
    // default. The HLE path leaves only a noise-floor residue, far below a tone.
    let mut off = Session::load(&path, ModelChoice::Sgb, &BootSpec::NONE, None).expect("load");
    assert_eq!(off.gb.model(), Model::Sgb);
    send_sgb_packet(&mut off.gb, &sound_packet());
    let off_peak = play_and_peak(&mut off.gb, 16);
    assert!(
        off_peak < 1e-3,
        "default built-in backend makes no tone for a bare SOUND command (peak {off_peak})"
    );

    // Coprocessor selected but no plugin directory set: the load fails and the
    // built-in APU stands (the golden-safe fallback) — no panic, still silent.
    let mut nodir = Session::load(&path, ModelChoice::Sgb, &BootSpec::NONE, None).expect("load");
    nodir.set_sgb_coprocessor(true);
    send_sgb_packet(&mut nodir.gb, &sound_packet());
    assert!(
        play_and_peak(&mut nodir.gb, 16) < 1e-3,
        "no plugin directory falls back to the silent built-in backend"
    );

    // With the two plugins present in a directory: the coprocessor loads them and
    // the same SOUND command drives the clean-room 65C816 -> SPC700 -> S-DSP chain
    // to audible PCM through the public frontend path. Skips if wasm is unavailable.
    if !build_sgb_plugins(&dir) {
        eprintln!("skipping coprocessor-injection half: wasm32 build unavailable");
        let _ = fs::remove_dir_all(&dir);
        return;
    }
    let mut on = Session::load(&path, ModelChoice::Sgb, &BootSpec::NONE, None).expect("load");
    on.set_sgb_coprocessor_dir(Some(dir.clone()));
    on.set_sgb_coprocessor(true);
    send_sgb_packet(&mut on.gb, &sound_packet());
    let on_peak = play_and_peak(&mut on.gb, 16);
    assert!(
        on_peak > 1e-2 && on_peak > off_peak * 50.0,
        "the injected coprocessor makes a bare SOUND command audible (peak {on_peak} vs {off_peak})"
    );

    // The choice survives a power-cycle (re-injected into the fresh machine from
    // the kept directory).
    on.reset();
    send_sgb_packet(&mut on.gb, &sound_packet());
    assert!(
        play_and_peak(&mut on.gb, 16) > 1e-2,
        "reset re-injects the coprocessor backend"
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
