//! Presentation: softbuffer surface with an integer-scaled, letterboxed,
//! nearest-neighbor blit of the core's 160x144 XRGB8888 frame.

use std::num::NonZeroU32;
use std::rc::Rc;

use slopgb_core::{SCREEN_H, SCREEN_PIXELS, SCREEN_W};
use winit::window::Window;

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

    /// Present `frame` at the largest integer scale that fits the window,
    /// centered on black. The core frame and the softbuffer pixel format are
    /// both `0x00RRGGBB` u32s, so pixels are copied verbatim.
    pub fn draw(
        &mut self,
        window: &Window,
        frame: &[u32; SCREEN_PIXELS],
    ) -> Result<(), softbuffer::SoftBufferError> {
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(()); // zero-sized (minimized): nothing to present
        };
        self.surface.resize(w, h)?;
        let mut buffer = self.surface.buffer_mut()?;
        buffer.fill(0); // letterbox bars
        blit(&mut buffer, size.width, size.height, frame, &mut self.row);
        window.pre_present_notify();
        buffer.present()
    }
}

/// Nearest-neighbor integer upscale of `frame` into the center of `dst`
/// (`dst_w` x `dst_h`). If the window is smaller than 160x144 the image is
/// drawn at 1x and clipped.
fn blit(dst: &mut [u32], dst_w: u32, dst_h: u32, frame: &[u32; SCREEN_PIXELS], row: &mut Vec<u32>) {
    let screen_w = SCREEN_W as u32;
    let screen_h = SCREEN_H as u32;
    let scale = (dst_w / screen_w).min(dst_h / screen_h).max(1);
    let img_w = (screen_w * scale).min(dst_w);
    let img_h = (screen_h * scale).min(dst_h);
    let x0 = (dst_w - img_w) / 2;
    let y0 = (dst_h - img_h) / 2;

    row.clear();
    row.resize(img_w as usize, 0);
    let mut cached_sy = u32::MAX;
    for dy in 0..img_h {
        let sy = dy / scale;
        if sy != cached_sy {
            cached_sy = sy;
            let src = &frame[sy as usize * SCREEN_W..][..SCREEN_W];
            for (dx, px) in row.iter_mut().enumerate() {
                *px = src[dx / scale as usize];
            }
        }
        let off = ((y0 + dy) * dst_w + x0) as usize;
        dst[off..off + img_w as usize].copy_from_slice(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        blit(&mut dst, SCREEN_W as u32, SCREEN_H as u32, &frame, &mut row);
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
        blit(&mut dst, w, h, &frame, &mut row);
        // Letterbox bars are untouched by the blit (`Video::draw` clears the
        // whole buffer to black first).
        assert_eq!(dst[0], u32::MAX);
        assert_eq!(dst[(w * h - 1) as usize], u32::MAX);
        // Top-left image pixel is frame[0], replicated 2x2 at offset (2, 1).
        let at = |x: u32, y: u32| dst[(y * w + x) as usize];
        assert_eq!(at(2, 1), frame[0]);
        assert_eq!(at(3, 1), frame[0]);
        assert_eq!(at(2, 2), frame[0]);
        assert_eq!(at(4, 1), frame[1]);
    }

    #[test]
    fn blit_window_smaller_than_screen_clips_at_1x() {
        let frame = test_frame();
        let (w, h) = (100u32, 90u32);
        let mut dst = vec![u32::MAX; (w * h) as usize];
        let mut row = Vec::new();
        blit(&mut dst, w, h, &frame, &mut row);
        assert_eq!(dst[0], frame[0]); // no centering possible, drawn from origin
        assert_eq!(dst[1], frame[1]);
    }
}
