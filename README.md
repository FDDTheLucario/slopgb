# slopgb

Cycle-accurate Game Boy / Game Boy Color emulator in Rust.

- `crates/slopgb-core` — emulator core: zero dependencies, no unsafe,
  deterministic. Emulates DMG0/DMG/MGB/SGB/SGB2/CGB/AGB models.
- `crates/slopgb` — cross-platform desktop frontend (winit + softbuffer +
  cpal).

Accuracy is validated against the
[mooneye-test-suite](https://github.com/Gekkio/mooneye-test-suite)
(439/439 rom×model cases pass) and the
[game-boy-test-roms](https://github.com/c-sp/game-boy-test-roms) v7.0
collection (gambatte, blargg, mealybug-tearoom, SameSuite, age,
gbmicrotest, the acid tests and more — 7047 rom×model cases run, every
residual failure pinned in a documented known-failure baseline), achieved
by emulating documented hardware behavior — never by special-casing test
ROMs (see `docs/ARCHITECTURE.md`).

## Building

Needs **Rust 1.85+** (the workspace is edition 2024); install via [rustup](https://rustup.rs).

The **core** (`slopgb-core`) is pure `std` — it builds anywhere with no system
libraries. The **frontend** (`slopgb`) draws with winit + softbuffer and plays
audio with cpal, so on Linux it needs the usual desktop dev libraries:

| Distro | Install |
|---|---|
| Arch | `sudo pacman -S base-devel alsa-lib libxkbcommon` (Wayland: `wayland`; X11: `libxcb libx11`) |
| Debian/Ubuntu | `sudo apt install build-essential pkg-config libasound2-dev libxkbcommon-dev libwayland-dev libxcb1-dev` |
| Fedora | `sudo dnf install @development-tools alsa-lib-devel libxkbcommon-devel wayland-devel libxcb-devel` |

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

## Running

```sh
cargo run --release -- path/to/game.gb   # or: ./target/release/slopgb game.gb
cargo run --release                      # no ROM → blank LCD; load via drag-drop or the menu
```

Optional boot ROM (Nintendo logo + chime): `--boot path/to/dmg_boot.bin` or the
`SLOPGB_BOOT` env var (boot ROMs are copyrighted and never bundled).
