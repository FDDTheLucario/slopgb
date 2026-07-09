use super::*;

#[test]
fn bare_form_parses_unbanked_regions() {
    assert_eq!(
        parse_one("02BF").unwrap(),
        Addr {
            bank: 0,
            addr: 0x02BF
        }
    );
    assert_eq!(
        parse_one("C123").unwrap(),
        Addr {
            bank: 0,
            addr: 0xC123
        }
    );
    assert_eq!(
        parse_one("FF40").unwrap(),
        Addr {
            bank: 0,
            addr: 0xFF40
        }
    );
}

#[test]
fn banked_form_parses_banked_regions() {
    assert_eq!(
        parse_one("03:7FFF").unwrap(),
        Addr {
            bank: 3,
            addr: 0x7FFF
        }
    );
    assert_eq!(
        parse_one("01:8000").unwrap(),
        Addr {
            bank: 1,
            addr: 0x8000
        }
    );
    assert_eq!(
        parse_one("02:A000").unwrap(),
        Addr {
            bank: 2,
            addr: 0xA000
        }
    );
    assert_eq!(
        parse_one("05:D000").unwrap(),
        Addr {
            bank: 5,
            addr: 0xD000
        }
    );
}

#[test]
fn banked_region_rejects_bare_form() {
    // ROMX/VRAM/SRAM/WRAMX need a bank.
    assert!(parse_one("7FFF").is_err());
    assert!(parse_one("8000").is_err());
    assert!(parse_one("A000").is_err());
    assert!(parse_one("D000").is_err());
}

#[test]
fn unbanked_region_rejects_banked_form() {
    // ROM0/WRAM0/echo take no bank.
    assert!(parse_one("00:02BF").is_err());
    assert!(parse_one("00:C000").is_err());
    assert!(parse_one("00:FF40").is_err());
}

#[test]
fn sram_takes_banked_form() {
    // Cart SRAM banks with the mapper — BB:AAAA like the other banked regions.
    assert_eq!(
        parse_one("00:A000").unwrap(),
        Addr {
            bank: 0,
            addr: 0xA000
        }
    );
    assert!(parse_one("A000").is_err(), "bare form needs a bank");
}

#[test]
fn garbage_never_panics() {
    for bad in ["", "xyz", "zz:0000", "03:", ":7FFF", "10000", "03:zzzz"] {
        assert!(parse_one(bad).is_err(), "{bad:?} should error");
    }
}

#[test]
fn range_within_one_bank_ok() {
    let (a, b) = parse_range("03:7FF0", "03:7FFF").unwrap();
    assert_eq!((a.bank, a.addr, b.addr), (3, 0x7FF0, 0x7FFF));
    let (a, b) = parse_range("0100", "0150").unwrap();
    assert_eq!((a.addr, b.addr), (0x0100, 0x0150));
}

#[test]
fn range_straddling_bank_boundary_rejected() {
    // The spec's example: 03:7ff0 04:400f must be two queries.
    assert!(parse_range("03:7FF0", "04:400F").is_err());
}

#[test]
fn range_straddling_region_boundary_rejected() {
    // ROM0 into ROMX.
    assert!(parse_range("3FF0", "04:4001").is_err());
    // WRAM0 into WRAMX.
    assert!(parse_range("CFF0", "05:D001").is_err());
}

#[test]
fn range_reversed_rejected() {
    assert!(parse_range("0150", "0100").is_err());
    assert!(parse_range("03:7FFF", "03:7FF0").is_err());
}
