# slopgb (frontend)

The BGB-style debugger/player frontend. External deps limited to
`winit` / `softbuffer` / `cpal` / `gilrs` (game controllers) (plus the internal
`slopgb-core`, `slopfp`, `slopgb-plugin-host`, `slopgb-sgb-coprocessor`, and
`slopgb-sf2`). `forbid(unsafe_code)` via the workspace `[lints.rust]`
(the unsafe in `gilrs`'s platform backends is contained in the dependency).
Per-area state lives in `docs/ui-state/<area>.md` — read the matching file first.

## Architecture

- `App` (`main.rs`) is the winit `ApplicationHandler`; the loop is in
  `app_handler.rs` — `about_to_wait` drives one paced step then the MCP + plugin
  pumps (the `if frames > 0` block is the per-rendered-frame hook).
- Pacing: `app_pacing.rs` — three pacers (turbo / audio / timer), each calling
  `run_one_frame`. Menus `app_menu.rs`; discrete actions `app_run.rs`;
  file/path dialogs `app_path.rs`; CLI parse `cli.rs`; startup resource
  resolvers (boot ROM / SGB BIOS bytes, the tier-1 plugin host, and the
  `PluginRegistry` whose manifests supply the plugin-contributed CLI flags)
  `app_boot.rs`; palette + redraw `app_draw.rs`; keyboard input + the
  key-binding wizard `app_keys.rs`.
- Debug windows: `windows/` (debugger, viewers, options), drawn by the
  software UI toolkit `ui.rs` into softbuffer XRGB8888 buffers.
- Read-only introspection into core: `mcp/` (opt-in MCP server) + the plugin
  pump; serial link `link.rs`; persistence `settings_file/`.
- Plugin seams (one per capability tier — all valid subsystem types supported,
  see [`../slopgb-plugin-host/CLAUDE.md`](../slopgb-plugin-host/CLAUDE.md)):
  `--plugins <dir>` / Options→Plugins feeds the tier-1 `INTROSPECTION` pump *and*
  is where the tier-3 SGB coprocessor auto-loads its plugins from — on an SGB
  machine, `spc700.wasm` + `w65c816.wasm` in that dir (each enabled in
  Options→Plugins), plus the optional `snes-ppu.wasm`, fill core's coprocessor slot
  (`slopgb-sgb-coprocessor`, via `Session::set_plugins_dir`); absent or
  disabled, the slot stays empty and there is no SNES side at all — core ships
  no SNES implementation. A per-plugin toggle applies at the next reset / ROM
  load, never mid-run. MSU-1 is
  part of that SGB coprocessor: `msu1.wasm` (optional, same plugins dir) driven at
  SNES `$2000-$2007` via the game's resident 65C816 handler; `--msu1` /
  `SLOPGB_MSU1` only selects the `.pcm` pack dir, which defaults to the loaded
  ROM's directory — no frontend cart-bus bridge. `sf2.wasm` is the exception the
  frontend drives itself: `session_sf2.rs` runs it on a `--sf2` cache miss to
  convert a SoundFont-2 into the N-SPC sample bank.
- Plugin CLI flags are **not** frontend-owned: `--msu1` / `--sf2` are contributed by
  their plugin's manifest, collected in `app_boot::build_registry` (two plugins
  claiming one role is fatal — exit 2), given their explicit value by
  `apply_plugin_flags` (CLI, else the `SLOPGB_<NAME>` env fallback), read back by
  `effective_plugin_flags` (else the manifest default), and threaded as opaque
  name/value pairs through `Session::set_plugin_flags` → `apply_sgb_coprocessor`.
  Adding a flag means editing a plugin manifest, not
  `cli.rs`. The tier-1 pump itself skips subsystem plugins in the dir (wrong
  loader, not an invalid plugin) — the coprocessor seam picks them up.

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
