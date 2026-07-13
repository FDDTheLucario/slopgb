use super::*;

#[test]
fn gameshark_decodes_like_bgb() {
    // bgb renders 01FF0AC1 as (C10A)=FF (address little-endian).
    assert_eq!(
        parse_code("01FF0AC1"),
        Some(Effect::Ram {
            addr: 0xC10A,
            value: 0xFF
        })
    );
    // Lowercase + whitespace tolerated.
    assert_eq!(
        parse_code(" 0100 00c0 "),
        Some(Effect::Ram {
            addr: 0xC000,
            value: 0x00
        })
    );
    // Non-01 GameShark type unsupported (no poke).
    assert_eq!(parse_code("9012C0DE"), None);
    // Garbage / wrong length (7 hex matches no format).
    assert_eq!(parse_code("nothex!!"), None);
    assert_eq!(parse_code("01FF0A1"), None, "7 hex matches no format");
}

#[test]
fn game_genie_decodes_like_the_standard() {
    // Standard GB Game Genie decode. 9-digit 123456789: value 0x12,
    // addr ((6^F)<<12)|0x345 = 0x9345, compare (0x79 ror2)^0xBA = 0xE4.
    assert_eq!(
        parse_code("123-456-789"),
        Some(Effect::Rom {
            addr: 0x9345,
            value: 0x12,
            compare: Some(0xE4)
        })
    );
    // 6-digit ABCDEF: value 0xAB, addr ((F^F)<<12)|0xCDE = 0x0CDE, no compare.
    assert_eq!(
        parse_code("ABCDEF"),
        Some(Effect::Rom {
            addr: 0x0CDE,
            value: 0xAB,
            compare: None
        })
    );
}

#[test]
fn gg_patches_from_enabled_game_genie_cheats() {
    let mut list = CheatList::default();
    list.add("gg", "ABCDEF"); // 6-digit -> a ROM patch
    list.add("gs", "01FF0AC1"); // GameShark -> not a GG patch
    let off = list.add("gg2", "123456789");
    list.set_enabled(off, false); // disabled -> excluded
    let p = list.gg_patches();
    assert_eq!(p.len(), 1, "only the enabled Game Genie cheat");
    assert_eq!(
        p[0],
        slopgb_core::GgPatch {
            addr: 0x0CDE,
            value: 0xAB,
            compare: None
        }
    );
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
fn cheat_file_round_trips_in_bgb_format() {
    let mut list = CheatList::default();
    list.add("infinite lives", "01FF0AC1");
    let off = list.add("gg cheat", "ABCDEF");
    list.set_enabled(off, false);
    let text = list.to_file_text();
    // bgb format: "cheat = NN" header, then "code <flag><name>".
    assert!(text.starts_with("cheat = 02\r\n"), "count header: {text:?}");
    assert!(text.contains("01FF0AC1 1infinite lives"), "enabled flag 1");
    assert!(text.contains("ABCDEF 0gg cheat"), "disabled flag 0");
    // Load into a fresh list reconstructs the same cheats.
    let mut back = CheatList::default();
    back.load_file_text(&text);
    assert_eq!(back.items(), list.items());
    // A bgb .cht: header skipped, flag digit stripped from the name.
    let mut c2 = CheatList::default();
    c2.load_file_text("cheat = 01\r\n0100C0FF 1coins\r\n");
    assert_eq!(c2.len(), 1);
    assert_eq!(c2.items()[0].comment, "coins");
    assert!(c2.items()[0].enabled);
}

#[test]
fn cheat_file_round_trips_codes_containing_spaces() {
    // parse_code accepts internal whitespace ("0142 20C0"), but the .cht line
    // format is space-delimited: a raw space in the code field splits wrong on
    // load. to_file_text must emit a whitespace-free code so the round-trip
    // preserves the cheat's effect instead of corrupting code + comment.
    let mut list = CheatList::default();
    list.add("coins", "0142 20C0"); // valid RAM cheat (C020)=42, but has a space
    let text = list.to_file_text();
    assert!(
        text.contains("014220C0 1coins"),
        "serialized code must carry no internal space: {text:?}"
    );

    let mut back = CheatList::default();
    back.load_file_text(&text);
    assert_eq!(back.len(), 1, "one cheat survives the round-trip");
    assert_eq!(back.items()[0].comment, "coins", "comment not corrupted");
    assert!(back.items()[0].enabled);
    assert_eq!(back.pokes(), vec![(0xC020, 0x42)], "effect preserved");
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
