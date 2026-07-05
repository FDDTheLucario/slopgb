# bgb-clone QA-fixes plan (friend's QA round, 2026-06-17)

Six findings on the merged bgb-UI clone. Functionally 1:1 with bgb, **not** code
parity. Core changes are golden-safe `&self` debug accessors only (gbtr golden
byte-identical, mooneye green). Files stay <1000 lines. TDD per task; review each
diff (cavecrew-reviewer / rust-diff-review), fix ALL findings, re-review, commit.

## Findings → tasks

1. **FF3x unreliable in I/O viewer.** `debug_read(0xFF30+i)` routes to gated
   `ch3.read_ram` (0xFF at low freq / volatile current byte at max freq on DMG)
   → flickers. Fix: golden-safe `GameBoy::wave_ram()->[u8;16]` returns the raw
   stored `apu.ch3.ram` (the canonical buffer `write_ram` populates); I/O viewer
   reads from it. (T1, T4)
2. **No current ROM/RAM bank indicator** (only rVBK=VRAM / rSVBK=WRAM shown).
   Fix: golden-safe `Cartridge::cur_rom_bank()` / `cur_ram_bank()` →
   `GameBoy::rom_bank()` / `ram_bank()`; surface in the debugger regs pane +
   I/O map, labeled distinctly from rVBK/rSVBK. (T2, T3, T5)
3. **DMG palette display.** VRAM Palettes tab always shows CGB palette RAM; on a
   DMG machine show BGP/OBP0/OBP1 shade mappings (rBGP/rOBP) via the GREYS ramp +
   `debug::dmg_palette_shades`. (T6)
4. **BG map unbounded.** Grid + frame span the whole content rect past the
   256×256 map. Fix: confine grid/frame to the actual drawn extent
   (cols*8*scale × rows*8*scale) per tab. (T9)
5. **VRAM viewer doesn't scale on resize.** Fixed scale (TILE=2, OAM=2, bgmap=1).
   Fix: `fit_scale` integer-scale-to-fit per tab natural size; 1px tile borders
   preserved (vline/hline already 1px). (T7, T8)
6. **Right-click menu clipped by window edge.** USER DECISION: make it its own
   **borderless winit window** (bgb-native-popup / tearable-90s-menu style) that
   can extend onto the desktop past the game window. (T10–T14)

## T10 — own-window menu architecture (decision)

- **Single borderless popup window** hosts the whole menu tree: the `MainMenu`
  plus the currently-open `SubMenu` are drawn side-by-side in **popup-local
  coordinates** (main at origin (0,0); submenu hung at `parent_row.right()`).
  One window, NOT one-window-per-submenu → sidesteps the nested-window
  focus-dismissal problem (opening a child window steals focus from the parent,
  which would otherwise close it).
- **Positioning:** screen origin = game-window `outer_position()` + cursor
  position within the game window. Clamped to stay on the monitor when the
  monitor size is known (winit `current_monitor().size()`), else unclamped.
  **Wayland caveat:** winit cannot place a top-level at arbitrary global coords
  on Wayland; the popup is still a separate, un-clipped window (the core fix),
  just compositor-placed. Documented, not worked around.
- **Window:** `WindowAttributes::with_decorations(false)`, sized via
  `popup_content_size` (union of `menu_bounds(main)` ∪ `menu_bounds(sub)`),
  resized when a submenu opens/closes. Reuses the toolwin softbuffer pattern.
- **Close policy:** item activation, off-menu click (lands on no row), `Esc`,
  or `Focused(false)` on the popup window. Activation runs the existing
  `MenuEffect`/`SubChoice` dispatch (`run_action`/`apply_sub_choice`).
- **Coordinate space:** the popup's own `CursorMoved`/`MouseInput` events are
  popup-local already (winit reports per-window), so `item_at`/`choice_at` work
  unchanged against popup-local origins.
- **Scope:** ONLY the right-click menu tree moves to the popup window. The
  `InfoBox` (Cart/System/About), Options dialog, path modal, and key-rebind
  wizard stay as game-window overlays (they are centered/modal, never clipped).
- **Module seam:** new `crates/slopgb/src/menupopup.rs` owns the popup window +
  surface + routing (mirrors `toolwin.rs`); pure size/positioning helpers live
  in `windows/mainwin.rs` (unit-tested). `App` owns `Option<MenuPopup>` and its
  `WindowId`. Keeps `main.rs`/`app_menu.rs` under 1000 lines.

## Test seams (pure, headless)

- `Wave::ram()` raw bytes; per-mapper `cur_rom_bank`/`cur_ram_bank`.
- `regs_lines` ROM/RAM bank text; `wave_row` over a `[u8;16]`.
- VRAM Palettes-tab row model (DMG 3 rows vs CGB 16); `fit_scale`; bounded extent.
- `popup_content_size` union; `popup_screen_origin` clamp; popup-local
  `item_at`/`choice_at` dispatch.

The winit glue (T13/T14 borderless window + cross-window routing) is **not**
headless-testable (synthetic keys don't reach winit; clicks do) — verified by
real screenshot captures, never hallucinated.
