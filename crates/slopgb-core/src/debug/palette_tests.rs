use super::*;

#[test]
fn cgb_palette_words_reads_little_endian_per_palette() {
    let mut cram = vec![0u8; 64];
    // palette 0, colour 0 = 0x7FFF (white).
    cram[0] = 0xFF;
    cram[1] = 0x7F;
    // palette 1, colour 3 = 0x1234.
    let i = 8 + 6;
    cram[i] = 0x34;
    cram[i + 1] = 0x12;
    assert_eq!(cgb_palette_words(&cram, 0)[0], 0x7FFF);
    assert_eq!(cgb_palette_words(&cram, 1)[3], 0x1234);
    // palette 7 colour 0 lives at byte 56.
    cram[56] = 0xAD;
    cram[57] = 0xDE;
    assert_eq!(cgb_palette_words(&cram, 7)[0], 0xDEAD);
}

#[test]
fn cgb_palette_words_out_of_range_is_zero() {
    assert_eq!(cgb_palette_words(&[], 0), [0; 4]);
}

#[test]
fn dmg_palette_shades_splits_two_bits_each() {
    // 0xE4 = 11_10_01_00 -> colour0=0, 1=1, 2=2, 3=3 (the identity palette).
    assert_eq!(dmg_palette_shades(0xE4), [0, 1, 2, 3]);
    // 0x1B = 00_01_10_11 -> 3,2,1,0 (inverted).
    assert_eq!(dmg_palette_shades(0x1B), [3, 2, 1, 0]);
    assert_eq!(dmg_palette_shades(0x00), [0, 0, 0, 0]);
}

#[test]
fn rgb555_expands_endpoints_and_channels() {
    assert_eq!(rgb555_to_rgb888(0x0000), (0, 0, 0)); // black
    assert_eq!(rgb555_to_rgb888(0x7FFF), (255, 255, 255)); // white
    assert_eq!(rgb555_to_rgb888(0x001F), (255, 0, 0)); // max red (low 5 bits)
    assert_eq!(rgb555_to_rgb888(0x03E0), (0, 255, 0)); // green (bits 5-9)
    assert_eq!(rgb555_to_rgb888(0x7C00), (0, 0, 255)); // blue (bits 10-14)
}
