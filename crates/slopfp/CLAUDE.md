# slopfp

Std-only, framework-agnostic file-picker state machine + view-model (no
rendering, no deps). Keeps its own inline `#![forbid(unsafe_code)]` (lib.rs) on
top of the workspace lint, so it stays safe standalone. Package metadata is
concrete (non-inherited) so the crate directory is liftable into any project by
copy-paste — keep it that way; `[lints] workspace = true` is the only inherited
line, and the inline `forbid` makes it droppable on a lift.

## Shape

- `lib.rs` — shared types + the `Picker` state machine.
- `model.rs` — pure navigation / selection model.
- `source.rs` — the **only** module touching `std::fs` (directory listing).

## Test

```sh
cargo test -p slopfp     # units + tests/nav.rs
```

## Rules

- Zero dependencies, forever. Zero rendering — the host draws the view-model.
- Keep `source.rs` the sole `std::fs` boundary; everything else stays pure and
  unit-testable without a filesystem.
