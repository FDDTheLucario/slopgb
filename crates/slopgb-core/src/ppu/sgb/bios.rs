//! Optional **user-supplied SGB BIOS** seam: install the SGB firmware's real
//! default border and its title→palette table.
//!
//! slopgb never runs the SNES CPU (it is a high-level SGB emulation), so it
//! cannot *execute* the firmware to reproduce Nintendo's border/palettes. The
//! frontend (which the user points at their own BIOS copy) locates the border
//! payload and the palette table in the firmware and hands them to these
//! entry points — the only path by which anything Nintendo-derived enters, and
//! only ever in the user's own runtime, never committed to this repo. Absent a
//! BIOS the original default border (`defaults.rs`) and the neutral DMG palette
//! stand.
//!
//! These are frontend seams: the `GameBoy`-level wrapper that reaches them is
//! [`crate::GameBoy::load_sgb_bios`] (via `lib/sgb_api.rs`), the single BIOS
//! entry point. The locators it funnels through find nothing verifiable in a
//! bare SNES image (slopgb never runs the 65816), so the seams are the wired
//! upgrade path a checked payload locator drops into.

use super::*;

impl SgbView {
    /// Install a border extracted from a user-supplied BIOS: fill the tile
    /// banks + tilemap/palette payload and mark the border ready (so
    /// [`SgbView::composite`] renders it), cross-fading from the current
    /// surface. `chr0`/`chr1` are the two 4096-byte SNES-4bpp tile banks
    /// (`chr1` may be empty), `pct` the 2176-byte tilemap + border-palette
    /// payload — the exact formats the `CHR_TRN`/`PCT_TRN` path produces.
    /// Rejects mis-sized payloads (returns `false`, keeps the default).
    fn install_border(&mut self, chr0: &[u8], chr1: &[u8], pct: &[u8]) -> bool {
        if chr0.len() != 4096 || pct.len() != 2176 || (!chr1.is_empty() && chr1.len() != 4096) {
            return false;
        }
        self.border_tiles[0..4096].copy_from_slice(chr0);
        if chr1.len() == 4096 {
            self.border_tiles[4096..8192].copy_from_slice(chr1);
        }
        self.border_raw.copy_from_slice(pct);
        self.has_chr = true;
        self.has_pct = true;
        self.fade_pending = true;
        true
    }

    /// Install a BIOS-extracted title palette: select `table[checksum % len]`
    /// (four BGR555 colours) and apply it to all four SGB palettes — the
    /// whole-screen colorization a non-SGB-aware cart receives (SameBoy
    /// `GB_sgb_load_default_data` / `palette_assignments`). No-op on an empty
    /// table (keeps the neutral DMG default).
    fn apply_title_palette(&mut self, title: &[u8], table: &[[u16; 4]]) {
        if table.is_empty() {
            return;
        }
        let entry = table[usize::from(title_checksum(title)) % table.len()];
        let colors = entry.map(bgr555_word);
        self.pal = [colors; 4];
    }
}

/// The documented ROM header title checksum: the 8-bit sum of the title bytes
/// (`0x0134..0x0143`). The CGB boot ROM uses this same sum to index its
/// DMG-compat palette table; the SGB firmware uses an analogous hash. The
/// *table* it indexes is Nintendo's and is **not** shipped here.
fn title_checksum(title: &[u8]) -> u8 {
    title.iter().copied().fold(0u8, u8::wrapping_add)
}

/// A BGR555 `u16` expanded to XRGB8888 (same `(c<<3)|(c>>2)` fill as
/// [`bgr555`], but from a packed word rather than two bytes).
fn bgr555_word(w: u16) -> u32 {
    bgr555(w as u8, (w >> 8) as u8)
}

impl Ppu {
    /// Install a border extracted from a user-supplied SGB BIOS, bypassing the
    /// `CHR_TRN`/`PCT_TRN` command path. A no-op off SGB; returns whether it
    /// was installed. Golden-safe (gated on `self.sgb`). Frontend seam.
    pub(crate) fn sgb_install_border(&mut self, chr0: &[u8], chr1: &[u8], pct: &[u8]) -> bool {
        self.sgb
            .as_mut()
            .is_some_and(|s| s.install_border(chr0, chr1, pct))
    }

    /// Apply a BIOS-extracted title→palette table for a non-SGB-aware cart:
    /// hash the header `title`, index `table`, install the palette. A no-op off
    /// SGB or on an empty table (the neutral DMG default stands). Golden-safe.
    /// Frontend seam.
    pub(crate) fn sgb_apply_bios_palette(&mut self, title: &[u8], table: &[[u16; 4]]) {
        if let Some(s) = self.sgb.as_mut() {
            s.apply_title_palette(title, table);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_checksum_is_byte_sum() {
        assert_eq!(title_checksum(&[1, 2, 3]), 6);
        assert_eq!(title_checksum(&[0xFF, 0x02]), 0x01); // wraps
        assert_eq!(title_checksum(b""), 0);
    }

    /// The title→palette hook installs the selected BGR555 4-colour set into all
    /// four SGB palettes; an empty table leaves the neutral DMG default.
    #[test]
    fn apply_title_palette_installs_and_neutral_default() {
        let mut ppu = Ppu::new(Model::Sgb);
        // Two-entry table; checksum of b"AB" = 0x83, %2 = 1 → entry 1 (white).
        let table = [[0, 0, 0, 0], [0x7FFF, 0x7FFF, 0x7FFF, 0x7FFF]];
        ppu.sgb_apply_bios_palette(b"AB", &table);
        let s = ppu.sgb.as_ref().unwrap();
        assert_eq!(
            s.pal, [[0xFF_FFFF; 4]; 4],
            "entry 1 white applied to all palettes"
        );

        // Empty table is a no-op: DMG greyscale default stands.
        let mut ppu = Ppu::new(Model::Sgb);
        ppu.sgb_apply_bios_palette(b"AB", &[]);
        assert_eq!(ppu.sgb.as_ref().unwrap().pal, [DMG_SHADES; 4]);

        // Off SGB: no panic, no view.
        let mut dmg = Ppu::new(Model::Dmg);
        dmg.sgb_apply_bios_palette(b"AB", &table);
        assert!(dmg.sgb.is_none());
    }

    /// A BIOS border install marks the border ready and renders through the ROM
    /// composite path; a mis-sized payload is rejected and keeps the default.
    #[test]
    fn install_border_validates_and_marks_ready() {
        let mut ppu = Ppu::new(Model::Sgb);
        let chr0 = vec![0u8; 4096];
        let mut pct = vec![0u8; 2176];
        // Tilemap entry (0,0) → tile 0 pal 0; border palette 4 colour 1 = red.
        pct[2048 + 2] = 0x1F;
        assert!(
            ppu.sgb_install_border(&chr0, &[], &pct),
            "well-sized install accepted"
        );
        ppu.sgb_composite_border();
        // Tile 0 is all-zero (colour 0) over a non-GB cell → backdrop, but the
        // border is now ready (ROM path), not the default.
        assert!(ppu.sgb.as_ref().unwrap().border_ready());

        // Wrong size rejected.
        let mut ppu = Ppu::new(Model::Sgb);
        assert!(!ppu.sgb_install_border(&[0u8; 10], &[], &[0u8; 2176]));
        assert!(!ppu.sgb.as_ref().unwrap().border_ready(), "default kept");
    }
}
