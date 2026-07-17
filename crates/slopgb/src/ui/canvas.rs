//! A software drawing surface over a `&mut [u32]` XRGB8888 pixel buffer — the
//! substrate the bgb-style debugger/viewer windows render into (we own every
//! pixel; no GUI toolkit). Everything clips to the buffer and to a settable
//! clip rectangle, so a widget can draw past its bounds without corrupting a
//! neighbor or panicking.

/// An axis-aligned rectangle in pixel space. Signed so a widget can sit partly
/// (or wholly) off-screen and still clip correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    #[must_use]
    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// True if `(px, py)` lies inside (right/bottom edges exclusive).
    #[must_use]
    pub const fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// The overlap of two rectangles (zero-area / negative if disjoint).
    #[must_use]
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let right = self.right().min(o.right());
        let bottom = self.bottom().min(o.bottom());
        Rect::new(x, y, (right - x).max(0), (bottom - y).max(0))
    }
}

/// A clipped drawing surface over a borrowed pixel buffer.
pub struct Canvas<'a> {
    buf: &'a mut [u32],
    w: i32,
    h: i32,
    clip: Rect,
    /// When `Some`, every [`Self::put`]/[`Self::fill_rect`] call's rect (as
    /// requested, pre-clip) is also appended here — the theme
    /// LAYOUT-INVARIANCE guard test's recorder (see [`Self::new_recording`]),
    /// proving a `Theme` swap only recolors pixels, never moves or resizes
    /// anything. Test-only: nothing in the running app needs this, so it (and
    /// the per-call check in `put`/`fill_rect`) is compiled out of a release
    /// build entirely, not just unused.
    #[cfg(test)]
    record: Option<Vec<Rect>>,
}

impl<'a> Canvas<'a> {
    /// Wrap a `w × h` XRGB8888 buffer. `buf.len()` must be at least `w * h`.
    #[must_use]
    pub fn new(buf: &'a mut [u32], w: usize, h: usize) -> Self {
        debug_assert!(buf.len() >= w * h, "buffer smaller than w*h");
        let (w, h) = (w as i32, h as i32);
        Self {
            buf,
            w,
            h,
            clip: Rect::new(0, 0, w, h),
            #[cfg(test)]
            record: None,
        }
    }

    /// Like [`Self::new`], but records every draw call's rect into
    /// [`Self::drawn`] — for the theme layout-invariance guard test: render
    /// the same widgets under different themes and assert the recorded rects
    /// are identical (only the pixel colours may differ).
    #[cfg(test)]
    #[must_use]
    pub fn new_recording(buf: &'a mut [u32], w: usize, h: usize) -> Self {
        let mut c = Self::new(buf, w, h);
        c.record = Some(Vec::new());
        c
    }

    /// Every rect passed to [`Self::put`] (as a 1×1 rect) or [`Self::fill_rect`]
    /// so far, in call order — empty unless built with [`Self::new_recording`].
    #[cfg(test)]
    #[must_use]
    pub fn drawn(&self) -> &[Rect] {
        self.record.as_deref().unwrap_or(&[])
    }

    /// The whole surface as a rectangle.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    /// Intersect the clip with `r`, returning the previous clip so the caller
    /// can restore it (a poor-man's save/restore for nested widgets).
    pub fn push_clip(&mut self, r: Rect) -> Rect {
        let old = self.clip;
        self.clip = self.clip.intersect(&r);
        old
    }

    /// Restore a clip returned by [`Self::push_clip`].
    pub fn set_clip(&mut self, r: Rect) {
        self.clip = r;
    }

    /// Set one pixel, clipped to the buffer and the clip rect.
    pub fn put(&mut self, x: i32, y: i32, color: u32) {
        #[cfg(test)]
        if let Some(rec) = &mut self.record {
            rec.push(Rect::new(x, y, 1, 1));
        }
        if x >= 0 && y >= 0 && x < self.w && y < self.h && self.clip.contains(x, y) {
            self.buf[(y * self.w + x) as usize] = color;
        }
    }

    /// Alpha-blend `fg` over the existing pixel at `(x, y)` by `coverage`
    /// (0 = keep dst, 255 = full fg) — the anti-aliased glyph compositor.
    /// Per-channel linear lerp; clipped exactly like [`Self::put`].
    pub fn blend_px(&mut self, x: i32, y: i32, fg: u32, coverage: u8) {
        if coverage == 0 {
            return; // fully transparent: nothing to draw (and no recorded rect)
        }
        #[cfg(test)]
        if let Some(rec) = &mut self.record {
            rec.push(Rect::new(x, y, 1, 1));
        }
        if x < 0 || y < 0 || x >= self.w || y >= self.h || !self.clip.contains(x, y) {
            return;
        }
        let idx = (y * self.w + x) as usize;
        if coverage == 255 {
            self.buf[idx] = fg;
            return;
        }
        let dst = self.buf[idx];
        let cov = i32::from(coverage);
        let lerp = |shift: u32| -> u32 {
            let d = ((dst >> shift) & 0xFF) as i32;
            let f = ((fg >> shift) & 0xFF) as i32;
            (d + (f - d) * cov / 255) as u32 & 0xFF
        };
        self.buf[idx] = (lerp(16) << 16) | (lerp(8) << 8) | lerp(0);
    }

    /// Fill a rectangle, clipped.
    pub fn fill_rect(&mut self, r: Rect, color: u32) {
        #[cfg(test)]
        if let Some(rec) = &mut self.record {
            rec.push(r);
        }
        let a = r.intersect(&self.clip).intersect(&self.bounds());
        for y in a.y..a.bottom() {
            let row = (y * self.w) as usize;
            for x in a.x..a.right() {
                self.buf[row + x as usize] = color;
            }
        }
    }

    /// Horizontal run of `len` pixels from `(x, y)` going right.
    pub fn hline(&mut self, x: i32, y: i32, len: i32, color: u32) {
        self.fill_rect(Rect::new(x, y, len, 1), color);
    }

    /// Vertical run of `len` pixels from `(x, y)` going down.
    pub fn vline(&mut self, x: i32, y: i32, len: i32, color: u32) {
        self.fill_rect(Rect::new(x, y, 1, len), color);
    }

    /// One-pixel rectangle outline (the four edges of `r`).
    pub fn outline_rect(&mut self, r: Rect, color: u32) {
        self.hline(r.x, r.y, r.w, color); // top
        self.hline(r.x, r.bottom() - 1, r.w, color); // bottom
        self.vline(r.x, r.y, r.h, color); // left
        self.vline(r.right() - 1, r.y, r.h, color); // right
    }

    /// Rectangle outline with the four corner pixels omitted — a subtly
    /// "rounded" frame for contemporary themes (vs the hard-cornered
    /// [`Self::outline_rect`]). Falls back to a hard outline when too small to
    /// round meaningfully.
    pub fn round_outline(&mut self, r: Rect, color: u32) {
        if r.w < 3 || r.h < 3 {
            self.outline_rect(r, color);
            return;
        }
        self.hline(r.x + 1, r.y, r.w - 2, color); // top (minus corners)
        self.hline(r.x + 1, r.bottom() - 1, r.w - 2, color); // bottom
        self.vline(r.x, r.y + 1, r.h - 2, color); // left
        self.vline(r.right() - 1, r.y + 1, r.h - 2, color); // right
    }

    /// Draw an 8×8 grid of 2-bit palette indices (e.g. from
    /// `slopgb_core::debug::tile_pixels`) through `palette` at integer `scale`
    /// — each source pixel becomes a `scale × scale` block — with the tile's
    /// top-left at `(x, y)`. Indices are masked to 0..=3. Clipped.
    pub fn blit_tile(
        &mut self,
        x: i32,
        y: i32,
        pixels: &[[u8; 8]; 8],
        palette: &[u32; 4],
        scale: i32,
    ) {
        for (row, line) in pixels.iter().enumerate() {
            for (col, &idx) in line.iter().enumerate() {
                let color = palette[(idx & 3) as usize];
                let px = x + col as i32 * scale;
                let py = y + row as i32 * scale;
                self.fill_rect(Rect::new(px, py, scale, scale), color);
            }
        }
    }
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
