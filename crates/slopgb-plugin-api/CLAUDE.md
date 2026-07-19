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
| `ToolPlugin` | `slopgb_tools!` (many) / `slopgb_tool_plugin!` (one) | tier 2: on-demand `call(args, &view) -> ToolResult`; a module lists several tools, each with `name`/`description`/`input_schema` for MCP `tools/list` |
| `Coprocessor` | `slopgb_coprocessor_plugin!` | tier 3: host-driven `reset`/`run_until`/comm-ports |

`GameBoyView` also carries the tool-only debug helpers (`read_banked`, `cdl_flag`,
`set_breakpoint`, and the bulk-result `registers_text`/`cdl_ranges`/`disassemble`/
`vram`/`screencap`/`expr`); `args::field` pulls a string out of the JSON request
without a JSON dep. These require the tool host, not the per-frame host.

**Subsystem plugins are first-class.** A `Coprocessor` (`SUBSYSTEM`) plugin — e.g.
the SPC700, 65C816, MSU-1 chips (`slopgb-{spc700,w65c816,msu1}-plugin`) — is as
valid a plugin as any introspection one. The plugin **system** must support
*every* valid subsystem type: `LoadedCoprocessor` is generic (any module
exporting the `reset`/`run_until`/`port_read`/`port_write` ABI loads). What
differs is the *loader*, not the validity — a `SUBSYSTEM` plugin exports the
coprocessor ABI, not `on_frame`, so it loads through `LoadedCoprocessor` (driven
by the frontend's coprocessor seams — the SGB coprocessor auto-loads from the
`--plugins` dir, MSU-1 from a `--msu1` pack), **not** the tier-1 `PluginHost`
per-frame pump. The tier-1 `--plugins` *scanner* skips a subsystem plugin in that
dir — a loader mismatch, never "an invalid plugin".

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
