# bgb menu clone — design decisions (RA0–RA2, MN6, MN7)

The analysis-gated decisions from [`bgb-rclick-menu-plan.md`](bgb-rclick-menu-plan.md),
resolved before the implementation tasks. Functional-1:1 of bgb's **menus**, not full
feature parity: every menu's *structure* matches a captured bgb screenshot, but heavy
subsystems are greyed/stubbed (recorded here with the exact future surface they'd need).
Hard constraints honored throughout: core is std-only, no new deps, `forbid(unsafe_code)`,
and the golden gate (debug introspection stays side-effect-free `&self`; new `&mut`
run/restore methods carry the same "live-debugger only, never a golden/test path" caveat
`run_until_breakpoint` already does).

## RA0 — focused-window key routing  *(scope: in; implemented M5b/M5c)*

bgb's keys are **focus-dependent**; slopgb adopts the same model by routing each key event
through the *kind of window it arrived on* (`main::window_event` already knows the
`WindowId`; add `ToolWindows::kind_of(id)`). `input::map` becomes
`map(code, mods: ModifiersState, focus: Focus) -> Option<Action>` with `pub enum Focus { Game,
Debugger }`; `App` tracks `modifiers` via `WindowEvent::ModifiersChanged`. The VRAM/iomap
windows route as `Focus::Game` (no debug keys); only the debugger window uses the alternate map.

- **Game focus** (game / VRAM / iomap windows) — keep slopgb's shipped convention unchanged
  (zero muscle-memory disruption, stays the way to open the tool windows): **F2/F3/F4 open the
  debugger/VRAM/iomap windows**, `Esc`=quit, `P`=pause, `R`=reset, `Tab`=turbo. F5–F8 left
  `None` (reserved for MN3 channel-mute). bgb's main-window save-state F-keys are **not** taken
  here (F2/F3/F4 are spent on window-opening); Quick Save / Quick Load are reached via the
  **main right-click menu** (MN1 State submenu) — functional-1:1 through the menu, the goal.
- **Debugger focus** — bgb's debugger keys are authoritative: **F2**=Toggle breakpoint,
  **F3**=Step Over, **F4**=Run to cursor, **F6**=Jump to cursor, **F7**=Trace (step into),
  **F8**=Step out, **Ctrl+G**=Go to, **F5**=VRAM viewer, **F10**=IO map, **F12**=Load ROM.
  M3a's current F7/F8 (step / step-over) re-map onto bgb's **F7=Trace, F3=Step Over**, with
  **F8=Step out** added.
- **F9 stays focus-INDEPENDENT** (global) for both entering and resuming a break — preserving
  M3a's safety property that a frozen machine can always be resumed even if the debugger window
  is closed/unfocused (never stranded).

New `Action` variants: `DbgStepOut, DbgToggleBreakpoint, DbgRunToCursor, DbgJumpToCursor,
DbgLoadRom, DbgGoto` (alongside existing `DbgBreak/DbgStep/DbgStepOver/ToggleTool/…`). In
`handle_key`, unimplemented arms are no-op stubs that consume the key until their task lands
(DbgStepOut→RM13, DbgToggleBreakpoint→RM6, DbgRunToCursor/DbgJumpToCursor→RM7, DbgGoto→RM5,
DbgLoadRom→MB2/MN4). `Esc` stays `Quit` for now (a deliberate divergence from bgb's main
`Esc`=Debugger; the right-click **Debugger** item (MN1) is the menu path — revisit once menus
exist). All RA0 changes are frontend-only; no core API is touched (golden gate untouched).

No clash remains: window-opening lives on the game window's F2/F3/F4 (no debugger is open
there yet), and bgb's debugger keys are live only while the debugger window holds focus.
Reverse/rewind chords (Shift+F7, Shift+F3, Ctrl+F4, Ctrl+E) are **left unmapped** (RA2
deferred) so there are no dead `Action` variants.

> **Modal-capture caveat (for RM3/RM4):** once a debugger modal text dialog is open,
> `handle_key` must early-return-capture keys (feed `DialogKey`) **before** this focus routing,
> so typing into a Go-to / Set-break field can't trigger debugger hotkeys.

## RA1 — breakpoint / cursor state + free-run halt  *(scope: in)*

Split the state by **who consults it**:

1. **Execution-model state** (breakpoint set, watchpoint set) → App-owned **`dbg::Debugger`**,
   the single source of truth, because both the key handler (`handle_key`) and the free-run
   loop (`about_to_wait`) reach it and the run loop has no access to `WinState`.
   - New `pub struct Breakpoints` in `dbg.rs` wrapping a `BTreeSet<u16>` (enabled PC
     breakpoints) + a `Vec<Watchpoint{addr,len,rw}>` (RM8); `Breakpoints::{toggle, contains,
     pc_list, is_empty}`. `Debugger` gains `bps`, `breakpoints()/breakpoints_mut()`,
     `set_broken(bool)`, `is_armed()` (debugger window open **and** breakpoints non-empty).
   - No dependency cycle: `dbg.rs` imports only `slopgb_core`; `windows.rs` may
     `use crate::dbg::Breakpoints` (windows → dbg → core).
2. **Per-window view state** → new **`WinState::Debugger(DebuggerState)`** in
   `windows/debugger.rs`, mirroring `VramState`: `{ disasm_base, mem_base, cursor:Option<u16>,
   pinned:bool, data_hints:BTreeSet<u16> }` (+ open-menu state added in RM4). `WinState::new`
   builds it for `ToolWindow::Debugger`.
3. **Render threading**: `windows::render` + `render_debugger` take `&DebuggerState` and
   `&dbg::Breakpoints`; `render_disasm` already returns rows, so a red gutter dot draws where
   `bps.contains(row.addr)`. The borrow threads through `toolwin::redraw` (new `bps` param) to
   the `main.rs` call sites, which pass `self.dbg.breakpoints()` (a separate `App` field from
   `self.tools`, so no borrow conflict). VRAM/iomap ignore the new param.
4. **Cursor resolution**: pure `target_at(read, area, &DebuggerState, pc, sp, px, py) ->
   ClickTarget` in `windows/debugger.rs` (`ClickTarget = Disasm(u16)|Memory(u16)|Stack(u16)|
   Reg(RegField)|Menu|None`). It computes `DebuggerLayout::for_size` internally, picks the pane
   by `rect.contains`, then **re-runs `disasm_rows` from the same view-base the renderer used**
   (variable-length instructions ⇒ render and hit-test can never disagree — the invariant
   `vram::layout` enforces). Headless-testable beside `debugger_tests.rs`.
5. **Click → action**: `toolwin::on_mouse_left` and a new `on_mouse_right` return
   `Option<DebugAction>` (enum in `dbg.rs`: `ToggleBreakpoint(u16)`, `RunToCursor(u16)`,
   `SetPc(u16)`, `CallCursor(u16)`, `SetWatchpoint{..}`, `EditReg(RegField,u16)`). View-only
   effects (Go to, pin toggle, data-hint toggle, menu open/close) mutate `DebuggerState`
   in-window and return `None`. `main::window_event` routes `MouseButton::Right` (today only
   Left) to `on_mouse_right`, then applies the `DebugAction` against `self.session.gb` +
   `self.dbg`.
6. **Free-run auto-halt** (in scope): new core
   `GameBoy::run_frame_until_breakpoint(&mut self, &[u16]) -> Option<u16>` beside `run_frame` /
   `run_until_breakpoint` — same frame-count-target + cycle-deadline loop, checking PC after
   each `step()`, same live-debugger-only caveat. `main.rs` gets `fn advance_frame(&mut self)`:
   when `dbg.is_armed()` run `run_frame_until_breakpoint(&pc_list)` and `set_broken(true)` on a
   hit, else plain `run_frame()`. The three pacers (`run_turbo`/`run_audio_paced`/
   `run_timer_paced`) call `advance_frame()` instead of `gb.run_frame()`; after pacing,
   `if dbg.is_broken() { update_title(); request_redraw_all() }` and the existing top guard
   idles to `Wait`. Run-no-break (Shift+F9) takes the plain `run_frame` path.

## RA2 — reverse execution / rewind  *(scope: deferred; menu items greyed)*

Defer the reverse engine (Trace reverse Shift+F7, Step Over reverse Shift+F3, Run cursor
reverse Ctrl+F4, Rewind cycles Ctrl+E). In the Run-menu `PopupMenu`, render all four
**present-but-disabled** so the dropdown stays item-for-item 1:1 with `menubar-run.png`;
`item_at` returns `None` over them; their chords stay **unmapped** in `input::map` (no dead
`Action` variants); `dbg::Debugger` gains no reverse methods yet.

Future core surface (shared with MN6, build once): (1) transitive `#[derive(Clone)]` across
the machine (verified: no non-Clone field types); (2) `GameBoy::snapshot()->Snapshot` +
`restore(&Snapshot)` in `lib.rs` (not `debug` — `restore` is `&mut`, golden-safe); (3)
frontend ring `VecDeque<(u64 cycle, Snapshot)>` (`rewind.rs`), push every N frames, capped;
(4) reverse algorithms live entirely frontend-side — restore the newest snapshot before the
target, replay forward via existing `step`/`step_over`/`run_until_breakpoint`. Optimization
note: `Cartridge.rom: Vec<u8>` is immutable yet cloned each snapshot — a future `Rc<[u8]>`
(or snapshot-mutable-state-only) avoids that. **MN6's Quick Save/Load is the better first
customer of the snapshot primitive** (one snapshot, no replay).

## MN6 — save states  *(scope: partial; Quick Save/Load now, disk path deferred)*

No whole-machine serialization exists today (only battery-RAM/RTC `save_data`). Do now:
(1) transitive `#[derive(Clone)]` (mechanical, runtime-inert, golden-safe — see RA2 list);
(2) `Session.quick_state: Option<Box<GameBoy>>` + `Session::quick_save`/`quick_load` (clone
in/out, then `resync_pacing`); (3) the State submenu's **Quick Save (F2)** / **Quick Load
(F4)** items call those, plus the per-RA0 game-window key path; (4) **Select F3 / Load
recovery state / Load state…** render **greyed** until the disk format lands.

Deferred disk path: `GameBoy::save_state()->Vec<u8>` (`&self`, golden-safe) / `load_state(&[u8])
->bool` in a new std-only `crates/slopgb-core/src/state.rs` — hand-rolled little-endian
`Writer`/`Reader` (no serde), header = magic + `u16` version + model + `u64` ROM-hash guard
(ROM not stored; reload from `Session.rom_bytes`, reject hash mismatch). Per-subsystem
`save`/`load` for cpu, interconnect (incl. sub-dot edge fields), ppu (incl. FIFO/pixel-pipe
state, vram, oam, palette RAM), apu (channels + scalars, **not** the sample Vecs — clear on
load), timer, cartridge+Mapper+Rtc (RAM + regs; ROM excluded), serial, joypad. TDD:
per-subsystem round-trip + save/run-N/load/run-N frame-equality + version/ROM-mismatch
rejection + a golden-gate guard that `save_state` leaves the machine bit-identical.

## MN7 — link / options / cheat / camera  *(scope: partial; structure now, subsystems deferred)*

Greying uses the `PopupMenu` `enabled:bool` flag (RM1) — disabled rows render grey and
`item_at` (RM2) returns `None`. The main-menu builder lives in a new
`crates/slopgb/src/windows/mainmenu.rs` (or inline in `windows.rs`).

- **Link** (Listen/Connect/Disconnect/Cancel listen) — all four greyed, correct labels/order,
  no action. Future: core `serial.rs` peer-bit exchange + external-clock completion + a
  per-transfer hook (`serial_attach_peer`/`serial_exchange_byte` on `&mut GameBoy`, frontend
  path only); frontend `link.rs` std::net TCP layer (bgb protocol, port 8765, 8-byte packets).
- **Options… (F11)** — partial stub: a titled "Options" modal (generalize the RM3 box to an
  info/Close variant) exposing the **one** real toggle that exists (sound mute) as a single
  checkbox; no fake tabs. Future: a frontend `Config` struct (System/Sound/Color/Input)
  persisted via `std::fs`; basics need no core API (scale; palette via existing
  `set_dmg_palette`; per-channel mute via the MN3 accessor).
- **Cheat… (F10)** — partial stub: a titled "Cheats" modal with an inert/empty code list +
  Close. Future: core `cheat.rs` (GameShark RAM poke + Game Genie ROM patch) applied once per
  frame via a new `&mut self` `set_cheats`/`apply_cheats` (frontend path only); frontend editor.
- **Camera control…** — greyed, gated on a Pocket Camera mapper; `Mapper` has no camera
  variant so the predicate is always false (matches bgb — always disabled). Future: core
  `Mapper::PocketCamera` (M64282FP sensor) + a still-image source (webcam needs a forbidden
  dep). Large + low value → deferred.

## Milestone order (derived)

- **M5a** — debugger right-click made functional: RM4 (routing) + RM5 (Go to, consumes the
  modal) + RM6 (breakpoint toggle + red gutter + free-run halt, consumes `Breakpoints` + the
  new core `run_frame_until_breakpoint`) + RM7 (run/jump/call cursor) + RM12 (stay-on-bank).
- **M5b** — RA0 keymap split; RM8 (watchpoints), RM9 (code/data hints), RM10 (copy), RM11
  (edit register — needs a core register-set accessor), RM14 (evaluate expr / user clocks),
  RM15 (bp/wp manager dialogs).
- **M5c** — MB1–MB5 (menu-bar dropdowns) + RM13 (Run/Debug execution set; reverse items greyed).
- **M5d** — MN1–MN5 (main right-click menu) + MN6 (Clone snapshot → Quick Save/Load) + MN7 stubs.
- **M5e** — RM16 integration + per-menu visual diff vs the captures.
