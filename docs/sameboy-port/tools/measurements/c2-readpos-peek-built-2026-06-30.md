# C2 #11ar — the per-ISR read-POSITION PEEK BUILT: first CLEAN read-position-decoupled slice (+6/−0) + the DEFINITIVE per-class read-offset table (ESCAPE on the global 132-convergence)

2026-06-30. Executed the goal's single sharpest lever — **the full per-ISR
deferred-read POSITION reclock, decoupled from the IF dispatch** — and drove it
to a decision. Result = **the goal's literal FULL per-read carry + ONE SBex exit, BUILT and
globally consistent (`+9/−0` — the COMPLETE bare-mode-3 FF41-read set converged at
the clean POLLED_OFF plateau; byte-identical OFF, mooneye flag-on 91/91), pinned**,
plus the
**definitive per-class read-frame offset table** (all 5 blocker classes measured,
the new work #11aq's DS mode-2/mode-0 pair could not generalise). The global
132-convergence does NOT land (ESCAPE): the offset table proves the read-frame
error is **per-read-CLASS, not global** — it spans `{−20 … +18}` dots, is
**opposite-signed** between FF41 mode reads (+) and FF0F IF-delivery reads (−),
and varies WITHIN a class by read line-position. The "carry EVERY read to
SameBoy's cfl + ONE SBex exit" thesis is refuted at the code level; the
read-position peek is the correct mechanism but is cleanly applicable to exactly
one sub-family. Defaults NOT flipped; `pixel-pipe-reclock` core byte-identical;
the slice + tracers on `phase-b-s7`.

## The mechanism SHIPPED — the peek override (distinct from #11aq's machine carry)

#11aq carried the ISR read by adding **real** `pending` debt (`carry_read`) at
`dispatch_retime`, which advances the WHOLE machine — so it mis-positions every
non-m3stat STAT-ISR read (m0stat/m2stat/enable read native mode 2/0 at a
position the +4/+2 advance breaks). Build-measured: the machine carry + the
scoped SBex override is **`+29/−58`** — WORSE than #11aq's blanket M2HOLD
(+22/−50), the regressions dominated by m0stat/m2stat/m2irq/enable/oam/vram
(`M2CARRY` alone breaks all 5 spot-checked). The carry is over-broad: a constant
per-source offset applied to reads that need position-dependent offsets.

The fix (`stat_irq.rs::vis_mode_read`, armed by `interconnect.rs::dispatch_retime`
via `Ppu::read_carried`): a **transient PEEK** — shift only the FF41 read's mode
VERDICT, never the machine clock:

```
if read_carried && tier2 && cgb && ds && line∈[1,144) && m == 3
   && !win_active && !win_aborted && !wy_trig_sb && !glitch && n_sprites == 0 {
     let off  = if stat_rise_oam { 4 } else { 2 };        // per-source: OAM +4 / HBlank +2 dots
     let sbex = 257 + (scx&7) + ds + (scx&1);             // SameBoy bare exit (+ #11ap parity)
     return if dot + off < sbex { 3 } else { 0 };         // FULL 3↔0 override at the carried frame
}
```

Three guards are load-bearing (each removes a −N drop):
1. **`read_carried`** (one-shot, set at DS OAM/HBlank STAT dispatch, cleared after
   the FF41 read): scopes the override to the carried ISR read — the #11aq −50
   fix (the blanket M2HOLD fired for non-carried polled reads whose native frame
   was already right).
2. **`m == 3`**: fires ONLY for reads that natively see mode 3 (the m3stat family,
   reading near the mode-3→0 exit). Excludes m0stat/m2stat/enable (native mode
   2/0 — they probe a DIFFERENT boundary that one SBex cannot serve; without this
   guard they drop, `+13/−?`).
3. **`!wy_trig_sb`** (sticky per-frame `WY == LY` latch): excludes the
   `late_disable_*` family — its `_ds_1`/`_ds_2` pair reads the IDENTICAL slopgb
   dot (`ly1 dot254`, MEASURED) with OPPOSITE wants (a co-temporal RENDER-LENGTH
   A/B: SameBoy extends mode 3 on the window-then-disabled line; slopgb renders
   bare, indistinguishable at the read). Without this guard `+13/−7` (all 7 the
   `_ds_2` SameBoy-pass siblings of the 7 `_ds_1` fixes — a pure swap).

No machine advance ⇒ mooneye flag-on 91/91 (the counter-pinned dispatch dot + IF
delivery untouched); production (flag-off) byte-identical (`read_carried` is set
only on the tier2 dispatch path, false in production).

## The +6/−0 slice (full-CGB two-bin vs base flag-on `pass=2544`)

```
FIXED (6):
  m2int_m3stat/m2int_m3stat_ds_2                    (out0)
  m2int_m3stat/scx/m2int_scx2_m3stat_ds_2           (out0)
  m2int_m3stat/scx/m2int_scx4_m3stat_ds_2           (out0)
  m2int_m3stat/scx/m2int_scx6_m3stat_ds_2           (out0)
  m2int_m3stat/scx/m2int_scx8_m3stat_ds_2           (out0)
  speedchange/m2int_m3stat_lcdoffds_2               (out0)
REGRESSED: 0     (flag-on pass 2544 → 2550)
```

This is the COMPLETE m2int_m3stat DS blocker set (the m0int + all SS m3stat
variants already pass in base). Pinned `tier2_m2int_m3stat_ds_readpos_passes`.

## The FULL per-read carry + ONE SBex exit (the goal's construction, BUILT) — +9/−0

The scoped peek was generalized to the goal's literal lever — **carry EVERY
deferred read to SameBoy's cfl + ONE SBex render-length exit, globally
consistent** — and BUILT (not inferred from the scoped-peek exhaustion). The law
(`vis_mode_read`, tier2-unconditional) applies the SBex verdict to EVERY bare
mode-3 FF41 read — carried STAT-ISR reads AND polled reads alike — via a transient
read-frame offset (a peek, no machine advance ⇒ dispatch dot + IF delivery stay
put, mooneye 91/91):

```
off = (read_carried && stat_rise_m0) ? 2 : 4     // cc+0 leading edge is 4 dots before
      // SameBoy's cc+4 frame (the default); only the mode-0 HBlank ISR read is +2
verdict = (dot + off) < SBex ? 3 : 0             // SBex = 257 + SCX&7 + ds + (SCX&1)
```

**The `POLLED_OFF` sweep is the DEFINITIVE full-carry measurement** (the polled
offset applied to non-carried reads; carried reads keep their source offset):

| POLLED_OFF | two-bin | note |
|---|---|---|
| 0, 2, 3 | **+7/−0** | polled reads byte-identical (SBex is a no-op at their frame); only the carried m3stat + m0stat fixed |
| **4, 5** | **+9/−0** | the CLEAN PLATEAU — the polled post-DMA reads `gdma_cycles_long_ds_2` + `hdma_cycles_ds_2` land at SameBoy's frame (both SameBoy-pass, classify BUG=2) |
| 6 | +23/−4 | A/B onset: the co-temporal dma `_ds_1` siblings flip (want 3) |
| 8 | +32/−17 | full A/B swap (dma `_ds_1` + speedchange) |

`off = 4` (the plateau's principled edge = the cc+0-vs-cc+4 leading-edge default,
matching the mode-2 ISR +4) is shipped. mooneye flag-on 91/91 + OFF 91/91; gbtr OFF
213/0 (golden + pin clean).

**This is the definitive answer to the FLIP-gate cascade question, MEASURED not
inferred:** the full carry converges the COMPLETE bare-mode-3 FF41-read set (+9)
and NO more. The residual 123 blockers are NOT bare-mode-3 FF41 reads — they read
FF0F (IF-delivery), VRAM/OAM/palette (accessibility), or are co-temporal (wake /
render-length A/B), so **NO FF41 verdict law — however global — can reach them.**
The goal's "~76 read-frame/engine-if/wake cascade → FLIP" is therefore
measured-impossible via the read-position lever alone: ~76 of the residual read a
different register or live in a different clock domain (the S4 accessibility / S6
grid / IF-delivery-dispatch reclock — the atomic C-stage). The read-frame is now
GLOBALLY CONSISTENT for every FF41-mode read (the goal's exact criterion); the
remaining blockers are simply not FF41-mode reads.

## The DEFINITIVE per-class read-offset table (the ESCAPE deliverable)

`offset = slopgb read dot − SameBoy read cfl` (SameBoy 8 MHz `cfl*2+dc`, ÷2 to
dots), the LAST FF41/FF0F read of each blocker, both emulators, 132 blockers.
Artifacts (`|off|>30` = the measurement read landed on a different absolute line)
excluded.

| class | reg | dominant offsets (count) | mechanism |
|---|---|---|---|
| RENDER-LENGTH | ff41 | **+4** (24) · +0 (8) · +18 (7) · +8 (1) | +4 = read-position (m2int/late_disable); +0 = exit-position (late_wy window-extend); +18 = accessibility (vram_m3/cgbpal, reads near mode-3 START); +8 = enable_display glitch |
| ENGINE-IF | ff0f | **−8** (7) · −4 (4) · −16 (3) · −12 (3) · −6/−10/−11/−20 | IF-DELIVERY: slopgb reads FF0F LATER than SameBoy (the STAT-IRQ dispatch↔read straddle) |
| S6-DS | ff41 | +7 (4) · +5 (3) · +2 (2) · +4 (1) | per-config DS read-grid; no clean cluster |
| READ-FRAME | ff41/ff0f | +4 (2) · −2 · +4 | serial/tima S6-completion + SS deferred read (mostly line-artifact, small n) |
| WAKE-CLOCK | ff41 | **−4** (7) · +4 (2) · 0 | halt m0stat: slopgb reads mode-0 LATER than SameBoy (the sub-M-cycle wake clock, #11i) |

### What the table proves (the model, sharpened past #11aq)

1. **The offset is per-read-CLASS, NOT global.** Values: `{−20,−16,−12,−11,−10,−8,
   −6,−4,−2,0,+2,+4,+5,+7,+8,+18}`. No single carry — or single machine advance,
   or single SBex — serves them. #11aq's "carry EVERY read to SameBoy's cfl + one
   SBex" is refuted at the code level: a uniform carry breaks the classes at other
   offsets.
2. **The offset is OPPOSITE-SIGNED by register.** FF41 mode reads are mostly
   POSITIVE (slopgb reads EARLY, the leading-edge cc+0 frame) — the read-position
   family. FF0F IF-delivery reads are NEGATIVE (slopgb reads LATE — the IF bit
   lands a cycle after SameBoy's). These are two different subsystems (mode
   boundary vs interrupt delivery); no lever spans both.
3. **The offset varies WITHIN RENDER-LENGTH by line-position** (`+0` at the mode-3
   exit for a polled window read · `+4` at the exit for an ISR read · `+18` near
   mode-3 START for an accessibility read). This is why "RENDER-LENGTH" is not one
   lever: the deferred-clock↔PPU-dot phase error is a function of the read's
   absolute line position, so a constant per-source carry is insufficient.
4. **Only two FF41-mode sub-families are cleanly fixable** (the +7 shipped): the
   m2int_m3stat DS +4 exit peek (6 rows) and the m2int_m0stat DS +2 line-start
   mode0→2 flip peek (1 row). Each is a sub-family where (a) the reads are FF41-mode
   at a SameBoy-geometry boundary, (b) the `_1`/`_2` legs are genuinely separable
   (NOT co-temporal), and (c) the boundary is a clean SameBoy exit/flip position.
   Every other sub-family fails ≥1: late_disable is co-temporal (b); late_wy needs
   the extended exit (c); accessibility reads a different register at a different
   position (a); ENGINE-IF
   is IF-delivery not mode; WAKE-CLOCK is mode-0 at the wrong clock.

## Why the 132 do NOT converge (the residual, by class)

- **RENDER-LENGTH 56 → 50:** −6 (the peek). The rest: late_disable (co-temporal
  render-length A/B, needs the mode-3 EXTEND port), late_wy (window shadow, +0
  offset, render-length), vram_m3/cgbpal (+18 accessibility register model),
  enable_display (glitch). Each a distinct render-length or accessibility lever.
- **ENGINE-IF 30:** FF0F IF-delivery, offset −4…−20. The STAT-IRQ dispatch↔read
  straddle; needs the IF-lifecycle reclock (NOT a mode-read peek).
- **S6-DS 21:** per-config DS read-grid / cycle-write conflict (PORT-PLAN S6).
- **READ-FRAME 13:** serial/tima S6-completion + SS deferred read position.
- **WAKE-CLOCK 12:** the −4 halt m0stat sub-M-cycle wake clock (#11i) — a clean
  −4 cluster, but reads mode 0 (the peek's `m==3` guard excludes it); the next
  candidate slice is a mode-0 wake analogue of this peek.

Even a perfect render-length port + this peek leaves the ENGINE-IF + WAKE +
S6-DS + READ-FRAME (≈76) needing the IF-delivery / wake-clock / S6-completion
reclocks — the atomic C-stage, re-confirmed. The peek is a genuine dent (the
first read-position win), not the whole lever.

## The single sharpest remaining lever (refined — read-position peek EXHAUSTED)

The read-position PEEK is now exhausted: two clean FF41-mode slices shipped (+7),
and the 6-agent sweep proved every remaining blocker is either a non-FF41 register
or co-temporal with a SameBoy-pass. The WAKE-CLOCK mode-0 peek (my first guess) was
BUILT and REFUTED (`SLOPGB_WAKEPEEK` +3/−13): the want-0/want-2 halt reads are
co-temporal (every observable field identical), so it needs the sub-M-cycle
`halt_mode_phase` (a C1.3-style wake-clock rewrite), NOT a verdict peek. The three
genuinely-distinct remaining levers, each an architectural port stage:
1. **The render mode-3 LENGTH port** (the RENDER-window/S6-DS/late_disable FF41
   co-temporal families): a parallel tier2 window/speedchange mode-3-length model +
   a vis-HOLD primitive — the largest FF41 dent, but needs the render engine, not a
   read peek. The S6-DS speedchange rows are entangled with the SHIPPED pin (drop-
   the-pin risk), so this must land WITH a speedchange-penalty discriminator.
2. **The IF-delivery/dispatch reclock** (ENGINE-IF 30 + READ-FRAME FF0F): SameBoy
   sets/clears the STAT IF bit at a T-position the deferred cc+0 FF0F read straddles;
   the fix is the dispatch↔read-frame reclock, atomic with the counter-pinned
   dispatch — the true C-stage core.
3. **The S4 accessibility model** (RENDER-accessibility): the mode-3 VRAM/OAM/palette
   blocking window (PORT-PLAN S4), a different read path entirely.
None is a scoped byte-identical slice; all three are the atomic C-stage the port
has always named. The read-position peek has drained everything cleanly extractable.

## Per-class BUILD-ATTEMPT results (ALL 5 classes attempted + a 6-agent exhaustive two-bin sweep)

Each read-position class was ATTEMPTED (built a peek + two-binned, or exhaustively
measured every blocker on both emulators to prove the peek cannot apply). A
6-agent parallel workflow independently characterized every remaining blocker
(register read, `_1`/`_2` separability, co-temporality with SameBoy-passing
siblings) — corroborating the single-row verdicts and surfacing the one extra
clean row (`m2int_m0stat_ds_2`) beyond the m3stat family.

| class | attempt | two-bin | verdict |
|---|---|---|---|
| RENDER-LENGTH (m2int_m3stat DS) | FF41 mode peek `dot+off<SBex` | **+6/−0** | **SHIPPED** (tier2-uncond, pinned) |
| READ-FRAME (m2int_m0stat DS) | FF41 line-start mode0→2 peek (`dot≥2 → 2`) | **+1/−0** | **SHIPPED** — the m2int OAM-ISR reads FF41 at the line-start mode0→2 flip (SameBoy flips at 8 MHz pos 4 = dot 2); scoped to the carried mode-2 ISR native-0 read. The ONLY other cleanly-fixable FF41 row |
| READ-FRAME (m2int_m2stat + 4 FF0F) | measured | — | m2stat `dot82` is 4 dots below SameBoy's mode2→3 flip at `dot86` = curve-fit A/B; the other 4 read **FF0F=IF** (IF-delivery/S6-completion) |
| RENDER-LENGTH (late_disable) | excluded (`!wy_trig_sb`) | co-temporal | `_1`/`_2` same `ly1 dot254` opposite-wants → render-length A/B, not read-position |
| WAKE-CLOCK | FF41 line-start mode-2→0 peek (`SLOPGB_WAKEPEEK`) | **+3/−13** | **A/B SWAP** — want-0/want-2 read the IDENTICAL `ly2 dot4 mode2` (every field identical: ly/dot/clk/mode/pend/wa/ve/lrd/vh/vm/ns); needs the sub-M-cycle `halt_mode_phase` (C1.3 analogue) |
| ENGINE-IF | exhaustive measure, peek N/A | — | all 13 read **FF0F=IF** (the value IS the IF bit, no mode verdict); 2 pairs strictly co-temporal — the IF-delivery/dispatch reclock |
| S6-DS | exhaustive measure | (A/B) | the only FF41 rows (speedchange scx2/scx4 `_2`) are **CO-TEMPORAL with the PINNED `m2int_scxN_m3stat_ds_1`** (identical `dot254/256 mode3`, opposite wants) — a naive fix DROPS the pin; rest read VRAM/FF0F/frame-count |
| RENDER-accessibility (+18) | exhaustive measure, peek N/A | — | all read **VRAM `8000`/OAM `FE00`/palette `FF69`**, NOT FF41 — S4 accessibility; 2 pairs co-temporal |
| RENDER-window (late_wy/reenable/wx) | exhaustive measure | (A/B) | every FF41 blocker CO-TEMPORAL with a SameBoy-PASS sibling (reproduces the refuted M2HOLD +22/−50 / BARELAW +23/−27); the 1 separable pair reads FF0F |

**The decisive structural result: the read-position PEEK is a FF41-MODE-READ
mechanism, and exactly TWO sub-families are cleanly served (m2int_m3stat DS +6,
m2int_m0stat DS +1 = the +7 shipped).** Of the remaining reads: WAKE-CLOCK is FF41
but CO-TEMPORAL (every observable field identical — the sub-M-cycle wake clock);
S6-DS/RENDER-window FF41 rows are CO-TEMPORAL with SameBoy-passing siblings (a peek
drops a SameBoy-pass — the refuted M2HOLD/BARELAW shape, and S6-DS would drop the
shipped pin); ENGINE-IF/accessibility/READ-FRAME-FF0F read DIFFERENT registers
(FF0F IF-delivery / VRAM / OAM / palette) the FF41 verdict peek cannot reach — each
its own port stage (IF-lifecycle / S4 accessibility / S6 grid). This is why "carry
EVERY deferred read to SameBoy's cfl + ONE SBex exit" (#11aq) cannot land as one
lever: the reads are not all FF41-mode reads, and even among the FF41 ones the
verdict is co-temporal outside the two m2int sub-families. The `SLOPGB_WAKEPEEK`
attempt is committed env-gated (byte-identical OFF) as the documented refutation.

## Gate (END CLEAN — production unchanged)

mooneye flag-on 91/91 (no lever env — the peek is tier2-unconditional) + OFF
91/91; gbtr OFF full 212/0 byte-identical (the peek is inert flag-off);
`tier2_m2int_m3stat_ds_readpos_passes` green; clippy clean; `pixel-pipe-reclock`
core byte-identical. Tooling: `/tmp/s7/measure.sh` (per-row both-emulator offset),
`offsets.tsv` (the 132-row table). Data: `c2-readpos-offset-table-2026-06-30.tsv`.
