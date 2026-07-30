# Frontend module layout

The frontend renders on softbuffer. Toolkit + content + per-window state:

- `ui::` toolkit — incl. `ui::menu` PopupMenu + `ui::dialog` modal.
- `windows::` content — per-window `WinState` (`Vram` / `Debugger` / `Memory`, plus
  `Stateless` for the I/O map) over the four `ui::ToolWindow` kinds; pure
  `layout`/`target_at`/`on_*_click` hit-tests.
- `toolwin::`/`dbg::` — breakpoint set + `DebugAction`/`step_out`.

## The <1000-line cap (split map)

Every `.rs` stays under 1000 lines (the project-wide rule). The frontend splits:

- `main.rs` → `cli`/`session`/`pacing` modules + nine cohesive `impl App` blocks:
  `app_boot` (startup resource resolvers: boot ROM / plugins / MSU-1 / SGB BIOS),
  `app_draw` (palette + redraw), `app_handler` (the winit `ApplicationHandler` impl +
  `about_to_wait` loop), `app_input` (deferred sub-frame joypad input), `app_keys`
  (keyboard + the key-binding wizard), `app_menu`, `app_pacing`, `app_path` (path
  dialogs), `app_run` (discrete actions). The `App` struct itself stays in `main.rs`;
  the keymap (`input::map`) is separate from `app_keys`.
- `session.rs` → `reverse.rs` (reverse execution / rewind engine) + `session_sf2.rs`,
  both `#[path]` `impl Session` submodules.
- `windows::debugger` → `windows/debugger/`: `menubar.rs` (menu bar + dropdowns),
  `disasm.rs` (decode/format + render), `interaction.rs` (clicks / dialogs /
  `accept_dialog`), `eval.rs` (expression evaluator), `search.rs`.
- Options dialog → `windows/options.rs` (framework) + `windows/options/state.rs` +
  `windows/options/tabs.rs` (`Field`/`Ctrl` + apply/reset) +
  `windows/options/tabs/builders.rs` (the per-tab `Ctrl` builders).
- `toolwin.rs` → `toolwin/debugger_ctl.rs`.
- Each module externalizes its tests to a `#[path]` `*_tests.rs` sibling (e.g.
  `debugger_misc_tests.rs`).
