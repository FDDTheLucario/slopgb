# Eager wake-clock port — Step 1: the wake-MASK is inert; the CGB residual is the halt-woken IME=1 re-fetch, unreachable on the eager read frame (2026-07-10, #11cy)

Step 0 (#11cx) named "per-consult-site wake-mask frame calibration" as the lever
for the 5 CGB halt rows. Step 1 traced it to ground and **overturns that
hypothesis**: the wake mask (`stat_vis_from_t` / `mask_hidden_m0_stat`) cannot
distinguish the passing from the failing rows — they wake at the byte-identical
instant. The real discriminator is the halt-woken **IME=1 dispatch's FF41 read
frame**, which the deferred clock shifts +4 T (SameBoy's re-fetch M-cycle) and
the eager read frame structurally cannot. No code shipped; tree byte-identical at
`64ccf6c`. Baselines re-confirmed EV CGB **358**, EV DMG **85**, tier2 CGB
**291**; golden byte-identical.

## The TRUE bar is 5 rows — but not the 5 the map named

Bar = OFF-pass ∩ EV-fail ∩ SameBoy-pass. Classified the EV-CGB halt fails against
`sameboy_tester` (`~/.cache/sbbuild/…`, `classify_cgb_regr.py`, BUG=5 / FLOOR=0):

| # | row | want | eager got |
|---|---|---|---|
| 1 | `late_m0int_halt_m0stat_scx2_3a` | 0 | 2 |
| 2 | `late_m0int_halt_m0stat_scx3_3a` | 0 | 2 |
| 3 | `late_m0irq_halt_dec_scx2_2` | 6 | 7 |
| 4 | `late_m0irq_halt_dec_scx3_2` | 6 | 7 |
| 5 | `late_m0irq_halt_m0stat_scx3_3b` | 2 | 0 |

**Correction to #11cw / the task brief:** the row repeatedly named
`late_m0int_halt_m0stat_scx3_3b` is **NOT a bar row** — it is OFF-fail, **EV-PASS
already**, and SameBoy-PASS. It is the row the CGB entry peek *drops*. The bar's
`_3b` is the **m0irq** twin (`late_m0irq_halt_m0stat_scx3_3b`).

## Experiment A — the CGB entry peek recovers ALL 5, drops exactly 1

Un-scoping the #11cv eager entry peek from DMG to CGB-single-speed
(`halt_entry_impl`, `!self.model.is_cgb()` → `(!is_cgb || !double_speed)`):

- **EV CGB 358 → 354.** Recovers all 5 bar rows.
- Breaks **exactly one** row: `late_m0int_halt_m0stat_scx3_3b`.
- That row is **SameBoy-PASS** (classifier BUG=1) → a shipped slice may not drop
  it. Peek cannot ship alone. (Confirms #11cw's hold — now with the bar behind it,
  not ahead.)

The peek is right because both IME paths consult `halt_entry_impl`
(`op_halt`: IME=0 → `pending_halt_entry` → halt-bug; IME=1 → `halt_entry_rewind`
→ rewind). The rise-within-4 peek arms the entry view SameBoy sees at t0+4.

## The wake MASK is NOT the lever (Step-0's hypothesis, refuted)

Dual-traced `late_m0int_halt_m0stat_scx3_3a` (want0, PASS) vs
`…scx3_3b` (want2, the dropped row). **Byte-identical at every mask consult site,
both clocks:**

| clock | entry consult | wake consult (2nd round) | 3a screen | 3b screen |
|---|---|---|---|---|
| tier2 | ly1 dot260 clk5112 w=02 → rewind; dot336 w=00 → halt | **ly2 dot260 clk5568 svt5564 w=02** → wake | 0 ✓ | 2 ✓ |
| eager+peek | ly1 dot256 clk5104 w=02 → rewind; dot332 w=00 → halt | **ly2 dot260 clk5564 svt=— w=02** → wake | 0 ✓ | 0 ✗ |

3a and 3b **rewind identically and wake at the identical instant** (ly2 dot260,
w=02). A `stat_vis_from_t` deadline is a single number consumed at that one wake
site — it fires or does not fire the same for both rows. **No calibration of the
wake mask can separate 3a (out0) from 3b (out2).** The 0-vs-2 divergence is
entirely downstream of the wake.

Per-consult-site clk offsets (tabulated as the task asked, for the record —
they are moot for the blocker): entry site eager −8 clk / −4 dot vs tier2
(dot256/clk5104 vs dot260/clk5112); wake site eager −4 clk, **same dot** vs tier2
(dot260/clk5564 vs dot260/clk5568). The tier2 mask works only because its
`stat_vis_from_t = machine_now` is calibrated to those deferred positions; but
since the mask cannot discriminate the two rows at all, re-calibrating it to the
eager positions is a non-lever.

## The real discriminator — the IME=1 halt-woken FF41 read is one M-cycle early

Traced the actual CPU FF41 reads (probe on `Bus::read`'s `leading` path):

| row (eager+peek) | ISR/answer read | native mode | out |
|---|---|---|---|
| `m0int_3b` (IME=1, want2) | **ly2 dot452** val=0x88 | 0 (late HBlank) | 0 ✗ |
| `m0int_3a` (IME=1, want0) | ly2 dot448 val=0x88 | 0 | 0 ✓ |
| `m0irq_3b` (IME=0, want2) | ly2 dot0 val=0x8a | 2 (OAM) | 2 ✓ |

SameBoy's cc+4 view puts `m0int_3b`'s read at **ly3 dot0** (OAM, mode 2). The
eager read lands 4 dots (one M-cycle) early at ly2 dot452. Crucially **this is not
zero-sum**: `m0int_3a` reads at dot448, and +4 → dot452 is still mode 0, so the
shift fixes 3b without breaking 3a. The IME=0 `m0irq_3b` needs no shift — its wake
reuses the prefetch and reads ly2 dot0 (mode 2) directly.

The +4 is SameBoy's **re-fetch M-cycle**: the IME=1 halt wake performs a fresh
`cycle_read` (no reused prefetch), shifting the resumed stream +4 T. The deferred
clock replicates it with `carry_read(4)` (`halt_wake_mid_impl` g2 path + the
`cgb_any` arm). **The eager clock structurally cannot:**

1. **`carry_read` is inert for the eager read frame** (measured, the definitive
   result). Un-gating the `cgb_any` `carry_read(4)` to eager moves the wake clk
   5760 → 5764 (+4) but leaves the read at **PPU dot 452** — the eager read peeks
   `self.dot` directly (advanced whole-M-cycle by `tick_machine`), not via
   `clock.now()`. So the clock read-debt the deferred frame rides has zero effect.
   (Pins WHY #11cw saw the un-gate "byte-identical".)
2. **A read-VALUE peek over-fires.** Adding a read-law arm "carried m0-ISR read in
   the last 4 dots of a line → next-line OAM mode 2": the narrow form
   (`read_carried && stat_rise_m0`) never fires — at dot452 the halt-woken read
   has `read_carried=0` (consumed one-shot by an earlier ISR read at ly1 dot296),
   indistinguishable from a bare late-HBlank poll. The broad form (drop
   `read_carried`) OVER-FIRES: **EV CGB 358 → 361**, recovered 13 / broke 17 — the
   `want out0` `…_1a`/`…_a`/`m0int_m0stat_scx2_1` rows read legitimate late-HBlank
   mode 0 that the arm flips to mode 2.
3. **A real +4-dot PPU advance at the wake ticks the timers 4 T early** — the
   #11cv/TIMA objection (`int_hblank_halt`, the `dec` bar rows) and the
   thrice-refuted "move the dispatch dot".

## Verdict

The 5 CGB halt bar rows ARE reachable via the CGB entry peek, but the peek drops
`late_m0int_halt_m0stat_scx3_3b` (SameBoy-PASS), whose fix requires the eager
IME=1 halt-woken CGB dispatch to gain SameBoy's re-fetch **+4-dot stream shift**.
That shift is a **read-frame (dot) capability**, not a wake-mask calibration:

- The wake mask is inert — 3a/3b wake at the byte-identical instant (measured).
- `carry_read` (the deferred clock's tool) is inert for the eager read frame — the
  eager read peeks `self.dot`, not clock debt (measured).
- A read-value peek is indistinguishable from a bare poll at the read site and
  over-fires (EV CGB → 361, measured).
- A real dot advance is the refuted timer-early lever.

So the per-site calibration cannot be made consistent in the sense Step 1 needed:
`m0int_3a` and `m0int_3b` are **identical at every point the eager clock can
intervene** (entry identical, wake identical, read-site indistinguishable) yet
demand opposite outcomes. The only place they diverge — the +4-dot re-fetch read
frame — is exactly the whole-dot/half-dot capability the eager read lacks. This is
the **HALFDOT read-frame** for the IME=1 halt wake, not a Step-1 mask tweak.

**Bar unchanged: 49 CGB + 40 DMG = 89.** The 5 CGB halt rows stay pinned to giving
the eager halt-woken IME=1 dispatch a genuine +4-dot read shift (a dot-frame
re-fetch, `self.dot`-advancing but timer-safe) — the same HALFDOT capability
#11bw/#11cx flagged, now localized to one mechanism and one dropped row.

## What this retires

- **"The 5 CGB halt rows need a per-consult-site wake-mask calibration" (#11cx
  Step 0).** The wake mask cannot discriminate the rows — they wake identically.
  Do not re-chase `stat_vis_from_t` framing for the halt residual.
- **"The CGB `carry_read(4)` re-host is the wake-clock lever" (implied by #11cw).**
  `carry_read` is structurally inert for the eager read (dot-peeked). Measured: clk
  +4, read dot unchanged.

## Reproduce

```
# baselines: EV CGB 358 / EV DMG 85 / tier2 291 via flagon_probe (SLOPGB_PROBE_EV /
#   PROBE_EV+dmg_rowlist / PROBE_RECLOCK). NOTE: SLOPGB_PROBE_DMG is a no-op — the
#   DMG side is the [Dmg]-tagged dmg_rowlist.txt under SLOPGB_PROBE_EV.
# Exp A (peek, EV CGB 354, drops the SameBoy-PASS m0int_3b):
#   speed.rs halt_entry_impl peek: `!self.model.is_cgb()` → `(!is_cgb||!double_speed)`.
# bar classify: python3 docs/sameboy-port/tools/classify_cgb_regr.py <rowlist>
# read trace: probe on Bus::read `leading` for addr==0xFF41 (ppu.scan_pos + val),
#   run_gambatte + SLOPGB_EAGER=1 (add `gb.set_eager_value(true)`) + SLOPGB_S5DBG=1
#   + --features port_probe. All probes/knobs reverted; tree byte-identical.
```

## Gate state

No code shipped; `git diff` empty at `64ccf6c`; golden_fingerprint byte-identical
(40.2s, real run, `SLOPGB_REQUIRE_ROMS=1`). EV CGB 358 / EV DMG 85 / tier2 291
re-confirmed.
