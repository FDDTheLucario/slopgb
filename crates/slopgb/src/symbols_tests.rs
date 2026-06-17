use super::*;

#[test]
fn parse_reads_bank_addr_name_and_skips_junk() {
    let t = SymbolTable::parse(
        "00:4000 Reset\n\
         ; a whole-line comment\n\
         \n\
         01:7FFF Foo ; trailing comment\n\
         [sections]\n\
         garbage line here\n\
         FF:zz not-hex-addr\n\
         8000 BareAddr\n",
    );
    // Two banked + one bare-address symbol parsed; junk skipped.
    assert_eq!(t.len(), 3);
    assert_eq!(t.name_at(0x4000), Some("Reset"));
    assert_eq!(t.name_at(0x7FFF), Some("Foo"));
    assert_eq!(t.name_at(0x8000), Some("BareAddr")); // bare addr -> bank 0
    assert!(t.name_at(0x1234).is_none());
}

#[test]
fn empty_input_is_empty() {
    let t = SymbolTable::parse("\n; only comments\n\n");
    assert!(t.is_empty());
    assert_eq!(t.len(), 0);
    assert!(t.name_at(0).is_none());
    assert!(t.resolve("x").is_none());
}

#[test]
fn lookups_find_exact_nearest_and_by_name() {
    let t = SymbolTable::parse("00:4000 Reset\n00:4010 Loop\n00:4030 End");
    // Exact address lookup.
    assert_eq!(t.name_at(0x4010), Some("Loop"));
    assert_eq!(t.name_at(0x4008), None);
    // Nearest preceding (for the memory-window status bar).
    assert_eq!(t.nearest_before(0x4008), Some(("Reset", 0x4000)));
    assert_eq!(t.nearest_before(0x4010), Some(("Loop", 0x4010)));
    assert_eq!(t.nearest_before(0x402F), Some(("Loop", 0x4010)));
    assert_eq!(t.nearest_before(0x3FFF), None);
    // By name, case-insensitive.
    assert_eq!(t.resolve("loop"), Some(0x4010));
    assert_eq!(t.resolve("END"), Some(0x4030));
    assert_eq!(t.resolve("nope"), None);
}

#[test]
fn parse_sorts_out_of_order_input() {
    let t = SymbolTable::parse("00:4030 End\n00:4000 Reset\n00:4010 Loop");
    // Regardless of file order, addresses resolve correctly.
    assert_eq!(t.name_at(0x4010), Some("Loop"));
    assert_eq!(t.name_at(0x4030), Some("End"));
    assert_eq!(t.resolve("reset"), Some(0x4000));
}
