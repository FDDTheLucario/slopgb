//! Port-semantics tests for the S-PPU memory state machines, pinned to
//! nocash *fullsnes* ("SNES Memory VRAM/CGRAM/OAM Access").

use super::*;

/// Write one VRAM word through `$2118/$2119` (increment-on-high mode
/// assumed set by the caller).
fn vram_word(ppu: &mut SnesPpu, lo: u8, hi: u8) {
    ppu.write(0x18, lo);
    ppu.write(0x19, hi);
}

/// VMAIN bit 7 picks which byte access increments the word address
/// (fullsnes 2115h/2118h): increment-on-high writes whole words
/// sequentially; increment-on-low lets `$2118` alone walk the low bytes of
/// consecutive words (the BG-map byte-update idiom).
#[test]
fn vmdata_write_increment_modes() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x15, 0x80); // increment on high, step 1
    ppu.write(0x16, 0x10); // VMADD = $0010
    ppu.write(0x17, 0x00);
    vram_word(&mut ppu, 0x34, 0x12);
    vram_word(&mut ppu, 0x78, 0x56);
    assert_eq!(ppu.vram()[0x10], 0x1234);
    assert_eq!(ppu.vram()[0x11], 0x5678);

    ppu.write(0x15, 0x00); // increment on low
    ppu.write(0x16, 0x20);
    ppu.write(0x17, 0x00);
    ppu.write(0x18, 0xAA); // -> word $20 LSB, then increment
    ppu.write(0x18, 0xBB); // -> word $21 LSB
    assert_eq!(ppu.vram()[0x20] & 0xFF, 0xAA);
    assert_eq!(ppu.vram()[0x21] & 0xFF, 0xBB);
}

/// VMAIN bits 1-0 select the increment step 1/32/128/128 (fullsnes 2115h);
/// step 32 is the BG-map column idiom. Word addresses wrap at 32 K (bit 15
/// unconnected — fullsnes 2116h).
#[test]
fn vmadd_step_and_mirror() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x15, 0x81); // increment on high, step 32
    ppu.write(0x16, 0x00);
    ppu.write(0x17, 0x00);
    vram_word(&mut ppu, 0x11, 0x00);
    vram_word(&mut ppu, 0x22, 0x00);
    assert_eq!(ppu.vram()[0x00] & 0xFF, 0x11);
    assert_eq!(ppu.vram()[0x20] & 0xFF, 0x22);

    ppu.write(0x15, 0x80);
    ppu.write(0x16, 0x05); // $8005 mirrors $0005
    ppu.write(0x17, 0x80);
    vram_word(&mut ppu, 0x77, 0x00);
    assert_eq!(ppu.vram()[0x0005] & 0xFF, 0x77);
}

/// The address-translation rotate (fullsnes 2115h): an 8-bit translation
/// thrice left-rotates the low 8 bits of the word address on access, while
/// the VMADD register itself keeps counting untranslated.
#[test]
fn vmain_address_translation_rotates_low_bits() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x15, 0x84); // increment on high, step 1, 8-bit translation
    ppu.write(0x16, 0x21); // low 8 = $21 -> rotl3 within 8 bits = $09
    ppu.write(0x17, 0x01); // high bits pass through: word $0121 -> $0109
    vram_word(&mut ppu, 0xCD, 0xAB);
    assert_eq!(ppu.vram()[0x0109], 0xABCD, "translated on access");
    // The next access sees VMADD = $0122 -> low 8 = $22 -> rotl3 = $11.
    vram_word(&mut ppu, 0x11, 0x00);
    assert_eq!(
        ppu.vram()[0x0111] & 0xFF,
        0x11,
        "VMADD kept counting untranslated"
    );
}

/// The RDVRAM prefetch glitch (fullsnes 2139h): prefetch fills AFTER an
/// address write but BEFORE the increment on reads — so the first word
/// after an address load is returned twice, then addresses run properly.
#[test]
fn rdvram_prefetch_returns_first_word_twice() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x15, 0x80);
    ppu.write(0x16, 0x40);
    ppu.write(0x17, 0x00);
    for w in [0x1111u16, 0x2222, 0x3333] {
        vram_word(&mut ppu, w as u8, (w >> 8) as u8);
    }
    ppu.write(0x16, 0x40); // reload address -> prefetch = word $40
    ppu.write(0x17, 0x00);
    let mut words = Vec::new();
    for _ in 0..4 {
        let lo = ppu.read(0x39);
        let hi = ppu.read(0x3A);
        words.push(u16::from(lo) | u16::from(hi) << 8);
    }
    assert_eq!(
        words,
        vec![0x1111, 0x1111, 0x2222, 0x3333],
        "first word twice, then sequential"
    );
}

/// CGRAM (fullsnes 2121h/2122h/213Bh): word writes land on the second
/// access of the shared flipflop, a `$2121` write resets the flipflop, the
/// second read byte masks the PPU2 open-bus bit, and the color address
/// auto-increments per completed word.
#[test]
fn cgram_word_latch_flipflop_and_readback() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x21, 3); // CGADD = color 3
    ppu.write(0x22, 0x34);
    ppu.write(0x22, 0xF2); // word = $F234 (bit 15 stored)
    ppu.write(0x22, 0x78);
    ppu.write(0x22, 0x56); // color 4 = $5678
    assert_eq!(ppu.cgram()[3], 0xF234);
    assert_eq!(ppu.cgram()[4], 0x5678);

    // A mid-pair CGADD write resets the flipflop: the dangling low byte
    // never lands.
    ppu.write(0x22, 0xEE);
    ppu.write(0x21, 3);
    assert_eq!(ppu.read(0x3B), 0x34, "1st read: low byte");
    assert_eq!(
        ppu.read(0x3B),
        0x72,
        "2nd read: high 7 bits, bit 7 open bus"
    );
    assert_eq!(ppu.read(0x3B), 0x78, "address stepped to color 4");
}

/// OAM (fullsnes 2102h-2104h/2138h): either OAMADD byte write copies the
/// whole 9-bit reload into the address (bit 0 = 0); low-table writes latch
/// even bytes and land words on odd bytes; the high table (`$200+`) takes
/// direct byte writes; `$220-$3FF` mirrors `$200-$21F`.
#[test]
fn oam_addressing_word_latch_and_high_table() {
    let mut ppu = SnesPpu::new();
    ppu.write(0x02, 0x02); // reload = 2 -> byte address 4
    ppu.write(0x03, 0x00);
    ppu.write(0x04, 0xAA); // even: memorize
    ppu.write(0x04, 0xBB); // odd: word lands at bytes 4/5
    assert_eq!(&ppu.oam()[4..6], &[0xAA, 0xBB]);

    // Rewriting $2102 re-copies the reload: the address rewinds.
    ppu.write(0x02, 0x02);
    ppu.write(0x04, 0x11);
    ppu.write(0x04, 0x22);
    assert_eq!(&ppu.oam()[4..6], &[0x11, 0x22]);

    // High table: $2103 bit 0 = reload bit 8 -> byte address $200; direct
    // byte writes, no latch.
    ppu.write(0x02, 0x00);
    ppu.write(0x03, 0x01);
    ppu.write(0x04, 0x5A);
    assert_eq!(ppu.oam()[0x200], 0x5A);

    // Reads return bytes at the live address ($201 after the write above),
    // and $220 mirrors $200.
    assert_eq!(ppu.read(0x38), 0x00, "byte $201");
    ppu.write(0x02, 0x10);
    ppu.write(0x03, 0x01); // reload = $110 -> byte address $220 -> mirrors $200
    assert_eq!(ppu.read(0x38), 0x5A, "$220 mirrors $200");

    // An even-latched byte with no odd partner never lands in the low table.
    ppu.write(0x02, 0x00);
    ppu.write(0x03, 0x00);
    ppu.write(0x04, 0x77); // memorized only
    assert_eq!(ppu.oam()[0], 0x00);
}
