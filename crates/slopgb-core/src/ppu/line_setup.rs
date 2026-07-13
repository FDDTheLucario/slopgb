//! Per-line setup at a line boundary (`start_line`). A behaviour-preserving
//! submodule of [`Ppu`] (a second `impl` block via `use super::*`); the fields
//! it touches live in the parent struct.

use super::*;

impl Ppu {
    pub(super) fn start_line(&mut self) {
        // Reset the eager line-153 FF45-write discriminator each line.
        self.l153_lyc_write_dot = u16::MAX;
        match self.line {
            0 => {
                self.ly = 0;
                // The WY latch is *assigned* at line 0 dot 2 (see
                // `step_dot`) — that sample is the frame reset.
                // gambatte M2_Ly0::f0: winYPos = 0xFF — the first
                // activation of the frame increments it to row 0.
                self.win_line = 0xFF;
                self.clear_line_flip_state();
            }
            1..=143 => {
                self.ly = self.line;
                self.clear_line_flip_state();
            }
            144 => {
                self.ly = 144;
                self.frame_count += 1;
                // SGB MASK_EN: freeze holds the last presented frame (no swap);
                // black/color-0 paint over it. Computed as owned values first
                // to avoid aliasing the `self.front` fill below. `None` on
                // every non-SGB model → byte-identical to the pre-SGB path.
                let hold = self.sgb.as_ref().is_some_and(|s| s.holds_frame());
                let mask_fill = self.sgb.as_ref().and_then(|s| s.mask_fill());
                if self.frame_skip {
                    // The first frame after an LCD enable is not displayed
                    // (Pan Docs "LCDC.7"; SameBoy display.c
                    // `GB_FRAMESKIP_LCD_TURNED_ON`): drop the rendered
                    // frame and present blank/white instead.
                    self.frame_skip = false;
                    let white = self.white();
                    self.front.fill(white);
                } else if !hold {
                    std::mem::swap(&mut self.front, &mut self.back);
                }
                if let Some(c) = mask_fill {
                    self.front.fill(c);
                }
                // SGB frame-boundary work: consume a pending `*_TRN` screen
                // capture (reads the just-rendered shade buffer) and recomposite
                // the border from the freshly presented `front`. Inert off SGB
                // (`self.sgb` is `None`) → byte-identical on DMG/CGB.
                self.sgb_frame_boundary();
            }
            _ => self.ly = self.line,
        }
    }

    /// Reset this line's render/flip tracking at a line boundary: the
    /// per-line state cleared identically by `start_line`'s line-0 and
    /// 1..=143 arms.
    fn clear_line_flip_state(&mut self) {
        self.line_render_done = false;
        self.flip_dot = 0;
        self.vis_early = false;
        self.vis_hold_until = 0;
        self.render_finished = false;
        self.hdma_lead = false;
        self.pal_open_dot = 0;
        self.render.active = false;
    }
}
