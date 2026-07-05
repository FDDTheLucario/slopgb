# Game-window right-click menu

bgb's own right-click menu (`rc-main.png`) — `windows::mainwin::MainMenu` rendered
in its **own borderless winit window** (`menupopup::MenuPopup`, `App.menu_popup`).
This was the QA fix for the old in-canvas overlay clipping at the game-window edge:
like bgb's native popup it now extends onto the desktop.

## Window + positioning

- One window hosts the whole tree (main menu + open submenu side-by-side in
  popup-local coords) — no nested-window focus problem.
- Sized via `mainwin::popup_content_size` (∪ of the menus' `menu_bounds`);
  positioned via `mainwin::popup_screen_origin` (game `outer_position()` + cursor,
  clamped to the monitor). **Wayland caveat:** the compositor places it, not pixel-exact.
- **Transparent gap.** The window spans the union of the main + submenu boxes, so a
  short submenu leaves an L-shaped gap (right of the main menu, above/below the
  submenu). The window is created `.with_transparent(true)`; `redraw` clears the
  buffer to `0x00000000` and forces opaque alpha **only** over the menu-box rects
  (`mainwin::popup_menu_boxes`), so the gap shows the desktop — bgb's submenu is a
  separate floating box, not a filled background.
  - **Do** clear the gap to `0x00000000`: compositors use premultiplied alpha, so a
    transparent pixel must have RGB=0. **Don't** clear to `theme.bg` (alpha-0 white)
    — an invalid premultiplied pixel renders white, not transparent.
  - A non-compositing WM (no ARGB visual) renders the gap black — an accepted edge case.
- Routed by `App::on_popup_event`/`on_popup_click`. Dismissed on item activation /
  click-away (a game-window click) / Esc / focus-loss **after** first focus
  (`menupopup::focus_dismiss` ignores the spurious on-map `Focused(false)` some WMs send).

## Rows + behaviour

Leaf rows run via `run_action`; rows carry a `MenuEffect` (Run/Submenu/None).

- **Pause** — check-marked while paused (`paused` threaded through
  `entries()`/`MainMenu::open`/`MenuPopup::open`).
- **Enable sound** — runtime `App.muted` gate, checked while audible.
- **Reset**.
- **Debugger** — also opens on **Esc** from the game/viewer windows when the
  Options "pressing Esc shows debugger" toggle is on (`Settings.esc_shows_debugger`,
  default on). Esc never quits.
- **Save screenshot** — writes the current frame to `slopgb-<ms>.bmp` (std-only
  24-bit BMP, `screenshot::to_bmp`).
- **Exit**.

Submenu (`▶`) rows **open on hover** as well as click: `menupopup::hover_open`
decides from the hovered effect + a tracked `open_kind`, so a per-pixel move over
the already-open row doesn't rebuild — `on_cursor_moved` returns the same `OpenSub`
a click takes. Esc peels the open submenu before the menu.

## Submenus

Each child is one `SubMenu` type with a `SubChoice` variant per kind.

| Submenu (`SubKind`) | Rows |
|---|---|
| **Window size** | 1×–6× + Full screen / Fullscreen stretched (active checked — `request_inner_size` / borderless fullscreen + a stretched `blit`) |
| **Sound channel** | channels 1-4 (F5-F8) mute toggles, checked while audible → core per-channel mute mask (`GameBoy::set_channel_mute`/`channel_muted` → APU `mix` gate; golden-safe, defaults all-audible, survives NR52 power cycles) |
| **Other** | Cart info / System info / About open a centred `InfoBox` overlay (`render_info`; any click or Esc closes — `App.info_box`); VRAM viewer toggles the tool window; cheat-searcher/Camera/clear-recent/debug-mode/Close-screen greyed |
| **State** | see [save-states-and-link.md](save-states-and-link.md) |
| **Recent ROMs** | lists `App.recent: Vec<PathBuf>` (most-recent-first, deduped, capped at 10 by `push_recent_into`); each row reloads that ROM (`SubChoice::LoadRecent`) |
| **Link** | see [save-states-and-link.md](save-states-and-link.md) |

- **Cart info** parses the header straight from `Session.rom_bytes`
  (`cart_info_lines`/`cart_type_name`).
- **Load ROM...** opens a path-entry text modal over the LCD
  (`App.path_dialog: Option<InputDialog>`); accept loads via the existing `load_dropped`.
- **Cheat...** stays a read-only info-box stub.
