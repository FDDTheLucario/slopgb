# SameBoy ground-truth tooling

These tools answer one question: does SameBoy 1.0.2 pass a row slopgb
baseline-fails? How to ask depends on how the ROM reports its result — HRAM
verdict, on-screen glyphs, or a pixel reference — so there is one tool per shape:

| File | Ground-truths |
|---|---|
| `build_sameboy_tracers.sh` | builds `sameboy_tester` (with the SBMODE/SBREAD/SBLEVEL/STAT_IRQ tracers, gated on `SB_TRACE`) into `$SBBUILD/SameBoy-1.0.2`, default `~/.cache/sbbuild`. Every tool below needs it first. |
| `hramdump.c` | gbmicrotest's `$FF80-82` HRAM verdict (below) |
| `classify_cgb_regr.py` | gambatte glyph rows, CGB: OCRs the tester's BMP and compares against the `cgb04c_out<hex>` want in the filename |
| `classify_dmg.py` | the same for DMG (`dmg08_out<hex>` / the shared `dmg08_cgb04c_out<hex>` form), with the +1px DMG glyph x-shift trial |
| `classify_pixel.py` | pixel-reference legs (gambatte/mealybug): palette-quantized diff of the tester's BMP against the sibling reference PNG. Needs `numpy` + `Pillow`. |

**A pixel row's `sameboy` verdict is weaker than a glyph row's.** `classify_pixel.py`
maps each SameBoy pixel to the *nearest* reference-palette entry, which is what
lets it compare emulators whose shades differ — but it therefore checks
GEOMETRY, not colour identity. A row whose only fault is a wrong colour at the
right position reads `PASS` even when SameBoy's frame plainly differs from the
reference. Measured case: `gambatte/scx_during_m3/scx_during_m3_spx2` [Cgb],
where SameBoy renders `(0,0,0)` against the reference's `(33,146,108)` and still
classifies PASS (see the class-F note in
[`../../hardware-state/ppu-render.md`](../../hardware-state/ppu-render.md)).
Before investing in a pixel row, diff the actual colours.

All three take `rowlist.txt outprefix` and write `<outprefix>_bug/_floor/_unk.txt`
(`classify_cgb_regr.py` defaults the prefix to `/tmp/s7/cls` when it is run by
hand) — that is the contract `census.py` reads them through, so a classifier that
writes elsewhere silently leaves every one of its rows `unknown`. All three
refuse to run without a
`sameboy_tester` (`SBT=` overrides the path) — classifying with a missing tester
is a vacuous result, not a bar.

## `hramdump.c` — gbmicrotest HRAM verdict reader

The stock SameBoy `sameboy_tester` is built for *games*: it mashes Start/A to
navigate menus and dumps the final-frame BMP. For **gbmicrotest** that is useless
— the verdict lives in HRAM (`$FF82` = `$01` pass / `$FF` fail; `$FF80` actual,
`$FF81` expected), the button-mashing perturbs the test, and gbmicrotest's
on-screen font is not the gambatte glyph font the classifiers' `ocr()` decodes.

`hramdump.c` loads a ROM headless (no input), runs ~400 frames (enough for the
DMG boot animation + the test), and prints `$FF80/$FF81/$FF82`. It disables the
debugger (`GB_debugger_set_disabled`) so the `LD B,B` software breakpoint that
SameBoy normally traps doesn't freeze the run.

### Build + run

```sh
docs/sameboy-port/tools/build_sameboy_tracers.sh   # once, from the repo root
cd ~/.cache/sbbuild/SameBoy-1.0.2          # its source tree + `make tester` build
cp <repo>/docs/sameboy-port/tools/hramdump.c .
clang -I. -std=gnu11 -D_GNU_SOURCE -DGB_VERSION='"1.0.2"' -DGB_COPYRIGHT_YEAR='"2025"' \
      -D_USE_MATH_DEFINES -fPIC -O2 -Wno-deprecated-declarations \
      hramdump.c build/obj/Core/*.c.o -lm -o /tmp/hramdump
/tmp/hramdump --dmg <rom.gb>          # or --cgb; boot ROM defaults to build/bin/tester/{dmg,cgb}_boot.bin
```

Output: `<rom> FF80=62 FF81=62 FF82=01 PASS`.

### Verified ground-truths (this tool, 2026-06-23)

`int_hblank_halt_scx0..7` (DMG) all **PASS** in SameBoy — `$FF80` = 62,62,62,63,
63,63,63,64 = the baked expected. A slopgb reading that lands under those values
is therefore a **port bug, not a hardware contradiction**.

## Note on game/visual ROMs

Tests whose result is the *screen* (gambatte glyph rows) use the stock
`sameboy_tester` BMP plus the classifiers above, which carry the glyph OCR inline
(the `RAW` table + `ocr()` in `classify_cgb_regr.py` / `classify_dmg.py`); tests
that run code from HRAM (e.g. `dma_basic`) are not valid `hramdump.c` targets
(`$FF80-82` are reused as code there — only the `$FF82 ∈ {01,FF}` verdict tests
are).
