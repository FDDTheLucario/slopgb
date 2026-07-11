# EAGER off-screen-window (WX=166) exit-arm cluster — 4 of 8 SHIPPED, 4 REFUTED with traces (EV CGB 298→297, EV DMG 62→59, clean flag-gated) (2026-07-11, #11dn)

Base: `finish-port-halfdot @ 2b7174b` (#11dm). The 8-row off-screen-window
`m2int_wxA5/A6` cluster from the C3-flip bar. Traced fresh (dual OFF/EV/tier2
`--features port_probe` on `examples/run_gambatte.rs` + SameBoy SBMODE
ground-truth); production + tier2 byte-identical (`golden_fingerprint` 9020 cases
match, twice).

---

## 0. Bottom line

The 8 candidate rows split into **THREE distinct mechanisms**, not one — the
task's "read-frame off-screen-window exit arm" is only two of them:

| mechanism | rows | verdict |
|---|---|---|
| **FF41 mode-3 exit** (off-screen WX=166 window-length arm, pre-activation read) | 2 (CGB `m2int_wxA6_scx5_m3stat_3`, DMG `m2int_wxA6_firstline_m3stat_2`) | **SHIPPED** |
| **OAM/VRAM accessibility** (stalled off-screen WX=166 window readback) | 2 (DMG `m2int_wxA6_{oam,vram}busyread_2`) | **SHIPPED** |
| **FF0F / IRQ-timing read-frame** (m0irq loop-iteration count) | 4 (CGB `m2int_wxA5_m0irq_2`; DMG `m2int_wxA5_m0irq_2`, `m2int_wxA6_m0irq_2`, `m2int_wxA6_m0irq2_2`) | **REFUTED** (not an FF41 read; dispatch-adjacent) |

**Net: 4 SameBoy-pass BUGs fixed (1 CGB + 3 DMG), 0 dropped either model.**
EV CGB 298→297, EV DMG 62→59; tier2 CGB 291 / DMG 116 unchanged.

---

## 1. The shipped mechanism — FF41 off-screen-window exit (2 rows)

The WX=166 (off-screen) window activates DURING HBlank (SameBoy
`wx_166_interrupt_glitch`), so slopgb's render sets `win_active` at the HBlank
match (~dot 256/264), NOT at the mode-3 start. The eager cc+0 FF41 read lands
ONE M-cycle (4 dots) BEFORE that:

```
CGB m2int_wxA6_scx5_m3stat_3 (scx5, want 0):
  EV bad  ly=1 dot=260 win_active=0 natm=3 pfd=265 → bare arm-8 exit 2*265+2=532, rphd 528 < 532 → mode 3 (WRONG)
  tier2   ly=1 dot=264 win_active=1          → arm 1 window-length exit 2*(259+5)=528, rphd 528 → mode 0 (RIGHT)
DMG m2int_wxA6_firstline_m3stat_2 (scx0, want 0):
  EV bad  ly=1 dot=252 win_active=0 natm=3 pfd=256 → bare arm-8 exit 2*256+2=514, rphd 512 < 514 → mode 3 (WRONG)
  tier2   ly=1 dot=256 win_active=1          → arm D1 wxA6-bare exit 2*(253+0)=506, rphd 512 → mode 0 (RIGHT)
```

The render's `projected_flip_dot` is already window-ELEVATED at the pre-activation
read (265/256 vs bare 259/254), so the bare arm-8 `2*flip+2` over-holds by 4 hd.
The deferred read escapes this because `read_deferred` lands it 4 dots later, at
`win_active=1`, where the window arm (arm 1 / D1) fires the closed-form exit.

**Fix (`stat_irq.rs` `eager_offscreen_win_arming` + arm 1 / D1 guard relax in
`read_laws.rs`):** fire the window-length arm for the eager pre-activation read —
`win_active || eager_offscreen_win_arming()`. The arming predicate: `eager_value
&& wx==0xA6 && LCDC.5 && wy2<=line && !win_aborted && !win_active`. CGB exit
`259+SCX&7`, DMG exit `253+SCX&7` (the existing arm bodies, unchanged).

## 2. The shipped mechanism — stalled-window OAM/VRAM accessibility (2 rows)

`m2int_wxA6_{oam,vram}busyread_2` [Dmg] want 5 (the accessible readback). Both
slopgb clocks BLOCK the read — a MISSING law present in NEITHER (these 2 rows
**fail tier2 too**):

```
DMG oam/vram busyread (scx0, want 5):
  EV    ly=1 dot=256 blocked=1 win_active=1 win_stalled=1 pfd=257 rphd 520 ≥ 2*257+6=520
  tier2 ly=1 dot=256 blocked=1 win_active=1 win_stalled=1   (also wrong)
```

The off-screen window activates during HBlank but renders NOTHING (`win_stalled`),
so SameBoy's mode-3 OAM/VRAM lock releases at the bare flip. slopgb keeps
`win_active` set → the lock runs to the render-done dispatch, and
`eager_access_released` is `!win_active`-scoped so it can't help.

**Fix (`blocking.rs` `eager_offscreen_win_access`, wired into
`oam_read_blocked` + `vram_read_blocked`):** an eager-only release keyed to the
stalled window past the emergent flip — `eager_value && !ds && wx==0xA6 &&
win_active && win_stalled && !win_aborted && ns==0 && read_pos_hd >=
2*projected_flip_dot + 6`. Same `+6` hd OAM/VRAM `m0Time` lag as
`eager_access_released`. Eager-only (tier2 still fails these — un-touched, so
tier2 116 holds; the eager clock diverges to the SameBoy-correct verdict).

## 3. REFUTED — the m0irq rows (4) are FF0F / IRQ-timing, not the FF41 exit arm

`m2int_wxA5_m0irq_2` [Cgb+Dmg], `m2int_wxA6_m0irq_2`, `m2int_wxA6_m0irq2_2` [Dmg]
all display **0** where want is **2** — but the displayed digit **never appears
in any FF41 read** (all FF41 reads on the measurement line return only {2,3}).
The output is derived from the FF0F/IF poll (SameBoy does 0 FF41 reads on these,
only `SBREAD ff0f`), and the EV vs tier2 divergence is a **loop-iteration COUNT**:

```
CGB m2int_wxA5_m0irq (want 2, EV displays 0):
  EV    7 FF41 reads on ly=0: dots 0,24,28,52,56,80,84  (paired, entry-shifted)
  tier2 4 FF41 reads on ly=0: dots 24,52,80,108          (single, 28-spaced)
DMG m2int_wxA6_m0irq (want 2, EV displays 0):
  EV    8 reads (paired 16,20/44,48/72,76/100,104);  tier2 4 (12,40,68,96)
```

The eager clock's M-cycle→dot realignment shifts how many poll iterations fit
before the mode-2→3 transition — a **read-frame IF-delivery / dispatch-count**
issue (the buildplan §4 DMG "m2int_m0irq | read-frame IF-delivery" bucket), NOT
the off-screen-window FF41 exit. The shipped FF41 + accessibility fixes leave
these unchanged (still failing) — confirming the decisive quantity is neither the
window exit nor OAM/VRAM accessibility. A fix needs the FF0F read law (the #11bk
`hblank_if` two-latch) ported to eager, or a dispatch move (forbidden) — a
separate lever, out of this slice.

---

## 4. Rows recovered (classified)

All 4 SameBoy-PASS = **BUGs** (`sameboy_tester --cgb`/`--dmg` match the `.bmp`):

- `window/m2int_wxA6_scx5_m3stat_3` — **BUG** [Cgb] (target)
- `window/m2int_wxA6_firstline_m3stat_2` — **BUG** [Dmg] (target)
- `window/m2int_wxA6_oambusyread_2` — **BUG** [Dmg] (target)
- `window/m2int_wxA6_vrambusyread_2` — **BUG** [Dmg] (target)

Zero NEW-fails on either rowlist (A/B `comm -13` empty both models).

---

## 5. Golden-safety

All three new predicates short-circuit on `self.eager_value` (false in production
AND under `tier2_reclock`), and the read_laws arm relaxations OR-in
`eager_offscreen_win_arming` (also `eager_value`-first) — the production and tier2
paths are provably unchanged. No render mutation, no new state, pure `&self`
compares. `golden_fingerprint` byte-identical (9020 cases, twice); tier2 two-bin
CGB 291 / DMG 116 IDENTICAL; OFF CGB 486 unchanged.

---

## 6. Gate results

| gate | result |
|---|---|
| `golden_fingerprint` byte-identical | **OK** (9020 cases, 41.9 s) |
| EV CGB | 298 → **297** (−1, 0 new-fails) |
| EV DMG | 62 → **59** (−3, 0 new-fails) |
| tier2 CGB / DMG | **291 / 116 IDENTICAL** |
| OFF CGB | **486 unchanged** |
| mooneye — default / RECLOCK / EAGER | **92/92 all three** (intr_2 tripwires incl.) |
| clippy `-D warnings` | **clean** |
| `.rs` < 1000 | stat_irq.rs 885, read_laws.rs 998 (unchanged), blocking.rs 494 |
| red-before-green pin | `cgb::eager_offscreen_wx166_window_exit_and_stalled_access` (FAILS neutered @ dot 260, passes with fix) |

---

## 7. What NOT to re-chase

- **"the m0irq rows are the FF41 off-screen-window exit arm"** — REFUTED: the
  displayed digit is never an FF41 read value; they are FF0F/IRQ-timing loop-count
  rows (EV vs tier2 do a DIFFERENT NUMBER of polls). Need the eager FF0F read law
  (#11bk port) or a dispatch move (forbidden), not a window exit arm.
- **"the busyreads are a clean eager read-frame re-host"** — they FAIL tier2 too;
  the fix is a NEW stalled-window accessibility release (eager-only, since tier2
  116 must hold), not a re-host of an existing tier2 law.
- **firing the arming arm for on-screen WX** — scoped to `wx==0xA6` (the glitch
  value); on-screen windows are caught by the render's own `win_active`.
