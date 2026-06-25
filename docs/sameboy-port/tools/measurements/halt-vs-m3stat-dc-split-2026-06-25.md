# C-stage flag-on family mechanism split — direct SameBoy `dc` evidence

2026-06-25 (#11i). The atomic-reclock-recipe frames the C-stage as one
"read-frame↔boundary" reclock plus a side note that the halt family is a
separate wake residual. This session measured the **sub-dot (`display_cycles`,
`dc`) read position on SameBoy** for the two canonical want-pairs and pinned the
split with hard numbers: the C-stage flag-on regressions divide into **three
mechanically distinct families**, and the headline read-frame reclock — even
done perfectly, eighth-grid included — fixes only one of them.

## Method

SameBoy 1.0.2 tester rebuilt with the `SBREAD ff41` patch (logs `ly/cfl/dc/mode`
at `read_high_memory` `case GB_IO_STAT`; `tools/stat-irq-trace.md`). `dc` =
`display_cycles`, SameBoy's sub-dot 8 MHz fraction — its OWN finest read-position
resolution, one grid below `cfl` (`cycles_for_line`). Run
`SB_TRACE=1 sameboy_tester --dmg --length 2 ROM`, isolate the measurement read as
the **count-1** `SBREAD ff41` row (setup frames repeat a different value).

## The decisive contrast

| family | want pair (DMG) | SameBoy measurement read | read positions |
|---|---|---|---|
| **kernel `m2int_m3stat`** | `_1`=3 / `_2`=0 | `cfl=256 dc=0 → mode 3` / `cfl=261 dc=-2 → mode 0` | **DIFFERENT** (5 dots apart) |
| **halt `m0stat` want0** (e.g. `m0int_m0stat_scx2_1`) | 0 | `ly2 cfl=0 dc=0 → mode 0` | (all halt) |
| **halt `m0stat` want2** (`late_m0irq_halt_m0stat_scx3_3b`) | 2 | `ly2 cfl=0 dc=0 → mode 2` | **IDENTICAL** to want0 |

Across the WHOLE halt `m0stat` family SameBoy reads the measurement read at the
same reported position `ly2 cfl=0 dc=0`, returning mode 0 for the want-0 rows and
mode 2 for the want-2 row — identical read coordinate, opposite mode, even at
SameBoy's finest sub-dot `dc`.

## Read it

- **Kernel pair = READ-OBSERVER.** The want-3 and want-0 reads land at *different*
  (`cfl`,`dc`) positions; the per-config mode-3→0 boundary sits between them, so
  the mode flips because the read frame samples a different side of the boundary.
  This is the recipe's read-frame↔boundary reclock: get the deferred read to
  SameBoy's `cfl`, re-derive the boundary, and where read and boundary land on
  the **same dot** the eighth-grid (`event_phase`/`ACCESS_PHASE`) orders the tie
  (kernel scx0: read `cfl256 dc0` wins over the boundary committing later in the
  dot → mode 3). The eighth grid is the right and necessary tool **here**.

- **Halt pair = WAKE-CLOCK.** The want-0 and want-2 reads land at the *identical*
  position `ly2 cfl0 dc0` — even at SameBoy's own finest sub-dot `dc` resolution
  — yet return mode 0 vs mode 2. **No read-position model can separate them from
  the read side.** The discriminator is upstream: the halt-wake / CPU-resume
  T-phase determines which committed STAT mode field the read samples at that
  identical instant. This is the first **direct SameBoy `dc` confirmation** of
  #11e's reverted-attempt diagnosis (#11e measured only slopgb's `cc`, which
  collapses; it *inferred* SameBoy distinguished by a finer phase — now shown:
  SameBoy's read position is genuinely identical, so the discriminator is the
  wake clock, not any read observer). The eighth-grid read-observer lever
  **cannot** fix the halt family. Its lever is the sub-M-cycle WAKE clock: record
  the mode-0 IRQ rise / CPU resume at its T-phase, not the M-cycle boundary
  (`stat-irq-trace.md` halt section; #11e REVERTED the cc-gated attempt because
  `cc` quantizes the wake phase away).

### slopgb side (current flag-on tier2, binary at `1fe0990`)

The full halt category flag-on: **21 fails** (vs 9 baselined-floor OFF-fails).
The failing `m0stat` rows, read trace (`SLOPGB_S5DBG` ff41 vs SameBoy):

| ROM | want | slopgb read | SameBoy read |
|---|---|---|---|
| `m0int_m0stat_scx2_1` | 0 | `ly2 dot4 mode2` ✗ | `ly2 cfl0 dc0 mode0` |
| `m0int_m0stat_scx3_2` | 0 | `ly2 dot4 mode2` ✗ | `ly2 cfl0 dc0 mode0` |
| `m0int_m0stat_scx4_2` | 0 | `ly2 dot4 mode2` ✗ | `ly2 cfl0 dc0 mode0` |
| `late_m0int_halt_m0stat_scx2_1a` | 0 | `ly2 dot4 mode2` ✗ | `ly2 cfl0 dc0 mode0` |
| `late_m0irq_halt_m0stat_scx3_3b` | 2 | `ly2 dot0 mode0` ✗ | `ly2 cfl0 dc0 mode2` |

**The discriminator is the sub-M-cycle wake phase vs slopgb's dot0-3 line-start
mode-hold.** `vis_mode` (`stat_irq.rs:10-56`) returns mode 0 for the first 4 dots
of every visible line (the documented DMG `lcdon_timing-GS` hold), then mode 2
from `mode3_entry_dot`. So in slopgb `ly2 dot0` = mode 0, `ly2 dot4` = mode 2.
The want-0 rows over-advance the deferred wake to `dot4` (mode 2 — the DMG +4
halt-wake over-advance, #11e) where they should sit at `dot0` (mode 0); the
want-2 row sits at `dot0` (mode 0) where it should read mode 2. **The want-0/want-2
split = whether the wake lands before (→ dot0 / mode 0) or after (→ dot4 / mode 2)
slopgb's line-start mode-0 hold** — exactly the sub-M-cycle wake phase, and the
SAME dot0-3 line-start mode-hold structure as #11h's m1/lyc engine root 1 (the
ly144 vblank-entry hold). A uniform −4 back-date of the wake read fixes the want-0
rows but forces the want-2 row to mode 0 — and `scx3_3b` is **SameBoy-passing**
(reads mode 2), so it must not be dropped → the #11e `+11/−3` back-date stays
refuted; only the per-wake T-phase clock resolves both directions.

> Correction to a stale #11e framing: the same-base pair
> `late_m0int_halt_m0stat_scx3_2a`/`_2b` **passes** flag-on now (2a reads
> `dot0 mode0`, 2b reads `dot4 mode2` — both correct). The live failing collapse
> is the cross-base want-0 vs `scx3_3b` want-2 set above, not 2a/2b.

## The three C-stage mechanisms (refined)

The 430 flag-on regr do NOT share one lever. By where the want-discriminator lives:

1. **READ-OBSERVER** — `m2int_m3stat` (11) + the read-vs-boundary geometry
   families (window length 107, m0enable 12, vram_m3 11, oam_access 11, …). The
   read frame samples the wrong side of a per-config mode-3 boundary. Lever = the
   read-frame reclock + per-config boundary geometry (window = the parallel
   mode-3-length model, #11g); eighth-grid resolves same-dot ties. Cascades to
   the global frame (moving the read +4 breaks the counter-pinned mooneye
   `intr_2`/DIV — recipe 2026-06-24 box).
2. **WAKE-CLOCK** — halt `*_m0stat_*` (32). Read position identical; the committed
   STAT mode differs by the halt-wake T-phase. Lever = the sub-M-cycle wake clock.
   **Independent of the read-observer reclock** (this session's result).
3. **ENGINE-DRIVER** — m1 (26) + lycEnable (26), #11h. Level-engine driver logic:
   missing vblank-entry mode-1 re-arm + spurious ly153→ly0 LYC wrap / late-disable.
   Independent of both the read phase and the wake clock.
   (sprites 87 + speedchange 13 DS = L2 geometry / S6-S7 DS, out of S5 scope.)

## Implication for the atomic lift

The "atomicity" is **not** "one reclock fixes everything." It is "≥3 distinct
sub-M-cycle / engine mechanisms, each of which moves SameBoy-passing rows in
isolation, so they all land together in the flip." A perfect read-frame
reclock (mechanism 1) still leaves the halt (2) and m1/lyc (3) families red.
Any single-mechanism worktree attempt is RED on the other two families'
SameBoy-passing rows → uncommittable until all land. This is why no clean
one-slice win remains and why every prior single-lever slice was refuted.

## Status

Measurement only — no code changed. SameBoy tester + slopgb `SLOPGB_S5DBG`
tracers byte-identical OFF. Defaults NOT flipped.
