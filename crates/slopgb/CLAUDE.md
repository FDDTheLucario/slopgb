# slopgb (frontend)

The BGB-style debugger/player frontend. External deps limited to
`winit` / `softbuffer` / `cpal` / `gilrs` (game controllers) (plus the internal
`slopfp`, `slopgb-plugin-host`, and `slopgb-sgb-coprocessor`). `forbid(unsafe_code)`
(the unsafe in `gilrs`'s platform backends is contained in the dependency).
Per-area state lives in `docs/ui-state/<area>.md` — read the matching file first.

## Architecture

- `App` (`main.rs`) is the winit `ApplicationHandler`; the loop is in
  `app_handler.rs` — `about_to_wait` drives one paced step then the MCP + plugin
  pumps (the `if frames > 0` block is the per-rendered-frame hook).
- Pacing: `app_pacing.rs` — three pacers (turbo / audio / timer), each calling
  `run_one_frame`. Menus `app_menu.rs`; discrete actions `app_run.rs`;
  file/path dialogs `app_path.rs`; CLI parse `cli.rs`.
- Debug windows: `windows/` (debugger, viewers, options), drawn by the
  software UI toolkit `ui.rs` into softbuffer XRGB8888 buffers.
- Read-only introspection into core: `mcp/` (opt-in MCP server) + the plugin
  pump; serial link `link.rs`; persistence `settings_file/`.
- Plugin seams (one per capability tier — all valid subsystem types supported,
  see [`../slopgb-plugin-host/CLAUDE.md`](../slopgb-plugin-host/CLAUDE.md)):
  `--plugins <dir>` / Options→Plugins feeds the tier-1 `INTROSPECTION` pump *and*
  is where the tier-3 SGB coprocessor auto-loads its plugins from — on an SGB
  machine, `spc700.wasm` + `w65c816.wasm` in that dir replace the built-in HLE
  `SgbApu` (`slopgb-sgb-coprocessor`, via `Session::set_plugins_dir`). MSU-1 loads
  from a `--msu1` pack (`msu1.rs` drives `msu1.wasm` via `LoadedCoprocessor`). The
  tier-1 pump itself skips subsystem plugins in the dir (wrong loader, not an
  invalid plugin) — the coprocessor seam picks them up.

## Golden-safe

Every core touch is read-only `&self` debug introspection or a default-off gated
mutation. `--mcp-port` / `--plugins` are opt-in; with them off the run loop is
byte-identical to golden.

## Test / run

```sh
cargo test -p slopgb --bins
cargo run --release -- [game.gb]     # no ROM = blank LCD (bgb-style)
```

## Rules

- No new external deps beyond winit/softbuffer/cpal + gilrs (game controllers).
  No god files (<1000 lines).
- UI state goes in `docs/ui-state/`; never invent bgb's UI from memory —
  `docs/bgb-reference/` is the capture rig.
