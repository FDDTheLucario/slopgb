# HALFDOT Part-A, EAGER clock — the emergent-flip accessibility release SHIPPED (EV CGB 304→298, EV DMG 66→62, clean flag-gated) (2026-07-11, #11dm)

Base: `finish-port-halfdot @ 8f2b069`. The narrow Part-A lever from
`eager-partA-buildplan-2026-07-10.md` (#11dh) §5 — the emergent half-dot
exit-record that bypasses the `mode0.rs` `early_lead` case-tower for the eager
accessibility residual. Built, measured, shipped flag-gated; production + tier2
byte-identical (`golden_fingerprint` ok twice, 41.7s / 49.2s).

---

## 0. The bottom line

The #11dh re-scoping is CONFIRMED and the residual is SMALLER + CLEANER than the
"half-dot render FSM" the prior maps scoped. **No render code was touched.** The
mode-3 exit-record grain never needed to move: `flip_hd` = `2*self.dot +
self.dhalf` is degenerate on the eager clock (`dhalf` is always 0 inside the
render — `tick_half` runs the whole-dot body only on the completing half), so
the "emergent half-dot flip" IS just `2 * projected_flip_dot()`. The sub-dot
resolution the accessibility read needs is supplied entirely by the READ frame
(`read_pos_hd`'s +8 hd SS read-debt), which already exists. So the fix is a
`&self` introspection compare in the accessibility predicates — golden-safe by
construction, no field added, no `m0_flip_events` change, no case-tower deletion.

The `early_lead` case-tower is BYPASSED (not deleted) under eager: the three
accessibility sites stop consuming the eager `vis_early` boolean (which fires off
the LE `early_lead = 3` residue, 2 dots too early → #11dg over-release) and
consume the emergent-flip compare instead. `vis_early` is left intact
(tier2-scoped consumers unchanged); it is simply no longer read by the eager
accessibility path.

---

## 1. The edit (`ppu/blocking.rs`, ~+42 lines, no render touch)

New `Ppu::eager_access_released(&self) -> bool` — the eager twin of the
tier2 `vis_early` release, keyed to the render's own projection:

```rust
fn eager_access_released(&self) -> bool {
    self.eager_value && !self.ds && !self.glitch_line
        && (1..144).contains(&self.line)
        && self.render.active
        && !self.render.win_active && !self.render.win_stalled && !self.render.win_aborted
        && self.render.n_sprites == 0
        && !self.wy_latch && self.wy2 != self.ly
        && self.read_pos_hd() >= 2 * i32::from(self.projected_flip_dot()) + 6
}
```

Wired into three release sites, each `eager_value`-gated:
- `oam_read_blocked`: `&& !self.eager_access_released()`
- `vram_read_blocked`: `|| self.eager_access_released()`
- `write_unblocked_early`: `|| self.eager_access_released()` (feeds
  `oam_write_blocked` + `vram_write_blocked`)

The bare-line SS guards mirror `vis_exit_hd` arm 8. The `+6` hd constant is the
OAM/VRAM `m0Time` accessibility lag past the mode-0 flip (SameBoy's
`m0Time = xpos lcd_hres+7` vs the IRQ at `+6`; the interconnect's
`tick.rs::take_m0_access_flip` comment describes the same trail).

---

## 2. The flip_hd whole-vs-half trace at the mode-3 exit (own probe, reverted)

`vram_m3/postread_scx3_2` [Cgb], ly=1, SCX=3, own probe in
`{oam,vram}_read_blocked` (`PROBE … rphd vearly lrd fdot`):

```
EAGER _2 dot=256 dh=0 rphd=520 vearly=true  lrd=false fdot=0   (want OPEN)
TIER2 _2 dot=256 dh=0 rphd=512 vearly=true  lrd=false fdot=0   (want OPEN, passes)
EAGER _1 dot=252 dh=0 rphd=512 vearly=false lrd=false fdot=0   (want BLOCKED)
```

`dhalf` is 0 in every render sample — the whole-dot flip and the "half-dot"
flip `2*dot+dhalf` are identical (`flip_hd == 2*projected_flip_dot`). The
discriminator is NOT a sub-dot flip grain; it is `read_pos_hd` vs `2*flip`:

| row (scx) | rphd | proj flip | `2*flip+6` | want | verdict |
|---|---:|---:|---:|---|---|
| `postread_scx3_1` | 512 | 257 | 520 | BLOCKED | 512 < 520 → blocked ✓ |
| `postread_scx3_2` | 520 | 257 | 520 | OPEN | 520 ≥ 520 → open ✓ |
| `postread_scx5_1` | 520 | 259 | 524 | BLOCKED | 520 < 524 → blocked ✓ |
| `postwrite_1` (scx0) | 512 | 254 | 514 | DROP | 512 < 514 → dropped ✓ |
| `postwrite_2_scx3` | 520 | 257 | 520 | LAND | 520 ≥ 520 → lands ✓ |

The `_1`/`_2` pairs separate WHOLE-DOT on the eager clock. `scx5_1` (rphd 520,
blocked) and `scx3_2` (rphd 520, open) share the SAME read position but differ in
projected flip (259 vs 257) — the boolean `vis_early` cannot split them (the LE
`early_lead=3` fires vis_early at BOTH); the emergent flip does. `+6` is the
unique even constant satisfying all five (read C ≤ 6, write/`scx5_1` C > 4).

---

## 3. Rows recovered (classified BUG/FLOOR)

**EV CGB 304 → 298 (clean +6 / −0), EV DMG 66 → 62 (clean +4 / −0).** Zero
NEW-fails on either rowlist (A/B `comm -13` empty both models).

CGB recovered (classify_cgb_regr.py, `sameboy_tester --cgb`):
- `oam_access/postwrite_2_scx3` — **BUG** (target)
- `vram_m3/postread_scx3_2` — **BUG** (target)
- `dma/hdma_start_scx3_1` — **BUG** (bonus: the VRAM-read release lands the HDMA readback)
- `vramw_m3end/vramw_m3end_scx3_3` — **BUG** (bonus)
- `vramw_m3end/vramw_m3end_scx3_5` — **BUG** (bonus)
- `oam_access/postread_scx3_2` — FLOOR (gambatte want 0, SameBoy reads 3). NOT a
  SameBoy-divergence: slopgb **production (OFF) AND tier2 both read 0** here (a
  pre-existing gambatte-vs-SameBoy disagreement all slopgb clocks side with
  gambatte on). Before this slice EV was the OUTLIER reading 3; the fix restores
  EV agreement with production/tier2. So recovering it is correct.

DMG recovered (classify_dmg.py, `--dmg`): all **4 BUGs** —
`oam_access/postwrite_2_scx3`, `vram_m3/postread_scx3_2`,
`vramw_m3end/vramw_m3end_scx3_3`, `vramw_m3end/vramw_m3end_scx3_5` (the two
targets' twins + the two vramw bonuses).

**Net: 9 SameBoy-pass BUGs fixed (5 CGB + 4 DMG), 0 SameBoy-pass dropped.**

### The one target NOT recovered — the DS sprite floor

`sprites/space/10spritesPrLine_wx7_m3stat_ds_2` [Cgb] (want 0) is a DS sprite
**FF41 mode-bit read**, not an OAM/VRAM accessibility read — a different lever
(the DS `vis_exit_hd` mid-dot floor, spec §4 "#11da park"). `eager_access_released`
is `!self.ds`-scoped and does not touch it; the DS m3stat-sprite cluster is
delicate (its `_ds_1` siblings SameBoy-pass on the lag). Left as the documented
residual — not gold-plated into this clean SS slice.

---

## 4. Golden-safety mechanism (how production stays byte-identical)

`eager_access_released` short-circuits on `self.eager_value` as its FIRST term.
`eager_value` is `false` in production AND under `tier2_reclock` (the eager clock
is a distinct vehicle). So all three release sites are provably unchanged for the
production and tier2 paths — no render mutation, no new recorded state, a pure
`&self` compare. Confirmed: `golden_fingerprint` byte-identical (twice); tier2
CGB two-bin 291 IDENTICAL row-set; tier2 DMG 116 IDENTICAL; OFF CGB 486 unchanged;
761 lib + 508 frontend unit tests green.

The render exit-record (`mode0.rs::m0_flip_events`, `flip_dot = self.dot`, the
`early_lead` case-tower) is **untouched** — the highest-render-risk path the task
flagged never moved. The emergent flip is computed from the render's live
`projected_flip_dot()` (already `pub(in crate::ppu)`, already used by arm 8 / the
boot law), not recorded.

---

## 5. Gate results

| gate | result |
|---|---|
| `golden_fingerprint` byte-identical | **OK** (41.7 / 49.2 s) |
| EV CGB | 304 → **298** (−6, 0 new-fails) |
| EV DMG | 66 → **62** (−4, 0 new-fails) |
| tier2 CGB / DMG | **291 / 116 IDENTICAL** |
| OFF CGB | **486 unchanged** |
| mooneye acceptance/ppu — default / RECLOCK / EAGER | **91/91 all three** |
| eager tripwires (both models) | `intr_2_mode0/mode3/oam_ok/2_0`, `di_timing-GS` **PASS** |
| clippy `-D warnings` | **clean** |
| `.rs` < 1000 | blocking.rs 464, read_laws.rs 998 (untouched) |
| red-before-green pin | `cgb::eager_emergent_flip_releases_accessibility_at_the_projected_exit` (FAILS neutered, passes with fix) |

---

## 6. What NOT to re-chase (adds to the #11cq/#11cr/#11ct/#11dh lists)

- **"Part-A needs a half-dot render FSM / `flip_hd` field / `m0_flip_events`
  edit"** — REFUTED for the accessibility residual: `dhalf` is always 0 in the
  render, so the recorded half-dot flip is degenerate; the READ-frame debt
  supplies the sub-dot resolution. A pure `&self` compare recovers the rows,
  golden-safe. The render exit-record grain does NOT move.
- **"Extend the `vis_early` boolean release to eager"** — REFUTED (#11dg, C
  +13/−9): the LE `early_lead=3` fires vis_early 2 dots early; `scx5_1` and
  `scx3_2` share a read position and cannot be split by a boolean.
- The DS sprite `m3stat` floor (`10spritesPrLine_wx7_m3stat_ds_2`) is a SEPARATE
  FF41-mode-read lever (DS `vis_exit_hd` mid-dot), not an accessibility release —
  do not fold it into the SS accessibility slice.

Next levers (unchanged from #11dh §6): L1 DS re-host (≈7 CGB), L2 DMG window
(≈16), the wake-clock port (5 CGB halt), line-frame/engine (≈10 DMG), then the
C3 flip when all buckets converge.
