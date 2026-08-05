# slopgb

[![CI](https://github.com/FDDTheLucario/slopgb/actions/workflows/ci.yml/badge.svg)](https://github.com/FDDTheLucario/slopgb/actions/workflows/ci.yml)

Cycle-accurate Game Boy / Game Boy Color emulator in Rust.

| Pokémon Crystal (CGB) | Space Invaders (Super Game Boy) |
|---|---|
| ![Pokémon Crystal running in slopgb](docs/screenshots/crystal-cgb.png) | ![Space Invaders in SGB mode with border](docs/screenshots/space-invaders-sgb.png) |

In the Super Game Boy shot the border, palettes and screen composite are the
core's own SGB emulation; the SNES *chips* are separate — SGB music comes from
clean-room SPC700 + 65C816 cores compiled to wasm coprocessor plugins, driven by
the running Game Boy.

- `crates/slopgb-core` — emulator core: no external dependencies (std only), no
  unsafe, deterministic. Emulates DMG0/DMG/MGB/SGB/SGB2/CGB/AGB models. It
  emulates no SNES chip of its own: the SGB border, palettes and command-packet
  handling are core-side, every SNES chip is a plugin.
- `crates/slopgb` — cross-platform desktop frontend (winit + softbuffer +
  cpal + gilrs) with a bgb-style debugger UI.
- A Rust→wasm plugin system (`slopgb-plugin-api` guest SDK +
  `slopgb-plugin-host` wasmi runtime) with three capability tiers:
  per-frame introspection, MCP tools, and coprocessor subsystems. Super
  Game Boy sound runs its SNES side as clean-room chip cores compiled to
  wasm coprocessor plugins — SPC700 + S-DSP audio (`slopgb-snes-apu`),
  65C816 (`slopgb-w65c816`), and optionally the SNES PPU
  (`slopgb-snes-ppu`) for titles that take the SNES over entirely — plus MSU-1
  streaming audio (`--msu1`) and a SoundFont-2 importer that replaces an SGB
  game's own instrument samples (`slopgb-sf2`, `--sf2`). Build the plugins with
  `cargo xtask stage-plugins <dir>` and point the emulator at that directory with
  `--plugins <dir>`. Without them the coprocessor slot stays empty and
  an SGB game plays no SGB music; the border and palettes are unaffected.

Accuracy is validated against the
[mooneye-test-suite](https://github.com/Gekkio/mooneye-test-suite)
(439/439 rom×model cases pass) and the
[game-boy-test-roms](https://github.com/c-sp/game-boy-test-roms) v7.0
collection (gambatte, blargg, mealybug-tearoom, SameSuite, age,
gbmicrotest, the acid tests and more — run in full across every applicable
hardware model, with all 215 suite tests green and every residual failure
pinned in a documented known-failure baseline of 654 rom×model cases),
achieved by emulating documented hardware behavior — never by special-casing
test ROMs (see `docs/ARCHITECTURE.md`).

## Building

Needs **Rust 1.97+** (the pinned toolchain in `rust-toolchain.toml`; the workspace
is edition 2024); install via [rustup](https://rustup.rs).

The **core** (`slopgb-core`) is pure `std` with no external crates — it builds
anywhere with no system libraries. The **frontend** (`slopgb`) draws with winit + softbuffer, plays
audio with cpal and reads game controllers with gilrs, so on Linux it needs
the usual desktop dev libraries (libudev is gilrs's controller-hotplug
backend):

| Distro | Install |
|---|---|
| Arch | `sudo pacman -S base-devel alsa-lib libxkbcommon` (Wayland: `wayland`; X11: `libxcb libx11`) |
| Debian/Ubuntu | `sudo apt install build-essential pkg-config libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev libxcb1-dev` |
| Fedora | `sudo dnf install @development-tools alsa-lib-devel systemd-devel libxkbcommon-devel wayland-devel libxcb-devel` |

macOS and Windows need only the Rust toolchain (no extra system packages).

```sh
cargo build --release             # whole workspace → target/release/slopgb
cargo build --release -p slopgb   # frontend only
cargo build -p slopgb-core        # core only (zero deps, no system libs)
```

**Optional runtime tools** (frontend, auto-detected, dep-free — each degrades
gracefully if absent): a file picker for the Load/Save dialogs
(`zenity` / `kdialog` / `yad` / `qarma`; otherwise a typed-path prompt) and a
clipboard for the debugger's copy commands (`wl-copy` / `xclip` / `xsel`).

## Tests

```sh
test-roms/download.sh        # fetch the pinned test-ROM bundles (~once)
cargo test --workspace       # unit tests + mooneye + game-boy-test-roms harnesses
```

Beyond accuracy, the suite pins robustness the way a shipping app needs it:
the untrusted parsers a user actually feeds — the ROM image, the `.sav`, the
savestate blob, the CDL file — are fuzzed with hundreds of thousands of random
and mutated inputs and must never panic (`tests/fuzz.rs`); every model emulates a
frame well inside the 16.7 ms real-time budget headless
(`tests/realtime_perf.rs`); and audio +
joypad carry signal end to end (`tests/audio_input_smoke.rs`). All of it, plus
`cargo fmt`/`clippy -D warnings`, runs in CI on Linux, Windows, and macOS.

## Running

```sh
cargo run --release -- path/to/game.gb   # or: ./target/release/slopgb game.gb
cargo run --release                      # no ROM → blank LCD; load via drag-drop or the menu
```

Optional boot ROM (Nintendo logo + chime): `--boot path/to/dmg_boot.bin` or the
`SLOPGB_BOOT` env var (boot ROMs are copyrighted and never bundled).

Other flags: `--model`, `--scale`, `--mute`, `--ram-init`, `--sgb-bios`,
`--plugins <dir>` (load the wasm plugins staged above; also `SLOPGB_PLUGINS_DIR`)
and `--mcp-port <n>` (expose the debugger to an LLM agent over MCP). Flags a
plugin contributes are only accepted while that plugin is in the plugins dir, so
`--msu1` and `--sf2` appear in `--help` exactly when their `.wasm` is present.

## Credits & references

slopgb was built by studying — and validating against — the Game Boy
reverse-engineering community's work. All trademarks and copyrights belong to
their respective owners; none of the projects below are bundled here.

**Reference implementations** (behaviour ported from or cross-checked against):

- [**SameBoy**](https://github.com/LIJI32/SameBoy) (Lior Halphon, MIT) — the
  core's cycle-exact timing is a port of SameBoy's sub-dot PPU / SM83 model
  (`display.c`, `sm83_cpu.c`), and is the primary accuracy reference.
- [**gambatte**](https://github.com/sinamas/gambatte) (Sindre Aamås, GPL-2.0) —
  the reference for undocumented-corner timing and the source of the gambatte
  hardware-test baselines. Parts of the HDMA/OAM-DMA engine and the STAT
  interrupt model are **derived from** its source; this is why slopgb is
  GPL-2.0.
- [**mooneye-gb**](https://github.com/Gekkio/mooneye-gb) (Joonas Javanainen /
  Gekkio, MIT) — reference emulator and test methodology.

**Hardware documentation:**

- [**Pan Docs**](https://gbdev.io/pandocs/) (gbdev) — the primary hardware
  reference.
- [**Game Boy: Complete Technical Reference**](https://github.com/Gekkio/gb-ctr)
  (Gekkio) — CPU / MBC timing and micro-ops.

**Debugger UI:**

- [**bgb**](https://bgb.bircd.org/) (beware) — the debugger, VRAM / I/O viewers,
  save states and right-click menus are a functional clone of bgb's UI,
  reproduced from screenshots (not its code).

**Test ROMs** (fetched by `test-roms/download.sh`, never redistributed here):

- [**mooneye-test-suite**](https://github.com/Gekkio/mooneye-test-suite) (Gekkio).
- [**game-boy-test-roms**](https://github.com/c-sp/game-boy-test-roms) (Christoph
  Sprenger / c-sp) — the pinned v7.0 collection, bundling work by many authors:
  blargg (Shay Green),
  [gambatte](https://github.com/sinamas/gambatte),
  [mealybug-tearoom-tests](https://github.com/mattcurrie/mealybug-tearoom-tests)
  and the [acid2](https://github.com/mattcurrie/dmg-acid2) tests (Matt Currie),
  [SameSuite](https://github.com/LIJI32/SameSuite) (Lior Halphon),
  [AGE](https://github.com/c-sp/AGE) (Christoph Sprenger),
  [gbmicrotest](https://github.com/aappleby/gbmicrotest) (Austin Appleby), and
  [wilbertpol](https://github.com/wilbertpol/mooneye-gb)'s test additions.

## License

**GPL-2.0-only** — see [`LICENSE`](LICENSE). Parts of the core are derived from
gambatte (GPL-2.0), which requires derivative works to carry the same licence;
the SameBoy timing port is Expat/MIT and its notice is reproduced as that
licence requires. Both, plus the reference-project attributions, are in
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).

`crates/slopfp` and `crates/slopgb-plugin-api` carry no derived code and stay
MIT, so plugin authors are not pulled into the GPL by the guest SDK.
