# Eager IME=1 halt-wake re-fetch read shift — REFUTED: the eager whole-M-cycle wake collapses the want-0/want-2 discriminator to one read (2026-07-10, #11cz)

The #11cy-rev "untested narrow one-shot re-fetch lever" was built and measured to
ground. **It is refuted.** The one-shot `halt_refetch` flag fires exactly where the
map predicted, and its read-position arm recovers `late_m0int_halt_m0stat_scx3_3b`
(the row the CGB peek drops) — but it fires **identically** on a whole family of
SameBoy-PASS `want-out0` rows that occupy the **byte-identical eager read position**
as the `want-out2` rows it fixes. A full-keylist A/B drops **9 SameBoy-PASS rows** to
recover 13; net EV CGB stays **354**. No read-side lever can separate them: the
discriminator is the **wake instant** (tier2 dot 256 vs 260), which the eager
whole-M-cycle wake clock rounds to a single dot 260, landing both reads at the same
dot 452 / `read_pos_hd` 912. This is the **HALFDOT wake-frame wall** — the same one
#11cx/#11cy named, now localized to one read and proven with a mutual-exclusion
dual-trace. No code shipped; tree byte-identical at `3edb8d1`. Baselines re-confirmed
EV CGB **358**, EV DMG **85**, tier2 CGB **291**.

## Reproduced first (trust nothing)

- Baselines via `flagon_probe`: EV CGB **358**, EV DMG **85**, tier2 CGB **291**.
- CGB entry peek un-scoped (`halt_entry_impl`: `!is_cgb` → `(!is_cgb || !double_speed)`):
  EV CGB 358 → **354**. Full-keylist diff vs base — recovers **exactly 5**, breaks
  **exactly 1**:
  - recovered (the TRUE bar): `late_m0int_halt_m0stat_scx{2,3}_3a` (want0),
    `late_m0irq_halt_dec_scx{2,3}_2` (want6), `late_m0irq_halt_m0stat_scx3_3b` (want2).
  - broken: `late_m0int_halt_m0stat_scx3_3b` (want2, OFF-fail flip-gain, SameBoy-PASS).

  Identical to #11cy-rev. The peek cannot ship: it drops a SameBoy-PASS row.

## The flag as built

Three coupled pieces, all `eager_value`-gated (byte-identical off):

1. **Entry peek un-scoped to CGB single-speed** (`speed.rs::halt_entry_impl`) — the
   #11cv `stat_m0_rise_within(4)` block guard `!is_cgb` → `(!is_cgb || !double_speed)`.
2. **One-shot `halt_refetch` flag on the PPU**, armed in `halt_wake_mid_impl`'s
   `cgb_any` arm (`speed.rs:100-108`) when `eager_value && cgb_any && (w & IF_STAT &&
   stat_rise_m0())` — precisely the "just woke IME=1 CGB on the m0-STAT rise" event
   the `carry_read(4)` re-fetch keys on. Not `read_carried`; a dedicated bool.
3. **Read-position arm** in `vis_mode_read` (after the line-start OAM back-date): when
   `halt_refetch && m==0 && line∈1..144 && !glitch && read_pos_hd() >= LINE_DOTS*2`,
   return **2**. Cleared on the boundary-crossing read (in `Bus::read`/`read_inc`) and
   at the next halt entry (`set_cpu_halted` backstop).

**Key correction to #11cy-rev's mechanism:** the map said "resolve at `read_pos_hd +
8hd`". Measured, `read_pos_hd` **already** carries the +8hd (cc+4) SS debt — the eager
read at ly2 dot452 has `read_pos_hd = 2·452 + 8 = 912 = ly3 dot0` (SameBoy's cc+4
view). `carry_read` is inert for eager precisely because the debt is already in
`read_pos_hd`, not because a shift is missing. So the arm needs **no extra shift**:
`read_pos_hd >= LINE_DOTS*2` (912) already means "crossed the line boundary into the
next line's OAM". Adding another +8 would over-shoot and mis-fire `_a` rows the other
way. The flag is what confines the arm to the one halt-woken read (a bare late-HBlank
poll at `read_pos_hd` 912 must stay mode 0 — the broad-peek 358→361 over-fire).

The flag fires exactly as intended: rf=1 on the IME=1 `m0int` rows, rf=0 on the IME=0
`m0irq` answer read (traced — the IME=0 wake reads next-line OAM natively at ly dot 0).

## The full-keylist A/B (the warning the map named — the flag leaks by construction)

EV CGB with peek + refetch arm: **354** (unchanged from peek-alone). But the row set
churns a whole family:

**vs the peek baseline** — recovers 10, breaks 10:

| recovered (want-out2, `scx3`/`scx4`) | broken (want-out0, `scx2`/`scx5`) |
|---|---|
| `late_m0int_halt_m0stat_scx3_{1,2,3,4}b` | `late_m0int_halt_m0stat_scx2_{1,2,3,4}a` |
| `late_m0irq_halt_m0stat_scx3_{1,2}b` | `late_m0irq_halt_m0stat_scx2_{1,2}a` |
| `m0int_m0stat_scx3_2`, `m0int_m0stat_scx4_2` | `m0int_m0stat_scx2_1`, `m0int_m0stat_scx5_1` |
| `m0irq_m0stat_scx3_2`, `m0irq_m0stat_scx4_2` | `m0irq_m0stat_scx2_1`, `m0irq_m0stat_scx5_1` |

**SameBoy verdict on the 10 broken (via `sameboy_tester`, `classify_cgb_regr.py`):
BUG=9 / FLOOR=0** on the vs-base drop set — **every dropped row is SameBoy-PASS.** A
shipped slice may not drop one. (The recovered set is BUG=13 / FLOOR=0, all
SameBoy-PASS gains — the arm is genuinely doing the right thing for the `_b`/`_2`
rows; it just cannot avoid the `_a`/`_1` collateral.)

The pattern is exact: the arm flips the eager read at **dot 452 / `read_pos_hd` 912**
to mode 2. That is the correct answer for `scx3_*b`/`scx4_2` (want2) and the WRONG
answer for `scx2_*a`/`scx5_1` (want0) — **the two land on the same eager read**.

## The mutual-exclusion dual-trace (why no read-side lever exists)

Two independent pairs, each a `want-out0` row and a `want-out2` row that are
**byte-identical at every eager-intervenable point** yet demand opposite outputs:

### Pair 1 — the `late_` halt family

| row (eager) | want | entry rewind | wake | answer read | native mode |
|---|---|---|---|---|---|
| `late_m0int_halt_m0stat_scx2_3a` | **0** | ly2 dot256→332 w=02 | ly2 dot260 clk5564 | **ly2 dot452 `rph`912** | 0 |
| `late_m0int_halt_m0stat_scx3_3b` | **2** | ly2 dot256→332 w=02 | ly2 dot260 clk5564 | **ly2 dot452 `rph`912** | 0 |

### Pair 2 — the non-halt `m0stat` family

| row (eager) | want | wake | answer read | native mode |
|---|---|---|---|---|
| `m0int_m0stat_scx2_1` | **0** | ly1 dot260 clk5108 | **ly1 dot452 `rph`912** | 0 |
| `m0int_m0stat_scx3_2` | **2** | ly1 dot260 clk5108 | **ly1 dot452 `rph`912** | 0 |

In both pairs the ONLY difference at the flagged read is the inert `SCX` register (2
vs 3). `line`, `dot`, `read_pos_hd`, the wake dot, the entry rewind, `halt_refetch` —
all identical. The mode-0→2 line-wrap is at dot 456 (`read_pos_hd` 912),
SCX-independent, so any principled read-position law gives both the SAME verdict. A
SCX-value gate would be a test-ROM special-case (forbidden) and, from two data points
where 1 SCX = a 4-dot read swing, does not describe a hardware boundary anyway.

**tier2 separates the pairs — at the WAKE, not the read:**

| pair-1 row | tier2 wake | tier2 read (via `carry_read(4)`) | tier2 out |
|---|---|---|---|
| `scx2_3a` | ly2 dot **256** clk5564 | dot 448 → mode 0 | 0 ✓ |
| `scx3_3b` | ly2 dot **260** clk5568 | dot 452 → mode 0→(+4)→ mode 2 | 2 ✓ |

tier2's deferred wake resolves the two at **dot 256 vs 260** (a 4-dot / one-M-cycle
gap — the ROM's per-SCX code-path difference the deferred clock preserves), so the
answer reads land 4 dots apart (448 vs 452) and `carry_read(4)` lifts each correctly.
The **eager whole-M-cycle wake collapses 256 and 260 to a single dot 260** (the halt
loop samples on the 4k grid), landing BOTH answer reads at dot 452. From that point no
downstream read law can recover a distinction that no longer exists in the frame.

**This is a superset of #11cy's wake-mask refutation.** #11cy showed a single
`stat_vis_from_t` deadline cannot separate rows that wake at the byte-identical eager
instant. This shows the **read-position** lever cannot either, for the same root
cause: the discriminator lives in the wake INSTANT, and the eager wake clock has
whole-M-cycle resolution where tier2 has sub-M-cycle. The `_3b` twin the peek drops
(`late_m0int_halt_m0stat_scx3_3b`, want2) and the bar row the peek recovers
(`late_m0int_halt_m0stat_scx2_3a`, want0) are **the same eager read** — there is no
setting of "what a dot-452 halt-refetch read returns" that satisfies both.

## Verdict — Part-C, unreachable on the eager read frame

- The CGB entry peek stays **DMG-scoped as shipped** (#11cv). Un-scoping it recovers
  the 5 CGB halt bar rows but drops `late_m0int_halt_m0stat_scx3_3b` (SameBoy-PASS);
  no read-position lever fixes that drop without dropping ≥9 other SameBoy-PASS rows,
  because the fix and the drop share the eager read position.
- **Bar unchanged: 49 CGB + 40 DMG = 89.** The 5 CGB halt rows are pinned to the
  **eager wake-clock port** — give the eager halt wake a sub-M-cycle (half-dot) wake
  instant so `scx2_3a` and `scx3_3b` wake at dot 256 vs 260 (as tier2 does) and their
  reads separate to dot 448 vs 452, at which point the `read_pos_hd >= LINE_DOTS*2`
  arm above fires correctly with zero collateral. That is the multi-session HALFDOT
  wake rebuild #11cx measured net-negative from the pre-#11cv baseline and #11cw
  found welded to the deferred `machine_now`; it is the genuine remaining lever, not a
  read-frame tweak.

## What this retires

- **"The narrow one-shot `halt_refetch` read shift can separate the `_3b` rows"
  (#11cy-rev, the untested lever).** Built and measured: it fires identically on the
  `want-0` `_a`/`_1` family (SameBoy-PASS) that shares the eager read position. The
  read-position vein for the CGB halt residual is now EXHAUSTED alongside the
  wake-mask (#11cy) and `carry_read` (#11cw) veins.
- **The mechanism note "`read_pos_hd + 8hd`" (#11cy-rev).** `read_pos_hd` already
  carries the cc+4 debt (measured `rph`=912 at ly dot 452); the arm needs the boundary
  test on the existing value, not a second shift.

## Reproduce

```
# baselines: EV CGB 358 / EV DMG 85 / tier2 291 via flagon_probe
#   (SLOPGB_PROBE_EV [+ dmg_rowlist for DMG] / SLOPGB_PROBE_RECLOCK), SLOPGB_REQUIRE_ROMS=1.
# peek un-scope (EV CGB 354, recover 5 / break 1):
#   speed.rs halt_entry_impl peek guard `!is_cgb` -> `(!is_cgb || !double_speed)`.
# refetch arm (EV CGB 354, recover 13 / break 9 vs base, ALL SameBoy-PASS):
#   PPU field halt_refetch; arm in halt_wake_mid_impl cgb_any arm (eager_value && cgb_any
#     && w&IF_STAT && stat_rise_m0 -> ppu.set_halt_refetch(true)); vis_mode_read arm
#     (halt_refetch && m==0 && line 1..144 && !glitch && read_pos_hd()>=LINE_DOTS*2 -> 2);
#     clear on halt_refetch_crossed() FF41 read + set_cpu_halted backstop.
# dual-trace: run_gambatte + SLOPGB_EAGER=1 (add gb.set_eager_value(true)) + SLOPGB_S5DBG=1
#   + --features port_probe; ff41rd probe in leading_edge_sample (ly/dot/read_pos_hd).
# SameBoy classify: python3 docs/sameboy-port/tools/classify_cgb_regr.py <rels>
#   (~/.cache/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester).
# All probes/arms REVERTED; tree byte-identical at 3edb8d1.
```

## Gate state

No code shipped; `git diff` empty at `3edb8d1`. Baselines re-confirmed after revert:
EV CGB **358**, EV DMG **85**, tier2 CGB **291**. Bar unchanged **49 CGB + 40 DMG =
89**. The 5 CGB halt bar rows stay pinned to the eager wake-clock port (sub-M-cycle
wake instant), the sole surviving lever after the read-position vein joins the
wake-mask and `carry_read` veins as exhausted.

---

## REVIEWER VERIFICATION (#11cz-rev): the mutual-exclusion is REAL — traced independently

Dual-traced `scx2_3a` (want0) vs `scx3_3b` (want2) under tier2 and eager, own
build:

| | tier2 entry / re-entry / WAKE | tier2 screen | eager entry / WAKE | eager screen |
|---|---|---|---|---|
| `scx2_3a` want0 | ly1 256 / 332 / **ly2 dot256** | `0` ✓ | ly1 252 / **ly1 dot260** | `2` ✗ |
| `scx3_3b` want2 | ly1 260 / 336 / **ly2 dot260** | `2` ✓ | ly1 256 / **ly1 dot260** | `2` |

**Confirmed:** tier2 separates the two rows by a **4-dot gap at the wake instant**
(ly2 dot 256 vs 260) — the SCX difference (2 vs 3) shifts the mode-0 rise, the
entry, and the wake all by ~4 dots, and tier2's sub-M-cycle wake resolves it. The
eager whole-M-cycle wake collapses BOTH to dot 260, so both read the same mode and
emit the same digit. The read position (`read_pos_hd`) is identical for the two
rows — the read-shift vein cannot possibly separate them; the discriminator is
upstream, at the wake sample, at half-M-cycle resolution.

The refutation stands and is a genuine #11cp-style wall for the wake frame. Three
veins are now exhausted for the 5 CGB halt rows, all failing for the SAME reason
(the discriminator is the sub-M-cycle wake instant eager quantizes away):
1. wake-mask (`stat_vis_from_t`) — #11cy;
2. read-position / `carry_read` / one-shot re-fetch flag — #11cz;
3. entry peek alone — breaks the row (#11cw).

**The ONLY remaining lever for the 5 CGB halt rows is the sub-M-cycle wake clock**
— eager sampling the halt-wake at half-M-cycle (4-dot) resolution, the eager
analogue of tier2's `halt_wake_mid_impl` 4k+2 sample. `Ppu::tick_half`/`dhalf`
exist; the wake would key on the rise's within-M-cycle half. This is a
HALFDOT-magnitude build (same class as the render FSM), now PROVEN necessary
rather than conjectured — but for only 5 rows.

**Recommendation (strategic):** the #11cs flip-bar composition shows two
higher-yield levers untouched — L1 (CGB DS re-host, ~19 rows, mechanical
`!self.ds` un-scope of already-proven slices) and L2 (DMG window/`late_wy`
re-host, ~23 rows, the proven `|| eager_value` pattern). Both are re-host work
against shipped mechanisms, not new physics. Prefer them over a HALFDOT-magnitude
wake-clock build for 5 rows. The 5 CGB halt rows stay pinned to the sub-M-cycle
wake clock as the last, most expensive slice before the C3 flip.
