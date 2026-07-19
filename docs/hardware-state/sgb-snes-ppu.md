# SNES PPU (the `slopgb-snes-ppu` crate + its wasm plugin)

Clean-room S-PPU scanline renderer for the SGB's SNES side, sourced from
nocash fullsnes + Anomie's register docs only (no emulator source). The
crate is std-only/no-unsafe like core; the `slopgb-snes-ppu-plugin` wraps
it behind the generic tier-3 coprocessor ABI (`snes-ppu.wasm`, staged by
`cargo xtask stage-plugins`, auto-loaded by the SGB coprocessor from the
`--plugins` dir; absent = the audio-only backend, unchanged).

## What is implemented (all with inline citations + unit tests)

- **Ports**: VRAM `$2115-$2119` with the three VMAIN remap modes (8/9/10-
  bit rotate, applied at access time only), the RDVRAM prefetch glitch
  (the latch refills after an address write and *before* the increment on
  reads); CGRAM `$2121/22/3A` shared write/read flip-flop; OAM `$2102/03`
  9-bit reload → 10-bit address (bit 0 cleared), the `$220-$3FF` mirror.
- **Scroll registers**: the write-twice pair shares one `BG_old` latch,
  stored full-width (masking at store corrupts the `Reg>>8` term — the
  latch is masked at render instead).
- **Backgrounds**: modes 0 and 1, 8×8 tiles, per-BG scroll/char/screen
  bases, mode-0's per-BG CGRAM slices (BG n at `n*$20`).
- **OBJ**: OBSEL size/base/gap, 8×8/8×16 (the pilot's sizes), the
  no-carry tile-number math (x in bits 3-0, y in bits 7-4, bit 8 fixed:
  `$1FF`'s right neighbour is `$1F0`, below is `$10F`), priority per the
  MODE0/MODE1A/MODE1B rungs (BGMODE bit 3 selects 1A/1B).
- **Frame assembly**: 256×224 RGB555 framebuffer, per-scanline `render_line`
  API, INIDISP force-blank + brightness `×(N+1)/16`.

## How it is wired (the SGB coprocessor's flush)

The 65C816 plugin's MMIO ring captures guest `$2100-$213F` writes; the
coprocessor's `apply_mmio` routes them (and GP-DMA B-bus writes) into the
PPU plugin via `port_write`. The scanline pump renders rows up to the
SNES V-counter each flush (`PPU_HW_LINE`), latches a frame at the vblank
edge, and `GameBoy::take_snes_frame` hands it to the frontend (which
presents SNES > border > bare, `snes_rgb555_px` expanding BGR555).

## Renderer shape (interpreter-speed, oracle-pinned)

The scanline renderers are written for interpreted-wasm speed with
byte-identical output, each pinned by a fuzzed frozen-reference oracle in
the crate's tests before the rewrite:

- `bg_line` walks 8-pixel char-row runs (map entry + plane words load
  once per run; an X-flip mirrors a run onto one 8-aligned block, so
  only the walk direction reverses); plane decode goes through the
  `SPREAD` bit-spread LUT (planes OR into eight packed index nibbles).
- `render_line` merges rung-outer over a resolved-pixel mask with an
  all-resolved early-out; TM-disabled and rendered-empty layers skip
  their rungs wholesale.
- `obj_line` fetches + spreads each 8-pixel chunk's two VRAM words once.

The host/plugin boundary is batched (one wasm crossing per run, not per
byte): `HW_LINE` takes a `[y, count]` span, `HW_PORTS` applies a
`(port, val)` run in order, the flush batches consecutive captured
pure-PPU writes (host-consumed registers are order barriers; INIDISP
keeps its `snes_live` bookkeeping), and GP-DMA bulk-reads contiguous
A-bus runs (the ICD2 `$7800` window auto-increments per byte inside the
plugin) while batching pure-PPU destination bytes.

## Status

All host-side plumbing is exercised by the pilot (Space Invaders ARCADE):
the takeover game runs and its `$21xx` traffic routes through `apply_mmio`.
Renderer correctness is pinned by the crate's unit tests (27, including
the three fuzz oracles) + `slopgb-plugin-host`'s `snes_ppu_roundtrip`.
Probe fps (ARCADE takeover, every 500-frame window): wasmi >= 66
(gameplay 92-106), wasmtime several hundred.
