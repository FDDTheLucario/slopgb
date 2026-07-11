# EAGER FF0F window-m0irq IF-delivery SHIPPED — the #11dn "poll-count" refutation OVERTURNED; it is a clean read-frame value-peek (EV CGB 297→295, EV DMG 59→56, zero drops both models) (2026-07-11, #11do)

Base: `finish-port-halfdot @ 9bce5ca` (#11dn). The 4 `m2int_wxA5/A6_m0irq_2`
rows the #11dn window-exit agent isolated and REFUTED as an irreducible
"FF0F/IRQ-timing loop-iteration count" that "needs a dispatch move (forbidden)".
Traced fresh (dual OFF/EV/tier2 `--features port_probe` on
`examples/run_gambatte.rs` + `SLOPGB_S5DBG`, every FF41 + FF0F read dot/value +
the `m0rise`/`dispatch` events); production + tier2 byte-identical.

---

## 0. Bottom line

**#11dn's poll-count read is REFUTED.** EV does the SAME number of FF41 polls as
production — 4 reads on ly=0 at dots 0/28/56/84, byte-identical to OFF. There is
NO loop-count divergence. The digit comes from ONE FF0F read on ly=1, and the
whole cluster is the ordinary read-frame value-peek shape (#11db bucket 1), NOT
a dispatch/timing effect. Shipped as arm **(a-eager)** in `Ppu::ff0f_stat_peek`.

| target row | model | want | EV before | EV after |
|---|---|---|---|---|
| `window/m2int_wxA5_m0irq_2` | Cgb | 2 | 0 | **2** ✓ BUG |
| `window/m2int_wxA5_m0irq_2` | Dmg | 2 | 0 | **2** ✓ BUG |
| `window/m2int_wxA6_m0irq_2` | Dmg | 2 | 0 | **2** ✓ BUG |
| `window/m2int_wxA6_m0irq2_2` | Dmg | 2 | 0 | **2** ✓ BUG |

Plus 1 incidental CGB **FLOOR** gain (`m0enable/enable_wxA6_2x_spxA7_1`, gambatte
want 2 / SameBoy 0 — EV now matches the gambatte reference). Net: EV CGB
**297→295** (−2), EV DMG **59→56** (−3); tier2 CGB 291 / DMG 116 unchanged;
OFF CGB 486 unchanged. Zero drops (NEW-fails empty) on BOTH rowlists.

---

## 1. The poll-count trace (EV vs tier2 vs OFF, `m2int_wxA5_m0irq_2` [Cgb])

FF41 reads on ly=0 (the polling loop) and the ly=1 FF0F read that produces the
digit, plus the `m0rise` (STAT-IF mode-0 rise) event:

```
OFF   (result 2 ✓): ff41 ly0 dots 0,28,56,84    | m0rise ly1 dot=261 | ff0f ly1 dot=264 v=e2 → 2
EV    (result 0 ✗): ff41 ly0 dots 0,28,56,84    | m0rise ly1 dot=261 | ff0f ly1 dot=260 v=e0 → 0
TIER2 (result 2 ✓): ff41 ly0 dots 24,52,80,108  | m0rise ly1 dot=259 | ff0f ly1 dot=260 v=02 → 2
```

**EV's ly=0 poll count is IDENTICAL to OFF (4 reads, same dots).** #11dn's
"EV 7-8 paired polls vs tier2 4 single polls" conflated the EV dots (0/28/56/84)
with the tier2 dots (24/52/80/108) — two DIFFERENT read frames of the same
4-iteration loop, not a count difference. The decisive quantity is the single
FF0F read on ly=1 and its position relative to the mode-0 STAT rise R:

- **OFF** reads at cc+4 (dot 264), rise R=261 → 264 > 261 → sees the bit (E2).
- **EV** reads at cc+0 (dot 260, the read-debt) but keeps the PRODUCTION rise
  R=261 → 260 < 261 → misses it (E0). **The middle case:** the early (cc+0) read
  of tier2 with the late (production) rise of OFF.
- **TIER2** reads at cc+0 (dot 260) AND back-dates the rise to R=259 (its
  render-length reclock) → 260 > 259 → sees it (02).

This is exactly `Ppu::ff0f_stat_peek`'s DELIVER shape — a verdict-only value
peek, no machine advance, no dispatch move. **The #11db "read-frame reachable"
bucket, not the write-commit or dispatch bucket.**

---

## 2. The discriminator — window back-dates R by 2, non-window does not

The naive first cut (deliver iff `dot+4 >= R && dot < R`, mirroring the DMG
arm (a-dmg)) recovered all 4 targets but BROKE ~19 rows/model (EV CGB 297→312,
DMG 59→75): every plain `m0irq_1` (`m2int_m0irq_1`, `_scx2_1`, `lyc0int/m0int`,
`enable_display/frame*_count`) which wants CLEAR. Re-tracing EV read/rise against
tier2's reclocked rise gave the true rule:

| row | want | EV rd | EV R (prod) | win_active | tier2 R (SameBoy) |
|---|---|---|---|---|---|
| wxA5_m0irq_2 | deliver | 260 | 261 | **1** | 259 = R−2 |
| wxA6_m0irq_2 (dmg) | deliver | 256 | 257 | **1** | 255 = R−2 |
| wxA5_m0irq_1 | clear | 256 | 261 | 1 | 259 |
| wxA6_m0irq_1 | clear | 252 | 256 | 1 | 254 = R−2 |
| m2int_m0irq_1 | clear | 252 | 254 | **0** | 254 (no back-date) |
| m2int_m0irq_scx2_1 | clear | 252 | 256 | **0** | 256 (no back-date) |

**tier2 rule = visible iff `read_dot >= SameBoy_rise`.** When a window is active
the SameBoy/tier2 mode-0 STAT source rises exactly 2 dots (half an M-cycle)
EARLIER than slopgb's window-elevated production flip (`flip_projection` adds the
window start cost, tier2's render-length reclock back-dates it by 2). NON-window
rows keep SameBoy_rise == production R (no reclock), so their gap is empty and
their `_1` polls stay clear natively. `win_active` is the discriminator; the
window `_1` siblings read one M-cycle earlier (below R−2) and also stay clear.

## 3. Mechanism (SHIPPED) — arm (a-eager) in `Ppu::ff0f_stat_peek`

```rust
// eager + SS + window-active + HBLANK-source + visible line
if self.eager_value && !self.ds && self.render.win_active
    && self.eng_stat & STAT_SRC_HBLANK != 0 && (1..=143).contains(&self.line)
{
    if let Some(r) = self.m0_flip_dot() {         // production flip R (shared helper)
        if self.dot + 2 >= r && self.dot < r {    // read in [R-2, R) → deliver
            return IF_STAT;
        }
    }
}
```

`m0_flip_dot()` is the flip-dot core factored out of `dmg_m0_if_rise` (recorded
`flip_dot` once fired, else `projected_flip_dot()`), shared by the tier2 DMG arm
and this eager arm. Verdict-only: `intf` and the R dispatch are untouched (the
bit still folds at R for the real machine); the peek only restores the read VALUE
SameBoy's cc+4 events-first frame already delivered. `eager_value`-gated → the
tier2 `read_deferred` caller and production (`eager_value` false) are inert →
byte-identical. DS is scoped out (`!self.ds` — the DS (a) arm owns the DS grid).

### Refuted first
- **#11dn's dispatch-move claim** — REFUTED: the `m0rise` fires at the IDENTICAL
  dot 261 on EV and OFF; nothing about dispatch position or poll count differs.
  It is purely the FF0F read VALUE at cc+0 vs the production rise.
- **`dot+4 >= R` (mirror the DMG arm (a-dmg))** — over-delivers: fires the
  non-window `m0irq_1` reads (cc+4 crosses the un-back-dated R). EV CGB 297→312,
  DMG 59→75. The window `win_active` gate + the `[R-2, R)` (not `[R-4, R)`)
  window is required.

---

## 4. Rows recovered (classified)

- CGB (2): `window/m2int_wxA5_m0irq_2` **BUG** (target); `m0enable/enable_wxA6_2x_spxA7_1`
  **FLOOR** (`classify_cgb_regr.py` → BUG=1 FLOOR=1).
- DMG (3): `m2int_wxA5_m0irq_2`, `m2int_wxA6_m0irq_2`, `m2int_wxA6_m0irq2_2` — all
  **BUG** (`classify_dmg.py` → BUG=3).
- A/B `comm -13` (NEW-fails) EMPTY on BOTH rowlists.

**Not recovered (out of scope, correctly left):** the CGB `m2int_wxA6*` variants
(`_m0irq_2`, `_m0irq2_2`, `_spxA7_m0irq_2`) — these FAIL OFF too (production reads
dot 260 < R 261, got 0). Their CGB off-screen-window flip R is elevated to 261
where SameBoy rises far earlier; that is a RENDER off-screen-window flip-position
defect (production-shared), not a read-frame peek. `m2int_wxA6_spxA7_m0irq_2`
[Dmg] likewise stays 0. Both are floor-class OFF-fails, not flip-regressions.

---

## 5. Gate results

| gate | result |
|---|---|
| `golden_fingerprint` byte-identical | **OK** (9020 cases, 43 s) |
| EV CGB | 297 → **295** (−2, 0 new-fails) |
| EV DMG | 59 → **56** (−3, 0 new-fails) |
| tier2 CGB / DMG | **291 / 116 IDENTICAL** |
| OFF CGB | **486 unchanged** |
| zero-regression CGB / DMG | NEW-fails EMPTY both rowlists |
| mooneye `acceptance_ppu` — default / RECLOCK / EAGER | **91/91 all three** (intr_2 tripwires incl.) |
| clippy `-D warnings` | clean |
| `.rs` < 1000 | ff0f.rs 279, eager_web.rs 349 |
| red-before-green pin | `eager_web::eager_window_m0irq_deliver_passes` — FAILS with the arm guard forced false (targets read 0), passes with the arm |

Pin covers the 4 BUG targets (both models) + 3 want-0 scope guards
(`m2int_wxA6_m0irq_1`, `m2int_m0irq_1`, `m2int_m0irq_scx2_1`) that a broadened
arm (dropped `win_active` gate or `[R-4, R)` window) would re-set.

---

## 6. What NOT to re-chase

- **"the m0irq rows are a dispatch/poll-count wall (#11dn)"** — REFUTED: EV's
  poll count == production's; the `m0rise` dot is identical EV↔OFF; it is a plain
  read-frame value-peek.
- **A `[R-4, R)` window (the DMG arm (a-dmg) shape)** — over-delivers the
  non-window `m0irq_1` rows. The window back-date is exactly R−2 and requires the
  `win_active` gate.
- **The CGB `m2int_wxA6_*` variants as read-frame** — they FAIL OFF; the fix is
  the render off-screen-window flip position (R elevated to 261), a separate
  RENDER lever, not this peek.

## 7. Reproduce

```
# baselines: SLOPGB_PROBE_{OFF,EV,RECLOCK}=1 SLOPGB_ROWLIST=$PWD/scratchpad/{cgb,dmg}_rowlist.txt
#   flagon_probe → OFF 486 / EV CGB 297 / tier2 291 / EV DMG 59 / tier2 116
# trace: run_gambatte + SLOPGB_EAGER=1 (or SLOPGB_TIER2=1) + SLOPGB_S5DBG=1 --features port_probe,
#   add FF41/FF0F read (dot,value) + win_active/flip prints to Bus::read (all reverted this session).
# classify: python3 docs/sameboy-port/tools/classify_{cgb_regr,dmg}.py <gambatte/-prefixed rows>
```
