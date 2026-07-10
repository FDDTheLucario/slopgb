# EAGER ack-squash port — SHIPPED: the post-ack STAT-retrigger squash re-hosted onto the eager read-frame (EV CGB 330→325, DMG 78→74, all recovered rows BUG, zero-regression both models) (2026-07-10, #11de)

Task (#11de): port the `irq_precedence` post-ack STAT-retrigger rows the #11dd
write-commit map deferred as "an ack-squash miss, NOT write-commit". Lever:
`interconnect/speed.rs::ack_impl`, the LCD-bit (0|1) `ack_squash_dots` window.
Flag-gated behind `eager_value`; production + tier2 byte-identical.

## Baselines reproduced (Y — exact)

| frame | CGB | DMG |
|---|---|---|
| OFF (`SLOPGB_PROBE_OFF`) | 486 | — |
| EV (`SLOPGB_PROBE_EV`) | **330** | **78** |
| tier2 (`SLOPGB_PROBE_RECLOCK`) | 291 | — |

`flagon_probe` on the 3422-row `scratchpad/{cgb,dmg}_rowlist.txt`.

## The bar — the `_2` retrigger family under EV (before)

All ten `*_late_retrigger_2` rows fail under EV (want the retrigger SQUASHED,
got it DELIVERED = E2/2/1). But only the four `irq_precedence` rows PASS under
OFF (production 2-dot window) and tier2 — they are the EV-specific regressions.
The LYC/mode-1/vblank `_2` rows fail under OFF too (not EV-specific; out of
scope). All `_1` siblings PASS under EV (deliver, correct) — the keep-set.

## Trace (dual-trace OFF/EV/tier2, `irq_precedence/late_m0irq_retrigger_2`, ly1)

Instrumented `ack_impl` (ACK t) + `fold_ppu_events` (retrigger STAT fold t,
`dot_squash`), probes since reverted. `t` is absolute PPU dots (1 dot = 1 T @
4 MHz; a scanline = 456).

| clock | ISR ack (bit 1) | mode-0 retrigger fold | ack→retrigger | window | verdict |
|---|---|---|---|---|---|
| OFF `_2` (pass) | dot 252, **t=5100** | dot 254, t=5104 | 4 dots | `adots=2`, squashed (dotsq=02) | **E0** ✓ |
| EV `_2` (fail) | dot 248, **t=5096** | dot 254, t=5104 | **8 dots** | `adots=2` expired | E2 ✗ |
| EV `_1` (keep) | dot 244, t=5092 | dot 254, t=5104 | 12 dots | expired | E2 ✓ |
| tier2 `_2` (pass) | dot 254, **t=5106** | dot 254, t=5106 | 0 (co-instant) | ack clears the co-fold | E0 ✓ |

**Root cause.** The eager read-frame enters the STAT/OAM ISR — and so fires this
ack — the read-debt EARLIER than gambatte's cc+4 frame the production `2` is
tuned to (OFF ack t=5100 → EV ack t=5096, the +8hd = 4-dot SS #11by cc+0→cc+4
shift; the OAM rise that starts the ISR is `oamrise=1` at the EV ack, `=0` at
OFF). The mode-0 retrigger is a PPU event pinned to the same absolute dot
(t=5104), so the eager ack→retrigger gap grows from 4 (OFF, inside the 2-dot
window) to 8 dots (EV, outside it) → the retrigger re-delivers. tier2 squashes
`_2` by a DIFFERENT mechanism entirely — the deferred dispatch lands the ack
co-instant with the retrigger (t=5106) and the bare IF clear eats it; the eager
clock structurally cannot reproduce that (dispatch fixed at cc+4, must not
move).

## Refuted first (the task's named traps)

- **Naive mirror of tier2** (`ack_squash_dots=0` + `arm_ack_squash` under eager):
  EV CGB 330→**336** (WORSE). The PPU-frame `ack_squash_ppu=2` window is a 2-dot
  countdown that (a) never reaches the 8-dot-later retrigger, and its SS-m0
  `w_ack=0` (reclock.rs) never consumes an SS m0 rise anyway; (b) dropping
  `ack_squash_dots=2` lost 6 co-instant squashes. Reverted.
- **Full-shift DS window (4)**: EV CGB 330→**330** net-zero — recovers the DS
  `_2` rows but the LYC/mode-2/mode-1/vblank DS `_1` retriggers of the OTHER
  families (ly0/m1/m2int/lyc153int) sit one dot inside window 4, over-squashed
  (−6 BUG rows). DS family conflict — the m0 `_2` and non-m0 `_1` retriggers are
  not whole-dot separable at a single DS window.

## Mechanism (SHIPPED)

Widen the eager LCD-bit `ack_squash_dots` window by the read-frame shift:

```rust
self.ack_squash_dots = if self.tier2_reclock { 0 }
    else if self.eager_value { if self.double_speed { 3 } else { 6 } }
    else { 2 };
```

- **SS = 6** (`2` + the 4-dot read-debt): recovers all SS `_2` targets and the
  mode-2 SS bonus family; the one-M-cycle-later `_1` siblings (gap 12) stay
  outside and DELIVER. Zero SS regressions across all 3422 rows.
- **DS = 3** (`2` + 1, NOT the full +2): recovers `late_m0irq_retrigger_ds_2`
  while every DS `_1` of the other families stays delivered.
  `late_m0irq_retrigger_scx1_ds_2` (its +1-dot retrigger needs window 4) stays
  PARKED — window 4 over-squashes the other-family DS `_1` rows (−6). One DS
  target traded for six DS keeps.

Window sweep (fast single-ROM, 8 `irq_precedence` rows): the SS/DS split is the
only value pair that gets all keeps; no single window separates SS from DS.

## Rows recovered (EV before→after)

- **EV CGB 330 → 325**, +5 recovered, **0 regressions**. All 5 = **BUG**
  (`classify_cgb_regr.py` → `BUG(sb==want)=5 FLOOR=0`):
  1. `irq_precedence/late_m0irq_retrigger_2` (SS) — primary target
  2. `irq_precedence/late_m0irq_retrigger_scx1_2` (SS) — primary target
  3. `irq_precedence/late_m0irq_retrigger_ds_2` (DS) — target
  4. `lyc153int_m2irq/lyc153int_m2irq_late_retrigger_2` (SS) — bonus
  5. `m2int_m2irq/m2int_m2irq_late_retrigger_2` (SS) — bonus
- **EV DMG 78 → 74**, +4 recovered, **0 regressions** (the 4 SS rows above;
  DMG has no DS variant).
- PARKED: `irq_precedence/late_m0irq_retrigger_scx1_ds_2` (DS +1-dot retrigger,
  window-4 conflict with 6 other-family DS `_1` keeps).

## `_1` keep-set verified DELIVERED (want E2)

All eight `irq_precedence` `_1`/`_ds_1`/`scx1(_ds)_1` rows + the six other-family
DS `_1` rows (ly0/m1/m2int/lyc153int) stay E2/2/3/1 under the shipped 6/3 window
(single-ROM verified + confirmed in the full A/B: the CGB and DMG NEW-fail sets
are EMPTY).

## Gates (all green)

| gate | result |
|---|---|
| golden_fingerprint | byte-identical (42.25s) |
| EV CGB | 330 → **325** |
| tier2 CGB | **291** (unchanged) |
| EV DMG | 78 → **74** |
| zero-regression CGB / DMG | NEW-fails EMPTY on both |
| mooneye ppu OFF / EAGER / RECLOCK | 91/91 all three |
| intr_2 (0/mode0/mode3/sprites/oam_ok) eager | PASS Cgb + Dmg |
| clippy `-D warnings` | clean |
| file sizes | speed.rs 626, tick.rs 538 (< 1000) |
| red-before-green pin | `eager_ack_squash_retrigger_passes` — FAILS with window reverted to 2 |

Pin: `crates/slopgb-core/tests/gbtr/gambatte/eager_web.rs`.
