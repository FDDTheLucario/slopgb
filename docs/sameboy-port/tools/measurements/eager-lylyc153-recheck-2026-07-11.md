# EAGER `ly_lyc_153_write-{C,GS}` ×6 re-attack with the `rom-diff-weld` method — one representable sub-bug FOUND + FIXED-in-probe (the `lyc_event` protection window, correcting #11dx's Agb read-frame claim), but the FULL 6-row fix is AIRTIGHT Part-A: the coincidence cutoff is welded to the CPU read/dispatch frame (the ROM reads its interrupt count one M-cycle after the FF45 write, straddling the dot-4-vs-6 boundary). NO CODE SHIPPED, tree byte-identical @ d7b3e6f (2026-07-11, #11ei)

Base: `finish-port-halfdot @ d7b3e6f`. **NO CODE SHIPPED — tree byte-identical
@ d7b3e6f** (`git diff` empty; golden trivially unchanged — nothing entered
production). Every probe/experiment below was reverted.

## Task

Re-examine the 6 `wilbertpol/acceptance/gpu/ly_lyc_153_write-{GS,C}` non-gambatte
eager flip regressions with the `rom-diff-weld` method. Prior verdict (#11eg,
#11dx): "counter-pinned dispatch, Part-A"; but the only prior lever was #11dv's
UNIFORM whole-dot `ly_for_comparison` re-latch back-date (dot6→dot4) that shuffled
+17 CGB — the exact false-weld the skill warns about. Trace for a discriminator
those swept over.

## The ROM (fully reverse-engineered — prior maps never disassembled it)

`ly_lyc_153_write` is a **multi-round STAT-interrupt-COUNT** test. The STAT ISR
at `0x0048` is literally `inc b; reti` — **B counts STAT interrupts**. Each round
resets the LCD, arms `IE=STAT`, sets up LYC, EIs, spins a fixed NOP window across
the line-152→153 boundary, then reads B and stores the count to `C014..C018`. The
wilbertpol framework compares the stored counts against a table; a mismatch dumps
debug regs (the `B=48 C=BE …` failure signature) instead of Fibonacci.

Two round FAMILIES, both writing FF45 on line 153:

| family | rounds | setup | line-153 write | tests |
|---|---|---|---|---|
| **DISABLE** | C014, C015 | LYC=153 held (set line 144) | LYC 153→**F0** | does a late disable still let the held LY=153 coincidence fire? |
| **ENABLE** | C016, C017 | LYC=**F0** held | LYC F0→**153** | does a late fresh enable trigger the coincidence? |

Consecutive rounds differ by **one NOP** — they walk the write across the
coincidence cutoff by 4 dots. The reference (OFF, = SameBoy-pass):
`C014=0, C015=1, C016=1, C017=0` — a boundary between C014/C015 (disable) and
between C016/C017 (enable).

## The divergence (`cmp`-equivalent: per-round WRAM dump, OFF vs leading-edge)

| cell | OFF | LE | eager | meaning |
|---|:--:|:--:|:--:|---|
| C014 | 0 | 0 | 0 | disable in-time → prevented ✓ |
| **C015** | **1** | **0** | **0** | disable too-late → OFF fires, LE/eager MISS |
| C016 | 1 | 1 | 1 | enable in-time → fires ✓ |
| **C017** | **0** | ~0→**1** | **1** | enable too-late → OFF no-fire, LE/eager OVER-fire |

The leading-edge coincidence cutoff is **2 dots late**: C015 (disable just past
OFF's cutoff) lands just BEFORE the LE cutoff → prevented; C017 (enable just past
OFF's cutoff) lands just before the LE cutoff → fires. **Symmetric, opposite
direction** — the hallmark of a frame shift, not a one-sided lever.

## Root cause #1 — the `lyc_event` protection window (REPRESENTABLE; corrects #11dx)

Full-trace (`SLOPGB_S5DBG`, per-dot engine state on line 153, round C015):

```
dot 0  write LYC 153→F0 (protected: lyc_event stays 99)
dot 1-4  lyc=F0  lyc_event=99   (step_dot protects dots 1-4)
dot 5    lyc=F0  lyc_event=F0   ← protection EXPIRES, copy catches up
dot 6-7  ly_for_comparison=153  → compare against lyc_event=F0 → NO match → lycln=0 → no fire
```

The delayed `lyc_event` copy (`engine.rs::step_dot`, protected on line-153 dots
1-4 + 9-12) **expires at dot 5, ONE dot before** the SameBoy `GB_STAT_update`
engine's `ly_for_comparison==153` coincidence check at **dots 6-7** (CGB-C/DMG SS).
OFF's gambatte engine delivers the coincidence at **dot 4** (inside the 1-4
window) — so OFF fires; the deferred engine's dot-6 check reads the already-caught-up
disabled value → miss.

**FIX (probe-verified):** extend the line-153 `lyc_event` protection to cover dots
5-7 (`leading_edge_reads && line==153 && (5..=7)`). With it, the engine re-latches
`lyc_interrupt_line=1` at dot 6 and **`ly_lyc_153_write-C [Agb] C015 flips 0→1`**.

**This CORRECTS #11dx**, which claimed "`Model::Agb` already re-latches LYC=153 at
dot 4 yet still fails → pure read-frame coherence." The Agb C015 failure was NOT
read-frame — it was this representable `lyc_event`-window expiry. #11dx conflated
two bugs; the disable/`lyc_event` half is a fixable law.

## Root cause #2 — the CPU read/dispatch frame (AIRTIGHT Part-A, mechanism-proven)

Extending `lyc_event` makes the engine FIRE, but `C015` still reads 0 on CGB-C,
and a `lyc_if_delay=4` dot-4 delivery (matching OFF's dot) fixes CGB-C **under LE**
yet NOT under eager. The decisive trace (eager, round C015, with the dot-4
delivery probe):

```
dot 5   ifstat ly=153 dot=5   ← the STAT IF IS delivered (representable, tunable)
...     the CPU already executed 026E `ld a,b` (B read = 0) BEFORE servicing it
store C015=00
```

The ROM reads B at `0x026E` (`ld a,b`) **one M-cycle after** the FF45 write at
`0x026C`. Whether the interrupt is serviced before that read is set by **where the
`026C→026E` instruction boundary sits relative to the PPU dot** — i.e. the CPU
dispatch frame (cc+0 LE / cc+4 eager / cc+4 SameBoy), NOT any PPU-side latch. The
delivery DOT is representable and tunable (proven: `ifstat` fires at dot 4/5 on
demand); the **B-read position is the dispatch clock itself**. Delivering the
interrupt "at the right dot" cannot help when the CPU has already latched B.

The ENABLE half (C016/C017) seals it: the fresh LYC=153 write fires the engine at
dot 6, which the deferred read COUNTS but OFF does not — an OVER-fire that no
delivery law can suppress without re-introducing the cutoff shift (= #11dv's +17
shuffle). Disable wants the coincidence 2 dots EARLIER, enable wants it 2 dots
LATER, relative to the leading-edge frame — they need OPPOSITE corrections, and
the manifestation FLIPS between LE (C015 fixable, C017 breaks) and eager (both
break). A single flag-gated cutoff cannot satisfy both; only the coherent per-T
read+dispatch retime (Part-A) does.

## Verdict — 1 sub-bug corrected, full fix is AIRTIGHT Part-A (not a uniform sweep)

- **REFUTED (partial):** the `lyc_event` protection-window expiry is a representable
  law that recovers the Agb/disable coincidence — #11dx's "Agb read-frame" claim is
  wrong for that half.
- **CONFIRMED Part-A (airtight, mechanism-level):** the full 6-row fix is gated on
  the CPU read/dispatch frame. Proven by (a) the delivery-vs-read decoupling — the
  IF is deliverable at the correct dot yet the B-read (one M-cycle after the FF45
  write) misses it; (b) the enable/disable SYMMETRIC opposite-direction cutoff; (c)
  the LE↔eager manifestation flip. This is **not** #11dv's uniform-sweep false weld
  — it is the genuine coherent-frame wall #11eg/#11dx named, now proven at the
  mechanism level rather than asserted from "tier2 also fails."
- No representable single-latch discriminator separates the 6 rows from the
  gambatte `lyc153int_*`/`late_wy` family without shifting the cutoff frame. The
  rows land with the C3-flip coherent per-T retime (Part-A). Do NOT re-chase a
  flag-gated delivery/back-date port — it fixes at most the disable half and breaks
  the enable half (net zero complete rows recovered).

## Gates (all hold — no code shipped)

| gate | value |
|---|---|
| tree | `git diff` empty; byte-identical @ d7b3e6f |
| `golden_fingerprint` | unchanged by construction (no production code touched) |
| EV CGB / DMG | 287 / 38 (baseline, no ship) |
| tier2 CGB / DMG | 291 / 116 (inherited) |
| mooneye 93×3 | inherited |

## Do-not-re-chase ledger

- `ly_lyc_153_write ×6` fail because the line-153 LYC-coincidence CUTOFF is 2 dots
  off in the leading-edge frame, welded to the CPU read/dispatch clock (the ROM
  reads its `inc b` count one M-cycle after the FF45 write). Part-A.
- The `lyc_event` line-153 protection-window extension (dots 5-7) is a REAL fix for
  the Agb/disable coincidence — but it recovers ZERO complete rows alone (the enable
  half + CGB-C read frame still fail), so it is not shippable in isolation. Fold it
  into the Part-A retime batch (it is orthogonal and correct).
- Do NOT back-date the `ly_for_comparison=153` window (dot6→4) — #11dv's +17 CGB
  shuffle; the enable rounds need the opposite direction.

## Reproduction

```sh
ROM=test-roms/game-boy-test-roms-v7.0/mooneye-test-suite-wilbertpol/acceptance/gpu/ly_lyc_153_write-C.gb
# probe build (WRAMDUMP + per-dot engine trace were temp probes, reverted):
SLOPGB_WILBERT=1 run_mooneye $ROM cgb                    # OFF  → PASS (Fibonacci)
SLOPGB_WILBERT=1 SLOPGB_LE=1    run_mooneye $ROM cgb     # LE   → FAIL B=48
SLOPGB_WILBERT=1 SLOPGB_EAGER=1 run_mooneye $ROM cgb     # eager→ FAIL B=48
# STAT ISR = inc b @ 0x0048; per-round counts stored C014..C018; C015 is the divergence.
```
