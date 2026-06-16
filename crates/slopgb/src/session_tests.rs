use super::*;
use slopgb_core::Model;
use std::process;

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

    let mut s = Session::load(&path, Some(Model::Dmg)).expect("load");
    assert_eq!(s.gb.model(), Model::Dmg);
    // Switching to CGB rebuilds the machine.
    assert!(s.set_model(Some(Model::Cgb)));
    assert_eq!(s.gb.model(), Model::Cgb);
    // Re-applying the same model is a no-op.
    assert!(!s.set_model(Some(Model::Cgb)));
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
