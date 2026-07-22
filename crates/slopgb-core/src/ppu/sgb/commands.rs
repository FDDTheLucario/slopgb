//! SGB attribute-grid fills, palette-RAM select, and the sound / data / flag
//! command handlers. A second `impl SgbView` block (`use super::*`); layouts
//! cite Pan Docs "SGB Command $xx" and SameBoy `Core/sgb.c::command_ready`.

use super::*;

impl SgbView {
    /// ATTR_BLK ($04): byte 1 = number of 6-byte data sets (cap 18). Each set is
    /// `control` (bit0 inside / bit1 on-border / bit2 outside), `palettes`
    /// (bits0-1 inside, 2-3 border, 4-5 outside), then `x1,y1,x2,y2` in
    /// 20×18-cell coords. Following SameBoy: a set with only `inside` (or only
    /// `outside`) set also paints the border with that palette. A cell strictly
    /// inside the rect is "inside", on its perimeter "on-border", beyond it
    /// "outside" — each region recolored only if its (effective) control bit is
    /// set. (Pan Docs "SGB Command $04 — ATTR_BLK".)
    pub(super) fn attr_blk(&mut self, cmd: &[u8]) {
        let sets = usize::from(cmd[1]).min(18);
        for s in 0..sets {
            let base = 2 + s * 6;
            if base + 6 > cmd.len() {
                break;
            }
            let control = cmd[base];
            let pals = cmd[base + 1];
            let inside = control & 1 != 0;
            let outside = control & 4 != 0;
            let mut middle = control & 2 != 0;
            let inside_pal = pals & 3;
            let outside_pal = (pals >> 4) & 3;
            let mut middle_pal = (pals >> 2) & 3;
            // SameBoy: inside-only implies the border takes the inside palette;
            // outside-only implies the border takes the outside palette.
            if inside && !middle && !outside {
                middle = true;
                middle_pal = inside_pal;
            } else if outside && !middle && !inside {
                middle = true;
                middle_pal = outside_pal;
            }
            let (x1, y1) = (cmd[base + 2] & 0x1F, cmd[base + 3] & 0x1F);
            let (x2, y2) = (cmd[base + 4] & 0x1F, cmd[base + 5] & 0x1F);
            for cy in 0u8..18 {
                for cx in 0u8..20 {
                    if cx < x1 || cx > x2 || cy < y1 || cy > y2 {
                        if outside {
                            self.set_attr(cx, cy, outside_pal);
                        }
                    } else if cx > x1 && cx < x2 && cy > y1 && cy < y2 {
                        if inside {
                            self.set_attr(cx, cy, inside_pal);
                        }
                    } else if middle {
                        self.set_attr(cx, cy, middle_pal);
                    }
                }
            }
        }
    }

    /// ATTR_LIN ($05): byte 1 = number of 1-byte line entries. Each entry:
    /// bit7 = horizontal (a row) vs vertical (a column), bits5-6 = palette,
    /// bits0-4 = line index. (SameBoy `ATTR_LIN`.)
    pub(super) fn attr_lin(&mut self, cmd: &[u8]) {
        let count = usize::from(cmd[1]).min(cmd.len().saturating_sub(2));
        for i in 0..count {
            let d = cmd[2 + i];
            let horizontal = d & 0x80 != 0;
            let pal = (d >> 5) & 3;
            let line = d & 0x1F;
            if horizontal {
                if line <= 18 {
                    for x in 0..20 {
                        self.set_attr(x, line, pal);
                    }
                }
            } else if line <= 20 {
                for y in 0..18 {
                    self.set_attr(line, y, pal);
                }
            }
        }
    }

    /// ATTR_DIV ($06): split the screen on a row/column into three palettes.
    /// byte 1: bits0-1 = high (after the line), bits2-3 = low (before), bits4-5
    /// = middle (on the line), bit6 = horizontal. byte 2 bits0-4 = line.
    /// (SameBoy `ATTR_DIV`.)
    pub(super) fn attr_div(&mut self, cmd: &[u8]) {
        let high = cmd[1] & 3;
        let low = (cmd[1] >> 2) & 3;
        let middle = (cmd[1] >> 4) & 3;
        let horizontal = cmd[1] & 0x40 != 0;
        let line = cmd[2] & 0x1F;
        for cy in 0u8..18 {
            for cx in 0u8..20 {
                let coord = if horizontal { cy } else { cx };
                let pal = if coord < line {
                    low
                } else if coord == line {
                    middle
                } else {
                    high
                };
                self.set_attr(cx, cy, pal);
            }
        }
    }

    /// ATTR_CHR ($07): per-cell writes from a start cell in a specified order.
    /// bytes 1-2 = start x,y; bytes 3-4 = count (LE); byte 5 = direction
    /// (0 = left→right then down, 1 = top→bottom then right); bytes 6.. = 2
    /// bits/cell, 4 cells per byte, high pair first. (SameBoy `ATTR_CHR`.)
    pub(super) fn attr_chr(&mut self, cmd: &[u8]) {
        let mut x = cmd[1];
        let mut y = cmd[2];
        let count = u16::from(cmd[3]) | (u16::from(cmd[4]) << 8);
        let vertical = cmd[5] & 1 != 0;
        if x >= 20 || y >= 18 {
            return;
        }
        for i in 0..usize::from(count) {
            let byte = match cmd.get(6 + i / 4) {
                Some(&b) => b,
                None => break,
            };
            let pal = (byte >> (((!i) & 3) << 1)) & 3;
            self.set_attr(x, y, pal);
            if vertical {
                y += 1;
                if y == 18 {
                    y = 0;
                    x += 1;
                    if x == 20 {
                        break;
                    }
                }
            } else {
                x += 1;
                if x == 20 {
                    x = 0;
                    y += 1;
                    if y == 18 {
                        break;
                    }
                }
            }
        }
    }

    /// ATTR_SET ($16): install one of the 45 ATTR_TRN'd attribute files. byte 1
    /// bits0-5 = file index; bit6 also cancels MASK_EN. (SameBoy `ATTR_SET`.)
    pub(super) fn attr_set(&mut self, cmd: &[u8]) {
        self.load_attribute_file(cmd[1] & 0x3F);
        if cmd[1] & 0x40 != 0 {
            self.mask = 0;
        }
    }

    /// Expand a 45-file × 90-byte ATTR_TRN attribute file into the live 360-cell
    /// map: 2 bits/cell, 4 cells/byte, high pair first (SameBoy
    /// `load_attribute_file`). Indices past 0x2C are ignored.
    pub(super) fn load_attribute_file(&mut self, file: u8) {
        if file > 0x2C {
            return;
        }
        let base = usize::from(file) * 90;
        let mut out = 0usize;
        for i in 0..90 {
            let mut byte = self.attr_files[base + i];
            for _ in 0..4 {
                self.attr[out] = byte >> 6;
                byte <<= 2;
                out += 1;
            }
        }
    }

    /// PAL_SET ($0A): select 4 palettes from PAL_TRN RAM into palettes 0-3. Each
    /// selector is a 9-bit index (byte + low bit of the next byte). Entry 0 of
    /// all four is forced to palette-0 color 0 (the shared background). byte 9
    /// bit7 also runs an ATTR_SET (byte 9 bits0-5), bit6 cancels MASK_EN.
    /// (SameBoy `PAL_SET`.)
    pub(super) fn pal_set(&mut self, cmd: &[u8]) {
        for p in 0..4 {
            let idx = usize::from(cmd[1 + p * 2]) | (usize::from(cmd[2 + p * 2] & 1) << 8);
            for c in 0..4 {
                let off = (idx * 4 + c) * 2;
                // idx is 0-511, so off ≤ 511*4*2+6 = 4094 < 4096; `get` keeps a
                // crafted short RAM from panicking.
                let lo = self.ram_palettes.get(off).copied().unwrap_or(0);
                let hi = self.ram_palettes.get(off + 1).copied().unwrap_or(0);
                self.pal[p][c] = bgr555(lo, hi);
            }
        }
        let bg = self.pal[0][0];
        for p in &mut self.pal {
            p[0] = bg;
        }
        if cmd[9] & 0x80 != 0 {
            self.load_attribute_file(cmd[9] & 0x3F);
        }
        if cmd[9] & 0x40 != 0 {
            self.mask = 0;
        }
    }

    /// SOUND ($08): queue a sound-effect event (effect A, effect B, attenuation,
    /// effect-bank). Decode + state only; the S-DSP drains the
    /// queue. (Pan Docs "SGB Command $08 — SOUND".)
    pub(super) fn sound(&mut self, cmd: &[u8]) {
        self.push_capped_sound(SgbSound {
            effect_a: cmd[1],
            effect_b: cmd[2],
            attenuation: cmd[3],
            effect_bank: cmd[4],
        });
    }

    /// DATA_SND ($0F): store an inline packet written to SNES RAM (bytes 1..) for
    /// the SNES-side consumer to drain. (Pan Docs "SGB Command $0F — DATA_SND".)
    pub(super) fn data_snd(&mut self, cmd: &[u8]) {
        if self.data_snd.len() >= SOUND_QUEUE_CAP {
            self.data_snd.remove(0);
        }
        self.data_snd.push(cmd[1..16.min(cmd.len())].to_vec());
    }

    /// JUMP ($12): latch the SNES program-jump target (24-bit PC in bytes 1-3)
    /// for the SNES-side consumer. (Pan Docs "SGB Command $12 — JUMP".)
    pub(super) fn jump(&mut self, cmd: &[u8]) {
        self.jump = Some(u32::from(cmd[1]) | (u32::from(cmd[2]) << 8) | (u32::from(cmd[3]) << 16));
    }

    fn push_capped_sound(&mut self, s: SgbSound) {
        if self.sound_events.len() >= SOUND_QUEUE_CAP {
            self.sound_events.remove(0);
        }
        self.sound_events.push(s);
    }

    /// Write attribute cell `(cx, cy)`, bounds-checked (`cx<20`, `cy<18`).
    fn set_attr(&mut self, cx: u8, cy: u8, pal: u8) {
        if cx < 20 && cy < 18 {
            self.attr[usize::from(cy) * 20 + usize::from(cx)] = pal & 3;
        }
    }
}
