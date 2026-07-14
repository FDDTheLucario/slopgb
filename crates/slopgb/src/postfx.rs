//! Frontend-only presentation filters applied to the core's XRGB8888 frame just
//! before the blit. Every filter is a pure function over pixels and touches only
//! a frontend-owned scratch copy of the frame — the core buffer is never
//! mutated, so the golden path stays byte-identical (these run only when the
//! matching Options control is enabled). Wires the Graphics "frame blend" and
//! GB-Colors "Contrast" / "DMG on GBC LCD colors" controls.

use crate::windows::options::Settings;

/// Neutral contrast: the slider's midpoint leaves pixels unchanged.
pub const CONTRAST_NEUTRAL: f32 = 0.5;

/// Whether any presentation filter is enabled (so the caller can skip the
/// scratch copy entirely on the common all-off path).
#[must_use]
pub fn any_active(s: &Settings) -> bool {
    s.frame_blend || s.dmg_gbc_lcd || (s.contrast - CONTRAST_NEUTRAL).abs() > f32::EPSILON
}

/// Apply the enabled filters to `buf` (holding the current frame) in place.
/// `prev` is the previously presented frame, used only for blending; when its
/// length differs from `buf` (e.g. an SGB-border frame after a bare one) the
/// blend is skipped for that frame.
pub fn apply(buf: &mut [u32], prev: &[u32], s: &Settings) {
    if s.frame_blend && prev.len() == buf.len() {
        for (px, &p) in buf.iter_mut().zip(prev) {
            *px = blend_px(*px, p);
        }
    }
    if s.dmg_gbc_lcd {
        for px in buf.iter_mut() {
            *px = gbc_lcd_px(*px);
        }
    }
    let c = s.contrast;
    if (c - CONTRAST_NEUTRAL).abs() > f32::EPSILON {
        for px in buf.iter_mut() {
            *px = contrast_px(*px, c);
        }
    }
}

/// Per-channel average of two XRGB8888 pixels (a one-frame motion trail).
#[must_use]
pub fn blend_px(a: u32, b: u32) -> u32 {
    let mix = |sh: u32| (((a >> sh) & 0xFF) + ((b >> sh) & 0xFF)) / 2;
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

/// Contrast around mid-grey. `amount` is the slider fraction 0..=1 mapped to a
/// gain of `2*amount` (so 0.5 = ×1 identity, 1.0 = ×2, 0.0 = flat grey).
#[must_use]
pub fn contrast_px(px: u32, amount: f32) -> u32 {
    let gain = 2.0 * amount;
    let adj = |sh: u32| {
        let c = ((px >> sh) & 0xFF) as f32;
        ((c - 128.0) * gain + 128.0).clamp(0.0, 255.0) as u32
    };
    (adj(16) << 16) | (adj(8) << 8) | adj(0)
}

/// Push an 8-bit RGB pixel through the GBC LCD colour-correction curve so a DMG
/// game takes on the washed-out GBC-screen look (bgb's "DMG on GBC LCD colors").
/// The 8-bit channels are reduced to the 5-bit domain and run through SameBoy's
/// `rgb15 → rgb32` matrix (`GB_convert_rgb15_to_rgb32`), then expanded back.
/// ponytail: linear matrix only — SameBoy also applies a gamma pass; add it if
/// the tint reads too flat.
#[must_use]
pub fn gbc_lcd_px(px: u32) -> u32 {
    let r = ((px >> 16) & 0xFF) >> 3; // 8-bit -> 5-bit
    let g = ((px >> 8) & 0xFF) >> 3;
    let b = (px & 0xFF) >> 3;
    // SameBoy's integer weights; the >>2 keeps the result in 0..=240 (the GBC
    // panel never hits full 255, which is what gives the muted look).
    let nr = (r * 26 + g * 4 + b * 2).min(960) >> 2;
    let ng = (g * 24 + b * 8).min(960) >> 2;
    let nb = (r * 6 + g * 4 + b * 22).min(960) >> 2;
    (nr << 16) | (ng << 8) | nb
}

#[cfg(test)]
#[path = "postfx_tests.rs"]
mod tests;
