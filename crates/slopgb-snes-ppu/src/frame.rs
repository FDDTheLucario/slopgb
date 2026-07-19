//! Main-screen line assembly: the BG/OBJ layers merged per the fullsnes
//! "Background Priority Chart" (modes 0/1 columns), TM layer enables,
//! the CGRAM-0 backdrop, and INIDISP forced-blank/master-brightness
//! (fullsnes 2100h: black at N=0, else brightness × (N+1)/16).
//!
//! Sub-screen, color math, and windows are unsupported (ceiling): TS/TSW/
//! TMW/CGWSEL/CGADSUB write-through as inert registers.

use super::*;

/// One rung of the priority chart, top-most first: a BG layer at one
/// priority-bit level, or the OBJ layer at one OAM priority.
#[derive(Clone, Copy)]
enum Rung {
    Bg(usize, bool),
    Obj(u8),
}
use Rung::*;

/// Mode 0, top to bottom (backdrop implicit).
const MODE0: [Rung; 12] = [
    Obj(3),
    Bg(0, true),
    Bg(1, true),
    Obj(2),
    Bg(0, false),
    Bg(1, false),
    Obj(1),
    Bg(2, true),
    Bg(3, true),
    Obj(0),
    Bg(2, false),
    Bg(3, false),
];

/// Mode 1 with BGMODE bit 3 set: BG3.1 hoists above everything.
const MODE1A: [Rung; 10] = [
    Bg(2, true),
    Obj(3),
    Bg(0, true),
    Bg(1, true),
    Obj(2),
    Bg(0, false),
    Bg(1, false),
    Obj(1),
    Obj(0),
    Bg(2, false),
];

/// Mode 1 with BGMODE bit 3 clear: BG3.1 sits between OBJ.1 and OBJ.0.
const MODE1B: [Rung; 10] = [
    Obj(3),
    Bg(0, true),
    Bg(1, true),
    Obj(2),
    Bg(0, false),
    Bg(1, false),
    Obj(1),
    Bg(2, true),
    Obj(0),
    Bg(2, false),
];

impl SnesPpu {
    /// Render main-screen line `y` (0-based row of the 224-line frame) as
    /// 256 RGB555 pixels: TM-enabled layers merged per the priority chart
    /// over the CGRAM-0 backdrop, then master brightness; forced blank (or
    /// brightness 0) is black.
    pub fn render_line(&self, y: u16, out: &mut [u16; 256]) {
        let brightness = u16::from(self.inidisp & 0x0F);
        if self.inidisp & 0x80 != 0 || brightness == 0 {
            out.fill(0);
            return;
        }
        let mut bg = [[None; 256]; 4];
        let mut bg_used = [false; 4];
        for (i, buf) in bg.iter_mut().enumerate() {
            if self.tm & 1 << i != 0 {
                self.bg_line(i, y, buf);
                bg_used[i] = buf.iter().any(Option::is_some);
            }
        }
        let mut obj = [None; 256];
        let mut obj_used = false;
        if self.tm & 0x10 != 0 {
            self.obj_line(y, &mut obj);
            obj_used = obj.iter().any(Option::is_some);
        }
        let rungs: &[Rung] = match self.bgmode & 7 {
            0 => &MODE0,
            _ if self.bgmode & 8 != 0 => &MODE1A,
            _ => &MODE1B,
        };
        // Top rung first over the backdrop: each pixel keeps the first
        // opaque hit (the chart order), later rungs fill only what is
        // still unresolved; layers with nothing on this line skip whole.
        let backdrop = self.cgram[0] & 0x7FFF;
        out.fill(backdrop);
        let mut resolved = [false; 256];
        let mut left = 256usize;
        for rung in rungs {
            match *rung {
                Bg(b, _) if !bg_used[b] => continue,
                Obj(_) if !obj_used => continue,
                Bg(b, want) => {
                    for x in 0..256 {
                        if !resolved[x] {
                            if let Some((c, p)) = bg[b][x] {
                                if p == want {
                                    out[x] = c;
                                    resolved[x] = true;
                                    left -= 1;
                                }
                            }
                        }
                    }
                }
                Obj(want) => {
                    for x in 0..256 {
                        if !resolved[x] {
                            if let Some((c, p)) = obj[x] {
                                if p == want {
                                    out[x] = c;
                                    resolved[x] = true;
                                    left -= 1;
                                }
                            }
                        }
                    }
                }
            }
            if left == 0 {
                break;
            }
        }
        // Master brightness scales each channel by (N+1)/16
        // (fullsnes 2100h); N=15 is exact identity.
        if brightness != 15 {
            let f = brightness + 1;
            for px in out.iter_mut() {
                let r = (*px & 0x1F) * f / 16;
                let g = (*px >> 5 & 0x1F) * f / 16;
                let b = (*px >> 10 & 0x1F) * f / 16;
                *px = b << 10 | g << 5 | r;
            }
        }
    }
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
