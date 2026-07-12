# The eager-flip floor is ZERO SameBoy-pass regressions — both models (2026-07-12, #11cu census)

**Measurement-only. No code shipped, no defaults flipped. Base:
`finish-port-halfdot @ 5c91f68`** (the commit after the m1statwirq / LYC-153
cluster re-host landed). Isolated worktree; tree byte-identical.

## Bottom line (read this first)

The eager flip's **TRUE floor = 0 for BOTH models.** Every row the flip drops
that OFF passes is a gambatte-reference-specific value **SameBoy also does not
produce** — a rebaseline-OK A/B trade, not a real hardware regression.

- **CGB:** 37 flip-BUGs (OFF-pass ∩ EV-fail) → SameBoy classify **BUG=0 /
  FLOOR=37**. TRUE floor (SameBoy-pass drops) = **0**.
- **DMG:** 6 flip-BUGs → SameBoy classify **BUG=0 / FLOOR=6**. TRUE floor = **0**.
- **BATTERY floor (gbtr CI rows the flip drops that SameBoy passes) = 0.**

The flip is CLEAN on the gambatte-OCR battery: it recovers 236 CGB + 75 DMG
OFF-failing rows and introduces 37 CGB + 6 DMG rebaseline-OK gambatte-reference
trades, all SameBoy-fail. Net −199 CGB / −69 DMG fails.

This closes the arc the dispatch-web map opened: at `e307e7a` (EV CGB 358) there
were **16 SameBoy-pass TRUE-floor CGB rows** (the lycEnable-ff41 / m0enable /
m2int_m0irq / irq_precedence / ly0 / lyc153int_m2irq / m2enable / miscmstatirq
web). The intervening re-host sessions (through the 2026-07-12 m1statwirq /
LYC-153 cluster) drove EV CGB 358→287 and recovered ALL 16. What remains is the
SameBoy-FAIL residual the dispatch-web map already labelled "rebaseline-OK,
correctly left alone."

## 1. Two-bin counts (reproduced from scratch, all four tee files complete)

`flagon_probe` on `scratchpad/{cgb,dmg}_rowlist.txt` (both 3422 rows,
gambatte-only, md5 cgb=`35a1966…` dmg=`ff43f26…`), `SLOPGB_REQUIRE_ROMS=1`:

| bin | model | pass | fail | skip |
|---|---|---|---|---|
| OFF (`SLOPGB_PROBE_OFF=1`) | CGB | 2534 | **486** | 402 |
| EV  (`SLOPGB_PROBE_EV=1`)  | CGB | 2733 | **287** | 402 |
| OFF | DMG | 1500 | **103** | 1819 |
| EV  | DMG | 1569 | **34**  | 1819 |

OFF CGB 486 / EV CGB 287 reproduce the task baselines exactly. **EV DMG is 34,
not the task's stated 41** — the final cluster commit `5c91f68` ("re-host the
last 4 LYC-153 cluster siblings — DMG EV zero-drop") recovered 7 more DMG rows
than the pre-cluster figure. 34 is the authoritative post-`5c91f68` count (FAIL
lines recounted = 34, matches summary).

### Reproduction

```sh
cd <worktree>          # base 5c91f68; symlink test-roms/game-boy-test-roms-v7.0 from main repo
export CARGO_TARGET_DIR=target/census SLOPGB_REQUIRE_ROMS=1
cargo test -p slopgb-core --test gbtr --release --no-run
BIN=target/census/release/deps/gbtr-<hash>
SLOPGB_PROBE_OFF=1 SLOPGB_ROWLIST=$PWD/scratchpad/cgb_rowlist.txt \
  "$BIN" --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture
# repeat with SLOPGB_PROBE_EV=1 and dmg_rowlist.txt
```

## 2. flip-BUGs = OFF-pass ∩ EV-fail (NOT the whole EV-fail list)

Extract rel (field `$2`) from `FAIL` lines, `comm -23 ev_rels off_rels`:

| model | EV-fail | of which already OFF-fail (floored) | **flip-BUG (OFF-pass ∩ EV-fail)** |
|---|---|---|---|
| CGB | 287 | 250 | **37** |
| DMG | 34  | 28  | **6**  |

## 3. Classification (SameBoy tester `~/.cache/sbbuild/SameBoy-1.0.2/.../sameboy_tester`)

`classify_cgb_regr.py` (CGB) / `classify_dmg.py` (DMG). SameBoy-PASS (`sb==want`,
script bucket "BUG") = TRUE FLOOR; SameBoy-FAIL (`sb!=want`, bucket "FLOOR") =
rebaseline-OK. UNK=0 both (tester genuinely ran; real `sb` OCR values recorded).

| model | flip-BUGs | **TRUE FLOOR (SameBoy-pass)** | rebaseline-OK (SameBoy-fail) |
|---|---|---|---|
| CGB | 37 | **0** | 37 |
| DMG | 6  | **0** | 6  |

## 4. Battery membership — BATTERY FLOOR = 0

TRUE-floor set is empty → **no SameBoy-pass row the flip drops is in any gbtr
baseline. The load-bearing battery floor is 0.**

For completeness, of the 40 unique flip-BUG rels, 4 already appear in
`baselines/gambatte.txt` — but all 4 as `[Dmg]` keys, while they surface as
flip-BUGs under `[Cgb]`. So at flip time they need a new `[Cgb]` baseline entry
(a documented SameBoy-fail A/B trade), not a real regression:
`window/late_disable_scx{2,3,5}_1_…`, `miscmstatirq/lycstatwirq_trigger_ly00_10_50_1_…`.

## 5. Family breakdown of the flip-BUG set (all SameBoy-fail / rebaseline-OK)

**CGB (37):** m1 11 · lycEnable 8 · window 6 · m0enable 4 · lcd_offset 3 ·
m2enable 2 · miscmstatirq 1 · lyc153int_m2irq 1 · display_startstate 1.
DS/lcdoffset subset = 10/37.

**DMG (6):** m1 4 · miscmstatirq 1 · m0enable 1. DS subset = 0.

These are exactly the SameBoy-FAIL residual the dispatch-web map
(`eager-dispatch-web-reachability-2026-07-10.md`) enumerated as "rebaseline-OK,
correctly left alone" — the `m0enable/disable_*`, `lycdisable_ff45_scx*`, `m1/*`,
`m2enable/late_m1disable_ly0_*`, `lyc0_m1disable` families. SameBoy itself
produces a value (`sb=3` where want=1, `sb=E2` where want=E0, `sb=2` where
want=0, `sb=80` where want=84) that diverges from the gambatte cgb04c/dmg08
reference. These are gambatte hardware-revision-pinned values, not accuracy bugs.

## 6. Assessment — re-hostable vs counter-pinned

There is **nothing left to re-host on the gambatte-OCR battery**: the TRUE floor
is 0. The 37+6 flip-BUGs are NOT counter-pinned dispatch bugs — they are
gambatte-vs-SameBoy reference divergences (SameBoy fails them too), so no
read-frame lever, half-dot FSM, or dispatch retime would "fix" them; they are
correctly rebaselined at flip time.

**Scope caveat:** this census is the **gambatte-OCR battery only** (both rowlists
are 100% `gambatte/` rows). The eager flip's residual on the OTHER fronts is
tracked separately and is NOT measured here:
- **DS mid-dot floor** (`_ds`/`lcdoffset` accessibility, sprites m3stat_ds) —
  per `eager-partA-buildplan` the emergent-flip / case-tower residual; the 10
  DS flip-BUGs above are the OCR-visible slice and are all SameBoy-fail.
- **halt-wake** clock port (`eager-wake-clock-port-2026-07-11.md`).
- **HDMA / DMA-service** `defer_steal` eager replication.
- non-gambatte suites (gbmicrotest / mealybug / wilbertpol / mooneye) — run
  their own coherent-eager runners per the rom-diff-weld gate contract before the
  actual C3 flip.

**Recommendation for the flip:** the gambatte-OCR bar is CLEAN (0 SameBoy-pass
drops). The C3-FLIP-CHECKLIST's gambatte gate is met on this base; the flip's
remaining risk is entirely in the non-gambatte suites + the golden regen, not in
any counter-pinned gambatte dispatch web.
