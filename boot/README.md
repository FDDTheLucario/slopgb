# slopgb CGB boot ROM (original, open-source)

An **original** Game Boy Color boot ROM, written from public hardware
documentation (Pan Docs, the gbdev wiki, mooneye/gbmicrotest behaviour notes) —
not derived from any copyrighted boot ROM. It recreates the CGB power-on
experience: hardware init, a scrolling boot logo (the **slopgb** logo, in the
classic Game Boy boot style) + the two-tone CGB chime, the CGB colourisation
features (a default palette, per-game palette assignment by title hash, and the
manual palette-select button combos held at boot), then hand-off to the cart.

The 48-byte Nintendo logo a real cartridge carries at `$0104` is **read from the
cart** (mapped at `$0100-$01FF` during boot), never embedded here. The slopgb
logo art is original.

## Build

```sh
make            # -> slopgb_cgb_boot.bin (2304 bytes, CGB-class)
```

Needs RGBDS (`rgbasm`/`rgblink`). Load it in slopgb with
`cargo run --release -- --boot boot/slopgb_cgb_boot.bin <game.gbc>` or via the
Options → System bootrom path.

## Layout (the emulator's CGB-class mapping, `interconnect/boot_rom.rs`)

| Region | Contents |
|---|---|
| `$0000-$00FF` | entry + main boot routine; jumps past the header gap to `$0200` |
| `$0100-$01FF` | **cart header** shows through here (logo + title) — not boot ROM |
| `$0200-$08FF` | continuation: logo tiles, palette tables, chime, hand-off |

`FF50` bit 0 written 1 unmaps the boot ROM and hands control to the cart at
`$0100`.

## Plan / status

| Phase | What | Status |
|---|---|---|
| P1 | scaffold + minimal init + correct hand-off (game boots) | **done** (cgb-acid-hell: 0/23040 px diff vs direct boot) |
| P2 | slopgb logo tiles displayed (static) | **done** |
| P3 | **CGB colored-wipe animation** (palette colour sweep across the logo — the CGB boot effect, *not* the DMG drop-down) | **done** (rainbow sweeps left→right across the columns) |
| P4 | two-tone CGB chime | **done** (square ch1 "di-ding"; audio peak ~0.33) |
| P5 | CGB palettes: default + title-hash per-game + manual button-combo select | **done** (hash colorization byte-identical vs the reference across all checksums; 12 manual combos verified) |

## CGB compatibility palettes (P5) — provenance

The per-game palette assignment is **factual hardware-interop data** (the same
category as the core's existing `CGB_COMPAT_*` palettes), reproduced clean-room:

1. The selection **algorithm** (checksum = Σ title bytes $0134-$0143 → table search
   → 4th-letter $0137 collision tiebreak → palette assignment) is from public
   documentation (Pan Docs "Power-Up Sequence", TCRF) and confirmed against the
   reference ROM's disassembly.
2. The palette **values** are recovered by *black-box observation*: synthetic
   carts are booted through the reference `cgb_boot.bin` and the palette it
   installs is read back (`crates/slopgb-core/examples/cgb_palette_extract.rs`).
   No reference ROM code or data layout is copied — only the title→palette
   function is observed, then re-encoded in our own tables (`cgb_palettes.inc`)
   and our own lookup code.
3. `cgb_palette_extract ... verify <our.bin>` boots **every** checksum + collision
   letter through our boot ROM and diffs the installed palette against the
   reference — 0 mismatches ⇒ 100% compatible.

`cgb_palettes.inc` is generated; regenerate with
`cargo run -p slopgb-core --example cgb_palette_extract -- bootroms/cgb_boot.bin emit`.

License: MIT (original work).
