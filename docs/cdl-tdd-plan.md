# CDL (Code/Data Logging) — TDD task plan

Branch: continue on `ui-qa-features` (or a `cdl` branch). Ponytail v1: 64KB
CPU-address-space flag store (not physical ROM offset), hand-rolled RLE save,
coloring in the standalone Memory Viewer only. **Golden-safe:** `Option`-gated,
`None` default, every hook a no-op when off, excluded from save-state — verify
`cargo test -p slopgb-core --test gbtr` (golden_fingerprint). Flags: R=1 W=2 X=4.

```xml
<plan goal="FCEUX-style CDL: per-byte r/w/x flags, colored in the memory viewer, RLE save/load — golden-safe">

  <task id="1" model="sonnet" deps="none">
    <do>Core CDL store + API on Interconnect/GameBoy: cdl: Option&lt;Box&lt;[u8;65536]&gt;&gt; (None default, interconnect.rs:453/559), set_cdl(on) alloc/drop, cdl_flag(addr)-&gt;u8 (&amp;self, 0 when None), cdl_clear() zeroes; GameBoy forwarding (lib.rs:373). Exclude cdl from interconnect/state.rs serialization.</do>
    <test>lib_tests.rs: cdl_flag defaults 0; set_cdl(true) then cdl_flag still 0 (nothing logged yet); cdl_clear no-op when None; a save_state/load_state round-trip leaves cdl untouched (not serialized).</test>
    <done>Store toggles + accessor green; save-state ignores cdl.</done>
  </task>

  <task id="2" model="opus" deps="1">
    <do>R/W hook: in Interconnect::check_access (debug.rs:16) add `if let Some(b)=&amp;mut self.cdl { b[addr as usize] |= if is_write {2} else {1}; }` — the CPU read/write choke already wired into Bus::read/write/read_inc, already early-outs unarmed.</do>
    <test>Core test: set_cdl(true), step a tiny ROM that reads addr Ra and writes addr Wa; assert cdl_flag(Ra)&amp;1 != 0 and cdl_flag(Wa)&amp;2 != 0; with set_cdl(false) both stay 0.</test>
    <done>R/W bits recorded only when armed; no-op when None.</done>
    <why>Sits on the hot CPU read/write path; must be a provable no-op when None or it perturbs timing/golden.</why>
  </task>

  <task id="3" model="sonnet" deps="1">
    <do>X hook: in Interconnect::profile_pc (interconnect.rs:827) add `if let Some(b)=&amp;mut self.cdl { b[pc as usize] |= 4; }` (mirrors task 2). Note: only the opcode byte is X; operand bytes get R via the fetch read path (acceptable over-approx).</do>
    <test>Core test: set_cdl(true), step one instruction at PC P; assert cdl_flag(P)&amp;4 != 0; set_cdl(false) → stays 0.</test>
    <done>X bit set at the executed instruction address.</done>
  </task>

  <task id="4" model="sonnet" deps="2,3">
    <do>Belt-and-braces golden-safety test: run N stepped frames with cdl off, hash the framebuffer; repeat with set_cdl(true); assert the two hashes are identical (recording never perturbs emulation).</do>
    <test>Core test asserting frame-hash(cdl on) == frame-hash(cdl off) over the same ROM+steps; plus the existing gbtr golden_fingerprint still green (default-off path).</test>
    <done>CDL-on frame output byte-identical to CDL-off.</done>
  </task>

  <task id="5" model="sonnet" deps="1">
    <do>Bulk accessors for save/load: cdl_flags()-&gt;Option&lt;&amp;[u8]&gt; (&amp;self, the 64KB buffer or None) and load_cdl(&amp;[u8;65536]) (enable + copy in) on Interconnect/GameBoy.</do>
    <test>Core test: load_cdl(&amp;fixture) then cdl_flag(a)==fixture[a] for sampled addrs; cdl_flags() returns Some(slice) whose bytes equal the fixture; None before any set_cdl.</test>
    <done>Buffer round-trips through load_cdl/cdl_flags.</done>
  </task>

  <task id="6" model="haiku" deps="none">
    <do>Pure cdl_color(flag: u8) -&gt; Option&lt;u32&gt; in the frontend: None for 0 (unvisited), distinct XRGB tints for R/W/X and combos (code=red-ish, read=blue, write=green, blends).</do>
    <test>Unit: cdl_color(0)==None; cdl_color(1)/(2)/(4) are three distinct Some; a combo (e.g. 5=R|X) is Some and differs from its parts.</test>
    <done>Color map returns None for 0 and distinct colors per combo.</done>
    <why>Pure lookup fn, trivially testable.</why>
  </task>

  <task id="7" model="haiku" deps="none">
    <do>Pure RLE codec (std-only) in the frontend: rle_encode(&amp;[u8])-&gt;Vec&lt;u8&gt; + rle_decode(&amp;[u8])-&gt;Vec&lt;u8&gt; (run-length, tuned for mostly-zero flag arrays).</do>
    <test>Unit: rle_decode(rle_encode(x))==x for an all-zero 65536 buffer, a sparse buffer (a few set bytes), and a dense/random buffer; assert the all-zero case encodes far smaller than 65536.</test>
    <done>Round-trip identity for all three; zero buffer compresses hard.</done>
    <why>Pure codec with an assert-based round-trip; no framework.</why>
  </task>

  <task id="8" model="opus" deps="1,6">
    <do>Color the standalone Memory Viewer: in render_memory_window, BEFORE render_memory draws text, fill each visible byte's cell bg with cdl_color(gb.cdl_flag(addr)) when Some. Reuse the edit-cursor byte-column math (char = 10 + 3*col + (col&gt;=8); GLYPH_W). Off (all flags 0) = no tint.</do>
    <test>Headless render test: build a machine, load_cdl a fixture with known flags at a couple addrs, render the mem window to a Canvas, assert the cell bg pixel at x=f(col),y=f(row) equals cdl_color(flag); an unflagged cell keeps theme.bg.</test>
    <done>Flagged bytes show their tint; text stays readable; unflagged unchanged.</done>
    <why>Net-new per-byte background pass over the whole dump; column math must match hex_row exactly and not clobber the glyphs.</why>
  </task>

  <task id="9" model="sonnet" deps="1">
    <do>CDL on/off + clear UI: a Debug-menu toggle + Action (e.g. DbgToggleCdl / DbgClearCdl) calling gb.set_cdl(on)/gb.cdl_clear(). Default OFF.</do>
    <test>Action/dispatch test: firing the toggle enables then disables cdl (observe via cdl_flag being logged vs 0 after a step); clear zeroes a logged buffer. Menu item count test updated.</test>
    <done>Menu/hotkey toggles logging and clears the buffer.</done>
  </task>

  <task id="10" model="sonnet" deps="5,7">
    <do>Save/load wiring: PathPurpose::CdlSave/CdlLoad in the path modal (app_path.rs, mirror SymbolFile/SaveState) → RLE-encode cdl_flags() to a file / read+rle_decode+load_cdl; Debug-menu Save CDL/Load CDL items.</do>
    <test>Round-trip test: encode a fixture flag set → bytes → decode → load_cdl → cdl_flag matches the fixture (the file step exercised via a tempdir or the pure codec seam).</test>
    <done>CDL saves to a compressed file and reloads to the same flags.</done>
  </task>

</plan>
```

**10 tasks: 2 haiku, 6 sonnet, 2 opus.** Critical path:
`1 (core store) → 2 (r/w hook) → 4 (golden verify)`; task 1 gates everything,
save/load chains `1→5→10` (+7). Independent leaves: 6 (color), 7 (RLE).
