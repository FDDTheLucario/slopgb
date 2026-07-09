# Super Game Boy (SGB) presentation

The SNES-side colorization, borders and sound state driven by SGB command
packets. The command-packet **receiver** (P1 pulse decoding, 16-byte packets,
multi-packet commands, MLT_REQ) lives in [`crate::joypad`] and is documented in
[io-misc.md](io-misc.md); this file covers the **presentation** half: what
happens to the completed non-MLT_REQ commands.

Faithful HLE port of SameBoy `Core/sgb.c` (`command_ready` / `GB_sgb_render`).
Sources: Pan Docs "SGB Functions" / "SGB Command $xx"; SameBoy `sgb.c` +
`sgb.h`. Only `Model::Sgb` / `Model::Sgb2` with an SGB-flagged cartridge
(`Cartridge::supports_sgb`: header 0x146 == 0x03 and 0x14B == 0x33) activate the
receiver.

## Wiring

`joypad.rs` `command_ready()` executes MLT_REQ (the only command with a
Game-Boy-bus-visible effect) and stashes every other completed command into
`Sgb::pending_cmd`. At the interconnect P1 write site (`memory.rs` `io_write`,
`0xFF00`), `Joypad::take_sgb_command()` is drained after the write and forwarded
to `Ppu::sgb_command(&cmd)`. `Ppu::sgb` is an `Option<SgbView>`
(`crate::ppu::sgb`), `Some` **only** on Sgb/Sgb2, so every non-SGB model is a
no-op end to end.

`SgbView` (`ppu/sgb.rs`) holds: `pal[4][4]` XRGB palettes, `attr[360]`
attribute map (row-major, `y/8*20 + x/8`), `mask` mode, the live `shade_buf`,
the transfer buffers (`ram_palettes`, `attr_files`, `border_tiles`,
`border_raw`), the recomposited `border_fb`, and the sound/flag/JUMP state. The
module is split into `ppu/sgb.rs` (struct + dispatch + dmg_shade + save-state) +
`ppu/sgb/{commands,transfer,border}.rs` (second `impl SgbView`/`impl Ppu` blocks
via `use super::*`), tests in `ppu/sgb_tests.rs`.

## The VRAM-transfer trap (the critical design point)

The `*_TRN` commands do **not** read VRAM. The SNES captures **the rendered
Game Boy screen** and reads its 2-bit pixel shades as packed 4bpp data — 160×144
pixels → 4096 bytes (Pan Docs "SGB Functions — VRAM Transfer"; SameBoy
`GB_sgb_render`'s `pixel_to_bits`). Since `Ppu::frame()` is already XRGB8888, the
shade is retained separately:

- `SgbView::shade_buf: [u8; 160*144]` — the live 2-bit shade of every rendered
  pixel, filled in `render/sprite.rs::output_pixel` **only** when
  `self.sgb.is_some()` (SGB-gated, so non-SGB pays nothing / stays
  byte-identical).
- A `*_TRN` command *latches* a destination (`pending_transfer`); at the next
  frame boundary (`line_setup.rs::start_line` line 144 → `sgb_frame_boundary`),
  `run_pending_transfer` decodes `shade_buf` and routes the bytes.
- `decode_tiles(shade, n_tiles)` reads the screen as a 20-tile-wide grid, each
  8×8 tile → 16 bytes of standard 2bpp (low byte = bit0 plane, high byte = bit1
  plane, x=0 = bit 7) — the universal representation every consumer reads.

## Implemented commands

| Cmd | Name | Effect |
|---|---|---|
| `$00`–`$03` | PAL01/23/03/12 | Set two named palettes from 7 BGR555 colors; color 0 is the shared entry-0 of all four palettes. |
| `$04` | ATTR_BLK | Rect fill; SameBoy quirk: inside-only (or outside-only) also fills the block border with that palette. |
| `$05` | ATTR_LIN | Per-line row/column fill (bit7 = horizontal, bits5-6 palette, bits0-4 line). |
| `$06` | ATTR_DIV | Screen split on a row/column into low/middle/high palettes. |
| `$07` | ATTR_CHR | Per-cell writes from a start cell, H or V order, 4 cells/byte high-pair first. |
| `$08` | SOUND | Queue an effect event (effect A/B, attenuation, effect-bank). Decode + state only. |
| `$09` | SOU_TRN | `*_TRN` → 4096-byte SPC700 program payload. |
| `$0A` | PAL_SET | Select 4 palettes (9-bit indices) from PAL_TRN RAM; byte9 bit7 → ATTR_SET, bit6 → cancel mask. |
| `$0B` | PAL_TRN | `*_TRN` → 512 palettes × 4 BGR555 colors into `ram_palettes`. |
| `$0C`–`$0E`,`$19` | ATRC_EN/TEST_EN/ICON_EN/PAL_PRI | Store flag (bit0), expose read-only. |
| `$0F` | DATA_SND | Store an inline SNES-RAM write packet (drained by host). |
| `$10` | DATA_TRN | `*_TRN` → 4096-byte SNES-RAM payload. |
| `$12` | JUMP | Latch the 24-bit SNES PC target (Phase 2). |
| `$13` | CHR_TRN | `*_TRN` → 4bpp border tiles; byte1 bit0 selects bank 0 (tiles 0-127) / 1 (128-255). |
| `$14` | PCT_TRN | `*_TRN` → 32×32 tilemap (offset 0) + border palettes 4-7 (offset 0x800). |
| `$15` | ATTR_TRN | `*_TRN` → 45 attribute files × 90 bytes. |
| `$16` | ATTR_SET | Load one of the 45 files into `attr`; byte1 bit6 cancels mask. |
| `$17` | MASK_EN | 0 cancel / 1 freeze / 2 black / 3 palette-0 color-0. |
| `$18` | OBJ_TRN | `*_TRN` → 4096-byte OBJ payload (stored, exposed). |

**Rendering** (`render/sprite.rs::output_pixel` → `Ppu::dmg_shade`): the DMG
paths compute a 2-bit shade (through BGP/OBP) then look it up as
`pal[attr[cell]][shade]` when an `SgbView` is present, else straight through
`dmg_palette` (byte-identical). The same shade is recorded into `shade_buf`.
BGR555→XRGB8888 uses the same `(c<<3)|(c>>2)` 5→8 expansion as `cgb_color`.

**MASK_EN** applies at the frame boundary (`line_setup.rs`): freeze skips the
buffer swap; black / color-0 fill the presented front buffer. Transfers read the
*live* `shade_buf` regardless of freeze (SameBoy reads `screen_buffer`, not the
frozen `effective_screen_buffer`).

## Border compositing model

`GameBoy::sgb_border() -> Option<&[u32; 256*224]>` returns the SNES border
surface (32×28 tiles of 8×8) with the colorized 160×144 GB screen composited as
an inset at (48, 40), or `None` until **both** a CHR_TRN and a PCT_TRN have
landed (or off SGB). Recomposited at each frame boundary (and after a state
load) into `SgbView::border_fb` from `front`:

1. Backdrop-fill (palette-0 color 0), then blit the GB inset.
2. For each 32×28 map entry (LE u16: tile index bits0-9 with `0x300` = "unused"
   skip, palette bits10-11, X-flip bit14, Y-flip bit15): draw the 4bpp tile
   (planes at `base`, `+1`, `+16`, `+17`). Color 0 over the GB area is
   transparent (inset shows through); color 0 elsewhere = backdrop; else
   `border_colors[color + palette*16]` (border palettes 4-7).

`Ppu::frame()` stays `&[u32; 160*144]` (the golden hash reads it). The frontend
(`crates/slopgb`: `video.rs` blit generalized to `(src_w, src_h)`, `main.rs`
`redraw`) renders `sgb_border()` in place of `frame()` automatically when it is
`Some` — letterboxed/scaled, no new option.

## Phase-2/3 audio + SNES-RAM seams

Sound is **decode + state only** this phase (no synthesis). Exposed on
`GameBoy`, read-only / drain:

- `sgb_take_sound_event() -> Option<SgbSound>` — drains the SOUND ($08) queue.
- `sgb_sou_trn_data() -> Option<&[u8]>` — the SOU_TRN SPC700 program.
- `sgb_take_data_snd() -> Option<Vec<u8>>` — drains DATA_SND ($0F) packets.
- `sgb_data_trn_data()` / `sgb_obj_trn_data()` — DATA_TRN / OBJ_TRN payloads.
- `sgb_flags() -> Option<SgbFlags>` — ATRC_EN/TEST_EN/ICON_EN/PAL_PRI + JUMP.

**Phase 2 (SPC700)** plugs into `sgb_sou_trn_data()` (program upload),
`sgb_take_data_snd()` (RAM writes), and `SgbFlags::jump` (program jump).
**Phase 3 (S-DSP)** plugs into `sgb_take_sound_event()` (effect triggers). The
SOUND/DATA_SND queues are bounded (`SOUND_QUEUE_CAP`, oldest dropped) so a
never-draining host cannot leak.

## Save-states

All durable `SgbView` state round-trips (`ppu/sgb.rs::write_state`/`read_state`,
called from `ppu/state.rs`): palettes, attr map, mask, `shade_buf`,
`pending_transfer`, `ram_palettes`, `attr_files`, `border_tiles`, `border_raw`,
`has_chr`/`has_pct`, the OBJ/SOU/DATA payloads, the DATA_SND + SOUND queues, the
flags + JUMP. `border_fb` is derived (recomposited on load, not serialized).
`pending_cmd` (in `joypad.rs`) is transient (set + drained inside one P1 write).

## Golden-safety

`Ppu::sgb` is `None` on every model the golden set runs (`Dmg` / `Cgb`), so
`dmg_shade` (None → `dmg_palette`, byte-identical), the `shade_buf` write (gated
on `self.sgb.as_mut()`), `sgb_frame_boundary` (gated), the border composite
(gated), and the state stream (a single `bool(false)` when `None`) all reduce to
the pre-SGB path bit-for-bit. `golden_fingerprint` was **not** run (it hangs in
this environment); golden-safety is proven by inspection — every new path is
behind an `sgb.is_some()` check. Verified: mooneye 439/439 rom×model
(`SLOPGB_REQUIRE_ROMS=1`); core lib + frontend tests green; clippy `-D warnings`
clean.

## Tests

- `ppu/sgb_tests.rs` — parser units: PAL01/12, ATTR_BLK (incl. the inside-only
  border promotion), ATTR_LIN/DIV/CHR, ATTR_SET, PAL_SET, PAL_TRN screen-decode,
  SOUND/DATA_SND queues, flags/JUMP, the queue cap, the border composite
  (transparency + tile draw), and a full save-state round-trip.
- `lib_tests.rs::sgb_pal01_colorizes_rendered_frame` — end-to-end: a PAL01
  packet driven through the real `Joypad` P1 pulse stream recolors a rendered
  frame (joypad → interconnect → ppu → render).
- `video.rs` tests — the generalized `(src_w, src_h)` blit/stretch.

## Deferred / not this phase

- **SPC700 CPU (Phase 2)** + **S-DSP audio synthesis (Phase 3)** — the seams
  above are the plug points; the queues/payloads are stored, not consumed.
- **Boot intro animation / jingle** (SameBoy `render_boot_animation` /
  `render_jingle`) — cosmetic SNES boot sequence, not emulated.
- **Border fade animation** (`border_animation`) — the border is committed
  immediately on CHR_TRN+PCT_TRN, no cross-fade.
- **Built-in default border / palette-by-title** (SameBoy
  `GB_sgb_load_default_data` / `palette_assignments`) — `sgb_border()` returns
  `None` until the ROM sends its own border.
