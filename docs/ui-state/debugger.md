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

## Pane scrolling (mouse wheel + draggable scrollbars)

The wheel scrolls whichever debugger pane the cursor is over (`toolwin::on_wheel`,
3 notches/step): **memory** (`scroll_memory`, 16 bytes/row), **disasm**
(`scroll_disasm`, per-instruction — forward decodes one insn, backward back-scans
the 1..=3 preceding bytes for the decode that lands exactly on the base; detaches
follow like a Go-to by setting `pinned`), and **stack** (`scroll_stack`, words below
SP, clamped `[0, 0x800]`; the SP highlight shows only at offset 0). `DebuggerState`
holds `disasm_base` (authoritative disasm top) + `stack_off`.

**Draggable scrollbars** on each scrollable pane (disasm/memory/stack + the
standalone memory viewer). One widget (`ui::widgets`: `vscroll_track` /
`vscrollbar` / `vscroll_frac`, `SCROLLBAR_W = 8`) draws a dim track + bright thumb
on the pane's right-edge strip (`scroll_content` shrinks the content so text
doesn't run under it). Each pane exposes a `(frac, vis)` model + a `set_*_scroll`
(`DebuggerState::disasm_scroll`/`mem_scroll`/`stack_scroll`, `MemoryView::scroll_frac`)
over its range (64 KiB for disasm/memory — `frac` = base/`u16::MAX`; `[0,0x800]`
for stack). Drag: a left-press on a track (`toolwin::scrollbar_at`) starts a drag
(`scroll_drag: Option<(WindowId, ScrollBar)>`), `on_cursor_moved` re-applies the
frac, left-release (`on_mouse_up`) ends it; a disasm drag pins (stops PC-follow).
The press is consumed before pane-click routing, so the strip never selects a row.

**Disasm follows PC in place while running:** each redraw calls
`DebuggerState::disasm_follow`, which re-bases to PC only when unpinned AND PC has
scrolled outside the visible decoded window — so a free run keeps the listing fixed
until PC leaves the pane, then re-pages. `pinned` (stay-on-bank / Go-to / a manual
scroll) freezes it entirely.

**Tracing centers PC:** every trace action (F7 step / F3 over / F8 out / F6 jump /
F4 run-to-cursor) and "go to PC" (Ctrl+A) call `Tools::center_debugger_on_pc` →
`DebuggerState::center_disasm_on_pc`, which unpins and re-bases so PC lands on the
middle row (walks back `visible/2` instructions via `prev_disasm_addr`). So you can
scroll away (which pins), and the next F7 snaps the traced instruction back to
center. A breakpoint toggle (F2) does **not** recenter — it acts on the cursor
without moving the view.

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

## CDL (code/data logging)

FCEUX-style per-byte access log. Core: `cdl: Option<Box<[u8;65536]>>` on
`Interconnect` (**golden-safe**, `None`-gated like the profiler — R=1/W=2/X=4 set
by `check_access` (r/w) + `profile_pc` (x), no-op when off, excluded from
save-state, never read back so it can't perturb a cycle;
`set_cdl`/`cdl_flag`/`cdl_flags`/`cdl_clear`/`load_cdl` on `GameBoy`). **Bank-aware:
keyed by physical offset**, not CPU address — the buffer is sized to the machine
(`ROM | VRAM | SRAM | WRAM | tail 0xFE00-0xFFFF`, `Interconnect::cdl_layout`), and
one shared `cdl_index(addr)` translates the live banking for *both* the mark hooks
and `cdl_flag` (the `rom_bank_for` no-divergence pattern). So 0x4000-0x7FFF maps to
the real ROM bank, and SRAM/WRAM/VRAM banking is per physical byte; an access to
disabled/absent SRAM (or an RTC register) maps to no byte and logs nothing.
Offsets come from `Cartridge::rom_offset`/`ram_offset` + `wram_index` +
`Ppu::vram_bank`. Operands get R via the fetch path (opcode-only X). Debug menu:
**CDL logging** (Ctrl+D toggle) / Clear CDL / Save CDL... / Load CDL.... The
**standalone Memory Viewer** tints each visited byte's cell background
(`cdl::cdl_color`: X=red, W=green, R=blue, combos blend), drawn before the dump
text so glyphs stay readable; off = no tint; the status bar names the tint's live
bank (`mem_bank_label`, e.g. `ROM05:4000`). Save/load use the path modal
(`PathPurpose::CdlSave/CdlLoad`) with a std-only RLE codec (`cdl::rle_encode/decode`,
all-zero → 6 bytes). `load_cdl` validates the buffer length against the machine's
layout and rejects a foreign `.cdl` (`#[must_use]` bool). Ponytail ceiling:
length-only guard — a same-size ROM/RAM config would still load; embed the cart
header checksum in the file if it bites. Follow-ups: integrated-pane coloring;
an arbitrary-bank browser (viewer shows the live-mapped bank only).

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
