# slopgb-plugin-host

Native runtime that loads slopgb wasm plugins and drives them against a live
`GameBoy`. The **only** crate that depends on `wasmi` — isolated here so
`slopgb-core` stays zero-dep and the frontend keeps its winit/softbuffer/cpal
rule. Guest SDK: `slopgb-plugin-api`.

## Safe under `forbid(unsafe_code)`

The whole crate is `forbid(unsafe_code)`; wasmi's host API is fully safe. The
per-frame + coprocessor paths keep the store data `'static` by copying observable
state into an owned `Snapshot` (64KB bus image + registers) before each call, so
those imports read a frame-consistent copy and never borrow the `GameBoy`. The
tool path instead lets the wasmi store hold **borrowed** data (a `&mut dyn
ToolContext`) for the duration of one call, so its imports run the exact same
core/frontend tool code — no copy, byte-identical to the built-in tools. Guest
memory crosses only via the bounds-checked `Memory::read`/`write`, never a raw
pointer.

## The loaders (one per capability)

| Type | Tier | Driven by |
|---|---|---|
| `PluginHost` | 1 | `pump(&GameBoy)` once per rendered frame → each plugin's `on_frame` |
| `LoadedTool` | 2 | `call(idx, args, &mut dyn ToolContext) -> ToolResult` on demand; a module may expose several tools (`tools()`) |
| `LoadedCoprocessor` | 3 (`SUBSYSTEM`) | `reset` / `run_until` / `port_*`, host-clocked |

**Every valid subsystem type is supported.** A `SUBSYSTEM` plugin (SPC700 /
65C816 / MSU-1 / any future chip) is a first-class plugin, and `LoadedCoprocessor`
is **generic** — it loads any module exporting the `slopgb_reset` /
`slopgb_run_until` / `slopgb_port_write` / `slopgb_port_read` ABI, with no
per-subsystem special-casing. The host is therefore obligated to support ALL
valid subsystem plugins; a new subsystem needs no new loader, only a caller that
drives it (the SGB coprocessor drives up to four loaded plugins — spc700 +
w65c816 + the optional snes-ppu + the optional msu1, MSU-1 being part of the SGB
bridge, driven at SNES `$2000-$2007`, not a separate loader).
The three loaders are **peers, one per capability** — a `SUBSYSTEM` plugin is NOT
"lesser" than a tier-1 one, it simply exports the coprocessor ABI instead of
`on_frame`, so `PluginHost` (the tier-1 per-frame pump) is the wrong loader for
it and skips it. That skip is a loader mismatch, never a verdict that the plugin
is invalid.

`build_linker` (`host.rs`) registers the per-frame/coprocessor imports (`host_read`
/ `host_reg` / `host_log` / `host_emit`) over the owned `HostState`. `build_tool_linker`
(`tool.rs`) registers those plus the tool imports (`host_read_banked` / `host_cdl_flag`
/ `host_set_breakpoint` and the bulk-result `host_registers` / `host_cdl_ranges` /
`host_disasm` / `host_screencap` / `host_vram` / `host_expr`) over the borrowed
`ToolContext`, which the caller (the frontend) supplies. Every loader checks
`ABI_VERSION` then gates capabilities — a plugin asking for more than the tier
serves is rejected at load; `host_set_breakpoint` no-ops unless the module
declared `MUTATE`.

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
- **Support every valid subsystem type.** `LoadedCoprocessor` must stay generic
  over the `reset`/`run_until`/`port_*` ABI — never special-case, hardcode, or
  reject a `SUBSYSTEM` plugin by which chip it claims to be. A new subsystem is a
  new caller, not a host change.
