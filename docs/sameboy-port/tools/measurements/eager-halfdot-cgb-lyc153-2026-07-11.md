# HALFDOT wall-1 CGB `ly_lyc_153_write` arm — REFUTED at PREMISE: the target ROMs write FF45 (LYC), never the FF41 two-phase `eng_stat_pending` the task named; the divergence is the piece-2 LYC re-latch dot (DMG, a measured +5/+17 family shuffle == #11dv) + read-frame coherence (CGB), neither reachable by the odd-half `eng_stat_half` substrate (2026-07-11, #11dx)

Base: `finish-port-halfdot @ 8d6bf0d` (= #11dw substrate: `stat_update_half`,
`eng_stat_half`). **NO CODE SHIPPED — tree byte-identical @ 8d6bf0d.** All
experiments were env-gated / reverted; every measurement below is on the base
tree except the two labelled A/B experiments.

## TL;DR — REFUTE (premise mismatch + measured shuffle)

The task hypothesis was: *"the 6 CGB `ly_lyc_153_write` rows need the two-phase
`eng_stat_pending` resolved on the odd half."* Three independent measurements
refute it:

1. **The target ROMs never write FF41.** `ly_lyc_153_write-{C,GS}` write **FF45
   (LYC)=153** on line 153 (traced). `eng_stat_pending` (the FF41 two-phase
   engine view) is armed ONLY by FF41 writes (`regs.rs:510`) → it is never armed
   for these ROMs. There is no two-phase to resolve on the odd half. The
   odd-half `eng_stat_half` substrate (#11dw) is the FF41-write-COMMIT sub-dot
   (piece 4); these rows have no FF41 write.

2. **The DMG-family half (GS×4) IS the piece-2 LYC re-latch dot — and moving it
   is a measured family shuffle, exactly #11dv.** Back-dating the line-153
   `ly_for_comparison` re-latch dot 6→4 (eager-gated) recovers `ly_lyc_153_write-GS`
   but regresses **EV DMG 52→57 (+5)** and **EV CGB 295→312 (+17)** — the +17 CGB
   is #11dv's piece-2 refutation reproduced to the row.

3. **The CGB half (C×2) is NOT the re-latch dot at all — it is read-frame
   coherence.** `Model::Agb` already re-latches LYC=153 at dot 4 (the `model >
   CGB_C` arm, `reclock.rs:891`) yet `ly_lyc_153_write-C` [Agb] still FAILS
   B=48. The eager engine fires the sync STAT at the SameBoy dot but reads FF44/FF41
   at cc+0 while SameBoy reads cc+4 → an incoherent measurement frame. This is
   the coherent per-T retime (HALFDOT Part A / C3-flip), not a flag-gated law.

This matches the INDEPENDENT classification in
`eager-nongambatte-rehost-2026-07-11.md` (Group D): *"All six models break with
B=48 … tier2 ALSO fails … a read-law `|| eager_value` cannot move a
counter-pinned dispatch/read frame … Lands with the C3-flip per-T retime, not a
flag-gated law port."*

## The rows — production PASSES, the leading-edge frame REGRESSES them

The "6 CGB" are **2 ROMs × model** (mooneye-test-suite-wilbertpol,
`acceptance/gpu/`), NOT gambatte-OCR rows (absent from `{cgb,dmg}_rowlist.txt`):

| ROM | models | FF45 path | OFF | LE | tier2 | eager |
|---|---|---|:--:|:--:|:--:|:--:|
| `ly_lyc_153_write-C` | Cgb, Agb | `write_lyc_cgb` | PASS | B=48 | B=48 | B=48 |
| `ly_lyc_153_write-GS` | Dmg, Mgb, Sgb, Sgb2 | `write_lyc_dmg` | PASS | B=48 | B=48 | B=48 |

`PASS` = Fibonacci `B,C,D,E,H,L = 03,05,08,0D,15,22`. **Production (OFF) passes
all 6.** Every `leading_edge_reads` frame (LE-only, tier2, eager) fails the
IDENTICAL `B=48 C=BE D=02 E=FC H=FF L=40` — so the fault is the leading-edge
frame itself, shared by all three, NOT an eager-only or odd-half concern. These
are Group-D eager REGRESSIONS of production-passing rows, not wall-1 gambatte
blockers.

## The mechanism (traced, `run_mooneye --features port_probe`, `SLOPGB_S5DBG`+read/ACK trace)

The ROM writes `FF45 = 153` on line 153, then syncs on the resulting
LYC-coincidence STAT interrupt and cycle-counts a measurement loop.

| frame | sync STAT fires (ly=153) | STAT ISR ACK (ly=153) |
|---|:--:|:--:|
| OFF (gambatte `stat_events_tick`) | dot **4** | dot **16** |
| eager (`GB_STAT_update`) | dot **6** | dot **20/28** |

- OFF: `compare_ly_irq` matches `Some(153)` at line-153 **dots 4-7** → fires dot 4.
- eager: `ly_for_comparison_line_153_at` (CGB-C/DMG single speed) latches 153 at
  **dots 6-7** (SameBoy `GB_SLEEP(14,4)`, `reclock.rs:899-910`) → fires dot 6.

The 2-dot-late sync shifts the whole downstream cycle count → the ROM lands in a
different measurement round → `B=48`.

### Ruled out — `lyc_if_delay` (the FF45 CGB delivery delay)

`write_lyc_cgb` sets `lyc_if_delay = 4` on a firing FF45 write. Sweeping it
`4→3→2→1→0` (`tune_engcommit`) changed the `-C` result by **nothing** (still
B=48). The `fire`-path delay is not the sync source; the sync is the
`ly_for_comparison` LYC re-latch in `stat_update_tick`.

## The two A/B experiments (both reverted)

**Experiment 1 — `lyc_if_delay` sweep (`-C` [Cgb], eager):** delay ∈ {0..4} →
B=48 unchanged. Not the lever.

**Experiment 2 — line-153 `ly_for_comparison` re-latch back-date dot 6→4**
(eager-gated, `SLOPGB_LYFC153`):

| row / metric | base | back-dated | Δ |
|---|:--:|:--:|:--:|
| `ly_lyc_153_write-GS` [Dmg] | B=48 | **PASS** | recovered |
| `ly_lyc_153_write-C` [Cgb] | B=48 | B=48 | **no change** |
| `ly_lyc_153_write-C` [Agb] | B=48 | B=48 | **no change** |
| EV DMG (dmg_rowlist) | 52 | **57** | **+5 REGRESS** |
| EV CGB (cgb_rowlist) | 295 | **312** | **+17 REGRESS** |

The DMG-family recovery costs a +5/+17 gambatte family shuffle — a strict
one-sided A/B swap on the line-153 LYC frame the gambatte `lycEnable` cluster is
calibrated to (dot-6 SameBoy `ly_lyc_153-C` pins the base). This is #11dv's
piece-2 refutation, reproduced (+17 CGB to the row). The CGB rows are not even
touched by it (Agb already at dot 4).

## Why the odd-half `eng_stat_half` substrate cannot reach either half

- **Piece 4 (FF41 write-commit, what #11dw built):** never armed here — no FF41
  write. Inapplicable.
- **Piece 2 (LYC re-latch dot):** #11dw already stated it is at WHOLE dot 6 with
  **no sub-dot phase** — there is no odd half to resolve it onto, and moving the
  whole-dot value shuffles the family (Experiment 2 = the proof).
- **CGB read-frame coherence:** the sync fires at the correct SameBoy dot but the
  measurement reads sample cc+0 vs SameBoy's cc+4 — a whole-frame coherence
  problem the odd-half engine (which re-evaluates the STAT *level*, not the CPU
  read frame) does not address.

## Gates (all hold — no code shipped)

1. `golden_fingerprint` byte-identical (verified at base; tree unchanged @ 8d6bf0d).
2. EV CGB **295**, EV DMG **52** — reproduced at base, unchanged (no ship).
3. tier2 CGB **291**, tier2 DMG **116** — inherited from base (byte-identical tree).
4. mooneye 93×3 — inherited (byte-identical tree).
5. No file grew → no split needed.

## Residual (the real path)

`ly_lyc_153_write ×6` land with the coherent per-T dispatch/read retime
(HALFDOT Part A / C3-flip): fire the sync at the SameBoy dot-6 re-latch AND make
the ROM's downstream FF44/FF41 reads cc+4-coherent, in ONE frame. Neither the
odd-half STAT-level engine nor any single flag-gated read/write law separates
them — Experiment 2 (piece-2) shuffles, `lyc_if_delay` is inert, and the CGB
half is read-frame, not engine-level. Do NOT re-chase a flag-gated odd-half
port of these rows.
