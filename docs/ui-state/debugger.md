# Debugger window

bgb functional clone. Keys are **focus-dependent** like bgb.

## Focus-dependent keys

| Window | Keys |
|---|---|
| **game** | F2/F3/F4 open the debugger / VRAM viewer / I/O map (also `SLOPGB_OPEN_TOOLS=debugger,vram,iomap`) |
| **debugger** | F2 toggle breakpoint · F3 step over · F4 run to cursor · F6 jump to cursor · F7 trace (step) · F8 step out · Ctrl+G go to · Ctrl+H/Ctrl+J/Ctrl+K open the breakpoint/watchpoint/freeze manager · F5/F10 open VRAM/iomap |
| **memory viewer** (standalone) | arrows move the byte cursor (a byte / a row), PageUp/Down a window (auto-scroll) · **hex digits type over the cursor byte in place** (2nd nibble commits via `debug_write`) · Esc cancels a pending edit · Ctrl+G go to |
| **any** | F9 break/resume (focus-independent so a frozen machine is always resumable) |

Non-debugger tool windows are `Focus::Viewer` (game hotkeys but no joypad — only the
game window drives buttons).

## Right-click context menus (every pane)

Go to (modal hex prompt) · set/clear breakpoint (red gutter dot; the paced loop
free-runs to it via core `run_frame_until_breakpoint`) · run-to-cursor ·
jump-to-cursor / call-cursor (core golden-safe `debug_set_pc`/`debug_call`) · set
watchpoint (free-run halts on a matching memory access — core golden-safe
`set_watchpoints` checked in `run_frame_until_breakpoint`; empty list = inert, never
set on a golden path) · **freeze value** (memory pane only: locks the cursor byte to
its current value, re-forced each frame — see Freeze) · stay-on-bank pin · code/data
hints (`db` markers that re-flow the disasm) · **edit register** on the registers pane
(a hex prompt seeded with the live value → core `debug_set_reg`).

**Copy data/code** copies 16 hex bytes / 16 disasm rows at the cursor to the system
clipboard — dep-free via a `std::process` shell-out to `wl-copy`/`xclip`/`xsel`
(`clipboard::copy`, no Cargo dep — respects the winit/softbuffer/cpal-only rule;
`toolwin::debugger_copy_text` builds the text, `Action::DbgCopyData`/`DbgCopyCode`
carry the clicked addr). A host with no clipboard tool logs a hint (non-fatal).

## Modal prompt

One `InputDialog` + key/click plumbing backs every `DialogKind` (Go to… / edit
register / Search string / Evaluate expression + a display-only result box). An
accept routes back to `main` as a `MenuOutcome` (`feed_dialog`/`dialog_click`).

- **Evaluate expression** parses a bgb-style expr (`windows::debugger::eval_expr`:
  hex-by-default, registers take precedence, `[x]` byte-deref, `+ - *` + parens,
  totally panic-free) → hex+dec result.
- **Set user clocks counter** zeroes the regs-pane `cnt` (emulated cycles since the
  reset, `gb.cycles()` − a per-window baseline).

## Menu bar (File/Search/Run/Debug/Window/Execution profiler)

Items reuse the keyboard dispatch via `MenuChoice::Command(input::Action)` →
`MenuOutcome` → `main::run_action`, so a menu item and its hotkey never diverge.

- **Run** — Run/Trace/Step Over/Step out/Reset/Run to Cursor/Jump to cursor/Call cursor.
- **Window** — VRAM viewer / I/O map.
- **File** — save screenshot · save memory_dump (64 KiB `debug_read` dump) · save
  asm... (4096 disasm rows from the base, honouring code/data hints →
  `slopgb-asm-<ms>.txt`, `toolwin::debugger_disasm_dump`) · Save state... (Ctrl+W) /
  Load state... (Ctrl+L) via the shared path modal. ROM-load rows greyed pending a picker.
- **Search** (fully live) — Search string (Ctrl+F, `find_match`: a hex-byte
  sequence `3E 01` or a case-insensitive mnemonic substring `ld a,`, wrapping the
  address space) · Continue search (Ctrl+C) · go to next/previous bookmark
  (Ctrl+N/Ctrl+B, walking bookmarks ∪ breakpoints via `next_mark`) · go to PC
  (Ctrl+A, unpins the disasm so it follows PC). Numbered bookmark slots 0-9: set
  Ctrl+Shift+digit / jump Ctrl+digit.
- **Execution profiler** — logging mode / break mode / stop (checked radio) / clear
  buffer / "N addresses seen" drives a **golden-safe core per-PC instruction tally**:
  an `Option<BTreeMap<u16,u64>>` on `Interconnect` updated by `Bus::profile_pc` at
  instruction-execute (inert/`None` on every golden path), surfaced via
  `GameBoy::set_profiling`/`clear_profile`/`profile_count`/`profile_seen`/`set_profile_break`,
  per-line counts in the disasm gutter. Break mode halts the free run on each
  address's first execution (`take_prof_break_hit`).

## Disassembler

Core `debug::decode_with(bytes, pc, Syntax)` renders **RGBDS by default** (`$`-hex,
`[mem]`, `ld [hli]/[hld],a`, `ldh [$ffNN],a`, `db $xx`) or bgb/no$gmb. `decode()`
stays a `Bgb` wrapper so the bgb ground-truth + gbtr fingerprint stay byte-identical
(decode is debug-only; `Insn.target`/`branch_target` likewise). The Options→Debug
**RGBDS syntax** checkbox (`Settings.rgbds_disasm`/`DisasmFmt.rgbds`, default on)
flips it live.

## .sym symbols

`symbols::SymbolTable` (tolerant `BB:AAAA name` parser): `debugger::annotate_symbols`
inserts `name:` label rows (with a **blank spacer row above** each, skipped at the top
of the pane) + substitutes the operand target hex (`replace_last`, via
`DisasmRow.target`/`is_label`). The **memory pane** appends a row's symbol name when
its base is an exact match (`memory_rows` takes `&SymbolTable`; appended text keeps the
16-byte row layout so the click hit-test is untouched). Go-to accepts a symbol name
(resolve-then-hex; also `$`/`0x`-prefixed hex); the bp/wp/freeze manager rows append
the name. **Auto-load:** on ROM load a sidecar `foo.sym` beside `foo.gb` loads
automatically (`app_path::sym_sidecar`, `exists()`-gated silent no-op), applied to both
the disassembler and the memory viewer via the existing `set_symbols` fan-out. Also
loaded manually via Debug→"Load symbols..." (`PathPurpose::SymbolFile`).

## Freeze

App-owned `dbg::FreezeList` (`addr → value`, beside breakpoints/watchpoints) re-applied
after **every emulated frame** in `run_one_frame` via the golden-safe `debug_write`.
Empty by default → zero writes → **byte-identical** golden path (the inert-when-empty
law shared with bp/wp). "Freeze value" (memory-pane menu) locks the cursor byte to its
current value (`DebugAction::ToggleFreeze`, read live in `Debugger::apply`); the
**Freezes** manager (Ctrl+K / Debug menu) lists + clears entries via the shared
`address_list_menu` (generalized to a `fn(u16) -> DebugAction` clear action). No cheat
engine — a small bespoke list, not a Game Genie decoder.

## Standalone memory viewer

`ToolWindow::MemoryViewer`/`WinState::Memory`, opt-in via Options→Debug "memory
viewer in own window" (reconciled in `about_to_wait`; also the Window menu). Hex dump
+ a nearest-symbol status bar (`SymbolTable::nearest_before`). A **byte-edit cursor**
(`MemoryView.cursor`) that arrows move (auto-scrolling via `ensure_cursor_visible`); hex
digits type over it in place (`edit_hex_digit`, 2nd nibble commits via `debug_write`,
`edit_hi` holds the pending high nibble); the cursor byte is highlighted (blue while
typing). **Ctrl+G** opens a goto dialog (`MemoryView.goto`, `apply_goto` = symbol/hex).
Keys routed in the mem-window branch of `app_handler` (`mem_dialog_active`/
`feed_mem_dialog`/`mem_edit_digit`/`mem_cancel_edit`/`mem_window_key`).
Follow-ups: mouse click-to-place-cursor; freeze trigger from this window (currently
only the integrated pane + Ctrl+K manager).

## UX

- **Key-repeat guard** (`input::accept_key` + `App.held_keys`) — winit's `repeat` is
  unreliable on Wayland, so held F7/F3/F8 step once. Do rely on `accept_key`, not the
  winit flag.
- **Double-click a disasm line** toggles a breakpoint (`on_double_click` + `ToolView`
  400ms/3px timing).
- The integrated memory pane scrolls (wheel/arrows/PageUp-Down, `DebuggerState::scroll_memory`).
