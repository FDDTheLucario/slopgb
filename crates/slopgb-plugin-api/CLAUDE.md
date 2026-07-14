# slopgb-plugin-api

Guest SDK for slopgb plugins: authored in Rust, compiled to
`wasm32-unknown-unknown`, loaded at runtime by `slopgb-plugin-host`. A plugin
implements one capability trait and invokes its macro. Human guide:
`docs/ui-state/plugin-api.md`.

## The unsafe rule (why this crate is `deny`, not the workspace `forbid`)

Crate-level `#![deny(unsafe_code)]` — a wasm guest needs two linkage MARKERS the
workspace `forbid` rejects: the `unsafe extern` import block (`abi.rs`) and the
`#[unsafe(no_mangle)]` exports (the macros). Both are scope-`allow`ed and are the
ENTIRE unsafe surface. No `unsafe` blocks, no raw pointers, no `from_raw_parts`.
Host→guest bulk crosses via a guest-owned scratch buffer read by safe indexing;
guest→host via the guest's own `as_ptr`/`len` (host reads through wasmi's
bounds-checked `Memory`).

## Capabilities (one trait + macro each)

| Trait | Macro | Shape |
|---|---|---|
| `Plugin` | `slopgb_plugin!` | tier 1: per-frame read-only `on_frame(&GameBoyView)` |
| `ToolPlugin` | `slopgb_tool_plugin!` | tier 2: on-demand `call(args, &view) -> ToolResult` |
| `Coprocessor` | `slopgb_coprocessor_plugin!` | tier 3: host-driven `reset`/`run_until`/comm-ports |

Wire contract (`abi.rs`): `ABI_VERSION`, `Reg`, the `host_*` imports — the host
must agree. `abi.rs` is cfg-split (real wasm imports vs off-wasm `unreachable!`
stubs) so the crate builds for BOTH targets; the host build reuses the contract
constants.

## Build / test

```sh
cargo build  -p slopgb-plugin-api --target wasm32-unknown-unknown   # the real target
cargo clippy -p slopgb-plugin-api --target wasm32-unknown-unknown -- -D warnings
cargo test   -p slopgb-plugin-api                                   # host: units + doctest
```

## Rules

- Never add an `unsafe` block or `from_raw_parts`; extend the scalar / own-ptr
  ABI instead.
- Any import/export shape change ⇒ bump `ABI_VERSION` (the host rejects a
  mismatch at load).
- Terse rustdoc on every public item; narrative lives in `docs/ui-state/plugin-api.md`.
