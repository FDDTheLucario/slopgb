# Third-party licenses & attributions

slopgb is licensed under the **GNU General Public License, version 2** (see
[`LICENSE`](LICENSE)). It is GPL-2.0 because parts of the emulator core are
derived from gambatte, which is GPL-2.0 — see the gambatte section below. This
file reproduces the license notices of third-party projects whose **code**
slopgb incorporates or ports, as those licenses require, and lists the projects
it merely **studied** (documentation and behavioural references) for
transparency. No third-party code, ROM, or asset is bundled in this repository;
test ROMs are fetched separately by `test-roms/download.sh`.

---

## Ported code — license notices reproduced as required

### SameBoy

The emulator core's cycle-exact timing (the sub-dot PPU / SM83 model) is a Rust
port of SameBoy's Core implementation (`Core/display.c`, `Core/sm83_cpu.c`), as is
the SGB HLE layer (`Core/sgb.c`); behaviour in `Core/memory.c`, `Core/timing.c`,
`Core/mbc.c` and `Core/apu.c` is ported or cited the same way, file by file, in
the core's comments. SameBoy's Core is distributed under the Expat License (the
MIT license), which requires its copyright and permission notice to be
reproduced:

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

Upstream: <https://github.com/LIJI32/SameBoy> — Expat/MIT. (SameBoy's `iOS/`
and `HexFiend/` directories carry additional terms; slopgb ports only Core
files, which are under the Expat grant above.)

### gambatte

Parts of the core — chiefly the HDMA/OAM-DMA engine, the STAT interrupt and
mode-event model, and corners of the PPU fetch and window machinery — were
written while reading gambatte's source, and the comments quote its internals
directly (for example `ioamhram_[0x155] = halted() ? ioamhram_[0x155] | 0x80`
in `interconnect/hdma.rs`, and identifiers such as `haltHdmaState_`,
`dmaSource_`, `eventTimes_`, `flagGdmaReq`). That knowledge is not obtainable
from running the test ROMs, so these parts are treated as **derived from**
gambatte rather than independently reimplemented.

gambatte is distributed under the GNU General Public License, version 2. GPL-2.0
requires derivative works to be distributed under the same terms, which is why
slopgb as a whole is GPL-2.0-only.

Upstream: <https://github.com/sinamas/gambatte> — GPL-2.0. gambatte's test ROMs
are run via a slopgb-authored harness implementing the documented
`testrunner.cpp` protocol; no gambatte source files are vendored in this
repository.

> An earlier revision of this file described gambatte as "studied, not copied"
> with "no license obligation on slopgb". That was inaccurate — the comment
> citations quote gambatte source — and the project was relicensed from MIT to
> GPL-2.0-only when it was checked.

---

## Linked crates.io dependencies (fetched by cargo, not vendored here)

Nothing below is in this repository — cargo resolves and downloads it, so these
crates' own notices ship with the crates. They do link into a built `slopgb`
binary, so a *binary* distribution has to carry their notices. Licences as
declared in each crate's manifest, at the versions in `Cargo.lock`:

| Crate | Version | Declared license | Used by |
|---|---|---|---|
| `winit` | 0.30.13 | Apache-2.0 | `crates/slopgb` (windowing) |
| `softbuffer` | 0.4.8 | MIT OR Apache-2.0 | `crates/slopgb` (software framebuffer) |
| `cpal` | 0.16.0 | Apache-2.0 | `crates/slopgb` (audio out) |
| `gilrs` | 0.11.2 | Apache-2.0/MIT | `crates/slopgb` (game controllers) |
| `wasmi` | 1.1.0 | MIT/Apache-2.0 | `crates/slopgb-plugin-host` (the sole wasm engine) |
| `wat` | 1.253.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `crates/slopgb-plugin-host` dev-dependency (tests only) |

These are the only external dependencies any workspace crate declares;
`slopgb-core` is std-only. Their transitive dependencies are pinned in
`Cargo.lock` and are **not** enumerated here — generate that tree from the lock
file when preparing a binary distribution.

---

## Reference-only — studied, not copied (no license obligation on slopgb)

The projects below were used as behavioural oracles and documentation. slopgb's
implementation is independent Rust informed by them; no source code was copied,
so their licenses impose no condition beyond the notices reproduced above. They are credited here (and in
the README) out of respect and for provenance.

- **mooneye-gb** (Joonas Javanainen / Gekkio) — GPLv3 (the emulator). Referenced
  for test methodology only; no code copied. The separate **mooneye-test-suite**
  (MIT) test ROMs are run, not bundled.
  <https://github.com/Gekkio/mooneye-gb>
- **Game Boy: Complete Technical Reference** (Gekkio) — documentation.
  <https://github.com/Gekkio/gb-ctr>
- **Pan Docs** (gbdev) — documentation, CC-licensed.
  <https://gbdev.io/pandocs/>
- **bgb** (beware) — the debugger UI is a functional reimplementation from
  screenshots; no bgb code or image assets are used. <https://bgb.bircd.org/>

Test-ROM suites (run, never redistributed here): mooneye-test-suite (Gekkio),
game-boy-test-roms (c-sp) and its constituents — blargg, mealybug-tearoom-tests
& acid2 (Matt Currie), SameSuite (Lior Halphon), AGE (Christoph Sprenger),
gbmicrotest (Austin Appleby), and wilbertpol's additions. See the README for
links.

---

## Workspace crates under a different licence

`crates/slopfp` (dep-free file-picker state machine) and
`crates/slopgb-plugin-api` (the guest SDK that Rust->wasm plugins compile
against) contain no gambatte- or SameBoy-derived code — verified by grep, zero
references — and remain under the **MIT** licence declared in their own
manifests. Keeping the guest SDK permissive means a plugin author is not forced
to license their plugin under the GPL.
