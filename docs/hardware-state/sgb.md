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
`border_raw`), the recomposited `border_fb`, the boot-intro/cross-fade state (`fade`,
`fade_from`, `fade_pending` — presentational, not serialized), and the
sound/flag/JUMP state. The module is split into `ppu/sgb.rs` (struct + dispatch
+ dmg_shade + save-state + frame boundary) + `ppu/sgb/{commands,transfer,border,
defaults,bios}.rs` (second `impl SgbView`/`impl Ppu` blocks via `use super::*`;
`defaults.rs` = original default border, `bios.rs` = the optional user-BIOS
seam), tests in `ppu/sgb_tests.rs` + `bios.rs`'s own `#[cfg(test)]`.

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
an inset at (48, 40). It is **always `Some` on an SGB** (`None` only off SGB):
the built-in **default border** (below) shows from power-on until a ROM sends
its own CHR_TRN+PCT_TRN, after which the ROM border replaces it.
`sgb_composite_border` picks the path each frame boundary — `border_ready()`
(`has_chr && has_pct`) → the ROM `composite`, else `default_composite`.
Recomposited at each frame boundary (and after a state load) into
`SgbView::border_fb` from `front`:

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

## Default border (original — no BIOS needed)

On real hardware the SGB's built-in border lives in the SNES-side firmware and
is uploaded by SNES code. slopgb is a **high-level** SGB emulation — it never
runs the SNES CPU — so that firmware never executes, and a plain DMG game would
otherwise show no border. `ppu/sgb/defaults.rs`'s `default_composite` instead
draws an **original**, procedurally-generated frame (a neutral slate backdrop, a
steel-blue beveled bezel around the GB inset, and a thin outer edge line — plain
rectangle fills, `outline()`). **No Nintendo artwork is embedded or copied.**
Seeded in `SgbView::new()` so `sgb_border()` is valid from power-on. Shown until
a ROM's CHR_TRN+PCT_TRN lands, then the ROM border takes over.

## Boot intro & cross-fade (presentational)

`ppu/sgb/border.rs::apply_fade` blends the border in over `FADE_LEN` (24) frames
at the frame boundary, driven by a `fade` counter (linear, settles exactly at
100% target). Two triggers:

- **Boot intro** — `new()` sets `fade = FADE_LEN`, `fade_from = black`, so the
  default border fades up from black over the first ~0.4 s.
- **Cross-fade** — a CHR_TRN/PCT_TRN transfer (`run_pending_transfer`) or a BIOS
  border install sets `fade_pending`; the next frame boundary snapshots the old
  surface into `fade_from` and restarts the fade, cross-fading old → new.

The blend covers the whole surface (inset included) — the intended "fade the
border in" effect, and a brief, barely-visible ghost of the live screen during
a mid-game border swap (games change borders rarely). It is **purely
presentational**: nothing is serialized (a loaded state resolves to a settled
border — `read_state` zeroes `fade`), and it never touches `frame()`.

`ATRC_EN` ($0C) — the SGB "attraction" flag — is decoded and exposed via
`sgb_flags()`. Its documented role is the firmware's idle attraction *demo*
(a SNES-side animation slopgb does not emulate), **not** the per-border fade, so
the boot intro / cross-fade is not gated on it (it always plays).

## Optional user-supplied SGB BIOS (`ppu/sgb/bios.rs`)

### The single BIOS entry point

`GameBoy::load_sgb_bios(&[u8])` (`lib/sgb_api.rs`) is the **one** BIOS entry
point. The frontend loads a user-owned SGB image (`--sgb-bios <path>` /
`SLOPGB_SGB_BIOS`, mirroring `--boot`) and hands the bytes here; the call funnels
to everything a BIOS can feed:

1. **Audio** — the image goes to the APU (`SgbApu::load_bios`) exactly as
   before. See [sgb-audio.md](sgb-audio.md).
2. **Border + title→palette** — the two `Ppu` seams below, reached only through
   this entry point (there is no second public way in).

The two `Ppu` seams (`pub(crate)`, `Model::Sgb`/`Sgb2`-gated) install
Nintendo-derived data that may only ever enter at **runtime from the user's own
file — nothing Nintendo-derived is committed to this repo**:

- `Ppu::sgb_install_border(chr0, chr1, pct)` — install a real border: the two
  4096-byte SNES-4bpp tile banks + the 2176-byte tilemap/palette payload (the
  exact CHR_TRN/PCT_TRN formats). Validates sizes, marks the border ready, and
  cross-fades it in. Returns `false` (keeps the default) on a mis-sized payload.
- `Ppu::sgb_apply_bios_palette(title, table)` — the **palette-by-title** hook
  for non-SGB-aware carts: `title_checksum` (the documented 8-bit sum of the
  header title bytes `0x134..0x143` — the same shape the CGB boot ROM uses)
  indexes the BIOS-extracted `table`, whose four BGR555 colours fill all four
  SGB palettes. **No Nintendo table is shipped**, so standalone the neutral DMG
  greyscale default stands; the table is BIOS-supplied.

The seams are wired but reachable **only** via `load_sgb_bios`, so `bios.rs`
compiles without an `allow(dead_code)` blanket (removed 2026-07-09).

### What a user BIOS enables today — and the honest limit

**Today `--sgb-bios` feeds only the audio path.** slopgb is a high-level SGB
emulation: **it never runs the SNES 65816.** So it can neither *execute* the
firmware to build the border/palette, nor trust a raw byte offset for them — the
border tiles and title→palette table live in the SNES ROM at
firmware-revision-specific locations that are not discoverable from a bare image
without a documented, checked structure. An unverifiable guess would ship a
**wrong** border dressed up as right, so `load_sgb_bios`'s two locators
(`sgb_bios_border` / `sgb_bios_palette` in `lib/sgb_api.rs`) return `None`, the
seams stay unfed, and the **original default border + neutral palette stand**.
The frontend logs this plainly on load.

**What remains impossible without emulating the SNES 65816:** the *real*
Nintendo border and the per-title palette table. Recovering them needs the SNES
CPU to run the firmware (decompress/DMA the border, run the title hash), which
slopgb does not have. The seams are the documented upgrade path: a locator that
first validates a documented BIOS structure drops into those two helpers with no
other change, and the border/palette then light up through the same entry point.
The honest refusal is pinned by
`lib_tests/sgb.rs::load_sgb_bios_keeps_default_border_off_sgb_noop`.

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
the pre-SGB path bit-for-bit. The **default border, boot intro/cross-fade, and
BIOS seams add no new golden risk**: every one lives inside `SgbView` (reached
only through `Some`), `sgb_border()` is only ever `Some` on Sgb/Sgb2, and
`frame()` stays an unmodified `&[u32; 160*144]` — the golden set never calls
`sgb_border()`. The fade adds **zero** serialized bytes (it is transient). Since
`SgbView::new()` runs only for Sgb/Sgb2, the power-on `default_composite` seed
never executes on Dmg/Cgb. The BIOS entry point (`load_sgb_bios`) is likewise
gated: the audio path is `Model::Sgb`/`Sgb2`-only, and the two border/palette
seams route through `SgbView` (reached only through `Some`). Verified:
`golden_fingerprint` **passes byte-identically** (`SLOPGB_REQUIRE_ROMS=1`);
mooneye 91/91; core lib + frontend tests green; clippy `-D warnings` clean.

## Tests

- `ppu/sgb_tests.rs` — parser units: PAL01/12, ATTR_BLK (incl. the inside-only
  border promotion), ATTR_LIN/DIV/CHR, ATTR_SET, PAL_SET, PAL_TRN screen-decode,
  SOUND/DATA_SND queues, flags/JUMP, the queue cap, the ROM border composite
  (transparency + tile draw), the **default border** (frame + inset), the **boot
  fade-in** and **CHR/PCT cross-fade restart**, and a full save-state round-trip.
- `ppu/sgb/bios.rs` `#[cfg(test)]` — `title_checksum`, the title→palette hook
  (install + empty-table neutral + off-SGB no-op), and the border install seam
  (size validation + ready flag).
- `lib_tests.rs::sgb_pal01_colorizes_rendered_frame` — end-to-end: a PAL01
  packet driven through the real `Joypad` P1 pulse stream recolors a rendered
  frame (joypad → interconnect → ppu → render).
- `video.rs` tests — the generalized `(src_w, src_h)` blit/stretch.

## Deferred / not this phase

- **SPC700 CPU (Phase 2)** + **S-DSP audio synthesis (Phase 3)** — the seams
  above are the plug points; the queues/payloads are stored, not consumed.
- **Boot jingle** (SameBoy `render_jingle`) — the SNES boot sound, not emulated
  (the border boot intro *is* — see above).
- **Real firmware border/palette extraction end-to-end** — the core seams exist
  (`sgb_install_border` / `sgb_apply_bios_palette`), but the `GameBoy`/frontend
  plumbing to point at a user BIOS and locate the payloads is not yet wired
  (`lib.rs` is another work-package's file).
- **Per-game Nintendo palette table** — deliberately not shipped (legal); the
  hook applies a BIOS-supplied table, else the neutral default.
