//! Presentation: softbuffer surface with an integer-scaled, letterboxed,
//! nearest-neighbor blit of the core's 160x144 XRGB8888 frame.

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::window::Window;

use crate::ui::Canvas;

/// Owns the softbuffer context + surface for one window.
pub struct Video {
    /// Kept alive alongside the surface (softbuffer's display connection).
    _context: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    /// Scratch: one scaled output row, rebuilt per source row.
    row: Vec<u32>,
}

impl Video {
    pub fn new(window: Rc<Window>) -> Result<Self, softbuffer::SoftBufferError> {
        let context = softbuffer::Context::new(window.clone())?;
        let surface = softbuffer::Surface::new(&context, window)?;
        Ok(Self {
            _context: context,
            surface,
            row: Vec::new(),
        })
    }

    /// Present the `src_w`×`src_h` `frame`, then let `overlay` draw on top (the
    /// bgb-style popup menu — pass a no-op closure when there is nothing to
    /// overlay). `src_w`/`src_h` are usually 160×144, but the SGB border surface
    /// is 256×224 — the blit scales/letterboxes whatever it is given. With
    /// `stretch` the frame fills the whole window (bgb's "Fullscreen stretched",
    /// aspect not preserved); otherwise it is the largest integer scale, centered
    /// on black. The core frame and the softbuffer pixel format are both
    /// `0x00RRGGBB` u32s, so pixels are copied verbatim; the opaque-alpha pass
    /// runs *after* the overlay so menu pixels (drawn with a 0 top byte) become
    /// opaque too.
    pub fn draw(
        &mut self,
        window: &Window,
        frame: &[u32],
        src_w: usize,
        src_h: usize,
        stretch: bool,
        overlay: impl FnOnce(&mut Canvas),
    ) -> Result<(), softbuffer::SoftBufferError> {
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(()); // zero-sized (minimized): nothing to present
        };
        self.surface.resize(w, h)?;
        let mut buffer = self.surface.buffer_mut()?;
        if stretch {
            blit_stretch(&mut buffer, size.width, size.height, frame, src_w, src_h);
        } else {
            blit(
                &mut buffer,
                size.width,
                size.height,
                frame,
                src_w,
                src_h,
                &mut self.row,
            );
        }
        {
            let mut canvas = Canvas::new(&mut buffer, size.width as usize, size.height as usize);
            overlay(&mut canvas);
        }
        // Force opaque alpha: softbuffer leaves the top byte 0, which a 32-bit
        // ARGB compositor reads as fully transparent (the window would show the
        // desktop through it). softbuffer itself ignores the top byte. Runs last
        // so both the LCD and the overlay end up opaque.
        for px in buffer.iter_mut() {
            *px |= 0xFF00_0000;
        }
        window.pre_present_notify();
        buffer.present()
    }
}

/// Nearest-neighbor integer upscale of the `src_w`×`src_h` `frame` into the
/// center of `dst` (`dst_w` x `dst_h`), painting the letterbox margins black.
/// Every pixel of `dst` is written, but only the margins are cleared — the image
/// region is overwritten directly, so a large window doesn't pay for a full
/// clear plus a full blit each frame. If the window is smaller than the source
/// the image is drawn at 1x and clipped.
fn blit(
    dst: &mut [u32],
    dst_w: u32,
    dst_h: u32,
    frame: &[u32],
    src_w: usize,
    src_h: usize,
    row: &mut Vec<u32>,
) {
    let screen_w = src_w as u32;
    let screen_h = src_h as u32;
    let scale = (dst_w / screen_w).min(dst_h / screen_h).max(1);
    let img_w = (screen_w * scale).min(dst_w);
    let img_h = (screen_h * scale).min(dst_h);
    let x0 = (dst_w - img_w) / 2;
    let y0 = (dst_h - img_h) / 2;

    // Letterbox bars above and below the image.
    dst[..(y0 * dst_w) as usize].fill(0);
    dst[((y0 + img_h) * dst_w) as usize..].fill(0);

    row.clear();
    row.resize(img_w as usize, 0);
    let mut cached_sy = u32::MAX;
    for dy in 0..img_h {
        let sy = dy / scale;
        if sy != cached_sy {
            cached_sy = sy;
            let src = &frame[sy as usize * src_w..][..src_w];
            for (dx, px) in row.iter_mut().enumerate() {
                *px = src[dx / scale as usize];
            }
        }
        // Left bar, image row, right bar.
        let line = ((y0 + dy) * dst_w) as usize;
        let off = line + x0 as usize;
        dst[line..off].fill(0);
        dst[off..off + img_w as usize].copy_from_slice(row);
        dst[off + img_w as usize..line + dst_w as usize].fill(0);
    }
}

/// Nearest-neighbor stretch of the `src_w`×`src_h` `frame` to fill the entire
/// `dst` (no letterbox), for bgb's "Fullscreen stretched". Aspect ratio is not
/// preserved. Every dst pixel maps to the proportional source pixel;
/// `dy*src_h/dst_h` and `dx*src_w/dst_w` stay in-bounds for all `dy<dst_h`,
/// `dx<dst_w`.
fn blit_stretch(
    dst: &mut [u32],
    dst_w: u32,
    dst_h: u32,
    frame: &[u32],
    src_w: usize,
    src_h: usize,
) {
    if dst_w == 0 || dst_h == 0 {
        return;
    }
    for dy in 0..dst_h {
        let sy = (dy * src_h as u32 / dst_h) as usize;
        let srow = &frame[sy * src_w..][..src_w];
        let drow = &mut dst[(dy * dst_w) as usize..][..dst_w as usize];
        for (dx, px) in drow.iter_mut().enumerate() {
            let sx = (dx as u32 * src_w as u32 / dst_w) as usize;
            *px = srow[sx];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slopgb_core::{SCREEN_H, SCREEN_PIXELS, SCREEN_W};

    fn test_frame() -> Box<[u32; SCREEN_PIXELS]> {
        let mut f = vec![0u32; SCREEN_PIXELS].into_boxed_slice();
        for (i, px) in f.iter_mut().enumerate() {
            *px = i as u32;
        }
        f.try_into().unwrap()
    }

    #[test]
    fn blit_1x_exact_fit_is_identity() {
        let frame = test_frame();
        let mut dst = vec![u32::MAX; SCREEN_PIXELS];
        let mut row = Vec::new();
        blit(
            &mut dst,
            SCREEN_W as u32,
            SCREEN_H as u32,
            &frame[..],
            SCREEN_W,
            SCREEN_H,
            &mut row,
        );
        assert_eq!(&dst[..], &frame[..]);
    }

    #[test]
    fn blit_2x_replicates_pixels_and_letterboxes() {
        let frame = test_frame();
        // 2x image (320x288) centered in a 324x290 window: 2px side bars,
        // 1px top/bottom bars.
        let (w, h) = (324u32, 290u32);
        let mut dst = vec![u32::MAX; (w * h) as usize];
        let mut row = Vec::new();
        blit(&mut dst, w, h, &frame[..], SCREEN_W, SCREEN_H, &mut row);
        let at = |x: u32, y: u32| dst[(y * w + x) as usize];
        // Letterbox bars are cleared to black by the blit itself.
        assert_eq!(dst[0], 0); // top bar
        assert_eq!(dst[(w * h - 1) as usize], 0); // bottom bar
        assert_eq!(at(1, 5), 0); // left bar
        assert_eq!(at(w - 1, 5), 0); // right bar
        // Top-left image pixel is frame[0], replicated 2x2 at offset (2, 1).
        assert_eq!(at(2, 1), frame[0]);
        assert_eq!(at(3, 1), frame[0]);
        assert_eq!(at(2, 2), frame[0]);
        assert_eq!(at(4, 1), frame[1]);
    }

    #[test]
    fn blit_writes_every_destination_pixel() {
        // The blit owns the whole buffer now (no full clear beforehand), so
        // no pixel may survive from the previous frame.
        let frame = test_frame(); // values 0..SCREEN_PIXELS, never u32::MAX
        let (w, h) = (324u32, 290u32);
        let mut dst = vec![u32::MAX; (w * h) as usize];
        let mut row = Vec::new();
        blit(&mut dst, w, h, &frame[..], SCREEN_W, SCREEN_H, &mut row);
        assert!(dst.iter().all(|&px| px != u32::MAX));
    }

    #[test]
    fn blit_stretch_fills_every_pixel_and_maps_corners() {
        let frame = test_frame();
        let (w, h) = (500u32, 300u32); // non-integer, non-aspect-preserving
        let mut dst = vec![u32::MAX; (w * h) as usize];
        blit_stretch(&mut dst, w, h, &frame[..], SCREEN_W, SCREEN_H);
        // No letterbox: every pixel written (none left as the sentinel).
        assert!(dst.iter().all(|&px| px != u32::MAX), "whole window filled");
        // Corners map to the frame corners.
        assert_eq!(dst[0], frame[0], "top-left");
        assert_eq!(dst[(w - 1) as usize], frame[SCREEN_W - 1], "top-right");
        let bottom_left = ((h - 1) * w) as usize;
        assert_eq!(
            dst[bottom_left],
            frame[(SCREEN_H - 1) * SCREEN_W],
            "bottom-left"
        );
    }

    #[test]
    fn blit_window_smaller_than_screen_clips_at_1x() {
        let frame = test_frame();
        let (w, h) = (100u32, 90u32);
        let mut dst = vec![u32::MAX; (w * h) as usize];
        let mut row = Vec::new();
        blit(&mut dst, w, h, &frame[..], SCREEN_W, SCREEN_H, &mut row);
        assert_eq!(dst[0], frame[0]); // no centering possible, drawn from origin
        assert_eq!(dst[1], frame[1]);
    }
}
