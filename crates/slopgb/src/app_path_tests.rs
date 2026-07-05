use super::*;
use crate::windows::options::BootromSlot;

#[test]
fn sym_sidecar_found_only_when_the_file_exists() {
    // Auto-load derivation: rom.with_extension("sym"), gated on exists().
    let dir = std::env::temp_dir().join(format!("slopgb_symsc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rom = dir.join("game.gb");
    std::fs::write(&rom, b"x").unwrap();
    assert_eq!(sym_sidecar(&rom), None, "no sidecar -> None");
    let sym = dir.join("game.sym");
    std::fs::write(&sym, b"").unwrap();
    assert_eq!(sym_sidecar(&rom), Some(sym), "sidecar present -> Some");
    // Extensionless ROM: game.sym for "noext" would be "noext.sym", absent.
    let noext = dir.join("noext");
    std::fs::write(&noext, b"x").unwrap();
    assert_eq!(sym_sidecar(&noext), None, "no matching sidecar -> None");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn link_connect_never_opens_a_file_picker() {
    // host:port is not a file — it must go straight to the typed modal.
    assert_eq!(pick_kind(PathPurpose::LinkConnect), PickKind::None);
}

#[test]
fn save_state_uses_the_save_dialog() {
    assert_eq!(pick_kind(PathPurpose::SaveState), PickKind::Save);
}

#[test]
fn file_purposes_use_the_open_dialog() {
    assert_eq!(pick_kind(PathPurpose::LoadRom), PickKind::Open);
    assert_eq!(pick_kind(PathPurpose::LoadState), PickKind::Open);
    assert_eq!(pick_kind(PathPurpose::SymbolFile), PickKind::Open);
    assert_eq!(
        pick_kind(PathPurpose::Bootrom(BootromSlot::Dmg)),
        PickKind::Open
    );
}
