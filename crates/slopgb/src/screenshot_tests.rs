use super::*;

#[test]
fn bmp_header_dimensions_and_first_pixel() {
    // A 4×2 frame: top row red, bottom row blue (XRGB8888).
    let red = 0x00FF_0000u32;
    let blue = 0x0000_00FFu32;
    let frame = vec![red, red, red, red, blue, blue, blue, blue];
    let bmp = to_bmp(&frame, 4, 2);

    assert_eq!(&bmp[0..2], b"BM", "magic");
    // 4×3 = 12 bytes/row, already 4-aligned; 2 rows → 24 px bytes + 54 header.
    assert_eq!(bmp.len(), 54 + 24);
    assert_eq!(
        u32::from_le_bytes(bmp[2..6].try_into().unwrap()),
        78,
        "file size"
    );
    assert_eq!(
        u32::from_le_bytes(bmp[10..14].try_into().unwrap()),
        54,
        "pixel offset"
    );
    assert_eq!(
        i32::from_le_bytes(bmp[18..22].try_into().unwrap()),
        4,
        "width"
    );
    assert_eq!(
        i32::from_le_bytes(bmp[22..26].try_into().unwrap()),
        2,
        "height (positive = bottom-up)"
    );
    assert_eq!(
        u16::from_le_bytes(bmp[28..30].try_into().unwrap()),
        24,
        "bpp"
    );

    // Bottom-up: the first pixel row written is the frame's BOTTOM row (blue).
    // Blue XRGB 0x0000FF → BGR bytes FF 00 00.
    assert_eq!(&bmp[54..57], &[0xFF, 0x00, 0x00], "bottom row is blue");
    // The next row (frame top, red) → BGR 00 00 FF.
    let second_row = 54 + 12;
    assert_eq!(
        &bmp[second_row..second_row + 3],
        &[0x00, 0x00, 0xFF],
        "top row is red"
    );
}

#[test]
fn bmp_pads_odd_widths_to_a_4_byte_stride() {
    // width 3 → 9 bytes/row → padded to 12 (stride multiple of 4).
    let frame = vec![0u32; 3];
    let bmp = to_bmp(&frame, 3, 1);
    assert_eq!(bmp.len(), 54 + 12, "row padded from 9 to 12");
}

#[test]
fn bmp_with_a_short_frame_does_not_panic() {
    // A frame shorter than w*h emits a valid full-size BMP (missing pixels
    // black) instead of slicing out of bounds.
    let frame = vec![0x00FF_FFFFu32; 2]; // only 2 px for a 4×2 image
    let bmp = to_bmp(&frame, 4, 2);
    assert_eq!(bmp.len(), 54 + 24, "still a full 4×2 BMP");
}
