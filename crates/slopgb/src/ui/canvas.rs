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
        }
    }

    /// The whole surface as a rectangle.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    /// The active clip rectangle.
    #[must_use]
    pub fn clip(&self) -> Rect {
        self.clip
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
        if x >= 0 && y >= 0 && x < self.w && y < self.h && self.clip.contains(x, y) {
            self.buf[(y * self.w + x) as usize] = color;
        }
    }

    /// Fill a rectangle, clipped.
    pub fn fill_rect(&mut self, r: Rect, color: u32) {
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
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
