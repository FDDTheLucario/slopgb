# slopgb-core

The cycle-accurate GB/GBC emulator. Zero external (crates.io) deps — std only,
plus one in-tree path dep, `slopgb-snes-apu`, re-exported for the shared
save-state `Reader`/`Writer` and `StateError`.
`forbid(unsafe_code)`. The root `CLAUDE.md` and `docs/ARCHITECTURE.md` are
authoritative — **read `docs/ARCHITECTURE.md` before touching timing.**

## Non-negotiables

- **Timing contract:** tick-then-access M-cycles (ARCHITECTURE.md). The eager
  cycle-exact clock is the only clock.
- **Golden-safe:** every debug hook is read-only `&self` or a default-off gated
  mutation. Verify any change byte-identical via `golden_fingerprint` + the
  mooneye matrix before trusting it.
- **Baselines are A/B-swept trades:** read the floor-class index header in
  `tests/gbtr/baselines/gambatte.txt` before touching baselined behavior — a
  one-sided "fix" regresses the now-green siblings.
- **Core emulates no SNES chip.** No HLE SNES audio/CPU/PPU lives here — every
  SNES-side capability arrives through the `sgb::AudioCoprocessor` slot, which
  starts `None` on every model and only `set_audio_coprocessor` (SGB/SGB2 only)
  fills. SGB border, palettes and the ATTR/PAL packet handling (`ppu/sgb/`) ARE
  core PPU HLE and stay. Save states carry a SNES tail only when a coprocessor is
  installed (`STATE_VERSION` 18; older states are rejected, there is no migration).
- **No new deps, ever.** No god files (<1000 lines): split into `foo.rs` + `foo/`
  (second `impl` via `use super::*`), externalize tests to `#[path] *_tests.rs`;
  `tests/source_size.rs` (`LIMIT = 1000`) enforces it.
- **No jargon comments:** no fork/session codenames or A/B-sweep stories; keep the
  pinning ROM + hardware citation inline; re-verify a comment when you touch it.

## Per-subsystem state

`docs/hardware-state/<subsystem>.md` — read the matching file before touching a
subsystem, and write state changes THERE, not in code comments. Hardware
questions: `docs/hardware-state/` → gbctr → Pan Docs → the failing test's `.s`
asm → SameBoy/mooneye/gambatte source.

## Test

```sh
cargo test -p slopgb-core --lib <module>
cargo test -p slopgb-core --test mooneye     # 439/439
cargo test -p slopgb-core --test gbtr        # 219/219 green (470 baselined), ~4 min
cargo run  -p slopgb-core --example run_mooneye -- <rom> [model]
```
