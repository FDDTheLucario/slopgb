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

## Frame handoff (zero-copy out of the plugin)

`take_snes_frame` pulls the whole 112 KB framebuffer once per vblank through
`LoadedCoprocessor::read_ram(PPU_HW_FB, …)`. The plugin serves that pull from
`Coprocessor::emit_ram` (the guest half of `slopgb_read_ram`, defaulted to
`read_ram` + `__emit`): its `fb` is `[u16]` in little-endian wasm memory, which
already *is* the RGB555 byte stream, so a word-aligned request wholly inside the
frame is handed over as a region (`__emit_words`) and the host's existing bulk
`Memory::read` copies it once. Everything else (odd start or length, past the
last pixel, outside the host window) still goes through `read_ram`, which
zero-fills what it cannot serve; `fb_words` decides which, and its agreement with
`read_ram` is pinned byte-for-byte by
`fb_word_range_is_byte_identical_to_read_ram` (native) plus
`whole_frame_pull_matches_the_byte_by_byte_path` (across the wasm boundary).

Materializing those bytes in the guest instead cost **~4.5 ms per frame** — a
per-byte interpreted loop against a ~4 µs host memcpy of the same bytes — and was
roughly half of all arcade-takeover wall time. Removing it took the headless
arcade bench from ~96 to ~184 fps median (interleaved A/B of the two
`snes-ppu.wasm` builds, same host binary, 500 frames x3 each). The
`SLOPGB_PERF=1` sections only cover `SgbCoprocessor::flush`, so the win shows up
as the ~2.4 s/500 frames that used to sit *outside* the accounted total
disappearing: after the change, arcade wall time and the perf total agree.

No ABI change: the export shape (`slopgb_read_ram(addr, len) -> i32`, emitting
`EMIT_KIND_RAM`) is untouched, `emit_ram` is a defaulted guest-side trait method,
so `ABI_VERSION` stays 7.

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

## Fast-forward throughput

`AudioCoprocessor::set_render_enabled` (default on) gates only the
`PPU_HW_LINE` scanline rasterization in `SgbCoprocessor::flush`; the
`$21xx`/DMA register capture and both chips' `run_until` stay unconditional,
so chip timing is untouched. `crates/slopgb/src/app_pacing.rs`'s `run_turbo`
(fast-forward) disables it; `run_audio_paced`/`run_timer_paced` always
restore it for normal-speed play. Headless bench (`slopgb-sgb-coprocessor`'s
`examples/throughput.rs`, driving the coprocessor directly — no SGB ROM ships
in this repo): median fps, 600 frames/run x3, one representative run on a
shared/sandboxed build machine (run-to-run variance was +-20% on repeats —
treat the `SLOPGB_PERF=1` section breakdown as the more reliable signal):

| workload | render | fps | x real-time |
|---|---|---|---|
| plain SGB (spc700+w65c816) | on | 152.8 | 2.56x |
| plain SGB (spc700+w65c816) | off | 147.8 | 2.48x (no PPU to skip: within noise, as expected) |
| arcade (spc700+w65c816+snes-ppu) | on | 128.2 | 2.15x |
| arcade (spc700+w65c816+snes-ppu) | off | 154.5 | 2.59x |

`SLOPGB_PERF=1` confirms the mechanism: per 8572-flush window, arcade's `ppu`
section drops from ~755-796 ms (render on) to ~0.5-0.6 ms (render off), while
`spc`/`cpu` (the mandatory chip execution) stay flat (~1.2-1.3 s / ~25 ms) —
the skip is real and touches only rasterization. Targets (2x arcade / 4x
plain SGB, normal render-on play): arcade sits near the 2x line (1.95-2.55x
across repeats); plain SGB fell short of 4x (2.15-2.94x across repeats) — the
`spc`/`cpu` chip-execution cost (not gateable, since it must run every flush
regardless of render) dominates both workloads' flush time, so it, not the
PPU, is the ceiling on this build machine.
