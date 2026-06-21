//! Per-line setup at a line boundary (`start_line`). A behaviour-preserving
//! submodule of [`Ppu`] (a second `impl` block via `use super::*`); the fields
//! it touches live in the parent struct.

use super::*;

impl Ppu {
    pub(super) fn start_line(&mut self) {
        match self.line {
            0 => {
                self.ly = 0;
                // The WY latch is *assigned* at line 0 dot 2 (see
                // `step_dot`) — that sample is the frame reset.
                // gambatte M2_Ly0::f0: winYPos = 0xFF — the first
                // activation of the frame increments it to row 0.
                self.win_line = 0xFF;
                self.line_render_done = false;
                self.render_finished = false;
                self.hdma_lead = false;
                self.render.active = false;
            }
            1..=143 => {
                self.ly = self.line;
                self.line_render_done = false;
                self.render_finished = false;
                self.hdma_lead = false;
                self.render.active = false;
            }
            144 => {
                self.ly = 144;
                self.frame_count += 1;
                if self.frame_skip {
                    // The first frame after an LCD enable is not displayed
                    // (Pan Docs "LCDC.7"; SameBoy display.c
                    // `GB_FRAMESKIP_LCD_TURNED_ON`): drop the rendered
                    // frame and present blank/white instead.
                    self.frame_skip = false;
                    let white = self.white();
                    self.front.fill(white);
                } else {
                    std::mem::swap(&mut self.front, &mut self.back);
                }
            }
            _ => self.ly = self.line,
        }
    }
}
