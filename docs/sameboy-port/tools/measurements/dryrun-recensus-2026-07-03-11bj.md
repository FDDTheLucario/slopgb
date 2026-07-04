# #11bj fresh dry-run re-census (2026-07-03)

Re-ran the C3 step-3 dry run (temp-flip `GameBoy::new → new_inner(…, true)`,
full gbtr battery, REVERTED — not committed) with the DMG window port shipped,
to re-census §3b.

## Battery result (defaults temp-flipped)

| suite | #11bi dry run | #11bj dry run | Δ |
|---|---|---|---|
| **gambatte_matrix new-fail** | **276** | **220** | **−56** |
| gbmicrotest (DMG) | 68 | 68 | 0 |
| mealybug | 20 | 20 | 0 |
| wilbertpol | 10 | 10 | 0 |
| age | 3 | 3 | 0 |
| golden_fingerprint | 985 drift | 985 drift | 0 |
| mooneye/blargg/acid/same_suite/smallsuites | flip-clean | flip-clean | 0 |

Full run: **230 passed / 7 failed** (the 7 = the suite matrices that still
carry flip-regressions: gambatte, gbmicrotest, mealybug, wilbertpol, age,
same_suite, golden). vs #11bi identical suite set.

## Verdict — the window port is the ONLY §3b lever that moved

The gambatte flip-regression count dropped **exactly 56** (276 → 220) — the 56
DMG window blockers the #11bj port fixed, and **nothing else changed**. This
confirms:

1. The DMG window arms are FF41-read-only: zero effect on the engine suites
   (gbmicrotest/wilbertpol/age unchanged) or the render suites
   (mealybug/golden unchanged) under the flip.
2. The CGB side is byte-identical (the two-bin already showed 291/291); the
   −56 is entirely the DMG window family.

## §3b residual (the 220 gambatte + 101 non-gambatte, all measured atomic)

- **DMG window: 6 residual** (wxA6/wxA5 carried-read sub-dot wall + scx5
  non-linear + late_scx + render-trigger) — atomic, same classes CGB parks.
- **CGB-OCR 37** — census-0 holds (SameBoy-FAIL, rebaseline).
- **non-window DMG-OCR singles 8** + **engine 79** (gbmicrotest 68 +
  wilbertpol 10 + age 1) — the dispatch/boot-frame/read-clock atomic core
  (`dmg-engine-set-classify-2026-07-03.md`).
- **pixel 100 SameBoy-PASS blockers** — the mode-3 render-reclock atomic core
  (`pixel-classify-2026-07-03.md`); 13 DMG rebaseline; 12 golden-review.
- golden 985 regen (C4).

**The §3b LAW-shaped levers are DRAINED.** Every residual is the counter-pinned
global dispatch reclock (engine) or the production render reclock (pixel) —
i.e. the C3 flip event itself, not an incremental flag-gated §3b slice. The
window family was the last cleanly-decoupled (FF41-read-verdict) lever, and it
is shipped. Defaults NOT flipped.
