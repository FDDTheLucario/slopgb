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

**Plugin/mediator menu rows** splice in right after "Save screenshot" — one row
per `menu` record the live engaged SGB coprocessor's manifest declares (e.g.
"Export SPC", declared by the native `SgbCoprocessor` mediator, not by
`spc700.wasm` — see
[`plugin-api.md`](plugin-api.md#menu-records-and-who-declares-them) and
[`../hardware-state/sgb-audio.md`](../hardware-state/sgb-audio.md)). `App`
snapshots the table (`App.plugin_menu_rows: Vec<PluginMenuRow>`) when the popup
opens (`build_plugin_menu_rows`, parsing `GameBoy::coprocessor_manifest` via
`slopgb_plugin_host::Manifest::parse`), so a click's `Action::PluginMenu(i)`
always indexes what was actually shown even if the live machine changes before
the click lands; `run_plugin_menu` looks the row up, calls
`GameBoy::coprocessor_export`, and writes the blob to
`slopgb-<unix-millis>.<ext>`. **Deliberate new contract:** with no engaged
coprocessor (or one that declares no rows) the row
is **absent entirely**, not greyed; a declared row is greyed only when its
`AudioCoprocessor::export_ready` is false right now. `mainwin::MainMenu::open`
takes the row table (`&[(label, enabled)]`) as a plain parameter — the widget
layer carries no SPC-specific string or logic.

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
- **Cheat...** opens a full cheat dialog (`App.cheat_dialog`,
  `cheat_ui::CheatDialog`), modeled on bgb's Cheat window
  ([`../bgb-reference/cheat/`](../bgb-reference/cheat/README.md)): a cheat list +
  bgb's button grid (Add / Edit / Delete / Enable / Disable / Enable all /
  Disable all / Poke / Load / Save / Advanced / Close), a two-field Comment/Code
  editor (Tab switches fields), and an Advanced toggle that adds the decoded
  `(addr)=val` column. Model in `cheat.rs` (`CheatList`): GameShark `01vvaaaa` →
  per-frame RAM poke (`debug_write` in `app_pacing::run_one_frame`, addr
  little-endian); Game Genie → a golden-safe core ROM patch
  (`GameBoy::set_gg_patches`, pushed once per pacing wake). Left-click selects
  rows / fires buttons; keys drive the editor + arrows/Space/Delete/Esc.
  Load/Save read/write a cheat file (`+ code comment`, `-` = disabled) via the
  path modal; cheats are in-memory per session.
