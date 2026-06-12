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

## Tests

```sh
test-roms/download.sh        # fetch the pinned test-ROM bundles (~once)
cargo test --workspace       # unit tests + mooneye + game-boy-test-roms harnesses
```

## Running

```sh
cargo run --release -- path/to/game.gb
```
