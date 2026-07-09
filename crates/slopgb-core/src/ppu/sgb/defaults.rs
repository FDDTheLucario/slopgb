//! The SGB's built-in **default border**: an original, procedurally-generated
//! frame drawn around the Game Boy inset.
//!
//! On real hardware the default border lives in the SGB's SNES-side firmware
//! (the "BIOS"), uploaded by SNES code. slopgb is a *high-level* emulation — it
//! never runs the SNES CPU — so that firmware is not executed and would show no
//! border at all. [`SgbView::default_composite`] instead draws an **original**
//! neutral bezel (no Nintendo artwork embedded), shown from power-on and until
//! a ROM sends its own `CHR_TRN`+`PCT_TRN`. A user-supplied BIOS's real border
//! and title→palette table are installed through the seam in `bios.rs`; only
//! *then* is anything Nintendo-derived, and only in the user's own copy — never
//! committed to this repo.

use super::*;

/// The original default-border colours (XRGB8888). Neutral slate with a muted
/// steel-blue accent — deliberately unlike any Nintendo border.
const BACKDROP: u32 = 0x0F_1420; // deep slate
const FRAME_SHADOW: u32 = 0x05_0810; // near-black bevel line
const FRAME_ACCENT: u32 = 0x35_5A7A; // steel-blue frame
const FRAME_EDGE: u32 = 0x1C_2740; // dim outer edge line

impl SgbView {
    /// Draw the original default border into `border_fb`. When `front` is
    /// `Some`, the live colorized Game Boy screen is blitted into the inset;
    /// `None` (power-on seed) leaves the inset area as backdrop.
    ///
    /// The inset occupies `x∈[48,208), y∈[40,184)` (tile (6,5), 160×144). The
    /// frame is a set of concentric rectangle outlines around it plus a thin
    /// outer edge line — all plain fills, no bitmap art.
    pub(super) fn default_composite(&mut self, front: Option<&[u32; SCREEN_PIXELS]>) {
        self.border_fb.fill(BACKDROP);

        // Inset rectangle (exclusive right/bottom).
        let (ix0, iy0) = (INSET_X, INSET_Y);
        let (ix1, iy1) = (INSET_X + SCREEN_W, INSET_Y + SCREEN_H);

        // A beveled bezel hugging the screen: dark shadow, steel accent, dark.
        self.outline(ix0 - 1, iy0 - 1, ix1 + 1, iy1 + 1, FRAME_SHADOW);
        self.outline(ix0 - 3, iy0 - 3, ix1 + 3, iy1 + 3, FRAME_ACCENT);
        self.outline(ix0 - 4, iy0 - 4, ix1 + 4, iy1 + 4, FRAME_ACCENT);
        self.outline(ix0 - 6, iy0 - 6, ix1 + 6, iy1 + 6, FRAME_SHADOW);
        // A decorative outer frame inset 8px from the panel edge.
        self.outline(8, 8, BORDER_W - 8, BORDER_H - 8, FRAME_EDGE);

        if let Some(front) = front {
            for gy in 0..SCREEN_H {
                let dst = (INSET_Y + gy) * BORDER_W + INSET_X;
                let src = gy * SCREEN_W;
                self.border_fb[dst..dst + SCREEN_W].copy_from_slice(&front[src..src + SCREEN_W]);
            }
        }
    }

    /// Draw a 1px-thick rectangle outline `[x0,x1) × [y0,y1)` in `color`,
    /// clipped to the surface. Used only by [`Self::default_composite`].
    fn outline(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, color: u32) {
        let x1 = x1.min(BORDER_W);
        let y1 = y1.min(BORDER_H);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for x in x0..x1 {
            self.border_fb[y0 * BORDER_W + x] = color;
            self.border_fb[(y1 - 1) * BORDER_W + x] = color;
        }
        for y in y0..y1 {
            self.border_fb[y * BORDER_W + x0] = color;
            self.border_fb[y * BORDER_W + (x1 - 1)] = color;
        }
    }
}
