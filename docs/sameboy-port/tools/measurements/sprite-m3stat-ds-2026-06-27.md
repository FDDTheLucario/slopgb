# DS sprite m3stat read-grid snap (2026-06-27, #11t)

The next concentrated batch after the CGB lcd-offset class (#11q/r/s): **sprites
(114 flag-on CGB fails)**, dominated by ~84 `sprites/*_m3stat_ds_1` rows that were
**tier2 read-frame REGRESSIONS** (flag-OFF passes out3, flag-ON read out0).

**Result: ONE clean Tier-2 slice, +87/−3 (net +84) flag-on (full 3524-CGB
two-bin). 0 SameBoy-passing rows dropped.** The 3 "−" are gambatte-reference
floors (SameBoy reads mode 3, already baselined in production). Flag-gated
(`tier2_reclock`), byte-identical OFF, defaults NOT flipped.

## Root cause (build-measured, not reasoned)

`3spritesPrLine_m3stat_ds_1_cgb04c_out3` flag-ON traced:
- slopgb flag-ON FF41 read `ly1 dot288 mode=0` (regression; want 3).
- slopgb flag-OFF FF41 read `ly1 dot288 OVERRIDE raw=0 -> 3` (the production
  `stat_mode_edge` override forces mode 3).
- SameBoy `SBREAD ff41 ly1 cfl290 mode=3` (visible flip was at `cfl257`, so the
  FF41 mode bits LAG the visible mode by ~33 dots — the DS sprite-line quirk).

So the DS sprite-line FF41 read wants the **lagging mode 3**, supplied in
production by the `stat_mode_edge` override (INC-DS-1 / INC-G3 task 6,
`interconnect/memory.rs`: a DS sprite m3→m0 flip holds the FF41 mode bits at 3
for the read M-cycle). That override is armed ONLY by the `m0_stat_flip` stamp,
set ONLY by `m0_flip_events` (`render/mode0.rs`).

**Why flag-ON missed it:** the single-speed sprite read-grid snap (#10 B5) snaps
the sprite-line mode-0 dispatch to the CPU read grid (`snap_ok = dot % 4 == 0`).
That term applied in DOUBLE speed too. The natural DS sprite flip is at lx159
(dot 287, odd); the `% 4` snap pushed it to dot 288 = the pipe end (lx 160).
`step_dot` runs `render_step` (→ `advance_lx`'s lx=160 fallback sets `m0_src`)
BEFORE `m0_flip_events`, so `m0_flip_events` early-returned (`m0_src` already
set) — **the `m0_stat_flip` stamp never armed** → no override → the deferred cc+0
read saw the already-flipped visible mode 0.

`vis_early` is the WRONG lever here (it anticipates mode 0; these reads want the
lagging 3) and stays `!self.ds`-gated.

## Fix (`render/mode0.rs`, one line)

```rust
let snap_ok = !(self.tier2_reclock && has_sprites && !self.ds) || self.dot % 4 == 0;
//                                                  ^^^^^^^^ added: snap single-speed ONLY
```

DS sprite lines now flip at the natural dot (287), arm the `m0_stat_flip` stamp,
and the deferred read straddles the production override → mode 3. CGB DS only
(`self.ds` ⇒ CGB); DMG byte-identical (`ds` always false). Production
byte-identical (`tier2_reclock` false ⇒ `snap_ok = true`, no snap, as before).

## Two-bin (target/gbtr fix vs target/lint reverted, 3524 CGB rows)

- baseline flag-on: pass=2424 fail=599
- fix flag-on:      pass=2508 fail=515  → net +84

FIXED (87): 84 `sprites/*_m3stat_ds_1` (out3) + 3 `late_sizechange_sp{00,01,39}_ds_2`
(out3). REGRESSED (3): `late_sizechange_sp{00,01,39}_ds_1` (out0).

## The 3 "−" are gambatte-reference floors, NOT dropped SameBoy-passes

SameBoy `SBREAD ff41` for the 6 `late_sizechange_sp{00,01,39}` rows (both `_1` out0
AND `_2` out3) lands at the IDENTICAL `ly8 cfl268 mode=3`. Both same-line reads
fall in one M-cycle → the override forces both to mode 3 (no `event_phase` offset
separates two reads in one M-cycle). So:
- `_ds_2` (out3) — SameBoy mode 3 = the gambatte expectation → joins the lift.
- `_ds_1` (out0) — SameBoy mode 3 ≠ gambatte out0 → a gambatte-reference floor,
  ALREADY in `baselines/gambatte.txt` (flag-OFF fails it too). flag-ON now also
  reads mode 3 = matching SameBoy + production. **Not a dropped SameBoy-pass.**

This is the same in-cluster A/B swap the production INC-G3 task-6 lift made
(`+84/−3`, ladder line ~89). Spot-checked fixed rows (8spr / 10spr_2overlap5 /
1spr_BgPrior `_ds_1`) all SameBoy mode 3 = want out3.

## Gate (all green)

- gbtr + mooneye OFF: byte-identical (ratchet UNCHANGED, the 3 `_1` floors were
  already baselined; production byte-identical).
- mooneye flag-on (`SLOPGB_MOONEYE_RECLOCK=1`): 91/91 (incl.
  `intr_2_mode0_timing_sprites`, the single-speed sprite snap — untouched by the
  `!self.ds` gate).
- lib 660; clippy `-D warnings` clean; fmt touched-files clean (pre-existing
  637-976 one-liners untouched).
- New pin `tier2_sprite_m3stat_ds_passes` (21 tier2 pins).

## Remaining 30 sprite fails — ALL build-measure-confirmed floors (NONE a tier2 slice)

Flag-OFF probe of the 30 post-fix sprite fails: **pass=0 fail=30** → flag-OFF ==
flag-ON for every one ⇒ baselined production floors, NOT tier2 read-frame
regressions. All 30 are in `baselines/gambatte.txt`. SameBoy spot-traced:

| cluster (~count) | floor class | evidence |
|---|---|---|
| `space/*wx*_m3stat_ds` (~16) | **C2 window-length render** | SameBoy passes (`9pos8_wx08_ds_1` cfl322 mode3; `wx0_ds_2` cfl325 mode0) but slopgb reads INVERTED in BOTH flag states (`_1`→0, `_2`→3). The INC-DS-1 known floor: a window line's mode-3 flip goes through `advance_lx` (silent), which never sets `m0_stat_flip`, so the override can't arm — AND slopgb's window mode-3 LENGTH differs from SameBoy. Render-level = the C2 parallel window-length model, not a read-frame snap. |
| `late_disable_ds_{1,3}` (~2) | **C2 render** (aborted-extend) | SameBoy `ly2 cfl358/364 mode3`, slopgb reads 0 both modes — the late sprite-disable aborts slopgb's mode-3 early; SameBoy keeps it. |
| `late_sizechange*_ds` (~7) | gambatte-reference + render | `late_sizechange_ds_1` (out0): SameBoy `cfl360 mode3` == slopgb 3 (both modes) ≠ gambatte out0 → gambatte-reference floor (slopgb already SameBoy-correct). `late_sizechange2_*_ds_1` (out3): mode-3-length / size-change render. |
| `*_dmg08*`, `*_m0irq_2_dmg08*` (~4) | out of DS scope | DMG-derived ROMs on CGB; the `!self.ds` change does not touch them; pre-existing baselined. |

So the DS sprite m3stat **read-frame regression** batch is EXHAUSTED at +87/−3. The
window/late-disable/size-change residue is C2 render (production-level, would break
byte-identical OFF) — the goal's separately-scoped "window family, render C2" work,
not a tier2 read-grid slice.

## Tooling

- slopgb trace: `target/trace/release/examples/run_gambatte <rom> cgb`,
  `SLOPGB_TIER2=1 SLOPGB_S5DBG=1` (committed `SLOPGB ff41` read-dot tracer). Temp
  `SLOPGB m0flip` (mode0.rs dispatch dot) + `ff41val` (memory.rs override path)
  tracers used to pin the mechanism, reverted after.
- SameBoy `--cgb --length 4` (DS), `SB_TRACE=1` → `SBREAD ff41` + `SBMODE`.
- Two-bin `flagon_probe` over the 3524-CGB rowlist.
