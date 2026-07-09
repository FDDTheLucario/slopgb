# Super Game Boy (SGB) presentation

The SNES-side colorization of the DMG picture, driven by SGB command packets.
The command-packet **receiver** (P1 pulse decoding, 16-byte packets,
multi-packet commands, MLT_REQ) lives in [`crate::joypad`] and is documented in
[io-misc.md](io-misc.md); this file covers the **presentation** half: what
happens to the completed non-MLT_REQ commands.

Sources: Pan Docs "SGB Functions" / "SGB Command $xx". Only `Model::Sgb` /
`Model::Sgb2` with an SGB-flagged cartridge (`Cartridge::supports_sgb`:
header 0x146 == 0x03 and 0x14B == 0x33) activate the receiver.

## Wiring

`joypad.rs` `command_ready()` executes MLT_REQ (the only command with a
Game-Boy-bus-visible effect) and stashes every other completed command into
`Sgb::pending_cmd`. At the interconnect P1 write site (`memory.rs` `io_write`,
`0xFF00`), `Joypad::take_sgb_command()` is drained after the write and
forwarded to `Ppu::sgb_command(&cmd)`. `Ppu::sgb` is an `Option<SgbView>`
(`crate::ppu::sgb`), `Some` **only** on Sgb/Sgb2, so every non-SGB model is a
no-op end to end.

`SgbView { pal: [[u32; 4]; 4], attr: [u8; 360], mask: u8 }` — four 4-color
palettes, a 20×18-cell attribute map (row-major, `y/8*20 + x/8`), and the
MASK_EN mode. Defaults reproduce the DMG greyscale, so an un-commanded SGB
renders like a plain DMG.

## Implemented commands

| Cmd | Name | Effect |
|---|---|---|
| `$00`–`$03` | PAL01 / PAL23 / PAL03 / PAL12 | Set two named palettes from 7 BGR555 LE colors: color 0 is the shared entry-0 of all four palettes, colors 1-3 fill the first named palette's entries 1-3, colors 4-6 the second's. Pair: PAL01→{0,1}, PAL23→{2,3}, PAL03→{0,3}, PAL12→{1,2}. |
| `$04` | ATTR_BLK | Byte 1 = data-set count (cap 18). Each 6-byte set: `control` (bit0 inside / bit1 on-border / bit2 outside), `palettes` (bits0-1 inside, 2-3 border, 4-5 outside), `x1,y1,x2,y2` in cells. A cell strictly inside the rect is "inside", on its perimeter "on-border", beyond it "outside"; each region is recolored only if its control bit is set. |
| `$17` | MASK_EN | Byte 1 bits 0-1: 0 = cancel, 1 = freeze (hold the last presented frame), 2 = black, 3 = palette-0 color 0. |

**Rendering** (`render/sprite.rs` `output_pixel` → `Ppu::dmg_shade`): the DMG
paths compute a 2-bit shade (through BGP/OBP) then look it up as
`pal[attr[cell]][shade]` when an `SgbView` is present, else straight through
`dmg_palette` (byte-identical). BGR555→XRGB8888 uses the same `(c<<3)|(c>>2)`
5→8 expansion as `cgb_color`.

**MASK_EN** applies at the frame boundary (`line_setup.rs` `start_line`,
line 144): freeze skips the buffer swap; black / color-0 fill the presented
front buffer. `mask == 1` alone encodes "frozen" — no separate flag.

Save-states round-trip the `SgbView` (`ppu/state.rs`); `pending_cmd` is
transient (set and drained inside one P1 write) and not serialized.

## Deferred (`// ponytail:` in `ppu/sgb.rs`)

- **PAL_TRN / PAL_SET** (`$0A` / `$0B`) — need a VRAM snapshot of the 512-entry
  SNES system palette table transferred by the game.
- **CHR_TRN / PCT_TRN + borders** (`$13` / `$14`) — need a 256×224 output
  surface and frontend work (the GB screen is a 160×144 inset of the SNES
  border frame).
- **ATTR_LIN / ATTR_DIV / ATTR_CHR** (`$05`–`$07`) — the other attribute-map
  fill modes; only ATTR_BLK is implemented.
- **Sound commands** (`$08` SOU / `$09` SOU_TRN, etc.) — SNES-side audio.

Upgrade path: add the VRAM-transfer hook + a wider output buffer, then extend
the `SgbView::sgb_command` match.

## Golden-safety

`Ppu::sgb` is `None` on every model the golden set runs (`Dmg` / `Cgb`; its
`boot_regs-sgb.gb` rows are ROM *names*, not `Model::Sgb`), so `dmg_shade`,
the MASK_EN frame handling, and the state stream all reduce to the pre-SGB
path bit-for-bit. Verified: gbtr `golden_fingerprint` byte-identical; mooneye
439/439 rom×model; clippy `-D warnings` clean.

## Tests

- `ppu/sgb_tests.rs` — `SgbView::sgb_command` unit tests: PAL01/PAL12 palette
  layout, ATTR_BLK region fill + control-bit gating, MASK_EN modes, BGR555
  expansion, short-command guard.
- `lib_tests.rs` `sgb_pal01_colorizes_rendered_frame` — end-to-end: a PAL01
  packet driven through the real `Joypad` P1 pulse stream recolors a rendered
  frame (joypad → interconnect → ppu → render).
