//! RGB frame-vs-reference-image comparators for test-ROM suites whose
//! expected output ships as full-color reference images (decoded PNG:
//! width/height/`Vec<[u8; 3]>`), as opposed to the shade-class assets the
//! mooneye harness vendors (`compare_frame_exact_dmg` in `common/mod.rs`).
//!
//! The comparators take plain slices so they are independent of any
//! particular image decoder.

use slopgb_core::SCREEN_W;

/// How an emulator pixel is mapped into a reference image's color space
/// before comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CgbColorMap {
    /// Compare the pixel's low 24 bits as-is. This covers every suite whose
    /// reference PNGs use the c-sp collection's "common palette", which is
    /// exactly the core's output: straight 5→8 expansion `(x << 3) | (x >> 2)`
    /// for CGB colors (`Ppu::cgb_color`, src/ppu/render.rs) and the default
    /// FF/AA/55/00 greys for DMG (src/ppu/mod.rs) — the acid2s, mealybug,
    /// age, blargg, ...
    Identity,
    /// The gambatte suite's PNGs are rendered through gambatte's own
    /// CGB-to-RGB conversion; re-encode the emulator pixel the same way
    /// before comparing (see [`gambatte_rgb`]).
    Gambatte,
}

/// Re-encode one XRGB8888 emulator pixel through gambatte's CGB-to-RGB
/// conversion (gambatte-core `gbcToRgb32`):
///
/// ```text
/// r8 = (r5*13 + g5*2 + b5     ) / 2
/// g8 = (        g5*3 + b5     ) * 2
/// b8 = (r5*3  + g5*2 + b5*11  ) / 2
/// ```
///
/// integer arithmetic, maximum output 248 (for 31/31/31).
///
/// The 5-bit channels are recovered with `>> 3`. This is lossless because
/// the core's 5→8 expansion `(x << 3) | (x >> 2)` keeps the top 5 bits equal
/// to `x` (the `| (x >> 2)` part only fills the low 3 bits) —
/// `five_bit_recovery_is_lossless` proves it exhaustively.
///
/// Public for the gambatte suite's hex-screen comparator, which re-encodes
/// emulator pixels through this conversion before masking
/// (`gbtr/gambatte.rs::masked_pixel`).
pub fn gambatte_rgb(px: u32) -> u32 {
    let r5 = (px >> 19) & 0x1F;
    let g5 = (px >> 11) & 0x1F;
    let b5 = (px >> 3) & 0x1F;
    let r = (r5 * 13 + g5 * 2 + b5) / 2;
    let g = (g5 * 3 + b5) * 2;
    let b = (r5 * 3 + g5 * 2 + b5 * 11) / 2;
    (r << 16) | (g << 8) | b
}

/// Compare an emulator frame (XRGB8888, X byte ignored) against a reference
/// image's RGB triples, pixel for pixel, after mapping the emulator pixel
/// per `map` (for [`CgbColorMap::Gambatte`] the mismatch report therefore
/// shows both sides in gambatte's color space).
///
/// Errors carry the total mismatch count plus up to 8 sample pixels; a
/// frame/image length mismatch is an `Err` too, never a panic.
pub fn compare_frame_rgb(
    frame: &[u32],
    expected: &[[u8; 3]],
    map: CgbColorMap,
) -> Result<(), String> {
    if frame.len() != expected.len() {
        return Err(format!(
            "frame has {} pixels but reference image has {}",
            frame.len(),
            expected.len()
        ));
    }
    let mut mismatches = 0usize;
    let mut samples = Vec::new();
    for (i, (&px, rgb)) in frame.iter().zip(expected).enumerate() {
        let got = match map {
            CgbColorMap::Identity => px & 0x00FF_FFFF,
            CgbColorMap::Gambatte => gambatte_rgb(px),
        };
        let want = (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2]);
        if got != want {
            mismatches += 1;
            if samples.len() < 8 {
                samples.push(format!(
                    "{}: expected #{want:06X} got #{got:06X}",
                    super::pixel_coords(i, frame.len())
                ));
            }
        }
    }
    if mismatches == 0 {
        Ok(())
    } else {
        Err(format!(
            "{mismatches} pixel(s) differ from reference image: {}",
            samples.join("; ")
        ))
    }
}

/// [`compare_frame_rgb`] against a decoded reference [`png::Image`],
/// rejecting any image that is not exactly screen-shaped first — a
/// transposed or otherwise mis-sized reference with the right pixel count
/// would compare against wrong coordinates if only the length were checked.
pub fn compare_frame_image(
    frame: &[u32],
    img: &super::png::Image,
    map: CgbColorMap,
) -> Result<(), String> {
    if (img.w, img.h) != (SCREEN_W, slopgb_core::SCREEN_H) {
        return Err(format!(
            "reference image is {}x{}, want {}x{}",
            img.w,
            img.h,
            SCREEN_W,
            slopgb_core::SCREEN_H
        ));
    }
    compare_frame_rgb(frame, &img.rgb, map)
}

/// Render a frame as ASCII art for failure triage: one char per pixel, rows
/// of [`SCREEN_W`] pixels, four luminance buckets from bright to dark —
/// `' '`, `'.'`, `'o'`, `'#'`.
pub fn frame_ascii(frame: &[u32]) -> String {
    let mut out = String::with_capacity(frame.len() + frame.len().div_ceil(SCREEN_W));
    for row in frame.chunks(SCREEN_W) {
        for &px in row {
            // Channel sum is 0..=765, so sum/192 is 0..=3 — bucket
            // boundaries at per-channel averages 64/128/192.
            let lum = ((px >> 16) & 0xFF) + ((px >> 8) & 0xFF) + (px & 0xFF);
            out.push([' ', '.', 'o', '#'][(3 - lum / 192) as usize]);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use slopgb_core::{SCREEN_H, SCREEN_PIXELS};

    /// The core's 5→8 channel expansion (`Ppu::cgb_color`,
    /// src/ppu/render.rs).
    fn expand5(x: u32) -> u32 {
        (x << 3) | (x >> 2)
    }

    /// XRGB8888 pixel the core would emit for an RGB555 triple.
    fn xrgb(r5: u32, g5: u32, b5: u32) -> u32 {
        (expand5(r5) << 16) | (expand5(g5) << 8) | expand5(b5)
    }

    // --- Identity map ---

    #[test]
    fn identity_accepts_exact_match_and_ignores_x_byte() {
        let frame = [0xFFFF_FFFF, 0x12AA_AAAA, 0x0055_5555, 0x0000_0000];
        let expected = [
            [0xFF, 0xFF, 0xFF],
            [0xAA, 0xAA, 0xAA],
            [0x55, 0x55, 0x55],
            [0x00, 0x00, 0x00],
        ];
        assert_eq!(
            compare_frame_rgb(&frame, &expected, CgbColorMap::Identity),
            Ok(())
        );
    }

    #[test]
    fn identity_reports_single_diff_with_coordinates() {
        let mut frame = vec![0x00FF_FFFF; SCREEN_PIXELS];
        let expected = vec![[0xFFu8, 0xFF, 0xFF]; SCREEN_PIXELS];
        frame[2 * SCREEN_W + 3] = 0x00FF_FFFE;
        let err = compare_frame_rgb(&frame, &expected, CgbColorMap::Identity).unwrap_err();
        assert_eq!(
            err,
            "1 pixel(s) differ from reference image: (3,2): expected #FFFFFF got #FFFFFE"
        );
    }

    #[test]
    fn identity_counts_all_mismatches_but_caps_samples_at_eight() {
        let mut frame = vec![0x00FF_FFFF; SCREEN_PIXELS];
        let expected = vec![[0xFFu8, 0xFF, 0xFF]; SCREEN_PIXELS];
        for i in 0..10 {
            frame[i * 7] = 0x0012_3456;
        }
        let err = compare_frame_rgb(&frame, &expected, CgbColorMap::Identity).unwrap_err();
        assert!(err.starts_with("10 pixel(s) differ"), "{err}");
        assert_eq!(
            err.matches("expected #FFFFFF got #123456").count(),
            8,
            "{err}"
        );
    }

    // --- Gambatte map ---

    #[test]
    fn five_bit_recovery_is_lossless() {
        // gambatte_rgb relies on `r8 >> 3 == r5`; exhaustive over all 5-bit
        // values of the core's expansion.
        for x in 0..32 {
            assert_eq!(expand5(x) >> 3, x, "expansion of {x} lost its top bits");
        }
    }

    #[test]
    fn gambatte_white_maps_to_248() {
        let frame = [xrgb(31, 31, 31)];
        let expected = [[248u8, 248, 248]];
        assert_eq!(
            compare_frame_rgb(&frame, &expected, CgbColorMap::Gambatte),
            Ok(())
        );
    }

    #[test]
    fn gambatte_asymmetric_triples_match_hand_computed_values() {
        // Hand-computed through gambatte's formulae:
        //   (31, 0, 0): r=(31*13)/2=201        g=0                b=(31*3)/2=46
        //   ( 0,31, 0): r=(31*2)/2=31          g=(31*3)*2=186     b=(31*2)/2=31
        //   ( 0, 0,31): r=31/2=15              g=31*2=62          b=(31*11)/2=170
        //   (10,20, 5): r=(130+40+5)/2=87      g=(60+5)*2=130     b=(30+40+55)/2=62
        let frame = [
            xrgb(31, 0, 0),
            xrgb(0, 31, 0),
            xrgb(0, 0, 31),
            xrgb(10, 20, 5),
        ];
        let expected = [[201u8, 0, 46], [31, 186, 31], [15, 62, 170], [87, 130, 62]];
        assert_eq!(
            compare_frame_rgb(&frame, &expected, CgbColorMap::Gambatte),
            Ok(())
        );
    }

    #[test]
    fn gambatte_mismatch_reports_both_sides_in_gambatte_space() {
        let frame = [xrgb(31, 31, 31)];
        let expected = [[0u8, 0, 0]];
        let err = compare_frame_rgb(&frame, &expected, CgbColorMap::Gambatte).unwrap_err();
        assert_eq!(
            err,
            "1 pixel(s) differ from reference image: #0: expected #000000 got #F8F8F8"
        );
    }

    // --- length mismatch ---

    #[test]
    fn length_mismatch_is_err_not_panic() {
        let frame = [0u32; 2];
        let expected = [[0u8; 3]; 3];
        let err = compare_frame_rgb(&frame, &expected, CgbColorMap::Identity).unwrap_err();
        assert!(err.contains('2') && err.contains('3'), "{err}");
    }

    #[test]
    fn image_wrapper_rejects_wrong_dimensions() {
        // Right pixel count, wrong shape (transposed) — must be an Err
        // before any pixel comparison happens.
        let img = super::super::png::Image {
            w: slopgb_core::SCREEN_H,
            h: SCREEN_W,
            rgb: vec![[0, 0, 0]; slopgb_core::SCREEN_PIXELS],
        };
        let frame = vec![0u32; slopgb_core::SCREEN_PIXELS];
        let err = compare_frame_image(&frame, &img, CgbColorMap::Identity).unwrap_err();
        assert!(err.contains("144x160"), "{err}");

        let ok = super::super::png::Image {
            w: SCREEN_W,
            h: slopgb_core::SCREEN_H,
            rgb: vec![[0, 0, 0]; slopgb_core::SCREEN_PIXELS],
        };
        compare_frame_image(&frame, &ok, CgbColorMap::Identity).unwrap();
    }

    // --- ASCII rendering ---

    #[test]
    fn frame_ascii_buckets_default_dmg_palette() {
        // The four default DMG greys land in the four buckets, bright→dark.
        let frame = [0x00FF_FFFF, 0x00AA_AAAA, 0x0055_5555, 0x0000_0000];
        assert_eq!(frame_ascii(&frame), " .o#\n");
    }

    #[test]
    fn frame_ascii_full_frame_is_screen_shaped() {
        // Rows alternate white / dark-grey: 144 lines of 160 chars each.
        let mut frame = vec![0u32; SCREEN_PIXELS];
        for (y, row) in frame.chunks_mut(SCREEN_W).enumerate() {
            row.fill(if y % 2 == 0 { 0x00FF_FFFF } else { 0x0055_5555 });
        }
        let art = frame_ascii(&frame);
        let lines: Vec<&str> = art.lines().collect();
        assert_eq!(lines.len(), SCREEN_H);
        assert!(lines.iter().all(|l| l.len() == SCREEN_W));
        assert_eq!(lines[0], " ".repeat(SCREEN_W));
        assert_eq!(lines[1], "o".repeat(SCREEN_W));
    }
}
