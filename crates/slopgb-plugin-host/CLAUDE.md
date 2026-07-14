# slopgb-plugin-host

Native runtime that loads slopgb wasm plugins and drives them against a live
`GameBoy`. The **only** crate that depends on `wasmi` — isolated here so
`slopgb-core` stays zero-dep and the frontend keeps its winit/softbuffer/cpal
rule. Guest SDK: `slopgb-plugin-api`.

## Safe under `forbid(unsafe_code)`

The whole crate is `forbid(unsafe_code)`; wasmi's host API is fully safe. wasmi's
`'static` store bound is met by copying observable state into an owned `Snapshot`
(64KB bus image + registers) before each call, so no `GameBoy` is borrowed —
imports read the snapshot. Guest memory crosses only via the bounds-checked
`Memory::read`/`write`, never a raw pointer.

## The loaders (one per capability)

| Type | Tier | Driven by |
|---|---|---|
| `PluginHost` | 1 | `pump(&GameBoy)` once per rendered frame → each plugin's `on_frame` |
| `LoadedTool` | 2 | `call(args, &GameBoy) -> ToolResult` on demand |
| `LoadedCoprocessor` | 3 | `reset` / `run_until` / `port_*`, host-clocked |

`build_linker` (`host.rs`) registers the host imports (`host_read` / `host_reg` /
`host_log` / `host_emit`); `HostState` is the shared store data. Every loader
checks `ABI_VERSION` then gates capabilities — a plugin asking for more than the
tier serves is rejected at load.

## Golden-safe

Loading is opt-in; with no plugins loaded the pump is a no-op, so the golden path
is byte-identical. Read-only tiers cannot perturb the machine; mutation is a
separate gated opt-in.

## Test

```sh
cargo test -p slopgb-plugin-host   # wat-driven units + fixture round-trips
```

`tests/fixtures/*` are standalone `wasm32` crates (own `[workspace]`) built on the
fly by the round-trip tests.

## Rules

- Keep `wasmi` here only — never leak it into core or the frontend's dep list.
- A new capability = a new loader + a fixture round-trip proving it end to end.
