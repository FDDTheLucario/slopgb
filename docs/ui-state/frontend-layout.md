# Frontend module layout

The frontend renders on softbuffer. Toolkit + content + per-window state:

- `ui::` toolkit — incl. `ui::menu` PopupMenu + `ui::dialog` modal.
- `windows::` content — per-window `WinState` for VRAM + debugger; pure
  `layout`/`target_at`/`on_*_click` hit-tests.
- `toolwin::`/`dbg::` — breakpoint set + `DebugAction`/`step_out`.

## The <1000-line cap (split map)

Every `.rs` stays under 1000 lines (the project-wide rule). The frontend splits:

- `main.rs` → `cli`/`session`/`pacing` modules + `app_run`/`app_menu`/`app_pacing`/
  `app_input`/`app_path` (cohesive `impl App` blocks). The winit `ApplicationHandler`
  impl + the `App` struct stay in `main.rs`.
- `windows::debugger` → `windows/debugger/menubar.rs` (menu bar + dropdowns) +
  `windows/debugger/disasm.rs` (decode/format + render).
- Options dialog → `windows/options.rs` (framework) + `windows/options/tabs.rs`
  (per-tab `Ctrl` builders).
- Each module externalizes its tests to a `#[path]` `*_tests.rs` sibling (e.g.
  `debugger_misc_tests.rs`).
