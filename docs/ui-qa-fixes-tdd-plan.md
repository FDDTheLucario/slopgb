# UI QA fixes — TDD task plan

Branch `ui-qa-features`. Ponytail: laziest working path, reuse existing machinery,
shortest diff. Golden-safe law holds — the one core touch (F1 `debug_write`) is inert
until a live UI action calls it. **CDL is deferred** (separate follow-up pass).

Scope = 7 items (A–G). Hook points + test seams are from the codebase map; verify any
core touch with `cargo test -p slopgb-core --test gbtr` (golden_fingerprint).

```xml
<plan goal="7 bgb-debugger QA fixes: two-bank VRAM, auto-sym, sym-in-memview, disasm nit, memview goto, in-place byte edit, freeze">

  <task id="1" model="sonnet" deps="none">
    <do>Add pub fn debug_write(&mut self,addr,value) on GameBoy in lib.rs — one-line wrapper over existing bus.debug_write; doc-comment live-debugger-only (mirror debug_set_reg caveat).</do>
    <test>lib_tests.rs: debug_write(0xC000,0x42) then assert_eq!(debug_read(0xC000),0x42); mirror the debug_call round-trip test.</test>
    <done>Round-trip test green; gbtr golden_fingerprint unchanged (path stays uncalled).</done>
  </task>

  <task id="2" model="haiku" deps="none">
    <do>Blank spacer row above each name: label in annotate_symbols (disasm.rs:168): push DisasmRow{text:"",target:None,is_label:true} guarded by !out.is_empty().</do>
    <test>disasm_tests.rs:150 extend: assert a blank spacer precedes a mid-list label AND rows[0]==label for a top-of-pane symbol (the guard).</test>
    <done>annotate_symbols test green; render + target_at hit-test stay coherent (single hook).</done>
    <why>3-line insert into one pure Vec→Vec fn, no design judgment.</why>
  </task>

  <task id="3" model="haiku" deps="none">
    <do>Pure helper fn sym_sidecar(rom:&Path)->Option&lt;PathBuf&gt; returning Some iff rom.with_extension("sym").exists().</do>
    <test>app_path_tests.rs (tempdir): foo.gb+foo.sym→Some(foo.sym); foo.gb alone→None; extensionless rom→None.</test>
    <done>Three-case helper test green.</done>
    <why>Trivial stdlib one-liner, isolated for a clean unit test.</why>
  </task>

  <task id="4" model="sonnet" deps="3">
    <do>Wire auto-load: in load_dropped (main.rs, Ok(new) branch after rom_loaded=true) call load_symbols(&sym) when sym_sidecar returns Some; change load_symbols to pub(crate).</do>
    <test>app_path_tests.rs / integration: after load_dropped on a ROM beside a valid .sym, App.symbols is non-empty; ROM with no sidecar leaves symbols untouched (no eprintln).</test>
    <done>Auto-load fires only when sidecar exists; set_symbols already fans to both views.</done>
  </task>

  <task id="5" model="sonnet" deps="none">
    <do>Show sym names in the memory dump body: add syms:&SymbolTable param to memory_rows+render_memory (debugger.rs:168/179), append name_at(base) to the row string; pass &st.symbols at both call sites (windows.rs:128,195).</do>
    <test>Pure memory_rows(read,base,count,&syms) test: a symbol at a row base appears appended on that row; rows stay 1-per-16-bytes (target_at math untouched).</test>
    <done>memory_rows test green; both integrated pane and standalone viewer show names.</done>
  </task>

  <task id="6" model="haiku" deps="none">
    <do>Pure goto fn: parse a hex string → Option&lt;u16&gt; and set MemoryView.mem_base on Some; no-op on junk.</do>
    <test>windows_tests.rs: "C000"→mem_base==0xC000; "zzz"/empty→mem_base unchanged.</test>
    <done>parse+set test green.</done>
    <why>Isolated pure parse, mirrors existing apply_goto.</why>
  </task>

  <task id="7" model="sonnet" deps="6">
    <do>Wire Ctrl+G in the standalone MemoryViewer: give MemoryView Option&lt;InputDialog&gt;; on Ctrl+G (mem_window_key, toolwin.rs:456) open a goto dialog feeding the fn from task 6 via dialog_key_from.</do>
    <test>toolwin/windows test: Ctrl+G opens the dialog; accepting "8000" sets mem_base; Esc closes without change.</test>
    <done>Standalone window gets goto (integrated pane already had it).</done>
  </task>

  <task id="8" model="opus" deps="none">
    <do>Two-bank Tiles render on CGB: split l.content into left/right halves+gutter with per-column extents via fit_scale(half_w,h,128,192) (vram_geom two-column branch, windows.rs:220-267); render bank0 left, bank1 right; DMG keeps the single call.</do>
    <test>vram_tests.rs:114 pattern: distinct bytes in bank0 [..0x2000] and bank1 [0x2000..] → bank1 pixels land in the RIGHT rect, bank0 in the LEFT; extend layout partition test (line 412) for two columns.</test>
    <done>CGB shows both grids side-by-side non-overlapping; DMG unchanged.</done>
    <why>Layout/geometry-fiddly: two-column extents + grid-overlay/frame for two grids, easy to overrun content.w.</why>
  </task>

  <task id="9" model="sonnet" deps="8">
    <do>Fix tile_details hover (windows.rs:528-539): map lx to left/right half→bank and print the real bank in the address label ({bank}:{:04X}) instead of hardcoded 0:.</do>
    <test>Hover test: an lx in the right half yields bank 1 and a "1:" address label; left half yields "0:".</test>
    <done>Hover reports the correct bank+address for either grid.</done>
  </task>

  <task id="10" model="opus" deps="none">
    <do>Byte-column hit-test: extend target_at (debugger.rs:444-447) to resolve a memory click to (addr, nibble-col) using hex_row's fixed layout (GLYPH_W=7, label len + inter-group gap before byte 8), not just row base.</do>
    <test>Pure hit-test: given x pixel + row layout, assert the resolved byte index/addr against hex_row's known column offsets (col 0, col 7/8 gap boundary, col 15).</test>
    <done>A click maps to the exact byte+nibble; row-only math replaced.</done>
    <why>Inverting hex_row's variable-width column layout (inter-group gap) is the fiddliest bit; off-by-one lands on the wrong byte.</why>
  </task>

  <task id="11" model="opus" deps="10">
    <do>Cell renderer: add render_memory_cells drawing label + each byte at x=base+charcount*GLYPH_W with a highlight fill_rect behind the cursor byte (parallel to scroll_list, which can't color one byte).</do>
    <test>Cell-render test: with a cursor at (row,col), assert the highlight fill_rect lands at the expected x=f(col) derived from hex_row's fixed layout.</test>
    <done>Single-byte cursor highlight renders at the right column; row text still correct.</done>
    <why>Net-new per-byte draw path (shared infra a future CDL coloring reuses); column math must match task 10 exactly.</why>
  </task>

  <task id="12" model="opus" deps="1,10,11">
    <do>Edit state machine: view-state edit buffer (addr+partial nibble); hex keys shift-in nibbles, 2nd nibble commits via gb.debug_write + advances cursor; Esc cancels; arrows move cursor. Route via mem-window/debugger key paths.</do>
    <test>Pure edit-state fn over a fixture read/write closure: keys 'A','5' commit 0xA5 to the cursor addr and advance; Esc mid-edit leaves memory unchanged; arrow moves cursor without writing.</test>
    <done>Typing hex over a byte writes live memory; cancel/navigation behave.</done>
    <why>Two-nibble commit + cursor advance + cancel over live memory is a coupled state machine with several edge transitions.</why>
  </task>

  <task id="13" model="sonnet" deps="none">
    <do>App-owned FreezeList in dbg::Debugger: Vec/BTreeMap of (u16,u8) with toggle/list/remove/is_empty (copy the Watchpoints pattern; App-side, not per-window).</do>
    <test>dbg_tests.rs: toggle(addr,val) adds; toggle again removes; is_empty true by default; list returns entries (mirror breakpoint tests).</test>
    <done>FreezeList unit tests green; empty by default.</done>
  </task>

  <task id="14" model="sonnet" deps="1,13">
    <do>Per-frame re-apply in run_one_frame (app_pacing.rs:179): after the frame advances, for &(addr,val) in freeze { gb.debug_write(addr,val); }. Empty list = zero writes.</do>
    <test>Headless: run_one_frame with freeze=[(0xC000,0x42)] over a frame where the ROM writes 0xC000 → debug_read==0x42; empty freeze → gbtr golden_fingerprint unchanged.</test>
    <done>Frozen byte re-forced each frame; empty list byte-identical.</done>
  </task>

  <task id="15" model="sonnet" deps="13">
    <do>Freeze UI: "Freeze value" row in the mem-pane right-click menu (debugger.rs:503-545) snapshots (cursor, debug_read(cursor)) onto the list; reuse the existing address-list popup (app_run.rs:131-155) to list/unfreeze rows.</do>
    <test>Menu/action test: activating Freeze on a cursor addr pushes (addr,value); the popup lists it; selecting a row removes it. No new ToolWindow.</test>
    <done>Freeze reachable from the menu; frozen values listed/undoable in the popup.</done>
  </task>

</plan>
```

**15 tasks: 3 haiku, 8 sonnet, 4 opus.** Critical path (longest chain):
`10 (byte hit-test) → 11 (cell renderer) → 12 (edit state machine)`; F1 (`debug_write`,
task 1) also gates 12 and 14. Independent quick wins parallel from the start: 2 (nit),
3→4 (auto-sym), 5 (sym-in-memview), 6→7 (goto), 8→9 (two-bank VRAM), 13 (FreezeList).
