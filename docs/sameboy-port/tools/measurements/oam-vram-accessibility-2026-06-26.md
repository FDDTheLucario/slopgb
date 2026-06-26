# OAM/VRAM read-accessibility — the `vis_early` coupling (#11o, 2026-06-26)

MECH 1 read-observer, accessibility-coupling slice. Shipped `d524267`,
flag-gated (`tier2_reclock`), byte-identical OFF, defaults NOT flipped.

## Root cause

The cc+0 deferred read samples TWO things at the same dot:
- **FF41 mode** — keyed on `vis_mode` / `vis_early` (the visible mode→0 flip,
  which on the cc2 boundary lands one dot before the render-done dispatch, #11n).
- **OAM/VRAM read-accessibility** — keyed on `line_render_done` (the render-done
  dispatch), one dot LATER.

On SameBoy the OAM/VRAM unblock is **coincident with the visible mode→0 flip**:
the `postread_scx2` accessibility read goes accessible at the SAME `cfl261` the
FF41 read returns mode 0. slopgb's deferred read therefore saw mode 0 yet OAM/VRAM
still locked → the accessibility read rendered `3` (blocked → `0xff`) where
SameBoy reads accessible (`out0`).

## Fix (`ppu/blocking.rs`, tier2-gated)

```
oam_read_blocked:  ... && !(self.tier2_reclock && self.vis_early)
vram_read_blocked: if ... || (self.tier2_reclock && self.vis_early) { return false }
```

`vis_early` is never set in production / LE-only → byte-identical OFF (gbtr 197/0
ratchet unchanged). Release the OAM/VRAM lock on the visible flip, matching the
FF41 mode the same read observes.

## Full-suite two-bin (target/gbtr fix vs target/lint reverted, /tmp/allgb.txt)

`flagon_probe[ON]`: revert fail=733 → fix fail=727. comm: **+6/−0**, zero
SameBoy-passing dropped.

| ROM | models | want | was | now |
|---|---|---|---|---|
| `vram_m3/postread_scx3_2` | Dmg+Cgb | 0 | 3 | 0 |
| `oam_access/postread_scx3_2` | Cgb (Dmg `xout1`) | 0 | 3 | 0 |
| `dma/hdma_start_scx3_1` | Cgb | 0 | 3 | 0 |
| `vramw_m3end/vramw_m3end_scx3_3` | Dmg+Cgb | 0 | 3 | 0 |

All are SCX&7=3 (cc2 dispatch dot ≡1 mod4, el=1) — the boundary's M-cycle is read
at the dot the visible flip lands.

## Floored / out of scope (do NOT chase)

- **`postread_scx2_2` / `postread_scx5_2` (el=0, read-collapse).** The OAM read
  lands at the boundary's dot but a sub-dot BEFORE its M-cycle start; `vis_early`
  (== the dispatch at el=0) does not release it. slopgb reads scx2@256 / scx5@260
  `v=ff` blocked; SameBoy unblocks at cfl261/264. Read-side sub-dot floor, same
  class as `late_scx4` — needs S7 sub-dot read resolution, not a boundary fix.
- **`oam_access/postwrite_2_scx3` (write direction, SEPARATE).** Writes block
  LONGER than reads (`lcdon_write_timing-GS` dots 80-83); the WRITE unblocks too
  early (want1 got0), the opposite direction. `oam_write_blocked` /
  `vram_write_blocked` boundary, its own ground-truth — not this fix.

## Pin

`tier2_oam_vram_postread_scx3_passes` — vram_m3 (D+C), oam_access (C). 12 tier2
pins total.
