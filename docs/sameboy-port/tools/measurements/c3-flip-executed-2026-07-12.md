# C3 FLIP EXECUTED — the eager-value clock is the production default (2026-07-12, #11cu)

**Branch `c3-flip-exec` off `finish-port-halfdot @ 37a2aaa`. Isolated worktree,
NOT pushed.** The parent re-verifies golden scope + zero-drop before integrating.

The flip flips two `interconnect.rs` struct-literal defaults
(`leading_edge_reads` + `eager_value` → `true`; `tier2_reclock` stays `false`).
`post_boot_inner`'s deferred PPU re-arm makes the raw default coherent
(verified by construction — no `new_with_eager` needed for production; mooneye
93/93). The floor census
(`eager-flip-floor-census-2026-07-12.md`) proved the gambatte-OCR floor = 0;
this session CONFIRMED the non-gambatte suites are 0-drop and executed the
mechanical flip + rebaseline.

## Verdict: FLIPPED, CLEAN. Zero SameBoy-pass drops across ALL suites.

## STEP 1 — non-gambatte confirm (0 SameBoy-pass drops)

Ran the whole battery under `SLOPGB_GBTR_EAGER=1` (`new_with_eager`, byte-identical
to the flipped `new()` for ROM runs — see the #11cv verification below: `new()`
does NOT call `arm_eager_construction_default`, but with the struct-literal flip
`bus.eager_value()` is already `true`, so `post_boot_inner`'s suppress/re-arm
runs for `new()` too; `arm` is a redundant no-op inside `new_with_eager`). Every
non-gambatte
suite failed its OFF baseline with **only "now passing" (stale) entries — ZERO
"failing but not in baseline" regressions.** Pure gain:

| suite | now-passing (removed) | new regressions |
|---|---|---|
| age | 5 | 0 |
| gbmicrotest | 25 | 0 |
| mealybug | 3 | 0 |
| same_suite | 1 | 0 |
| wilbertpol | 42 | 0 |
| blargg / acid / mooneye2022 / smallsuites | 0 | 0 (no drift) |

The only suite with new regressions is gambatte (44). ABORT GATE not triggered.

## STEP 5 — gambatte rebaseline: 44 SameBoy-FAIL trades, 327 now-passing removed

All 44 new gambatte regressions classify SameBoy-FAIL (rebaseline-OK): the
gambatte cgb04c/dmg08 reference value SameBoy also does not produce. BUG
(sb==want, would ABORT) = **0**.

- **37 CGB-OCR** — `classify_cgb_regr.py` BUG=0 / FLOOR=37 / UNK=0.
- **6 DMG-OCR** — `classify_dmg.py` BUG=0 / FLOOR=6 / UNK=0.
- **1 CGB-PNG** — `scy/scy_during_m3_ds_5.gbc [Cgb]`: reference wants #000000 at
  (8–15, row 0); SameBoy `--cgb` produces a bright non-black pixel there
  `(0,255,255)` — does not match the gambatte reference → SameBoy-fail (the DS
  mid-dot floor the census flagged as tracked-separately / all-SameBoy-fail).

### 37 CGB-OCR floor (sb ≠ want)

```
display_startstate/stat_scx2_2_cgb04c_out84            sb=80 want=84
lcd_offset/offset1_lyc99int_m0stat_count_scx2_ds_2     sb=00 want=90
lcd_offset/offset3_lyc98int_ly_count_1                 sb=00 want=99
lcd_offset/offset3_lyc99int_m0stat_count_scx1_2        sb=00 want=90
lyc153int_m2irq/lyc153int_m2irq_ifw_1                  sb=0  want=2
lycEnable/late_ff41_enable_ds_lcdoffset1_2             sb=2  want=0
lycEnable/late_ff45_enable_ds_lcdoffset1_2             sb=2  want=0
lycEnable/lyc0_m1disable_2                             sb=E2 want=E0
lycEnable/lyc153_late_enable_m1disable_2               sb=E2 want=E0
lycEnable/lyc153_late_ff41_enable_ds_lcdoffset1_2      sb=E2 want=E0
lycEnable/lyc153_late_ff45_enable_ds_lcdoffset1_2      sb=E2 want=E0
lycEnable/lyc153_late_m1disable_2                      sb=E2 want=E0
lycEnable/lycwirq_trigger_ly00_stat50_ds_lcdoffset1_2  sb=E0 want=E2
m0enable/lycdisable_ff45_2                             sb=2  want=0
m0enable/lycdisable_ff45_scx1_2                        sb=2  want=0
m0enable/lycdisable_ff45_scx2_2                        sb=2  want=0
m0enable/lycdisable_ff45_scx3_2                        sb=2  want=0
m1/ly143_late_m0enable_2                               sb=3  want=1
m1/m1irq_late_enable_2                                 sb=2  want=0
m1/m1irq_late_enable_ds_lcdoffset1_2                   sb=2  want=0
m1/m1irq_m0disable_2                                   sb=3  want=1
m1/m1irq_m2disable_lycdisable_2                        sb=3  want=1
m1/m1irq_m2disable_lycdisable_3                        sb=3  want=1
m1/m1irq_m2disable_lycdisable_ds_2                     sb=3  want=1
m1/m1irq_m2enable_lyc_1                                sb=3  want=1
m1/m1irq_m2enable_lyc_ds_1                             sb=3  want=1
m1/m2m1irq_ifw_2                                       sb=3  want=1
m1/m2m1irq_ifw_ds_2                                    sb=3  want=1
m2enable/late_enable_m1disable_ly0_2                   sb=2  want=0
m2enable/late_m1disable_ly0_2                          sb=2  want=0
miscmstatirq/lycstatwirq_trigger_ly00_10_50_1         sb=E2 want=E0
window/arg/late_wy_1                                   sb=3  want=0
window/late_disable_late_scx03_wx0f_2                  sb=3  want=0
window/late_disable_scx2_1                             sb=3  want=0
window/late_disable_scx3_1                             sb=3  want=0
window/late_disable_scx5_1                             sb=3  want=0
window/late_wy_1                                       sb=3  want=0
```

### 6 DMG-OCR floor (sb ≠ want)

```
m0enable/lycdisable_ff41_scx3_2                        sb=0  want=2
m1/m1irq_m2disable_lycdisable_3                        sb=3  want=1
m1/m1irq_m2enable_lyc_1                                sb=3  want=1
m1/m1irq_m2enable_lyc_2                                sb=3  want=1
m1/m2m1irq_ifw_2                                       sb=3  want=1
miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_4  sb=E2 want=E0
```

4 of the 44 already existed in `gambatte.txt` as `[Dmg]` keys but surface as
`[Cgb]` flip-BUGs — added as new `[Cgb]` entries (census §4):
`window/late_disable_scx{2,3,5}_1`, `miscmstatirq/lycstatwirq_trigger_ly00_10_50_1`.

### Baseline edits

- `baselines/gambatte.txt`: −327 now-passing, +44 SameBoy-fail (C3 swap block).
- `baselines/gbmicrotest.txt`: −25. `mealybug.txt`: −3. `wilbertpol.txt`: −42.
- `tests/gbtr/age.rs` inline BASELINE: −5. `same_suite.rs` inline BASELINE: −1.

## STEP 4 — golden regen scope

`fingerprint.txt` 9020 → 9020 lines. **0 new keys, 0 missing keys** (`comm`),
**567 CHANGED values** — all eager-render frames: gambatte 458, gbmicrotest 63,
wilbertpol 25, mealybug 10, age 7, same-suite 2, scribbltests 1, mooneye 1.
**ZERO blargg/acid drift** (forbidden-zero suites untouched).

## Gate results (all green)

| gate | result |
|---|---|
| STEP 3 mooneye full matrix (`--test mooneye --release`) | **93/93** |
| STEP 6 gbtr battery (`--test gbtr --release`, post-rebaseline) | **278/0** (4 ignored) |
| core lib (`--lib`) | **762/0** (updated `production_new_is_c3_eager_default` + `ic()` OFF-neutralize) |
| frontend (`-p slopgb --bins`) | **508/0** |
| clippy (`--workspace --all-targets -D warnings`) | clean |
| line caps | my touched files < 1000 (interconnect 684, age 627, same_suite 361). 5 files >1000 (window.rs 1143, windows.rs 1127, lib_tests.rs 1102, cartridge.rs 1013, oam_vram.rs 1007) are **pre-existing at base 37a2aaa** (un-split finish-port-halfdot; the splits live on the SGB/main line) — NOT flip-introduced. |

## Test-alignment fixes (folded into the rebaseline commit)

The flip broke 5 OFF-calibrated unit tests; fixed faithfully:
- `lib_tests::production_new_is_reclock_off` → `production_new_is_c3_eager_default`
  (asserts the new default `(leading_edge, tier2) = (true, false)`).
- `interconnect_tests::ic()` / `ic_cgb_mode()` now `set_eager_value(false)` after
  construction — the raw struct-literal flip arms the bus eager without the
  `post_boot_inner` PPU-propagation (an incoherent half-armed machine); these
  interconnect micro-timing units are calibrated to the OFF clock, so they run on
  the coherent OFF path (the eager default's correctness is pinned by the battery
  + mooneye). Fixes `subdot::leading_edge_ff41_reads_*`, `irq::dispatch_ack_*`,
  `speed::speed_switch_pause_*`.

## Commits (SSH-signed, %G?=G, not pushed)

- (a) `800fe8b` feat(core): the 2-line flip.
- (b) `52dbcb3` test(gbtr): golden regen (567 values).
- (c) the rebaselines + test-alignment + this doc.

## Note for the parent integration

`CLAUDE.md`'s golden-safe-law line ("never flip the interconnect.rs defaults in a
pushed commit") and the State section still describe the pre-flip OFF world — the
parent should reconcile them on integration (this exec branch left CLAUDE.md
untouched to keep the flip diff focused).

## #11cv — production-`new()` re-verification: the construction bug does NOT exist

**Premise investigated:** a report that production `GameBoy::new()` (flipped
defaults) diverges from the `cfg(test)` `new_with_eager()` that all flip
verification used — i.e. `new()` ships a "half-armed" machine (eager bus,
non-eager PPU) that fails ~6 gbtr suites (272/6, e.g. `int_oam_incs` FF80=0x70 vs
self-check FF81=0x6f), while `new_with_eager()` passes. Prescribed fix: route
`new()` through `eager_default=true`.

**Root-cause trace.** The only code difference between the two paths is that
`new_with_eager` calls `Interconnect::arm_eager_construction_default()`
(`cycle.rs:174`) before `post_boot_inner`'s suppress/re-arm; `new()` does not.
But `arm_eager_construction_default` sets exactly the two fields
(`eager_value`, `leading_edge_reads`) that the **struct-literal flip already sets
to `true`** (`interconnect.rs:551/553`). So on the flipped tree it is a pure
no-op. Both paths then reach `post_boot_inner`'s `let eager = bus.eager_value();`
(`lib.rs:209`) with `eager_value == true`, run the same suppress
(`set_eager_value(false)`) → `apply_post_boot_state()` → re-arm
(`set_eager_value(true)`) that propagates the flags to the PPU. **The re-arm — the
actual coherence mechanism — is unconditional on `bus.eager_value()`, which the
struct-literal flip makes `true` for `new()` too.** The two constructors are
therefore identical, and the comments at `cycle.rs:165-173` /
`lib.rs:188-216` are **correct as written** (no update needed): `arm` really is
reached only via `new_with_eager`, and production really does pass
`eager_default=false` — it just doesn't matter, because the flip pre-sets the
fields `arm` would set.

**Empirical proof (two independent nets).**
1. Full-collection fingerprint diff `new()` vs `new_with_eager()` (frame-16 XRGB +
   frame-16 raw-audio FNV, both models, whole v7.0 collection): **0 divergent
   (rom,model) pairs of 9020.** (Throwaway `diag_new_vs_eager`, removed after.)
   Caveat: the fingerprint captures pixels+audio, not HRAM — so the self-check
   verdict suites are covered by net 2.
2. `gbmicrotest_dmg_matrix` (reads the FF80/FF81/FF82 self-check verdicts) run
   under production `new()` (NO `SLOPGB_GBTR_EAGER`): **pass.** `int_oam_incs`,
   `win0_b`..`win15_b` etc. all self-check-pass — the reported 272/6 does not
   reproduce.

The report's "temp-edit `new()` → `eager_default=true` fixes it" observation is
only explicable on an **un-flipped** tree (struct literal still `false`), where
`arm` is NOT a no-op and `new()` would indeed be non-eager. On `c3-flip-exec`
@ `72b098a` the flip is applied and `new()` is coherent by construction.

**Production-path gates (all under production `GameBoy::new()`, NO env toggles;
`CARGO_TARGET_DIR=target/flipfix`):**

| gate | result |
|---|---|
| gbtr battery `--test gbtr --release` (incl. `golden_fingerprint`) | **278/0** (4 ignored) — run **twice** |
| `golden_fingerprint` vs committed `fingerprint.txt` | **0 drift** (committed golden was already captured under `new()` — no re-capture needed) |
| mooneye `--test mooneye --release` | **93/93** |
| core lib `--lib --release` | **760/0** |
| frontend `-p slopgb --bins --release` | **508/0** |
| clippy `--workspace --all-targets -D warnings` | clean |
| flip floor spot-check: `run_gambatte m1statwirq_3_dmg08_out2.gb dmg`, flipped default (no env) | OCR = **2** (== `out2`) |

**Verdict: no construction fix, no golden re-capture, no baseline correction
needed. `new()` == `new_with_eager` byte-for-byte; the flip agent's
`new_with_eager` verification is transitively valid for production `new()`; the
flip is production-verified.** Only doc changes this session (the inaccurate "both
call `arm`" line above + this section).
