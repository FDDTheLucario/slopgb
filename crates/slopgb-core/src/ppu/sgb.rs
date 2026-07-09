//! Super Game Boy presentation layer: the SNES-side colorization of the DMG
//! output driven by SGB command packets. A behaviour-preserving submodule of
//! [`Ppu`] (a second `impl` block via `use super::*`).
//!
//! The ICD2 command-packet *receiver* lives in [`crate::joypad`]; a completed
//! non-MLT_REQ command is forwarded here from the interconnect's P1 write site
//! (`Joypad::take_sgb_command` → [`Ppu::sgb_command`]). Only the commands with
//! a screen-visible effect on the 20x18-cell attribute grid are implemented —
//! palettes, block attributes and the window mask (Pan Docs "SGB Functions").
//!
//! Golden-safe by construction: [`Ppu::sgb`] is `Some` only on
//! `Model::Sgb`/`Sgb2`, so `Dmg`/`Cgb` output is byte-identical (see
//! `docs/hardware-state/sgb.md`).

use super::*;

/// The SNES-side presentation state an SGB applies over the DMG picture.
///
/// `pal[p]` is the four XRGB8888 colors of SGB palette `p` (0-3); `attr[cell]`
/// selects which palette colors cell `cell` (row-major, 20 wide, `y/8*20 +
/// x/8`); `mask` is the current MASK_EN mode (0 = off, 1 = freeze, 2 = black,
/// 3 = palette-0 color 0). Defaults reproduce the standard DMG greyscale so an
/// SGB machine that receives no palette command renders like a plain DMG.
#[derive(Clone)]
pub(super) struct SgbView {
    pal: [[u32; 4]; 4],
    attr: [u8; 360],
    mask: u8,
}

/// The standard four DMG shades as XRGB8888 (white, light, dark, black) — the
/// [`Ppu::dmg_palette`] default, reused so an un-commanded SGB looks like DMG.
const DMG_SHADES: [u32; 4] = [0xFF_FFFF, 0xAA_AAAA, 0x55_5555, 0x00_0000];

/// A little-endian BGR555 color (Pan Docs "SGB Palette Commands") expanded to
/// XRGB8888 by the straight 5→8 bit fill `(c << 3) | (c >> 2)` — identical to
/// [`Ppu::cgb_color`]'s channel expansion, no color correction in the core.
fn bgr555(lo: u8, hi: u8) -> u32 {
    let raw = u16::from(lo) | (u16::from(hi) << 8);
    let expand = |c: u16| -> u32 { u32::from(((c << 3) | (c >> 2)) & 0xFF) };
    let r = expand(raw & 0x1F);
    let g = expand((raw >> 5) & 0x1F);
    let b = expand((raw >> 10) & 0x1F);
    (r << 16) | (g << 8) | b
}

impl SgbView {
    pub(super) fn new() -> Self {
        Self {
            pal: [DMG_SHADES; 4],
            attr: [0; 360],
            mask: 0,
        }
    }

    /// Parse one completed SGB command packet stream (`cmd` = the command's
    /// bytes, `cmd[0]` = command number × 8 + packet count). Only the
    /// presentation commands with a visible effect are handled; the rest are
    /// deferred (see the module doc / `docs/hardware-state/sgb.md`).
    fn sgb_command(&mut self, cmd: &[u8]) {
        // Every handled command is at least one 16-byte packet; a shorter
        // slice is a malformed transfer — ignore it rather than index past
        // the end (the bytes originate from ROM-driven P1 pulses).
        if cmd.len() < 16 {
            return;
        }
        match cmd[0] >> 3 {
            // Pan Docs "SGB Command $00/$01/$02/$03" (PAL01/23/03/12): bytes
            // 1..15 are 7 BGR555 colors — color 0 is the shared entry 0 of all
            // four palettes, colors 1-3 fill the first named palette's entries
            // 1-3, colors 4-6 the second's.
            0x00 => self.set_pal(cmd, 0, 1), // PAL01
            0x01 => self.set_pal(cmd, 2, 3), // PAL23
            0x02 => self.set_pal(cmd, 0, 3), // PAL03
            0x03 => self.set_pal(cmd, 1, 2), // PAL12
            // Pan Docs "SGB Command $04" (ATTR_BLK).
            0x04 => self.attr_blk(cmd),
            // Pan Docs "SGB Command $17" (MASK_EN): byte 1 bits 0-1 select the
            // mask mode (0 cancel, 1 freeze, 2 black, 3 color-0).
            0x17 => self.mask = cmd[1] & 3,
            // ponytail: DEFERRED. PAL_TRN/PAL_SET ($0A/$0B) need a VRAM
            // transfer of the 512-entry SNES palette table; CHR_TRN/PCT_TRN +
            // borders ($13/$14) need a 256x224 output surface + frontend work;
            // ATTR_LIN/ATTR_DIV/ATTR_CHR ($05-$07) and the sound commands are
            // unhandled. Upgrade path: add the VRAM-snapshot hook + a wider
            // output buffer, then extend this match.
            _ => {}
        }
    }

    /// PAL01/23/03/12: `a`/`b` are the two palettes the command names. Color 0
    /// (bytes 1-2) is the shared background written to entry 0 of *all four*
    /// palettes; colors 1-3 fill `a`'s entries 1-3, colors 4-6 fill `b`'s.
    fn set_pal(&mut self, cmd: &[u8], a: usize, b: usize) {
        let color = |k: usize| bgr555(cmd[1 + 2 * k], cmd[2 + 2 * k]);
        let bg = color(0);
        for p in &mut self.pal {
            p[0] = bg;
        }
        for e in 1..4 {
            self.pal[a][e] = color(e);
            self.pal[b][e] = color(3 + e);
        }
    }

    /// ATTR_BLK ($04): byte 1 = number of 6-byte data sets (cap 18). Each set
    /// is `control` (bit0 inside / bit1 on-border / bit2 outside), `palettes`
    /// (bits0-1 inside, 2-3 border, 4-5 outside), then `x1,y1,x2,y2` in
    /// 20x18-cell coords. A cell strictly inside the rect is "inside", on its
    /// perimeter "on-border", beyond it "outside" — each region recolored only
    /// if its control bit is set.
    fn attr_blk(&mut self, cmd: &[u8]) {
        let sets = usize::from(cmd[1]).min(18);
        for s in 0..sets {
            let base = 2 + s * 6;
            if base + 6 > cmd.len() {
                break;
            }
            let control = cmd[base];
            let pals = cmd[base + 1];
            let inside_pal = pals & 3;
            let border_pal = (pals >> 2) & 3;
            let outside_pal = (pals >> 4) & 3;
            let (x1, y1, x2, y2) = (cmd[base + 2], cmd[base + 3], cmd[base + 4], cmd[base + 5]);
            for cy in 0u8..18 {
                for cx in 0u8..20 {
                    let inside = cx > x1 && cx < x2 && cy > y1 && cy < y2;
                    let outside = cx < x1 || cx > x2 || cy < y1 || cy > y2;
                    let (bit, pal) = if inside {
                        (0x01, inside_pal)
                    } else if outside {
                        (0x04, outside_pal)
                    } else {
                        (0x02, border_pal)
                    };
                    if control & bit != 0 {
                        self.attr[usize::from(cy) * 20 + usize::from(cx)] = pal;
                    }
                }
            }
        }
    }

    /// MASK_EN freeze (1) holds the last presented frame (the render is not
    /// swapped in). See [`Ppu::start_line`]'s frame-boundary handling.
    pub(super) fn holds_frame(&self) -> bool {
        self.mask == 1
    }

    /// MASK_EN black (2) / color-0 (3): the XRGB8888 fill to paint over the
    /// presented frame, or `None` for cancel/freeze.
    pub(super) fn mask_fill(&self) -> Option<u32> {
        match self.mask {
            2 => Some(0x00_0000),
            3 => Some(self.pal[0][0]),
            _ => None,
        }
    }

    pub(super) fn write_state(&self, w: &mut crate::state::Writer) {
        for row in &self.pal {
            w.u32_slice(row);
        }
        w.bytes(&self.attr);
        w.u8(self.mask);
    }

    pub(super) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        for row in &mut self.pal {
            r.u32_slice_into(row)?;
        }
        r.bytes_into(&mut self.attr)?;
        self.mask = r.u8()?;
        Ok(())
    }
}

impl Ppu {
    /// Apply a completed SGB command forwarded from the P1 write site. A no-op
    /// on non-SGB models (`self.sgb` is `None`), so it can be called
    /// unconditionally.
    pub(crate) fn sgb_command(&mut self, cmd: &[u8]) {
        if let Some(sgb) = self.sgb.as_mut() {
            sgb.sgb_command(cmd);
        }
    }

    /// Map a 2-bit DMG `shade` (0-3, already through BGP/OBP) to an XRGB8888
    /// color: through the SGB palette that cell `lx/8, ly/8` selects when an
    /// [`SgbView`] is present, else straight through [`Self::dmg_palette`]
    /// (byte-identical to the pre-SGB path on every non-SGB model).
    pub(super) fn dmg_shade(&self, lx: u8, shade: usize) -> u32 {
        match &self.sgb {
            Some(s) => {
                let cell = (usize::from(self.ly) / 8) * 20 + (usize::from(lx) / 8);
                let pal = usize::from(s.attr.get(cell).copied().unwrap_or(0) & 3);
                s.pal[pal][shade]
            }
            None => self.dmg_palette[shade],
        }
    }
}

#[cfg(test)]
#[path = "sgb_tests.rs"]
mod tests;
