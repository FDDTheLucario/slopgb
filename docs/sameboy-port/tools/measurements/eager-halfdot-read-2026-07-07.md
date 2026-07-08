# The EAGER half-dot READ peek — INERT; the read re-syncs to even parity (2026-07-07, #11bx)

Task: wire the HALF-DOT read on the eager `Bus::read` so the FF41 peek is
sub-dot precise (HALFDOT Part B on the eager clock), enable the FF41 read laws
under `eager_value`, and sweep the read-frame shift N to dip the EV CGB two-bin
below the 578 whole-dot floor the prior map
(`eager-atomic-core-2026-07-07.md`) measured.

**Result: the half-dot peek is INERT — the eager FF41 read re-syncs to an EVEN
8 MHz half-dot every time, so `dhalf` never becomes 1 and no `_1`/`_2` pair
separates. Independently, the shift sweep OVERTURNS the prior map's "578 floor /
monotone" claim (it under-sampled at N∈{0,4,8}): the real curve is non-monotone
with a minimum EV CGB 566 at N=+14 hd — but that minimum is a within-
`speedchange` A/B SHUFFLE (fix 28 / break 16 speedchange rows, net −12), a
whole-dot frame OVERFIT, not a convergence. Tree reverted byte-identical to
`ace4d31`; this map is the whole deliverable.** The precise next lever is NOT
the read peek (inert) but porting the tier2 DS/STOP sub-dot alignment
(`lcd_shift_dots` / `sb_dsa8` half-dot phase) to the eager clock — that is where
the speedchange pair separation actually lives.

## What was wired (reverted after measuring)

1. **Edit A** (`ppu/stat_irq/read_laws.rs:65`): `if !self.tier2_reclock` →
   `if !(self.tier2_reclock || self.eager_value)` — enable the FF41 read-law web
   (`vis_exit_hd` + the window/bare-exit arms) under EV. Frame-80 entry is
   already preserved: `mode3_entry_dot()` (stat_irq.rs:94) and the glitch-74
   back-date (stat_irq.rs:34) both gate on `leading_edge_reads && !tier2_reclock`
   — true under EV — so EV keeps the LE frame-80/glitch-74 entry. No sibling
   `!tier2_reclock` guard exists inside `vis_mode_read`/`vis_exit_hd` (only the
   line-65 gate); `regs.rs:184` calls `vis_mode_read()` unconditionally.
2. **The half-dot peek** (`interconnect/cycle.rs::eager_ff41_peek` +
   `ppu/engine.rs::read_ff41_half`): on the eager `Bus::read` (interconnect.rs)
   the FF41 leading-edge peek is resolved to the read's 8 MHz half-dot before
   sampling — SS whole-dot (`dhalf=0`), DS `dhalf = clock.now() & 1` (each CPU-T
   is one half-dot, so the leading-edge T parity is the sub-dot phase). Restores
   `dhalf` after the peek so the following whole-dot advance is untouched.
   Dispatch stays cc+4 (only the peek's sub-dot phase moves).
3. **Edit B / the sweep knob** (`ppu/engine.rs::read_pos_hd` +
   `probe.rs::tune_evshift`, `SLOPGB_EVSHIFT`): `read_pos_hd += evshift` under
   `eager_value` (0 default → byte-identical), to sweep the EV frame against the
   deferred-frame exit constants. All three edits are `eager_value`-gated;
   production + tier2 + golden untouched.

## The sweep curve (EV CGB fail, `SLOPGB_PROBE_EV=1`, cgb_rowlist 3422 rows)

| N (hd) | fail | | N (hd) | fail | | N (hd) | fail |
|---:|---:|---|---:|---:|---|---:|---:|
| −2 | 602 | | 5 | 585 | | 12 | 570 |
| −1 | 602 | | 6 | **574** | | 14 | **566** |
| 0 | 601 | | 7 | **574** | | 16 | 580 |
| 1 | 601 | | 8 | 578 | | 18 | 582 |
| 2 | 607 | | 9 | 578 | | 20 | 587 |
| 3 | 607 | | 10 | **571** | |  |  |
| 4 | 585 | | 11 | **571** | |  |  |

Prior map's three points (N=0→601, +4→585, +8→578) reproduce EXACTLY — the
wiring is validated. But the prior map declared 578 the floor from those three
even-hd samples alone; the full curve is NON-MONOTONE with TWO sub-578 basins
(N=6→574 and N=10..14→571..566, min 566 at +14) that the 3-point sample skipped
over. The 578 at N=8 is a local bump, not the floor.

## Finding 1 — the half-dot peek is INERT (`dhalf` never varies)

**Every consecutive (2k, 2k+1) pair in the curve is EXACTLY equal** (−2=−1=602,
0=1=601, 2=3=607, 4=5=585, 6=7=574, 8=9=578, 10=11=571). If any FF41 read ever
landed at `dhalf==1`, its `read_pos_hd` (`2·dot + dhalf + N`) would be odd, and
the `< exit` verdict would flip between an even N and its odd successor for that
read — the pair counts would differ. They never do. So `dhalf` is 0 at every
eager FF41 read, and the peek's DS `clock.now() & 1` term is always 0.

**Why:** the eager clock re-syncs to a 4-T (4·M) grid at every READ. Writes
re-park odd conflict totals (`cycle_clock::write`: ReadNew→5, WriteCpu→3,
EarlyTwo→6, WxHold→3), so `clock.now()` is momentarily odd AT the write's commit
— but the following read pays the parked total and lands back at `clock.now() ≡
boot_offset (mod 4)`, an EVEN T (the per-M-cycle total conserves to 4). In DS
that is `dhalf == 0`. There is **no per-read sub-dot variation on the eager clock
without a persistent odd shift**, and EV omits every such shift by design (no −2
dispatch retime, no `carry_read`/`forgive` wake clock — those are all
`tier2_reclock`-gated). The passive read peek therefore cannot reach the odd-hd
position; HALFDOT Part B as a read-only peek is a no-op on the eager clock.

## Finding 2 — the sub-578 minimum is a `speedchange` A/B shuffle (overfit)

N=+14 (566) vs N=+8 (578, the native-verdict / deferred-84 frame), fail-list
delta by family:

| | family | n |
|---|---|---:|
| **fixed** (28+9) | speedchange | 28 |
| | late_scx / late_disable / hdma / m2int | 9 |
| **broke** (25) | speedchange_ly / speedchange | 16 |
| | offset / late_scx / late_disable / m2int | 9 |

The +12 net is a near-wash INSIDE the speedchange (DS) family — a whole-dot shift
that trades speedchange `_1`/`_2` rows against each other and lands a lucky
imbalance. This is the exact sub-dot-straddle signature the prior map named for
the window family at N=0 (fix ~13 / break ~19), one basin over. A uniform
whole-dot shift can only shuffle these pairs; it cannot SEPARATE them (that needs
the sub-dot, which the peek can't supply — Finding 1). **+14 is an OCR-min
overfit frame with no physical basis (+7 dots past the leading edge, +3 past the
deferred-84 frame), not a convergence — not committed.**

## The wall, restated precisely (and the corrected next lever)

The prior map located the wall at "the read must resolve to its true half-dot;
under EV `dhalf` is always 0." Wiring the peek proves the deeper cause: **`dhalf`
is 0 not because the machine isn't run half-dot on the read path, but because the
eager read's T is EVEN by construction (re-sync to 4·M).** The `_1`/`_2` sub-dot
straddle the pairs need is NOT in the read's own phase — it is in the PPU dot
grid's ALIGNMENT to the CPU M-cycle grid, which for the dominant speedchange
family is set by the STOP speed-switch. The tier2 path carries that alignment in
`lcd_shift_dots` + `sb_dsa8` (the `double_speed_alignment` shadow) + the STOP
leave shift (`note_switch_leave`/`dsa_pause_correction`) — ALL `tier2_reclock`-
gated, OFF under EV. With the alignment absent, both members of a speedchange
pair render/read at the same whole dot, so no read-frame shift can split them —
only shuffle the family.

**The exact next lever (single, precise):** port the DS/STOP sub-dot ALIGNMENT
to the eager clock — gate `lcd_shift_dots` / `sb_dsa8` / the STOP leave-shift on
`tier2_reclock || eager_value`, and drive the eager PPU advance T-granularly
across a STOP realignment so the post-switch dot grid carries the odd half-dot
phase. THEN the speedchange `_1`/`_2` pairs sit at genuinely different half-dots
and a read-frame law can separate them (co-landing with the render-length half-
dot, HALFDOT Part A, for the pairs whose exit MOVES rather than whose read
position moves). The read peek is a downstream consumer of that alignment, not a
source — wiring it before the alignment exists is why it is inert. This supersedes
the prior map's "wire the half-dot read" as the single next lever.

Do NOT re-attempt the passive read peek in isolation (measured inert here).
Do NOT commit a swept `evshift` constant (measured overfit here). The evshift knob
+ Edit A + the peek are documented above for a 2-minute re-wire once the alignment
lands; the tree is reverted byte-identical meanwhile.

## Reproduction

`CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release
--no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head
-1)`. Sweep: `SLOPGB_EVSHIFT=<N> SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt
SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture
| grep pass=` (exact test path — `--ignored flagon_probe` races 3 tests).
Family delta: capture `^FAIL` lines, `awk '{print $2}'`, `comm` two N's.
