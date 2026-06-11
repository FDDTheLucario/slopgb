# slopgb

Cycle-accurate Game Boy / Game Boy Color emulator in Rust.

- `crates/slopgb-core` — emulator core: zero dependencies, no unsafe,
  deterministic. Emulates DMG0/DMG/MGB/SGB/SGB2/CGB/AGB models.
- `crates/slopgb` — cross-platform desktop frontend (winit + softbuffer +
  cpal).

Accuracy is validated against the
[mooneye-test-suite](https://github.com/Gekkio/mooneye-test-suite);
the goal is a full pass, achieved by emulating documented hardware behavior —
never by special-casing test ROMs (see `docs/ARCHITECTURE.md`).

## Tests

```sh
test-roms/download.sh        # fetch the pinned mooneye ROM bundle (~once)
cargo test --workspace       # unit tests + mooneye integration harness
```

## Running

```sh
cargo run --release -- path/to/game.gb
```
