# slopgb (frontend)

The BGB-style debugger/player frontend. External deps limited to
`winit` / `softbuffer` / `cpal` (plus the internal `slopfp` and
`slopgb-plugin-host`). `forbid(unsafe_code)`. Per-area state lives in
`docs/ui-state/<area>.md` — read the matching file first.

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

- No new external deps beyond winit/softbuffer/cpal. No god files (<1000 lines).
- UI state goes in `docs/ui-state/`; never invent bgb's UI from memory —
  `docs/bgb-reference/` is the capture rig.
