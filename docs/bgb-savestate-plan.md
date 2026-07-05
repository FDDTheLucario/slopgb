# bgb on-disk save states (/tdd-test-plan)

Wire the greyed Save state… / Load state… (debugger File Ctrl+W/Ctrl+L; main-menu
State → Load state…). Manual byte (de)serialization of the whole machine — **no serde
dep** (core std-only), **no unsafe** (`forbid(unsafe_code)`). In-memory Quick Save/Load
(`GameBoy: Clone`) already works; this adds the on-disk format.

**Golden-safe**: `save_state` is `&self` (read-only), `load_state` is UI-only — never
on a golden/test path, so adding the methods leaves `gbtr golden_fingerprint`
byte-identical (verify with the parallel boot.rs stashed).

**ROM-keyed**: the header stores the cart title + header & global checksums; `load_state`
rejects a state from a different ROM (`RomMismatch`). ROM bytes are NOT serialized — load
restores volatile state into a machine already built from the same ROM.

**Oracle (the safety net for ~220 fields)**: save a machine after running a real test ROM,
build a fresh same-ROM machine, `load_state`, then run BOTH forward N frames and assert
byte-identical `frame()`/`cycles()`/regs/full memory. Any missed field diverges loudly.
Save at several points (quiet, mid-frame, post-DMA) to exercise the sub-dot pipeline.

```xml
<plan goal="On-disk save states: byte-exact whole-machine (de)serialization, golden-safe">
  <task id="1" model="sonnet" deps="none">
    <do>New slopgb-core `state` module: Writer (Vec&lt;u8&gt; + push u8/u16/u32/u64/bool/&amp;[u8] little-endian), Reader (&amp;[u8]+cursor, take_* -&gt; Result&lt;_,StateError&gt;), StateError {Truncated,BadMagic,BadVersion,RomMismatch}.</do>
    <test>state_tests: Writer→Reader round-trips each scalar + a byte slice; Reader past end → Err(Truncated); mixed-width sequence reads back in order.</test>
    <done>Primitives round-trip; truncation is an error not a panic.</done>
  </task>
  <task id="2" model="haiku" deps="1">
    <do>write_state/read_state for the small leaf structs: Cpu (regs+ime+ime_pending+halted+stopped+locked+debug), Timer, Serial, Joypad.</do>
    <test>core: a Cpu/Timer/Serial/Joypad mutated to non-default, write→read into a fresh one, assert field-equality (via existing accessors / a cfg(test) eq).</test>
    <done>Each leaf struct round-trips independently.</done>
    <why>Few flat fields, mechanical.</why>
  </task>
  <task id="3" model="sonnet" deps="1">
    <do>write_state/read_state for Cartridge + Mapper + Rtc: serialize cart RAM, MBC bank/mode/enable registers, RTC latched+live regs — NOT the ROM bytes.</do>
    <test>core: an MBC1/MBC3+RAM+RTC cart, write some RAM + switch banks + tick RTC, save, build fresh cart from the same ROM, load, assert RAM + banking + RTC equal.</test>
    <done>Cartridge volatile state round-trips; ROM bytes untouched.</done>
  </task>
  <task id="4" model="opus" deps="1">
    <do>write_state/read_state for Apu (mod + Pulse×2/Wave/Noise/Envelope/LengthCounter/frame-sequencer/sample state) — every field incl. phase/timer/duty-step/LFSR/wave-RAM.</do>
    <test>core: run a ROM that starts audio, save mid-tone, load into fresh APU, drain N raw samples from both, assert identical sample stream.</test>
    <done>APU round-trips to a byte-identical sample stream.</done>
    <why>Channel phase/envelope/LFSR/frame-sequencer state is delicate; a missed field desyncs audio silently.</why>
  </task>
  <task id="5" model="opus" deps="1">
    <do>write_state/read_state for Ppu (~65 fields: LCDC/STAT/regs, LY/LX/mode/dot counters, OAM, both VRAM banks, CGB palettes, the sub-dot fetcher/FIFO/PipeRegs/StagedWrite/Render pipeline, window line counter, OAM-DMA-active). Externalize to ppu/state.rs if mod.rs would pass 1000.</do>
    <test>core: run a ROM into mid-mode-3 rendering, save, load into a fresh PPU, run both 3 frames, assert byte-identical frame() + LY/STAT each step.</test>
    <done>PPU (incl. sub-dot pipeline) round-trips byte-identically across frames.</done>
    <why>The sub-dot pixel pipeline + fetcher/FIFO + staged writes are the most state-dense and timing-critical part of the machine.</why>
  </task>
  <task id="6" model="sonnet" deps="2,3,4,5">
    <do>write_state/read_state for Interconnect (~55 fields: wram+hram+ie+intf+cycles+cgb_mode+double_speed+dot_phase, OAM-DMA OamDmaStart/DmaConflict in-flight, HDMA state, vram-dma, if_late/if_stat_late/ack_squash, serial/timer wiring) delegating to the sub-structs; skip the debugger-only fields (watch/prof/exc — reset inert on load). Externalize to interconnect/state.rs (interconnect.rs is at 995).</do>
    <test>core: covered by the full-machine oracle in task 7 (interconnect has no standalone observable surface).</test>
    <done>Interconnect serializes every non-debugger field + delegates to sub-structs.</done>
  </task>
  <task id="7" model="opus" deps="6">
    <do>GameBoy::save_state(&amp;self)->Vec&lt;u8&gt; (magic+version+ROM-key header then cpu+interconnect) and load_state(&amp;mut self,&amp;[u8])->Result&lt;(),StateError&gt; (validate header+ROM key, then restore). pub const STATE_MAGIC/VERSION.</do>
    <test>core (the master oracle): for an MBC1 graphical ROM + an audio ROM, run K frames, save; fresh same-ROM machine, load_state, run BOTH 600 frames, assert byte-identical frame()+cycles()+cpu_regs()+full debug_read(0..=0xFFFF). Save at 3 different K (quiet / mid-frame / just-post-vblank). load_state of a different ROM's state → Err(RomMismatch); truncated/bad-magic/bad-version → their Err.</test>
    <done>Whole machine round-trips byte-identically; header + ROM-key + error paths enforced.</done>
    <why>This is the completeness gate; it must catch any field missed in tasks 2-6 across several save points.</why>
  </task>
  <task id="8" model="opus" deps="7">
    <do>Golden-safety gate: stash boot.rs, run gbtr golden_fingerprint (byte-identical) + mooneye smoke, pop. Confirm save_state/load_state are never reached on a golden path.</do>
    <done>Golden fingerprint byte-identical with the serializer present.</done>
    <why>Load-bearing core invariant; verify empirically.</why>
  </task>
  <task id="9" model="sonnet" deps="7">
    <do>Frontend: Session::save_state_to(path)/load_state_from(path) (fs + GameBoy::save_state/load_state, RomMismatch/BadVersion logged + machine intact). New Action::DbgSaveState/DbgLoadState + MainLoadState; reuse the App.path_dialog InputDialog (a DialogKind/PathPurpose so accept routes to save vs load vs rom).</do>
    <test>frontend: a save→load round-trip through a temp file restores the machine (over a real GameBoy); a bad/mismatched path leaves the session intact + returns an error.</test>
    <done>Save/Load state files round-trip via the path modal; bad paths are non-fatal.</done>
  </task>
  <task id="10" model="sonnet" deps="9">
    <do>Un-grey + wire the menu rows: debugger File → Save state… (Ctrl+W) / Load state… (Ctrl+L); main-menu State → Load state… (Quick Save/Load already live; Select/Load-recovery stay greyed). Keymap Ctrl+W/Ctrl+L (debugger focus).</do>
    <test>frontend: file_menu/state-submenu wiring tests — Save state/Load state are live (not disabled) and route to their Action; Ctrl+W/Ctrl+L map in debugger focus.</test>
    <done>The captured save/load-state rows are black + functional; Select/recovery stay greyed.</done>
  </task>
  <task id="11" model="haiku" deps="8,10">
    <do>Gates: core + frontend tests green; clippy --all-targets -D + fmt + cargo doc -D clean; no file &gt;1000; then /rust-diff-review + independent review, fix findings.</do>
    <test>cargo test both crates; clippy; fmt --check; wc -l touched files.</test>
    <done>All gates green; review clean; ready to commit.</done>
    <why>Verification sweep.</why>
  </task>
</plan>
```

Summary: 11 tasks (1 haiku-leaf + 1 haiku-gate, 4 sonnet, 4 opus, +1). Critical path: 1 → {2,3,4,5} → 6 → 7 → 8 (golden) and 7 → 9 → 10 → 11.
