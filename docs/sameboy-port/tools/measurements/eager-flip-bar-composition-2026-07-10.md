# The TRUE flip bar, reproduced from scratch and DECOMPOSED — 49 CGB + 46 DMG = 95, and 19+23 of it is re-host work, not new physics (2026-07-10, #11cs)

Independent re-derivation of the eager flip bar at `42c54f6`, built from raw
two-bin captures with no inherited intermediate file, then bucketed by family
and by speed. Purpose: know exactly what HALFDOT Part A-render must clear, and
what is left standing after it.

## Method (reproduce end-to-end; every prior bar number came from a script whose
inputs nobody re-checked)

```sh
CARGO_TARGET_DIR=target/verify cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/verify/release/deps/gbtr-* | grep -v '\.d$' | head -1)
run(){ SLOPGB_ROWLIST=$PWD/scratchpad/$1 SLOPGB_REQUIRE_ROMS=1 env $2 \
       $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture; }
run cgb_rowlist.txt SLOPGB_PROBE_OFF=1 > cgb_off.txt   # 486
run cgb_rowlist.txt SLOPGB_PROBE_EV=1  > cgb_ev.txt    # 361
run dmg_rowlist.txt SLOPGB_PROBE_OFF=1 > dmg_off.txt   # 103
run dmg_rowlist.txt SLOPGB_PROBE_EV=1  > dmg_ev.txt    # 92
# flip-BUGs = OFF-pass ∩ EV-fail  (comm -13 off.keys ev.keys)
# TRUE bar  = flip-BUGs ∩ SameBoy-pass
python3 docs/sameboy-port/tools/classify_cgb_regr.py cgb_flipbug.rels
SLOPGB_GBTR_ROOT=$PWD/test-roms/game-boy-test-roms-v7.0 \
  python3 docs/sameboy-port/tools/classify_dmg.py dmg_flipbug.rels outprefix
```

All four two-bins reproduced their documented values EXACTLY (486 / 361 / 103 /
92). Both classifiers returned **UNK = 0**.

| | OFF fail | EV fail | flip-BUGs (OFF-pass ∩ EV-fail) | **TRUE bar** (∩ SameBoy-pass) | floor (rebaseline-OK) | EV gains (OFF-fail ∩ EV-pass) |
|---|---:|---:|---:|---:|---:|---:|
| CGB | 486 | 361 | 91 | **49** | 42 | 216 |
| DMG | 103 | 92 | 55 | **46** | 9 | 66 |

The flip trades **95 regressions for 282 gains** on the gambatte OCR set.

### Tooling traps hit while reproducing (fix these before trusting a bar)

- The classifiers live in **`docs/sameboy-port/tools/`**, NOT `scratchpad/`
  (several maps and a memory said `scratchpad/` — wrong path, and the scripts
  are not there).
- **`classify_dmg.py`'s default `ROOT` points at the stale `phase-b-s7`
  worktree.** Run it without `SLOPGB_GBTR_ROOT` and every row silently lands in
  UNK — a vacuous classification, not a bar.
- `classify_cgb_regr.py` hard-codes `/tmp/s7`; `sed` it to a job-local dir or
  parallel background runs clobber each other's `cls.bmp`.
- SameBoy verdict rule (patched `Tester/main.c:524` + `tools/hramdump.c`):
  `ff82==0x01` PASS, `0xFF` FAIL, else NOVERDICT. The classifiers instead OCR
  SameBoy's `cls.bmp` and compare to the `_outN` suffix — equivalent, and the
  one actually used.

## CGB bar (49) — dominated by the dispatch/IRQ web, and 21/49 are double-speed

| family | SS | DS | family | SS | DS |
|---|---:|---:|---|---:|---:|
| lycEnable | 5 | — | m2int_m0irq | 2 | 3 |
| halt | 5 | — | irq_precedence | 2 | 2 |
| enable_display | — | 4 | vram_m3 | 2 | 2 |
| cgbpal_m3 | 2 | 2 | oam_access | 1 | 2 |
| m0enable | 2 | — | m2int_m2stat | — | 2 |
| ly0 | 2 | — | lcd_offset | — | 2 |
| window | 2 | — | m2int_m0stat | — | 1 |
| lyc153int_m2irq | 1 | — | sprites | — | 1 |
| miscmstatirq | 1 | — | m2enable | 1 | — |

**19 of the 21 DS rows have an SS sibling that PASSES under EV.** That is the
signature of an un-ported slice, not of new physics: the shipped eager slices
are `!self.ds`-scoped and return native behaviour at double speed. Confirmed in
code — `ppu/blocking.rs:334,357` and `ppu/render/mode0.rs:256,280` all read
`(tier2_reclock || eager_value) && is_cgb() && !self.ds`. The 6 DS accessibility
rows (`vram_m3` 2 + `oam_access` 2 + `cgbpal_m3` 2) fall straight out of
`blocking.rs` alone.

(The 2 exceptions, whose SS sibling also fails: `cgbpal_m3end_scx5_ds_2`,
`preread_ds_lcdoffset1_1`.)

## DMG bar (46) — dominated by ONE family

| family | rows | family | rows |
|---|---:|---|---:|
| **window** | **23** | m2int_m0irq | 2 |
| halt | 6 | irq_precedence | 2 |
| m2enable | 3 | vram_m3 | 1 |
| lycEnable | 3 | oam_access | 1 |
| ly0 | 3 | m2int_m3stat | 1 |
| | | enable_display | 1 |

The 23 `window` rows: **12 are `arg/late_wy_*`** (`10to0_ly1`, `1toFF`, `2toFF`,
`FFto0_ly2`, `FFto1_ly2`, `FFto2_ly2_scx{2,3}` — `_1`/`_2` pairs), the rest
`late_disable_*_2`, `late_reenable{,_wx0f}_2`, `late_scx_late_disable_0`,
`m2int_wxA{5,6}_*_2`.

`#11ck` slice 2 ("eager WY cross-line + DS un-latch") cleared **all recoverable
`late_wy` CGB rows** — and was CGB-scoped. The DMG `late_wy` class was never
re-hosted. Note `#11ck` separately refuted a DMG **write-commit debt** as an A/B
trade; that is the debt, NOT the WY latch, and does not pre-refute this.

## What this means for Part A-render

Directly in Part A-render's blast radius: DMG `window` 23 + CGB `window` 2 +
`sprites` 1 + `m2int_m2stat` 2, plus — if #11cq's hypothesis holds that the
`intr_2_mode0` kernel-pair collapse is a symptom of the render-frame weld and
not an independent blocker — the `halt` rows, 5 CGB + 6 DMG. That is **≈39 of
the 95**.

Standing after it, as two separable re-host levers (both deliberately NOT
started now: Part A-render moves the dot these compare against, so doing them
first means doing them twice):

- **L1 — DS re-host of the shipped SS eager slices.** Up to 19 CGB bar rows.
  Un-scope `!self.ds` on `blocking.rs:334,357` / `mode0.rs:256,280` and the
  read-law arms, with the DS read-debt (+4hd, vs SS +8hd) already established by
  #11bz. The `lcd_shift_dots`/`sb_dsa8` DS half-dot alignment is the known
  sub-lever (parked at #11by; un-scoping reached 539 via a DS pair-shuffle).
- **L2 — DMG re-host of the window render-length laws**, `late_wy` first. Up to
  23 DMG bar rows, 12 of them the `late_wy` pairs, using the proven
  `|| eager_value` pattern.

Residual after L1+L2 would be the CGB dispatch/IRQ web — `lycEnable` 5,
`m2int_m0irq` 5, `irq_precedence` 4, `enable_display` 4, `m0enable` 2, `ly0` 2,
`lcd_offset` 2, `m2enable`/`m2int_m0stat`/`miscmstatirq`/`lyc153int_m2irq` 1 each
— which the older maps call "counter-pinned, lands WITH the flip". **That
label deserves re-testing, not inheriting**: #11cl proved the eager dispatch is
already at its true SameBoy position (cc+4 = production), so these rows cannot
be waiting on a dispatch move. They are OFF-pass ∩ EV-fail ∩ SameBoy-pass —
real regressions the flip would introduce — and the most likely explanation is
the same `vis_exit_hd` render-frame weld #11cq pinned. If Part A-render clears a
slice of them, the "counter-pinned" story was wrong.

## The bar is still an UNDERCOUNT

It is measured on the gambatte OCR rowlists only. The 2026-07-09 dry-run flip
also regressed `wilbertpol intr_0_timing` (since FIXED, #11cm), `gbmicrotest`
DMG `hblank_int_*`, `mealybug m3_bgp_change [Dmg]`, and `age` halt-m0 / m3-bg.
The last three are `tier2_dmg_*`-gated fixes that never fire under
`eager_value`. Add them to the eager gate before the C3 flip.

## Gate state

Read-only measurement session; no `.rs` touched. golden byte-identical, tier2
CGB 291, EV CGB 361 / EV DMG 92, mooneye 92 flag-off — all re-verified at
`42c54f6` before capture.
