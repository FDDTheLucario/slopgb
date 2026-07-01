# C2 #11as — the render mode-3 LENGTH port build-measured: 1 clean DS accessibility slice (+1/−0) + the DEFINITIVE co-temporal barrier (ESCAPE on the 125-convergence)

2026-07-01, `phase-b-s7`. Executed the goal's PRIMARY lever — **the render mode-3
LENGTH port: make slopgb's rendered mode-3→0 exit match SameBoy's per-config length
so the native FF41/accessibility read returns SameBoy's verdict**. Result =
**one clean slice SHIPPED (the DS line-END OAM-read release, `+1/−0`, pinned,
byte-identical OFF)** plus the **definitive measurement that the render-length
blocker set is dominated by CO-TEMPORAL pairs** slopgb's whole-dot deferred frame
cannot split. The 125-blocker flip does NOT land (ESCAPE): every co-temporal pair
reads the IDENTICAL slopgb dot + native mode/blocked-state with OPPOSITE wants,
because the DISCRIMINATING input (the late window/LCDC write's sub-M-cycle position,
or a write instruction's M-cycle cost) is collapsed by slopgb's deferred-commit
clock. Defaults NOT flipped; `pixel-pipe-reclock` core byte-identical.

## Fresh base (HEAD `9811e6d` + this slice)

Full-CGB two-bin (3422 rows, `flagon_probe` ON vs OFF, both bins from source):

```
ON  (boot_with_reclock):  fail=466   (was 467; the +1 #11as shipped)
OFF (production):         fail=486
flip-BUGs (OFF-pass ∧ ON-fail):  176 → 175
  classify_cgb_regr.py:  125 SameBoy-PASS (must FIX) / 51 gambatte-ref (rebaseline-OK)
```

The 125 SameBoy-pass by top-dir: window 26 · halt 12 · lycEnable 11 · speedchange 7
· lcd_offset 7 · cgbpal_m3 7 · enable_display 6 · dma 6 · vram_m3 5 · miscmstatirq 5
· oam_access 4→3 · ly0 4 · … (the #11am 5-class partition, −1 oam_access this slice).

## SHIPPED — the DS line-END OAM-read release (`+1/−0`, pinned, byte-identical OFF)

`ppu/blocking.rs::ds_lineend_read_open`, wired into `oam_read_blocked` only. Under
CGB **double speed** SameBoy releases the mode-3 OAM read-lock one cycle LATER than
single speed: it SKIPS the `if (!cgb_double_speed)` early unblock
(`display.c:2104-2111`) and drops through to `:2118` (`oam_read_blocked = false`),
which lands the deferred cc+0 read's unblock at slopgb dot `254 + SCX&7`. slopgb's
production block ran to `line_render_done` (~2 dots later), so
`oam_access/postread_ds_2` (`ly135 dot254`, SameBoy accessible) read "3" (blocked)
while its `_1` sibling (dot252, still blocked) passed. Release on bare non-sprite
non-window non-glitch DS lines at `dot ≥ 254 + SCX&7`. Pin
`tier2_oam_postread_ds_passes` (the fix + the `_1` blocked regression guard).

**OAM-only** — the VRAM twin is NOT clean (see below). The scx5 sibling
(`postread_scx5_ds_2`, dot260) is unaffected: `line_render_done` has already fired
there (`lrd=true`), so the read is already accessible but reads a real 0xFF byte —
a value/read-frame residual, not an accessibility-window bug.

## The DEFINITIVE co-temporal barrier (why the render-length port does NOT cascade)

Build-measured every RENDER-LENGTH sub-family on both emulators (slopgb
`SLOPGB_S5DBG` FF41/VRAM/OAM read-dot + native mode/blocked; SameBoy `SB_TRACE`
SBMODE exit + framebuffer OCR). The `_1`/`_2` legs of a blocker pair read the SAME
slopgb dot with the SAME native mode/blocked-state and OPPOSITE wants — a
CO-TEMPORAL A/B no render or read law can split, because slopgb renders both configs
identically (the discriminating late-write / instruction-cycle offset collapses in
the deferred frame).

### window (26) — ALL co-temporal (late-write sub-M-cycle collapse)

The SS win-aborted family (`late_disable`/`late_reenable`/`late_wx`/`late_scx`),
measured read-dot + want:

| pair | read dot | native | want `_1`/`_2` | verdict |
|---|---|---|---|---|
| `late_disable_early_scx03_wx0f` | 256 | m3 | 0 / **3** | co-temporal (both dot256 m3) |
| `late_disable_early_scx03_wx1{0,1,2}` | 256 | m3 | 0 / 3 | co-temporal |
| `late_reenable` | 256 | m3 | **3** / 0 | co-temporal |
| `late_reenable_scx2` | 260 | m3 | **3** / 0 (`_3` m0 passes) | co-temporal |
| `late_scx_late_disable` | 256 | m3 | 0,0 / **3** | co-temporal (`_0`/`_1`/`_2`) |
| `late_wx_scx5` | 260 | m3 | 0 / **3** | co-temporal |
| `late_disable_spx10_wx0f` | 264 | m0 | 0 / **3** | co-temporal (both m0) |

The `_1`/`_2` differ ONLY in the late LCDC.5/WX/SCX write dot (1 M-cycle apart).
slopgb's deferred WRITE lands both at the same dot → both render identically → the
FF41 read at the shared dot cannot distinguish them. #11am's "`late_disable`
render-length is correct, read is −4" is CONFIRMED and generalized: the render
LENGTH is not the lever — the write's sub-M-cycle position is, and slopgb collapses
it. (The EXTEND-direction late_wy mid-line-write rows were shipped #11af `+5`; the
boundary-write / shorten-direction / off-screen-wxA5/A6 rows are these co-temporal
/ deferred-write-phase residuals.)

### DS accessibility (vram_m3 + oam_access) — OAM postread clean, VRAM co-temporal

`postread` IS separable (`_1` dot252 blocked / `_2` dot254 accessible) — the OAM
release ships it. But the VRAM release is an A/B swap: `vram_m3/postread_ds_2` (want
accessible @dot254) is CO-TEMPORAL with `vramw_m3end/vramw_m3end_ds_2` (want the
readback BLOCKED @dot254). Traced: the vramw write (`WR8000 @dot250`, correctly
`wblk=true`) costs a CPU M-cycle that shifts SameBoy's readback cfl relative to the
sprite-free `postread`, but slopgb's deferred frame collapses both to the SAME
dot254 read — so a VRAM read release fixes `postread_ds_2` (+1) and drops
`vramw_m3end_ds_2` (−1). `preread`/`prewrite`/`postwrite` DS all co-temporal
(`prewrite_ds_1`/`_2` both dot254; `postwrite` both dot286) or line-start grid
(`preread` dot80/82 collides with the `preread_lcdoffset1` variant). The VRAM DS
read grid is the parked S6 reclock.

### The rest — co-temporal or different-register

| family | rows | measured verdict |
|---|---|---|
| `m2int_m3stat/late_scx4` | 2 | `_1`/`_2` both `ly134 dot130 m3` (DS) / `ly0 dot8 m2` (SS), opposite wants → CO-TEMPORAL |
| `scx_during_m3/scx_m3_extend_ds` | 1 | **SEPARABLE** (`_1` dot330 m0 want3 / `_2` dot332 m0 want0) — a genuine mode-3 EXTEND (mid-m3 SCX write), but needs a tier2 SCX-during-m3 vis-HOLD render model; DS; unbuilt |
| `enable_display` | 6 | `ly0_late_scx7_m3stat` reads a dispatch COUNT (87 vs 84), `ly1_late_cgbpw` a palette VALUE (55 vs AA) — NOT mode reads; the glitch-line dispatch-count / palette-write lever |
| `cgbpal_m3` | 7 | DS accessibility (co-temporal with the palette write-readback, the vramw analogue); `m3end` co-temporal (#11ag framebuffer-confirmed `_1`→7 blocked / `_2`→0 accessible both SameBoy-pass) |

## The per-config render mode-3 length model (SameBoy ground truth, SBMODE)

For the FF41/accessibility read the exit that matters is SameBoy's CPU-visible
mode-3→0 dot; the deferred cc+0 read samples `read_offset` (4 SS / 3 DS) dots
before SameBoy reads the same `ldh a,(FF41)`. The shipped `vis_mode_read` full-carry
law already frames every BARE mode-3 read at `SBex = 257 + SCX&7 + ds + SCX&1`. The
per-config exits (measured):

| config | SameBoy mode-3 exit (cfl) | slopgb render | lever |
|---|---|---|---|
| bare | `257 + SCX&7` (+1 DS) | matches (full-carry SBex) | SHIPPED (#11ar) |
| triggering window (on-line WY) | `263 + SCX&7` (+1 DS) | matches (window length law + shadow WY) | SHIPPED (#11z/#11af/#11ag) |
| **win-abort (late_disable)** | `257` — **DROPS the SCX penalty** | over-extends to `257+SCX&7`, BUT co-temporal with `_2` | co-temporal, unsplittable |
| **DS OAM postread** | `254 + SCX&7` (read-lock) | was `line_render_done` +2 | SHIPPED this slice (OAM) |
| **DS VRAM postread** | `254 + SCX&7` (read-lock) | co-temporal with vramw write-readback | S6 DS write grid |
| **SCX-during-m3** | `+2` per mid-m3 SCX write | flat exit | separable, unbuilt (needs SCX-m3 vis-HOLD) |
| cgbpal m3end | palette release `~+3` into HBlank | co-temporal (`_1`/`_2` both SameBoy-pass) | co-temporal |

**Key finding: SameBoy's win-abort exit DROPS the SCX penalty** (`257`, not
`257+SCX&7`) — a genuine per-config render-length law. But it cannot be applied
because the win-abort `_1`/`_2` legs are co-temporal (identical slopgb dot, opposite
wants). The render-length model is CORRECT and now fully characterized; the barrier
is that slopgb's deferred frame collapses the discriminating write.

## Why the 125 do NOT converge (residual map, by class)

- **RENDER-LENGTH ~50:** the window shorten/abort + late_scx4 + cgbpal + DS
  accessibility co-temporal pairs (the late-write / write-instruction sub-M-cycle
  collapse) + the `scx_during_m3` EXTEND (separable, unbuilt) + enable_display
  (dispatch-count/palette-value, not a mode read). Even a perfect per-config
  render-LENGTH law leaves these because the discriminator is sub-M-cycle in the
  triggering WRITE, not the render length.
- **ENGINE-IF 30:** FF0F IF-delivery straddle — the counter-pinned dispatch↔read
  reclock (#11al: same edges a few dots apart, `_1`/`_2` identical dispatch opposite
  wants).
- **S6-DS 21:** the DS read/write grid + cycle-write conflict (the VRAM postread
  twin lives here).
- **READ-FRAME 13 / WAKE-CLOCK 12:** the serial/tima S6-completion + the sub-M-cycle
  halt `halt_mode_phase` (`SLOPGB_WAKEPEEK` +3/−13 co-temporal, #11ar).

The render-length port drained exactly the SEPARABLE reads (the +1 OAM postread this
session, on top of the +9 read-position + the window +13). Every other
render-length blocker is co-temporal with a SameBoy-pass sibling (an A/B swap) or
reads a different register — the same structural wall the read-position lever hit.

## The single sharpest remaining lever (unchanged, sharpened)

The barrier is the **deferred-commit clock collapsing the sub-M-cycle position of
the triggering WRITE** (late_disable's LCDC.5 clear, vramw's VRAM write, the
per-ISR dispatch): slopgb lands both legs of every co-temporal pair at the same
dot. The lever is the **sub-M-cycle write/dispatch reconciliation** — the same
atomic S6/S7 interrupt-service + register-write-position reclock the read-position
lever named. It is NOT a render-length law (that is now built + measured and drains
only the separable reads) and NOT a read-position peek (exhausted +9). The
`scx_during_m3` EXTEND (+1, separable) is the last incrementally-buildable
render-length row; everything else needs the atomic write/dispatch reclock.

## Gate (END CLEAN)

`+1/−0` (`oam_access/postread_ds_2`); pin `tier2_oam_postread_ds_passes` green;
mooneye flag-on 91/91 + OFF 91/91; gbtr OFF full battery byte-identical (the release
is `tier2_reclock`/`ds` gated, inert flag-off); clippy `-D` clean. Defaults NOT
flipped.
