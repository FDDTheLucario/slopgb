# bgb Options → Exceptions: functional break conditions (/tdd-test-plan)

Make the Options → Exceptions tab's "break on X" checkboxes **live**: the debugger
free-run halts when the armed condition occurs (bgb's exception breaks). Mirrors the
golden-safe profiler (MB5) / watchpoint (RM8) pattern — an `exc_mask: u16` on
`Interconnect` (0 ⇒ inert ⇒ fingerprint byte-identical) + `exc_hit: Option<u16>`
consumed by `run_frame_until_breakpoint`.

Conditions implemented (golden-safe, well-defined, testable):
1. **break on ld b,b (40h)** — opcode `0x40` at execute.
2. **break on invalid opcode** — undefined opcodes `0xD3|0xDB|0xDD|0xE3|0xE4|0xEB|0xEC|0xED|0xF4|0xFC|0xFD` (the exact set `cpu/execute.rs` hard-locks on).
3. **break on ram echo (E000-FDFF) access** — CPU read or write in `0xE000..=0xFDFF`.
4. **break on disabling LCD outside vblank** — `FF40` write clearing bit 7 while LCD enabled and PPU mode ≠ 1 (vblank).

Other tab rows (OAM DMA bad access, 16-bit inc/dec FE00-FEFF, SGB transfer, MBC,
inaccessible VRAM, halt+ints bug, uninitialized RAM) stay **faithfully inert** (no
clean golden-safe detector / no backend) — rendered like bgb, do nothing, per the
project's established faithful-but-inert convention.

```xml
<plan goal="Options Exceptions tab break conditions become functional (golden-safe)">
  <task id="1" model="sonnet" deps="none">
    <do>Add exc_mask:u16 + exc_hit:Option&lt;u16&gt; to Interconnect (default 0/None); GameBoy::set_exceptions(mask)/exceptions()-&gt;u16 delegating to bus; bus take_exc_hit(); pub const EXC_* bit flags in lib.rs.</do>
    <test>core: a fresh GameBoy has exceptions()==0; set_exceptions(EXC_LD_B_B) round-trips; take_exc_hit() is None initially.</test>
    <done>Mask plumbed, defaults inert, round-trips; cargo build -p slopgb-core green.</done>
  </task>
  <task id="2" model="sonnet" deps="1">
    <do>New Bus::check_exec(&amp;mut self,_pc:u16,_opcode:u8){} default-no-op; Interconnect override sets exc_hit on EXC_LD_B_B (op==0x40) / EXC_INVALID_OPCODE (the 11 undefined ops), early-return when exc_mask==0. Call bus.check_exec(pc,opcode) after each of the 3 profile_pc sites in cpu/execute.rs. run_frame_until_breakpoint consumes take_exc_hit().</do>
    <test>core: ROM with `ld b,b`, arm EXC_LD_B_B, run_frame_until_breakpoint halts (Some); unarmed → None. ROM with `0xDD`, arm EXC_INVALID_OPCODE → halts; ld b,b alone does NOT trip the invalid-op mask.</test>
    <done>ld b,b + invalid-opcode breaks halt only when their bit is armed.</done>
  </task>
  <task id="3" model="sonnet" deps="1">
    <do>Inline EXC_ECHO_RAM check in Interconnect Bus read/read_inc/write (addr in 0xE000..=0xFDFF) — guarded by exc_mask==0 early-return, beside check_watch.</do>
    <test>core: ROM writing/reading echo RAM (E000-FDFF), arm EXC_ECHO_RAM → run halts; unarmed → no halt; a C000 (work RAM) access does NOT trip it.</test>
    <done>Echo-RAM access halts only when armed; non-echo access never trips.</done>
  </task>
  <task id="4" model="sonnet" deps="1">
    <do>Inline EXC_LCD_OFF_VBLANK check in Interconnect Bus write: addr==0xFF40 &amp;&amp; value bit7==0 &amp;&amp; ppu.lcd_enabled() &amp;&amp; ppu.mode_bits()!=1 sets exc_hit. Guarded by exc_mask==0.</do>
    <test>core: enable LCD, step to a non-vblank scanline, arm EXC_LCD_OFF_VBLANK, write FF40=0x00 → halts; same write during vblank (mode 1) → no halt; writing FF40 while LCD already off → no halt.</test>
    <done>LCD-off-outside-vblank halts only outside vblank when armed.</done>
  </task>
  <task id="5" model="opus" deps="2,3,4">
    <do>Golden-safety gate: stash the parallel boot.rs, run `cargo test -p slopgb-core --test gbtr golden_fingerprint`, confirm byte-identical; run mooneye smoke; pop stash.</do>
    <done>gbtr golden fingerprint byte-identical with the exception code present (mask unset on every golden path).</done>
    <why>Golden-safety is the load-bearing core invariant; must be verified empirically, not assumed.</why>
  </task>
  <task id="6" model="haiku" deps="1">
    <do>Frontend Settings: add break_ld_b_b/break_invalid_op/break_echo_ram/break_lcd_off_vblank bools (invalid_op default true, rest false); Settings::exception_mask()-&gt;u16 from the core EXC_* consts.</do>
    <test>frontend: Settings::default().break_invalid_op is true, others false; exception_mask() sets exactly EXC_INVALID_OPCODE by default; all four set → all four bits.</test>
    <done>Settings carry the 4 flags; mask builder maps them to core bits.</done>
    <why>Mechanical struct fields + a bit-OR mapper.</why>
  </task>
  <task id="7" model="sonnet" deps="6">
    <do>Add 4 Field variants (BreakLdBB/BreakInvalidOp/BreakEchoRam/BreakLcdOffVblank); rewrite exceptions() so those 4 rows are Ctrl::live with checked-from-settings, the rest inert/grey; wire toggles in on_content_click + reset_defaults for the Exceptions tab.</do>
    <test>frontend: exceptions() hit-test on each of the 4 live rows returns its Field; toggling flips the settings bool; reset_defaults restores invalid-op true + others false; an inert row resolves to field None.</test>
    <done>The 4 checkboxes are live + reset to bgb defaults; other rows stay inert.</done>
  </task>
  <task id="8" model="sonnet" deps="2,3,4,6,7">
    <do>App::apply_exceptions() pushes Settings::exception_mask() to gb; call it at App::new (after apply_palette), in apply_settings, and after the ROM-load apply_palette; dbg_armed() also arms when gb.exceptions()!=0.</do>
    <test>frontend: over a real machine, apply_exceptions() makes gb.exceptions() match the settings mask; an App with the debugger open + invalid-op default reports dbg_armed-equivalent (exceptions()!=0).</test>
    <done>Mask reaches the machine at startup/load/apply; debugger arms on exceptions.</done>
  </task>
  <task id="9" model="haiku" deps="5,7,8">
    <do>Gates: 4 new core tests + frontend tests green; clippy --all-targets -D + fmt clean; no file &gt;1000 lines; cargo doc clean.</do>
    <test>cargo test -p slopgb-core --lib + -p slopgb; clippy; fmt --check; wc -l on touched files.</test>
    <done>All gates green; ready for /rust-diff-review.</done>
    <why>Pure verification sweep.</why>
  </task>
</plan>
```

Summary: 9 tasks (2 haiku, 5 sonnet, 1 opus + 1 haiku gate). Critical path: 1 → 2/3/4 → 5 (golden) and 1 → 6 → 7 → 8 → 9.
