# C2 #11at/#11au — the write-side render-length lever's TWO clean slices (pre-draw window-abort +4/−0, window-reenable +4/−0) + the #11as "co-temporal write collapse" diagnosis CORRECTED

**UPDATE #11au:** a SECOND clean slice landed by the same method — the CGB
window-REENABLE bare-tail (`+4/−0`, `late_reenable_{2,scx2_2,scx3_2,wx0f_2}`, pin
`tier2_window_reenable_passes`, commit `9c8420b`). Cumulative this session:
**124 → 117 SameBoy-pass blockers** (7 fixed across 2 slices, 0 dropped). Both slices
exploit the SAME correction: the "co-temporal" LCDC-toggle families are NOT co-temporal
— the toggle WRITE lands a whole M-cycle apart (slopgb resolves it), only the render
collapses. **#11au reenable:** a window disabled then re-enabled mid-mode-3 redraws
from the re-enable point; its mode-3 extends past the read iff the re-enable beat the
WX-match redraw start (`re-enable_dot <= wx_match_dot − 3`, MEASURED uniform: base
wxmatch97/boundary94, wx0f wxmatch105/boundary102). SCX&7 ≤ 3 (the fine-scroll shifts
the boundary at high SCX — scx5 boundary 98 not 94, MEASURED — the atomic reclock's).
The rest of the write-side (late_wy WY-latch / cgbpal palette-reg / late_scx boundary
constant / DS sub-half-dot / post-draw extend) stays atomic; details below.

---


2026-07-01, `phase-b-s7`. Executed the goal's PRIMARY lever — **the write-side
render-length reclock** — on the `late_disable` window family. Result: the first
CLEAN write-side slice SHIPPED (`+4/−0`, pinned, byte-identical OFF) **and a decisive
correction to the #11as co-temporal verdict**: the late-disable `_1`/`_2` legs are
NOT co-temporal — their LCDC.5-clear WRITES land a whole M-cycle apart (dot104 vs
dot108), which slopgb *resolves*; the collapse is the RENDER being insensitive to the
disable dot, and the abort exit is a per-config window-tile-completion LENGTH. The
one clean, incrementally-buildable sub-case is the **pre-draw abort** (window disabled
before its first fetch → SameBoy renders bare with the SCX penalty dropped).

## Base (HEAD `de41a65` + this slice)

Full-CGB two-bin (3422 rows, `flagon_probe` ON vs OFF):

```
BEFORE: ON fail 466 / OFF 486 → 175 flip-BUGs = 124 SameBoy-PASS + 51 rebaseline
AFTER : ON fail 462 / OFF 486 → 171 flip-BUGs = 120 SameBoy-PASS + 51 rebaseline
delta : +4/−0 (classify_cgb_regr.py: BUG 124→120, FLOOR 51 unchanged, 0 dropped)
```

Fixed (all 4 in the goal's RENDER-LENGTH window blocker list):
`late_disable_early_scx03_wx{0f,10,11,12}_1` (cgb04c_out0).

## The measured mechanism (both emulators)

The `late_disable` pair reads the IDENTICAL slopgb dot but SameBoy splits the FF41
read verdict:

| leg | want | slopgb LCDC.5-clear dot | slopgb read | SameBoy read |
|---|---|---|---|---|
| `late_disable_early_scx03_wx0f_1` | 0 | **dot104** | ly1 dot256 mode3 | ly1 cfl260 **mode0** |
| `late_disable_early_scx03_wx0f_2` | 3 | **dot108** | ly1 dot256 mode3 | ly1 cfl260 **mode3** |

**The #11as "deferred WRITE lands both at the same dot → co-temporal" diagnosis is
WRONG.** The disable writes land 4 dots apart (dot104 / dot108 — a whole M-cycle
`slopgb` resolves via `advance_machine_t`). What collapses is slopgb's **render**:
both legs render mode3 at the read dot because slopgb's whole-dot pixel pipe is
insensitive to whether the 4-dots-later disable caught the window's first tile.
SameBoy's finer timing: the dot104 disable lands before the window's first fetch
(bare, mode-3 exit cfl257 — SCX penalty DROPPED, mattcurrie §WIN_EN); the dot108
disable catches the first tile (mode-3 EXTENDS past cfl260).

So a read law consuming the latched disable dot CAN split them — no sub-M-cycle
needed for this sub-case. This is the write-side lever the goal named, landing its
first non-co-temporal reads.

## Why only the PRE-DRAW sub-case is clean (the residual is per-config length)

The clean discriminator is **pre-draw** (`win_mode` not yet set at the disable, i.e.
`window_abort` early-returns): the LCDC.5 clear before the window's first fetch. For a
pre-draw abort SameBoy renders BARE but drops the SCX fine-scroll penalty → mode-3
exit cfl257 (not the normal bare 257+SCX&7). slopgb's render over-extends; the read
law forces mode0 when `dot + 4 >= 257`.

But WITHIN the pre-draw class the bare/extend boundary is STILL config-dependent (the
window's first-tile-completion dot), NOT a uniform threshold — build-measured:

| family (all read dot256 / cfl260) | fire (bare, want0) | don't-fire (extend, want3) | boundary |
|---|---|---|---|
| `late_disable_early_scx03` | predraw dot104 | dot108 | ~106 |
| `late_scx_late_disable` | predraw dot124/128 | dot132 | ~130 |

Same read dot, boundary 106 vs 130 — the difference is the window's first-tile dot
(WX/SCX position). And ACROSS the non-predraw families the abort exit is non-monotonic
in the abort dot (early_scx03 abort104 → exit257 / want0, but non-early late_scx0
abort100 → exit>260 / want3 at the SAME read) — proof the exit is a per-config render
LENGTH, not a function of (abort_dot, scx, read_dot). So the general late_disable/
late_reenable/late_wx render-length remains the atomic render reclock.

The shipped slice scopes to `win_predraw_abort_dot <= 105` (the scx03 pre-first-tile
window), which isolates exactly the 4 scx03 blockers with 0 suite-wide drops. The
`_2` siblings (dot108, post-first-tile) and the later-boundary families (late_scx at
124-132) are excluded and left to the render reclock.

## Code (phase-b-s7, flag-gated, byte-identical OFF)

- `ppu/render.rs`: `Render::win_predraw_abort` + `win_predraw_abort_dot` fields
  (init/reset per line).
- `ppu/render/window.rs`: `window_abort` flags the pre-draw abort in its
  `!win_mode` early-return path (tier2 + CGB).
- `ppu/stat_irq.rs::vis_mode_read`: the bare-exit law (tier2 + CGB + SS + predraw +
  `win_abort_dot <= 105` + `LCDC_WIN_ENABLE == 0` [excludes late_reenable] + bare
  non-sprite non-glitch + `dot + 4 >= 257` → mode0).
- pin `tests/gbtr/gambatte.rs::tier2_window_predraw_abort_passes`.

## WAKE-CLOCK — the goal's #2 lever `halt_mode_phase` BUILT + REFUTED (#11av)

Built the goal's named #2 lever — `halt_mode_phase`, the FF41-mode twin of the shipped
`halt_ly_phase` (`interconnect.rs` field + `tick.rs` set at the mode-0 halt-wake +
`memory.rs` one-shot back-date of the first post-wake FF41 read: native mode 2 at
`dot < 4 + phase` → mode 0). The re-measurement supported it — the clean scx2 pairs
read DIFFERENT slopgb dots (`_1`/`a` want0 → dot4; `_2`/`b` want2 → dot8), so a
one-shot halt-wake-scoped back-date (unlike the un-scoped WAKEPEEK force) *looked*
separable. **REFUTED full-CGB two-bin: +5/−13** (`classify_cgb_regr.py` — all 13 are
`m0int/m0irq_m0stat_scx{3,4,5}_2` want-mode-2 rows forced to mode0). The reads SHIFT
with SCX (the mode-0 rise dot is `254 + SCX&7`, so the wake-resume dot moves): the
scx2 want-0 legs read dot4, but the scx3/4/5 want-2 legs ALSO read dot4-7 — no fixed
back-date window separates them, and the rise cc is IDENTICAL across each co-temporal
pair (same scx → same rise dot), so the `halt_ly_phase` cc-indexing cannot scope it
either. **CONFIRMED: WAKE-CLOCK is genuinely atomic** — the discriminator is the
sub-M-cycle halt-ENTRY/CPU-resume T-phase (SameBoy reads want-0 and want-2 at the
IDENTICAL `ly2 cfl0 dc0` half-dot, returning mode0 vs mode2 from a sub-dc STAT-update
T). Needs the T-granular halt clock (the atomic reclock). Code built + measured +
reverted-to-green (byte-identical OFF). The goal's #2 lever is REFUTED as a whole-dot/
cc back-date — same result class as WAKEPEEK (#11ar).

## Residual (120 SameBoy-pass blockers)

RENDER-LENGTH 46 (was 50 − 4 predraw) · ENGINE-IF 30 · S6-DS 20 · READ-FRAME 12 ·
WAKE-CLOCK 12. The remaining RENDER-LENGTH window rows (post-draw abort extend /
late_reenable / late_wx / late_scx / cgbpal / DS accessibility) need the per-config
window-tile-completion LENGTH model (moving `line_render_done` → the counter-pinned
dispatch — the atomic render reclock). ENGINE-IF/READ-FRAME/WAKE/S6-DS need the
sub-M-cycle dispatch/wake/completion reclock. The flip needs ALL 120 to converge; the
single sharpest lever is unchanged: the atomic half-dot render + T-granular
dispatch/wake/completion reclock, landing the render length + IF dispatch + read
frame together (RED intermediate, one rebaseline).

## Gate (END CLEAN)

`+4/−0` (`late_disable_early_scx03_wx{0f,10,11,12}_1`); flip-BUGs 175→171 (SameBoy-pass
124→120, 0 dropped); mooneye flag-on 91/91 + OFF 91/91; gbtr OFF full battery
byte-identical (golden + pins clean); clippy `-D` clean. Defaults NOT flipped.
