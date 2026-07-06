use super::*;

#[test]
fn gameshark_decodes_like_bgb() {
    // bgb renders 01FF0AC1 as (C10A)=FF (address little-endian).
    assert_eq!(parse_code("01FF0AC1"), Some(Effect::Ram { addr: 0xC10A, value: 0xFF }));
    // Lowercase + whitespace tolerated.
    assert_eq!(parse_code(" 0100 00c0 "), Some(Effect::Ram { addr: 0xC000, value: 0x00 }));
    // Non-01 GameShark type unsupported (no poke).
    assert_eq!(parse_code("9012C0DE"), None);
    // Garbage / wrong length (7 hex matches no format).
    assert_eq!(parse_code("nothex!!"), None);
    assert_eq!(parse_code("01FF0A1"), None, "7 hex matches no format");
}

#[test]
fn game_genie_recognized_but_not_applied() {
    assert_eq!(parse_code("ABC-DEF-123"), Some(Effect::RomPatch));
    assert_eq!(parse_code("ABCDEF"), Some(Effect::RomPatch), "6-hex GG");
}

#[test]
fn list_add_edit_remove_toggle() {
    let mut list = CheatList::default();
    assert!(list.is_empty());
    let i = list.add("infinite lives", "01FF0AC1");
    assert_eq!(i, 0);
    assert_eq!(list.len(), 1);
    assert!(list.items()[0].enabled, "new cheat enabled");
    list.edit(0, "inf lives", "0163C1C1");
    assert_eq!(list.items()[0].comment, "inf lives");
    assert!(!list.toggle(0), "toggle off");
    assert!(list.toggle(0), "toggle on");
    list.remove(0);
    assert!(list.is_empty());
    // Out-of-range ops are no-ops, not panics.
    list.edit(5, "x", "y");
    list.remove(5);
    assert!(!list.toggle(5));
}

#[test]
fn pokes_only_enabled_gameshark_cheats() {
    let mut list = CheatList::default();
    list.add("a", "01FF0AC1"); // enabled RAM -> poke
    list.add("b", "0142 20C0"); // enabled RAM (C020=42) -> poke
    let gg = list.add("c", "ABC-DEF-123"); // GG -> no poke
    let off = list.add("d", "0199 30C0"); // will disable -> no poke
    list.set_enabled(off, false);
    let _ = gg;
    let mut pokes = list.pokes();
    pokes.sort_unstable();
    assert_eq!(pokes, vec![(0xC020, 0x42), (0xC10A, 0xFF)]);
}

#[test]
fn enable_disable_all_and_poke_once() {
    let mut list = CheatList::default();
    list.add("a", "01FF0AC1");
    list.add("b", "0142 20C0");
    list.disable_all();
    assert!(list.pokes().is_empty());
    list.enable_all();
    assert_eq!(list.pokes().len(), 2);
    // Poke-once returns the single write regardless of enabled state.
    assert_eq!(list.poke_once(0), Some((0xC10A, 0xFF)));
    assert_eq!(list.poke_once(9), None, "out of range");
}
