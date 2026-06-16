# RM10 Copy data / Copy code (/tdd-test-plan)

Un-grey bgb's debugger disasm/memory right-click **Copy data** / **Copy code**.
The prior sessions called this "permanently blocked" because a clipboard write
seemed to need a forbidden Cargo dep (`arboard`). **That was wrong** — shelling
out to a system clipboard tool via `std::process::Command` is **std-only** (no
Cargo dep) and functionally 1:1 with bgb's copy. This un-blocks RM10.

Frontend-only, no core change → trivially golden-safe.

```xml
<plan goal="RM10 Copy data/code via a dep-free clipboard shell-out">
  <task id="1" model="sonnet" deps="none">
    <do>New frontend `clipboard` module: pure `clipboard_candidates()->[(prog,&[args]);3]` (wl-copy / xclip -selection clipboard / xsel -ib) + `copy(text)->bool` that spawns each in order, pipes text to stdin, returns true on the first success.</do>
    <test>clipboard_tests: clipboard_candidates() lists wl-copy, xclip, xsel in that order with the right args; copy("") on a host with a tool returns a bool without panicking (the pure candidate list is the real assertion — the spawn isn't unit-asserted).</test>
    <done>The candidate table is correct; copy() never panics.</done>
  </task>
  <task id="2" model="sonnet" deps="none">
    <do>toolwin `debugger_copy_text(gb,addr,code:bool)->String`: code => debugger::disasm_rows from addr (16 rows, the pane's data_hints + DisasmFmt) joined by '\n'; data => 16 bytes from addr via gb.debug_read as "XX XX ..." (pane lowercase-hex honored).</do>
    <test>toolwin/debugger test over a real GameBoy: copy-data at an addr yields 16 space-separated hex bytes matching debug_read; copy-code yields ≥1 line whose first token is the addr.</test>
    <done>Copy text is generated correctly for both code and data.</done>
  </task>
  <task id="3" model="haiku" deps="none">
    <do>Add Action::DbgCopyData(u16) + DbgCopyCode(u16) (menu-only); in windows/debugger.rs disasm_entries replace the two disabled("Copy ...") with MenuChoice::Command(Action::DbgCopyData(addr)) / Command(Action::DbgCopyCode(addr)) for both panes.</do>
    <test>debugger_tests: the disasm right-click menu's "Copy data"/"Copy code" rows are enabled and route to Command(DbgCopyData/Code(addr)).</test>
    <done>The Copy rows are black + carry their addr-bearing action.</done>
    <why>Mechanical enum + menu wiring.</why>
  </task>
  <task id="4" model="haiku" deps="1,2,3">
    <do>run_action: DbgCopyData(addr)/DbgCopyCode(addr) build the text via debugger_copy_text + clipboard::copy; log "no clipboard tool" (non-fatal) when copy returns false.</do>
    <test>frontend: run_action(DbgCopyData(addr)) doesn't panic over a real App (clipboard may be absent in CI); the menu→Command dispatch is the assertion (covered by task 3 + the shared run_action path).</test>
    <done>Copy items invoke the clipboard end-to-end, non-fatal if no tool.</done>
    <why>Thin wiring on the proven Command→run_action path.</why>
  </task>
  <task id="5" model="haiku" deps="4">
    <do>Gates + docs: tests + clippy --all-targets -D + fmt + doc clean; no file >1000; /rust-diff-review (self + independent), fix findings; un-grey note in CLAUDE.md (RM10 no longer blocked).</do>
    <test>cargo test -p slopgb; clippy; fmt --check.</test>
    <done>Green + reviewed; CLAUDE.md updated.</done>
    <why>Verification sweep.</why>
  </task>
</plan>
```

Summary: 5 tasks (3 haiku, 2 sonnet). Critical path: {1,2,3} → 4 → 5.
