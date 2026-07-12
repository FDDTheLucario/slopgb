# EAGER non-gambatte re-attack with the #11ec ROM-diff/full-trace/A-B method — all 15 residuals REFUTED as flag-gated read-laws; the flip is clean-modulo (7 render-LENGTH Part-B + 8 dispatch-frame Part-A). NO CODE SHIPPED, tree byte-identical @ b0f4cba (2026-07-11, #11eg)

Base: `finish-port-halfdot @ b0f4cba` (= #11ef). **NO CODE SHIPPED — tree
byte-identical @ b0f4cba.** Every experiment below was env-gated
(`SLOPGB_ARM8BIAS`, `SLOPGB_SPRTR`) or a throwaway test and REVERTED; `git status`
clean, `golden_fingerprint` byte-identical (41s).

## Task

The gambatte-OCR eager flip bar is 0 (CGB + DMG). The ONLY remaining flip
regressions are the 15 non-gambatte rows from #11dt
(`eager-nongambatte-rehost-2026-07-11.md`), classified "deep walls" BEFORE the
#11ec ROM-diff method and BEFORE #11ee/#11ef's STAT/FF0F/DS fixes. Re-measure
(some may have been fixed) and re-attack the survivors with the correct method
(`cmp -l` siblings → full-trace to the first WRITE divergence → representable
latch → recalibrate the eager-frame arm; the `rom-diff-weld` skill).

## STEP 1 — the CURRENT non-gambatte regression list: still exactly 15, none fixed by #11ee/#11ef

Coherent flip via `GameBoy::new_with_eager` (`SLOPGB_GBTR_EAGER=1`, the #11ds
construction path), the four matrices vs the OFF baseline:

| suite | regressions (OFF-pass → eager-fail, SameBoy-pass) |
|---|---|
| age | `m3-bg-bgp` [Cgb] — **1** |
| gbmicrotest | `ppu_sprite0_scx2_b`, `ppu_sprite0_scx6_b` [Dmg] — **2** |
| mealybug | `m3_bgp_change`/`m3_window_timing`/`m3_window_timing_wx_0`/`m3_wx_4_change_sprites` [Cgb], `m3_scx_high_5_bits`/`m3_scx_low_3_bits` [Dmg] — **6** |
| wilbertpol | `ly_lyc_153_write-{C[Cgb,Agb], GS[Dmg,Mgb,Sgb,Sgb2]}` — **6** |

**15, IDENTICAL to #11dt's set.** #11ee/#11ef fixed gambatte-OCR rows (a disjoint
set — the non-gambatte rows are absent from `{cgb,dmg}_rowlist.txt`); none of the
15 was touched. So all 15 still regress; re-attack each with the #11ec method.

## STEP 2 — every survivor REFUTED as a flag-gated read-law (trace + A/B backed)

Three groups, three distinct refutations. The upgrade over #11dt: the render rows
are now proven render-LENGTH by their pixel-shift signature (a read-law cannot
move a framebuffer pixel — the skill's one real exception), and sprite0 is now an
A/B-PROVEN rphd-weld (not just a grid-snap guess).

### Group A/B — the 7 render rows (5 CGB + 2 DMG SCX): render-LENGTH, unreachable by any read-law

The mealybug/age matrices OCR the **rendered framebuffer tiles**, not a STAT
digit. The pixel diffs are horizontal SHIFTS:

- `m3_bgp_change` [Cgb]: `(2,0) expected #7BFF31 got #FFFFFF` / `(14,0) expected
  #FFFFFF got #7BFF31` — the colour block that belongs at x=2 lands at x=14, a
  **12-px right shift** (the eager render commits mode-3 ~12 px late).
- `m3_window_timing`/`_wx_0` [Cgb]: the window column lands 4 px off (`(4,0)`
  black vs white for rows 0-7).
- `m3_scx_high_5_bits`/`m3_scx_low_3_bits` [Dmg], `m3_wx_4_change_sprites` [Cgb]:
  same — pixels displaced in x.

**A read-law (`vis_mode_read`, which only changes the FF41/FF44 register VALUE)
cannot move ANY rendered pixel** → these are render-LENGTH by construction, NOT a
read-frame miss. #11dt already swept the render-debt knob (`SLOPGB_WCOMMIT`,
{−24..+8}): uniform, recovers only 1/4, would shift DMG. The +12 px is the eager
render's inherent mode-3-START / accessibility frame offset — the coherent per-T
RENDER retime (HALFDOT Part B), a multi-session rewrite, not a flag-gated law.
These are render-only exemptions. (Same class as #11ef's parked
`10spritesPrLine_wx0..6` residual: "a RENDER-length fix, NOT a read-frame arm.")

### Group C — the 2 DMG `ppu_sprite0_scx{2,6}_b`: an A/B-PROVEN rphd weld with two gambatte rows (dispatch-frame)

`ppu_sprite0_scxN_b` reads **STAT (FF41)** and checks mode-3 vs mode-0 at the
mode-3→0 boundary: `_a` wants `0x83` (mode 3, still rendering), `_b` wants `0x80`
(mode 0, exited); the siblings' measurement reads sit one M-cycle apart. Full
trace (`SLOPGB_SPRTR`, `new_with_eager` vs `boot`), scx2:

| | measurement read PPU dot | rphd | render state | verdict |
|---|---|---|---|---|
| production `_b` (PASS) | raw **256** | 512 | flipped (`flip_dot=256`, native mode 0) | 0x80 ✓ |
| eager `_b` (FAIL) | raw **252** | 512 | pre-flip (`flip_dot=0`, proj 256) → arm-8 exit `2·256+2=514` | 512 < 514 → mode 3 → 0x83 ✗ |

The line is BARE (`n_sprites==0` at the read, flip 256 = `254+SCX&7`, NO sprite
penalty) — production passes at flip 256, so the render length is CORRECT; this is
NOT render-length. The `+8hd` read-debt aligns the VALUE position (both rphd 512),
but the eager CPU dispatches the read one M-cycle early (raw 252 vs 256), so the
read samples the pre-flip render and the exit law `2·flip+2` reconstructs mode 3.

**A/B proof it is a weld, not a mis-calibration** (`SLOPGB_ARM8BIAS` sweeps arm-8
SS bare exit `2·flip + bias`):

| bias | exit (scx2) | EV DMG fail | sprite0_scx2_b eager |
|---|---|---|---|
| **2** (ship) | 514 | **38** | FAIL (0x83) |
| 1 | 513 | 38 | FAIL (0x83) — 512 < 513 |
| 0 | 512 | **40 (+2 DROP)** | PASS (0x80) |

Recovering sprite0 needs `bias=0` (mode 0 at rphd ≥ 512) and that **drops exactly
two gambatte DMG rows** that read at the IDENTICAL rphd 512 wanting mode 3:
`gambatte/m2int_m3stat/m2int_m3stat_1` and `.../scx/late_scx4_1` (both
`_dmg08…out3`). An exact read-frame weld, opposite wants, ZERO representable
discriminator on the (bare, sprite-free) measured line — the only difference is
the counter-pinned CPU dispatch landing sprite0's read at raw dot 252. This is the
Part-A dispatch frame (#11dt Group C, now A/B-proven; the grid-snap lever it named
moves that dispatch → breaks `intr_2_*_sprites`, still true).

### Group D — the 6 `ly_lyc_153_write`: tier2 ALSO fails ⇒ no read-law to enable (dispatch/read-frame coherence)

All six models break `B=48` (not Fibonacci `03`) under eager **AND under pure
tier2** (`SLOPGB_GBTR_TIER2=1`, verified this session — identical `B=48`). tier2
also failing means there is NO ported `tier2_*` read-law to re-host under
`|| eager_value`; the fault is the counter-pinned dispatch/read frame itself. Per
the same-day #11dx trace (`eager-halfdot-cgb-lyc153-2026-07-11.md`): the eager
LYC=153 sync STAT fires at dot 6 vs production's dot 4, shifting the ROM's
downstream cycle count; the DMG-family back-date recovery is a **+17 CGB / +5 DMG
gambatte family shuffle** (a gate-violating one-sided A/B swap on the line-153 LYC
frame), and the CGB half is pure read-frame coherence (Agb already re-latches at
dot 4 yet still fails). Lands with the coherent per-T retime (Part A), not a
flag-gated law.

## Verdict — the flip is CLEAN-MODULO (render Part-B + dispatch Part-A)

The 15 non-gambatte residuals = **7 render-LENGTH (Part-B render retime)** + **8
dispatch/read-frame (Part-A per-T retime)**. NONE is a flag-gated `eager_value`
read-law — proven this session by pixel-shift signature (render), rphd-512 A/B
weld (sprite0), and tier2-also-fails + #11dx shuffle (ly_lyc_153). The simple
re-host / read-relatch vein is fully drained for the non-gambatte suites; the
remaining flip regressions are exactly the two coherent-retime walls the whole
port converges on. No further flag-gated attack surface.

## Gates (all hold — no code shipped)

| gate | value |
|---|---|
| `golden_fingerprint` (production, no port_probe) | **ok — byte-identical** (41s) |
| tree | `git status` clean; identical to `b0f4cba` |
| EV CGB / DMG | 287 / 38 (baseline, unchanged — no ship) |
| tier2 CGB / DMG | 291 / 116 (inherited, byte-identical tree) |
| mooneye OFF / RECLOCK / EAGER | 93 / 93 / 93 (inherited, byte-identical tree) |

## Do-not-re-chase ledger

- The 7 render rows OCR framebuffer pixels shifted in x (12 px on `m3_bgp_change`);
  a read-law cannot move a pixel. Render-LENGTH (Part-B), NOT a read-frame arm.
  Do not re-sweep `WCOMMIT` (uniform, refuted #11dt).
- `ppu_sprite0_scx{2,6}_b` welds at rphd 512 with `m2int_m3stat_1` / `late_scx4_1`
  (opposite wants); `bias=0` recovers sprite0 but drops those two gambatte rows.
  Dispatch-frame (Part-A). Do not lower arm-8's `+2` — it is load-bearing for the
  gambatte bare `out3` pair.
- `ly_lyc_153_write ×6` fail under tier2 too (no read-law exists); the DMG
  back-date is a +17 CGB gambatte shuffle (#11dx). Part-A. Do not re-chase a
  flag-gated port.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd12
cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/hd12/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# STEP 1 — the 15 (coherent flip):
for M in age::age_matrix gbmicrotest::gbmicrotest_dmg_matrix mealybug::mealybug_matrix wilbertpol::wilbertpol_matrix; do
  SLOPGB_GBTR_EAGER=1 SLOPGB_REQUIRE_ROMS=1 $BIN --exact $M --nocapture 2>&1 | grep "not in the known-failure"
done
# Group D — tier2 also fails ly_lyc_153:
SLOPGB_GBTR_TIER2=1 SLOPGB_REQUIRE_ROMS=1 $BIN --exact wilbertpol::wilbertpol_matrix --nocapture 2>&1 | grep ly_lyc_153
# Group C — the rphd-512 weld (needs the SLOPGB_ARM8BIAS env knob + spr_trace test, both reverted):
#   arm-8 bias 0 recovers sprite0 but drops gambatte m2int_m3stat_1 + late_scx4_1 (EV DMG 38→40).
```
