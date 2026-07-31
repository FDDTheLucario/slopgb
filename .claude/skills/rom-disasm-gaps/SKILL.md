---
name: rom-disasm-gaps
description: Disassemble a failing GB test ROM and derive what its reference ACTUALLY constrains, so a law is fitted only to rows that genuinely pin it. Use before or instead of sweeping emulator knobs whenever a sweep is not converging, a candidate arm scores "+N/-M" or needs a suspicious compound predicate (`ly >= 1 || fine != 0`), two ROMs look "welded" with identical state, or a fix would be justified by a single row. Also use to answer "is this even a timing bug?" — the ROM may never write the register you are timing. Triggers: "disassemble the ROM", "what does this test actually check", "is this a coverage gap", "why do these two rows disagree", "/rom-disasm-gaps", "analyse the kernel", or any moment you catch yourself sweeping a scalar for the third time.
---

# rom-disasm-gaps — what does this test actually constrain?

Sweeping asks "which knob value scores best". This asks "which answers is the ROM
even capable of rejecting". Most failed law-hunts in this tree were fits to rows
that constrained nothing, or to a difference that lives in the ROM's control flow
rather than in the emulator.

## Steps

1. **Diff the ladder siblings.** `cmp -l a.gbc b.gbc | awk '{printf "%d(0x%x) %o %o\n",$1,$1-1,$2,$3}'`.
   A ladder is almost always a single inserted `00` (one whole M-cycle) shifting a
   run. Note *which* write moves and in which direction — rungs often step two
   writes in **opposite** directions.
2. **Disassemble the measurement kernel**, not just the failing instruction:
   `cargo run -p slopgb-core --example disasm_region -- <rom> <start_hex> <end_hex>`
   (pipe through `grep -v "  00        nop"`). Find three things: the observable
   read (`ldh a,(FFxx)` / the rendered tile), the writes that set the condition,
   and **how the kernel is entered**.
3. **Read the vectors** (`disasm_region <rom> 40 60`). Entry determines phase. If
   the kernel is IRQ-driven, measure the dispatch dot per line (probe `ack`/
   `dispatch_interrupt`, print `(ly, dot)`). **A line whose dispatch dot differs
   from the others is a CPU instruction-phase artefact of the ROM's own control
   flow, not a render law** — do not build a render predicate on it.
4. **Derive ground truth from the reference.** For pixel ROMs, copy
   [`reference/mapdump.rs`](reference/mapdump.rs) into
   `crates/slopgb-core/examples/`, build it, then run
   [`tools/colreq.py`](tools/colreq.py):
   ```sh
   cp .claude/skills/rom-disasm-gaps/reference/mapdump.rs crates/slopgb-core/examples/
   CARGO_TARGET_DIR=target/probe cargo build --release -p slopgb-core --example mapdump
   python3 .claude/skills/rom-disasm-gaps/tools/colreq.py \
     target/probe/release/examples/mapdump <rom> <dmg|cgb> <ly>
   ```
   It prints, per on-screen tile position, the **set of map columns the reference
   accepts** — already classified. Delete the example again when done.
5. **Classify every row before fitting anything** (table below). Only
   DISCRIMINATING rows may justify a law.
6. **Sanity-check the premise**: does the ROM write the register you are timing at
   all? Does it take the interrupt you assumed? Count the dispatches.

## Coverage classes

| Class | Signature | What it means |
|---|---|---|
| DISCRIMINATING | reference accepts a narrow, exact column set | pins the law — build on these only |
| DEGENERATE | accepts ALL columns | constrains nothing here; a "fix" aimed at it is unfalsifiable |
| INSENSITIVE | map has runs of identical tiles | ±1 column shifts are invisible except at a block boundary — the row fails *only* at the seam |
| UNRESOLVED | matcher returns `[]` | fine scroll ≠ 0 splits tiles across two columns; this is **not** "no constraint", the whole-tile matcher just cannot read it |
| OFF-SCREEN | the object/tile never reaches x 0..159 | the row cannot discriminate anything (e.g. an OBJ at X=0 is fully clipped) |

## Rules

- **A predicate that separates two rows which are bit-identical in every tracked
  quantity is a coincidence fit, not a law.** Prove identity or difference by
  tracing both — hunt trace, commit dot *and half*, first-output state — and if
  they are identical, the discriminator is outside the render FSM. Go find it.
- **Compare the end of the line that actually fails.** Rows can be identical at
  the line start and diverge only at the last tile, where late writes land.
  Comparing the wrong end manufactures a weld that is not there.
- **Don't accept "+N/−M" as a floor.** It is the signature of a nearly-right law
  missing a discriminator. Do classify the −M rows first: if they are DEGENERATE
  or INSENSITIVE, the trade is not what the score says.
- **Don't fit to a single row.** Do state plainly when an exception rests on one
  constraint, and name the mechanism that would dissolve it.
- **Don't assume a pixel mismatch is a fetch/timing bug.** Do check the palette:
  if the reference colour exists in *neither* committed palette, the divergence is
  upstream of the PPU (a computed value, not a timing law).
- Every probe is reverted before commit. Landed laws carry a red-before-green pin
  verified by reverting each arm individually.

## Worked outcomes (this tree)

- `old/offset_3/_ds_1` looked welded against `scx_0360c0/_ds_3` — identical hunt,
  commit dot 90/same half, identical first-output state. Disassembly: the ROM
  takes **no VBlank interrupt at all**, so the CPU free-runs off its NOP sled into
  line 0 and dispatches STAT at dot 10 where every other line dispatches at dot 6.
  The 4-dot gap was the whole failure. A CPU-phase fingerprint, not a render law.
- `scx_during_m3_spx0/1/2` differ in **two bytes** (the OBJ X and the checksum).
  spx0 is fully clipped and spx1 shows only a transparent pixel, so neither
  sibling can discriminate anything — spx2 is the first leg with a visible sprite
  pixel, and its mismatch turned out to be a palette *value* the ROM computes at
  runtime, never a fetch column.
- `scx_0360c0/_ds_3` demanded an EVEN first column and an ODD last; only one
  index assignment satisfies both. That single DISCRIMINATING row justified a law
  that six prior scalar sweeps had missed.
