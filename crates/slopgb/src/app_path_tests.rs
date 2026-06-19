use super::*;
use crate::windows::options::BootromSlot;

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
