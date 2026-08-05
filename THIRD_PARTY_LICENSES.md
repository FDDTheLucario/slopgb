# Third-party licenses & attributions

Almost nothing in an accurate Game Boy emulator is discovered alone. The
hardware's undocumented corners — the dot the STAT flag really flips on, what a
mid-scanline SCX write does to the fetcher, which DMA cycle steals the bus —
were mapped over two decades by people who then gave the results away. slopgb is
downstream of that work in every sense, and this file records the debt: the
licenses it is obliged to reproduce, and the projects it leaned on even where no
license compels a word.

No third-party code, ROM, or asset is committed to this repository. Test ROMs
are fetched separately by `test-roms/download.sh`.

slopgb is licensed under the GNU General Public License, version 2 (see
[`LICENSE`](LICENSE)) — because parts of the core are derived from gambatte,
which is GPL-2.0. The gambatte section below explains where and why.

---

## Code this project is built from

### SameBoy — Lior Halphon

The core's cycle-exact timing is a Rust port of SameBoy's model: the sub-dot PPU
and SM83 implementation (`Core/display.c`, `Core/sm83_cpu.c`) and the SGB HLE
layer (`Core/sgb.c`), with behaviour from `Core/memory.c`, `Core/timing.c`,
`Core/mbc.c` and `Core/apu.c` ported or cited file by file in the comments. When
this emulator disagrees with hardware, the usual cause is that the port drifted
from SameBoy rather than that SameBoy was wrong. It is the reference the rest of
the core is measured against.

SameBoy's Core is under the Expat License, which asks that its copyright and
permission notice travel with the work:

> Copyright (c) 2015-2026 Lior Halphon
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

<https://github.com/LIJI32/SameBoy> — Expat/MIT. SameBoy's `iOS/` and
`HexFiend/` directories carry additional terms; only Core files are ported here,
and those are under the grant above.

### gambatte — Sindre Aamås

The HDMA and OAM-DMA engine, the STAT interrupt and mode-event model, and
several corners of the PPU fetch and window machinery were written with
gambatte's source open. The comments say so throughout, and they quote it:
`ioamhram_[0x155] = halted() ? ioamhram_[0x155] | 0x80` sits in
`interconnect/hdma.rs`, and names like `haltHdmaState_`, `dmaSource_`,
`eventTimes_` and `flagGdmaReq` appear across the interconnect. Those are
gambatte's private internals. No amount of running test ROMs would have revealed
them — that understanding came from reading Sindre's code, and the honest word
for the result is derived.

gambatte is under the GNU General Public License, version 2, which asks that
work built from it stay free on the same terms. That is why slopgb is GPL-2.0,
and it seems a small thing to give back for what the source taught this project
about the parts of the hardware nobody documented.

An earlier version of this file called gambatte "studied, not copied" and
claimed no obligation followed. That was wrong; the license was corrected to
GPL-2.0-only as soon as the comments were read properly.

<https://github.com/sinamas/gambatte> — GPL-2.0. Its test ROMs are run through a
harness written here against the documented `testrunner.cpp` protocol; no
gambatte source files are vendored.

---

## Reference implementations and documentation

These shaped the emulator without a line of their code reaching it. No license
requires their mention. They are here because the project would not exist
without them.

- **mooneye-gb** and the **mooneye-test-suite** — Joonas Javanainen (Gekkio).
  The test suite is the acceptance bar this core is held to; the emulator
  (GPLv3) informed methodology only. <https://github.com/Gekkio/mooneye-gb>
- **Game Boy: Complete Technical Reference** — Gekkio. Where the CPU and MBC
  timing questions get settled. <https://github.com/Gekkio/gb-ctr>
- **Pan Docs** — the gbdev community. The first place to look, for twenty years
  and counting. <https://gbdev.io/pandocs/>
- **bgb** — beware. The debugger, viewers and right-click menus here are a
  functional tribute, rebuilt from screenshots; none of bgb's code or artwork is
  used. <https://bgb.bircd.org/>

The test ROMs are their own act of generosity — each one is somebody's afternoon
spent proving what the hardware does. Run here, never redistributed:
mooneye-test-suite (Gekkio) and game-boy-test-roms (Christoph Sprenger), the
latter collecting work by blargg (Shay Green), mealybug-tearoom-tests and
dmg-acid2 (Matt Currie), SameSuite (Lior Halphon), AGE (Christoph Sprenger),
gbmicrotest (Austin Appleby), gambatte (Sindre Aamås) and wilbertpol. Links are
in the README.

---

## Dependencies resolved by cargo

None of these live in this repository — cargo fetches them, and their notices
ship with the crates. They do link into a built `slopgb` binary, so a binary
distribution has to carry those notices. Licenses as each manifest declares
them, at the versions pinned in `Cargo.lock`:

| Crate | Version | License | Used by |
|---|---|---|---|
| `winit` | 0.30.13 | Apache-2.0 | `crates/slopgb` (windowing) |
| `softbuffer` | 0.4.8 | MIT OR Apache-2.0 | `crates/slopgb` (software framebuffer) |
| `cpal` | 0.16.0 | Apache-2.0 | `crates/slopgb` (audio out) |
| `gilrs` | 0.11.2 | Apache-2.0/MIT | `crates/slopgb` (game controllers) |
| `wasmi` | 1.1.0 | MIT/Apache-2.0 | `crates/slopgb-plugin-host` (the wasm engine) |
| `wat` | 1.253.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `crates/slopgb-plugin-host`, tests only |

That is every external dependency any workspace crate declares — `slopgb-core`
is std-only. Transitive dependencies are pinned in `Cargo.lock` and not
enumerated here; generate that tree from the lock file when preparing a binary
release.

---

## Crates that stay MIT

`crates/slopfp` (the dependency-free file-picker state machine) and
`crates/slopgb-plugin-api` (the SDK that Rust→wasm plugins compile against)
contain no code derived from gambatte or SameBoy, and remain MIT under their own
manifests. The SDK especially: someone writing a plugin should be free to
license it however they like, and the GPL on the emulator ought not reach into
their work.
