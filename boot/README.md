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
| P3 | **CGB colored-wipe animation** (palette colour sweep across the logo — the CGB boot effect, *not* the DMG drop-down) | todo |
| P4 | two-tone CGB chime | todo |
| P5 | CGB palettes: default + title-hash per-game + manual button-combo select | todo |

License: MIT (original work).
