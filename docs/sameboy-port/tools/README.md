# SameBoy ground-truth tooling

The port's progress metric is "does SameBoy 1.0.2 pass the row we baseline-fail?".
Two harnesses ground-truth that, depending on how the ROM reports its result.

## `hramdump.c` — gbmicrotest HRAM verdict reader

The stock SameBoy `sameboy_tester` is built for *games*: it mashes Start/A to
navigate menus and dumps the final-frame BMP. For **gbmicrotest** that is useless
— the verdict lives in HRAM (`$FF82` = `$01` pass / `$FF` fail; `$FF80` actual,
`$FF81` expected), the button-mashing perturbs the test, and gbmicrotest's
on-screen font is not the gambatte glyph font `sb_ocr.py` decodes.

`hramdump.c` loads a ROM headless (no input), runs ~400 frames (enough for the
DMG boot animation + the test), and prints `$FF80/$FF81/$FF82`. It disables the
debugger (`GB_debugger_set_disabled`) so the `LD B,B` software breakpoint that
SameBoy normally traps doesn't freeze the run.

### Build + run

```sh
cd /tmp/sbbuild/SameBoy-1.0.2          # SameBoy 1.0.2 source + `make tester` build
cp <repo>/docs/sameboy-port/tools/hramdump.c .
clang -I. -std=gnu11 -D_GNU_SOURCE -DGB_VERSION='"1.0.2"' -DGB_COPYRIGHT_YEAR='"2025"' \
      -D_USE_MATH_DEFINES -fPIC -O2 -Wno-deprecated-declarations \
      hramdump.c build/obj/Core/*.c.o -lm -o /tmp/hramdump
/tmp/hramdump --dmg <rom.gb>          # or --cgb; boot ROM defaults to build/bin/tester/{dmg,cgb}_boot.bin
```

Output: `<rom> FF80=62 FF81=62 FF82=01 PASS`.

### Verified ground-truths (this tool, 2026-06-23)

`int_hblank_halt_scx0..7` (DMG) all **PASS** in SameBoy — `$FF80` = 62,62,62,63,
63,63,63,64 = the baked expected. So slopgb's Tier-2 reclock reading 60,60,61,61,
61,61,62,62 is a **port bug, not a hardware contradiction** (see
`../../hardware-state/ppu-subdot-ladder.md` THESIS RESULT #7).

## Note on game/visual ROMs

Tests whose result is the *screen* (gambatte glyph rows) still use the stock
`sameboy_tester` BMP + `/tmp/sb_ocr.py`; tests that run code from HRAM
(e.g. `dma_basic`) are not valid `hramdump.c` targets (`$FF80-82` are reused as
code there — only the `$FF82 ∈ {01,FF}` verdict tests are).
