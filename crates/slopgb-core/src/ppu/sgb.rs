//! Super Game Boy presentation layer: the SNES-side colorization of the DMG
//! output driven by SGB command packets. A behaviour-preserving submodule of
//! [`Ppu`] (a second `impl` block via `use super::*`).
//!
//! The ICD2 command-packet *receiver* lives in [`crate::joypad`]; a completed
//! non-MLT_REQ command is forwarded here from the interconnect's P1 write site
//! (`Joypad::take_sgb_command` → [`Ppu::sgb_command`]). This module is a faithful
//! HLE port of SameBoy `Core/sgb.c` (`command_ready` / `GB_sgb_render`) — the
//! palettes, attribute grid, window mask, the screen-capture VRAM transfers
//! (`*_TRN`), the 256×224 border composite, and the sound/flag/data command
//! state (Phase-2/3 audio seams).
//!
//! **The VRAM-transfer trap:** the `*_TRN` commands do NOT read VRAM. The SNES
//! captures the *rendered Game Boy screen* and reads its 2-bit pixel shades as
//! packed 4bpp data — 160×144 pixels → 4096 bytes (Pan Docs "SGB Functions —
//! VRAM Transfer"; SameBoy `GB_sgb_render`'s `pixel_to_bits` packing). Since
//! [`Ppu::frame`] is already XRGB8888, the shade information is retained in a
//! separate [`SgbView::shade_buf`] filled during render (SGB-gated).
//!
//! Golden-safe by construction: [`Ppu::sgb`] is `Some` only on
//! `Model::Sgb`/`Sgb2`, so `Dmg`/`Cgb` output is byte-identical (see
//! `docs/hardware-state/sgb.md`). Every field and code path here is reached
//! only through that `Some`.

use super::*;
use crate::{SgbSound, SCREEN_H, SCREEN_W};

mod bios;
mod border;
mod commands;
mod defaults;
mod transfer;

/// 256×224 SNES border surface dimensions (32×28 tiles of 8×8).
pub(super) const BORDER_W: usize = crate::SGB_BORDER_W;
pub(super) const BORDER_H: usize = crate::SGB_BORDER_H;
pub(super) const BORDER_PIXELS: usize = BORDER_W * BORDER_H;
/// The GB screen inset origin in the border surface: tile (6,5) → px (48,40).
pub(super) const INSET_X: usize = 48;
pub(super) const INSET_Y: usize = 40;

/// `pending_transfer` destination codes: which buffer the next captured screen
/// (4096 bytes) is routed into. Stored as a `u8` so the save-state stream stays
/// a flat scalar (SameBoy's `transfer_dest`).
pub(super) const TR_PAL: u8 = 0; // PAL_TRN  → ram_palettes (512 palettes × 4 colors)
pub(super) const TR_ATTR: u8 = 1; // ATTR_TRN → attr_files (45 files × 90 bytes)
pub(super) const TR_CHR0: u8 = 2; // CHR_TRN  → border tiles, bank 0 (tiles 0-127)
pub(super) const TR_CHR1: u8 = 3; // CHR_TRN  → border tiles, bank 1 (tiles 128-255)
pub(super) const TR_PCT: u8 = 4; // PCT_TRN  → border tilemap + palettes
pub(super) const TR_OBJ: u8 = 5; // OBJ_TRN  → obj_data (SGB OBJ palettes/attrs)
pub(super) const TR_SOU: u8 = 6; // SOU_TRN  → sou_trn (SPC700 program)
pub(super) const TR_DATA: u8 = 7; // DATA_TRN → data_trn (to SNES RAM)

/// The standard four DMG shades as XRGB8888 (white, light, dark, black) — the
/// [`Ppu::dmg_palette`] default, reused so an un-commanded SGB looks like DMG.
const DMG_SHADES: [u32; 4] = [0xFF_FFFF, 0xAA_AAAA, 0x55_5555, 0x00_0000];

/// Max queued SGB SOUND / DATA_SND events before the oldest is dropped: a
/// misbehaving ROM must never grow the queue without bound if the host never
/// drains it. Real ROMs emit a handful per frame.
const SOUND_QUEUE_CAP: usize = 64;

/// Border boot-intro / cross-fade length, in frames (~0.4 s at 60 Hz). The
/// power-on default border fades up from black; a later `CHR_TRN`/`PCT_TRN`
/// cross-fades from the previous border. Purely presentational.
const FADE_LEN: u16 = 24;

/// The SNES-side presentation state an SGB applies over the DMG picture.
///
/// `pal[p]` is the four XRGB8888 colors of SGB palette `p` (0-3); `attr[cell]`
/// selects which palette recolors cell `cell` (row-major, 20 wide, `y/8*20 +
/// x/8`); `mask` is the current MASK_EN mode (0 = off, 1 = freeze, 2 = black,
/// 3 = palette-0 color 0). Defaults reproduce the standard DMG greyscale so an
/// SGB machine that receives no palette command renders like a plain DMG.
///
/// Big buffers are boxed so `Option<SgbView>` (inline in [`Ppu`]) stays small
/// on the non-SGB models that never allocate one.
#[derive(Clone)]
pub(super) struct SgbView {
    pal: [[u32; 4]; 4],
    attr: [u8; 360],
    mask: u8,

    /// The live 2-bit shade of every rendered pixel (SameBoy `screen_buffer`),
    /// filled in `render/sprite.rs::output_pixel`. The source a `*_TRN` capture
    /// reads — NOT `front`, whose shade is lost to XRGB8888.
    shade_buf: Box<[u8; SCREEN_PIXELS]>,
    /// A `*_TRN` command latched a screen capture into this destination
    /// (`TR_*`); consumed at the next frame boundary. `None` when idle.
    pending_transfer: Option<u8>,

    /// PAL_TRN palette RAM: 512 SNES palettes × 4 colors × 2 bytes (BGR555 LE).
    ram_palettes: Box<[u8; 4096]>,
    /// ATTR_TRN attribute files: 45 files × 90 bytes (2 bits/cell, 360 cells).
    attr_files: Box<[u8; 4050]>,

    /// CHR_TRN border tile data: 256 tiles × 32 bytes (SNES 4bpp), banks 0/1.
    border_tiles: Box<[u8; 8192]>,
    /// PCT_TRN border data: 32×32 tilemap (LE u16, offset 0) + 4 border palettes
    /// 4-7 (16 BGR555 colors each, offset 0x800). SameBoy `border.raw_data`.
    border_raw: Box<[u8; 2176]>,
    /// A CHR_TRN and a PCT_TRN have both landed — the border is displayable.
    has_chr: bool,
    has_pct: bool,

    /// Recomposited 256×224 border surface (GB screen inset + border tiles).
    /// Derived state (rebuilt each frame boundary), so not serialized.
    border_fb: Box<[u32; BORDER_PIXELS]>,

    /// Boot-intro / cross-fade state (presentational). `fade` = frames left in
    /// the current transition (0 = settled); `fade_from` = the surface being
    /// blended *from*; `fade_pending` = a border-changing transfer landed this
    /// frame boundary and a new fade must start. None of it is serialized — a
    /// loaded state resolves to settled (see `read_state`).
    fade: u16,
    fade_from: Box<[u32; BORDER_PIXELS]>,
    fade_pending: bool,

    // --- Phase 2/3 seams: stored, exposed read-only, not consumed this phase ---
    /// OBJ_TRN ($18) captured payload (SGB OBJ palettes/attributes).
    obj_data: Option<Box<[u8; 4096]>>,
    /// SOU_TRN ($09) captured SPC700 program (Phase 3 S-DSP feeds this).
    sou_trn: Option<Box<[u8; 4096]>>,
    /// DATA_TRN ($10) captured payload destined for SNES RAM.
    data_trn: Option<Box<[u8; 4096]>>,
    /// DATA_SND ($0F) inline packets written to SNES RAM (drained by the host).
    data_snd: Vec<Vec<u8>>,
    /// SOUND ($08) effect events (drained by the host / Phase 3).
    sound_events: Vec<SgbSound>,

    /// ATRC_EN ($0C) / TEST_EN ($0D) / ICON_EN ($0E) / PAL_PRI ($19) flags.
    atrc_en: bool,
    test_en: bool,
    icon_en: bool,
    pal_pri: bool,
    /// JUMP ($12) SNES target (24-bit PC), latched for Phase 2.
    jump: Option<u32>,
}

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

fn boxed_u8<const N: usize>() -> Box<[u8; N]> {
    vec![0u8; N]
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

fn boxed_u32<const N: usize>(fill: u32) -> Box<[u32; N]> {
    vec![fill; N]
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

impl SgbView {
    pub(super) fn new() -> Self {
        let mut v = Self {
            pal: [DMG_SHADES; 4],
            attr: [0; 360],
            mask: 0,
            shade_buf: boxed_u8(),
            pending_transfer: None,
            ram_palettes: boxed_u8(),
            attr_files: boxed_u8(),
            border_tiles: boxed_u8(),
            border_raw: boxed_u8(),
            has_chr: false,
            has_pct: false,
            border_fb: boxed_u32(0),
            // Boot intro: the default border fades up from black.
            fade: FADE_LEN,
            fade_from: boxed_u32(0),
            fade_pending: false,
            obj_data: None,
            sou_trn: None,
            data_trn: None,
            data_snd: Vec::new(),
            sound_events: Vec::new(),
            atrc_en: false,
            test_en: false,
            icon_en: false,
            pal_pri: false,
            jump: None,
        };
        // Seed the default border so `sgb_border()` is valid before the first
        // frame renders (power-on shows the original built-in border).
        v.default_composite(None);
        v
    }

    /// Parse one completed SGB command packet stream (`cmd` = the command's
    /// bytes, `cmd[0]` = command number × 8 + packet count). Dispatch mirrors
    /// SameBoy `command_ready` (`Core/sgb.c`); the per-command layouts are cited
    /// on each handler. MLT_REQ ($11) never arrives here — it is executed by the
    /// joypad receiver, the only command with a Game-Boy-bus-visible effect.
    fn sgb_command(&mut self, cmd: &[u8]) {
        // Every handled command is at least one 16-byte packet; a shorter slice
        // is a malformed transfer — ignore it rather than index past the end
        // (the bytes originate from ROM-driven P1 pulses).
        if cmd.len() < 16 {
            return;
        }
        match cmd[0] >> 3 {
            // Pan Docs "SGB Command $00-$03" (PAL01/23/03/12).
            0x00 => self.set_pal(cmd, 0, 1),
            0x01 => self.set_pal(cmd, 2, 3),
            0x02 => self.set_pal(cmd, 0, 3),
            0x03 => self.set_pal(cmd, 1, 2),
            // Attribute-grid fills (Pan Docs "SGB Command $04-$07").
            0x04 => self.attr_blk(cmd),
            0x05 => self.attr_lin(cmd),
            0x06 => self.attr_div(cmd),
            0x07 => self.attr_chr(cmd),
            // Sound / audio (decode + state only this phase; see the seams).
            0x08 => self.sound(cmd),             // SOUND
            0x09 => self.latch_transfer(TR_SOU), // SOU_TRN
            // Palette RAM select / transfer (Pan Docs "SGB Command $0A/$0B").
            0x0A => self.pal_set(cmd),
            0x0B => self.latch_transfer(TR_PAL),
            // Flags (Pan Docs "SGB Command $0C-$0E/$19"): store, expose read-only.
            0x0C => self.atrc_en = cmd[1] & 1 != 0,
            0x0D => self.test_en = cmd[1] & 1 != 0,
            0x0E => self.icon_en = cmd[1] & 1 != 0,
            0x0F => self.data_snd(cmd),           // DATA_SND
            0x10 => self.latch_transfer(TR_DATA), // DATA_TRN
            0x12 => self.jump(cmd),               // JUMP
            0x13 => self.latch_transfer(if cmd[1] & 1 != 0 { TR_CHR1 } else { TR_CHR0 }),
            0x14 => self.latch_transfer(TR_PCT),
            0x15 => self.latch_transfer(TR_ATTR),
            0x16 => self.attr_set(cmd), // ATTR_SET
            // Pan Docs "SGB Command $17" (MASK_EN): 0 cancel, 1 freeze, 2 black,
            // 3 palette-0 color-0.
            0x17 => self.mask = cmd[1] & 3,
            0x18 => self.latch_transfer(TR_OBJ),    // OBJ_TRN
            0x19 => self.pal_pri = cmd[1] & 1 != 0, // PAL_PRI
            _ => {}
        }
    }

    /// PAL01/23/03/12 ($00-$03): `a`/`b` are the two palettes the command names.
    /// Color 0 (bytes 1-2) is the shared background written to entry 0 of *all
    /// four* palettes; colors 1-3 fill `a`'s entries 1-3, colors 4-6 fill `b`'s
    /// (SameBoy `pal_command`).
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

    /// Record the live 2-bit shade of a rendered pixel (called per-pixel from
    /// `output_pixel`, SGB-only). `idx` is the framebuffer index (`ly*160+lx`).
    pub(super) fn record_shade(&mut self, idx: usize, shade: u8) {
        if let Some(slot) = self.shade_buf.get_mut(idx) {
            *slot = shade & 3;
        }
    }

    /// The SGB-colorized XRGB8888 for a 2-bit DMG `shade` at screen `lx`,`ly`:
    /// through the palette that the cell `(lx/8, ly/8)` selects.
    fn shade_color(&self, lx: u8, ly: u8, shade: usize) -> u32 {
        let cell = (usize::from(ly) / 8) * 20 + (usize::from(lx) / 8);
        let pal = usize::from(self.attr.get(cell).copied().unwrap_or(0) & 3);
        self.pal[pal][shade]
    }

    pub(super) fn write_state(&self, w: &mut crate::state::Writer) {
        for row in &self.pal {
            w.u32_slice(row);
        }
        w.bytes(&self.attr);
        w.u8(self.mask);
        w.bytes(&self.shade_buf[..]);
        match self.pending_transfer {
            Some(d) => {
                w.bool(true);
                w.u8(d);
            }
            None => w.bool(false),
        }
        w.bytes(&self.ram_palettes[..]);
        w.bytes(&self.attr_files[..]);
        w.bytes(&self.border_tiles[..]);
        w.bytes(&self.border_raw[..]);
        w.bool(self.has_chr);
        w.bool(self.has_pct);
        write_opt_box(w, &self.obj_data);
        write_opt_box(w, &self.sou_trn);
        write_opt_box(w, &self.data_trn);
        w.u32(self.data_snd.len() as u32);
        for p in &self.data_snd {
            w.u32(p.len() as u32);
            w.bytes(p);
        }
        w.u32(self.sound_events.len() as u32);
        for s in &self.sound_events {
            w.u8(s.effect_a);
            w.u8(s.effect_b);
            w.u8(s.attenuation);
            w.u8(s.effect_bank);
        }
        w.bool(self.atrc_en);
        w.bool(self.test_en);
        w.bool(self.icon_en);
        w.bool(self.pal_pri);
        match self.jump {
            Some(t) => {
                w.bool(true);
                w.u32(t);
            }
            None => w.bool(false),
        }
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
        r.bytes_into(&mut self.shade_buf[..])?;
        self.pending_transfer = if r.bool()? { Some(r.u8()?) } else { None };
        r.bytes_into(&mut self.ram_palettes[..])?;
        r.bytes_into(&mut self.attr_files[..])?;
        r.bytes_into(&mut self.border_tiles[..])?;
        r.bytes_into(&mut self.border_raw[..])?;
        self.has_chr = r.bool()?;
        self.has_pct = r.bool()?;
        self.obj_data = read_opt_box(r)?;
        self.sou_trn = read_opt_box(r)?;
        self.data_trn = read_opt_box(r)?;
        let n = r.u32()? as usize;
        self.data_snd = Vec::new();
        for _ in 0..n {
            let len = r.u32()? as usize;
            self.data_snd.push(r.bytes_vec(len)?);
        }
        let n = r.u32()? as usize;
        self.sound_events = Vec::new();
        for _ in 0..n {
            self.sound_events.push(SgbSound {
                effect_a: r.u8()?,
                effect_b: r.u8()?,
                attenuation: r.u8()?,
                effect_bank: r.u8()?,
            });
        }
        self.atrc_en = r.bool()?;
        self.test_en = r.bool()?;
        self.icon_en = r.bool()?;
        self.pal_pri = r.bool()?;
        self.jump = if r.bool()? { Some(r.u32()?) } else { None };
        // The fade is transient presentation, not serialized: a loaded state
        // resolves to a settled border (no replayed boot intro / cross-fade).
        self.fade = 0;
        self.fade_pending = false;
        Ok(())
    }
}

fn write_opt_box<const N: usize>(w: &mut crate::state::Writer, b: &Option<Box<[u8; N]>>) {
    match b {
        Some(data) => {
            w.bool(true);
            w.bytes(&data[..]);
        }
        None => w.bool(false),
    }
}

fn read_opt_box<const N: usize>(
    r: &mut crate::state::Reader<'_>,
) -> Result<Option<Box<[u8; N]>>, crate::state::StateError> {
    if r.bool()? {
        let mut b = boxed_u8::<N>();
        r.bytes_into(&mut b[..])?;
        Ok(Some(b))
    } else {
        Ok(None)
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
            Some(s) => s.shade_color(lx, self.ly, shade),
            None => self.dmg_palette[shade],
        }
    }

    /// Record a rendered pixel's 2-bit shade for the SGB screen-capture (VRAM
    /// transfer) path. No-op off SGB. `idx` = `ly*160 + lx`.
    pub(super) fn record_sgb_shade(&mut self, idx: usize, shade: u8) {
        if let Some(s) = self.sgb.as_mut() {
            s.record_shade(idx, shade);
        }
    }

    /// SGB frame-boundary work (called from [`Self::start_line`] at line 144):
    /// consume a pending `*_TRN` screen capture, recomposite the border, and
    /// advance the boot-intro / cross-fade blend. A no-op off SGB (`self.sgb`
    /// is `None`) so the whole call is inert on DMG/CGB — golden-safe.
    pub(super) fn sgb_frame_boundary(&mut self) {
        if let Some(s) = self.sgb.as_mut() {
            s.run_pending_transfer();
            // A border-changing transfer landed: snapshot the *current* (still
            // pre-recomposite) surface as the cross-fade source, then start a
            // fade. Done before `sgb_composite_border` overwrites `border_fb`.
            if std::mem::take(&mut s.fade_pending) {
                s.fade_from.copy_from_slice(&s.border_fb[..]);
                s.fade = FADE_LEN;
            }
        }
        self.sgb_composite_border();
        if let Some(s) = self.sgb.as_mut() {
            s.apply_fade();
        }
    }
}

#[cfg(test)]
#[path = "sgb_tests.rs"]
mod tests;
