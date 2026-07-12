# DS extension of the eager SCX write-commit crack (2026-07-12, #11en)

Task: `rom-diff-weld` the 4 CGB **double-speed** `scx_during_m3_ds` eager pixel
regressions — the DS-branch extension of the shipped single-speed DMG SCX
write-commit cracks (#11el post-match / #11em pre-match). SS scoped `!self.ds`;
the DS (`self.ds`) branch fell through to the **uniform** CGB-DS render debt `4`,
which is the exact false-weld shape the skill warns of.

**Result: CRACKED. All 4 targets PASS eager (8px → 0px), ZERO OFF-passing rows
dropped, golden byte-identical, OCR flagon_probe EV 287/287 zero-drift.** The
uniform CGB-DS debt of 4 over-shot the eager cc+0 commit by exactly one whole dot
past the OFF/tier2 post-lock commit; the discriminated post-match arm at debt 2
lands it on the exact OFF dot.

## The 4 targets (verified pass OFF, fail eager; SameBoy byte-structural = ref)

* `scx_during_m3/scx_0060c0/scx_during_m3_ds_5.gbc` [Cgb]
* `scx_during_m3/scx_0060c0/scx_during_m3_ds_8.gbc` [Cgb]
* `scx_during_m3/scx_0063c0/scx_during_m3_ds_5.gbc` [Cgb]
* `scx_during_m3/scx_0063c0/scx_during_m3_ds_8.gbc` [Cgb]

(`scy/scy_during_m3_ds_5` is the pre-existing FLOOR — SameBoy matches neither ref
nor eager — left alone; it is the only remaining `pass OFF, fail EV` row in the
scx/scy universe after the fix.)

## The discriminator: same fine-scroll comparator lock as SS, DS grid

Each `scx_during_m3_ds` ROM writes SCX twice per line (`00→60`, `60→c0`) — BOTH
**POST-match**: after this line's fine-scroll comparator lock
(`Render::hunt_done` at `hunt_match_dot=89`), write dots 90/232 (`_ds_5`) and
96/226 (`_ds_8`), all `> 89`. A post-match write is a pure COARSE/tile shift with
no mode-3-length effect — the discard is already fixed. Trace of the FF43 commit
dot under the three coherent clocks (`run_gambatte` + `new_with_eager`,
`SLOPGB_S5DBG` probe on `stage_write`/`commit_eff` FF43 + the hunt), ly=100
representative:

| clock | write-A stage | write-A commit | write-B stage | write-B commit | verdict |
|---|---|---|---|---|---|
| OFF (pass, ground truth) | — | **dot 93** | — | **dot 235** | ref |
| tier2 (pass) | dot 91 (cc+4) | dot 93 | dot 233 | dot 235 | ref |
| **eager, debt 4 (fail)** | dot 90 (cc+0) | **dot 94** | dot 232 | **dot 236** | +1 dot late |
| **eager, debt 2 (fix)** | dot 90 (cc+0) | **dot 93** | dot 232 | **dot 235** | = OFF |

The eager write stages at the cc+0 leading edge (dot 90 — 1 dot before tier2's
cc+4 dot 91). The strobe advances per half-dot; `dots_hd = dots*2 + debt`, commit
= stage + `dots_hd`/2 whole dots. With the uniform CGB-DS debt 4 → `dots_hd`=8 →
commit dot 94 (**+1 late**). Debt **2** → `dots_hd`=6 → commit dot 93 = the exact
OFF/tier2 post-lock dot. `_ds_8` is identical shifted (stage 96 → commit 99 want,
100 at debt 4).

## The fix — a DS post-match SCX render debt (`Ppu::stage_write`, FF43 CGB DS)

```rust
} else if self.ds {
    match addr {
        0xFF43 if self.render.hunt_done && self.dot > self.render.hunt_match_dot => 2,
        _ => 4,
    }
} else { /* CGB single-speed per-register */ }
```

`eager_value` + `is_cgb` + `ds`-scoped. Post-match-guarded exactly like the SS
`!self.ds` arm (`hunt_done && dot > hunt_match_dot`) — the `dot > hunt_match_dot`
guard rejects a line-start write whose `hunt_done` is stale from the previous
line. Pre-match / line-start DS SCX writes keep the uniform 4.

### Debt sweep (measured, not assumed)

| debt | 4 targets (pixel) | full scx/scy `pass OFF, fail EV` |
|---|---|---|
| 4 (old uniform) | 0/4 (+1 dot late) | 5 (4 targets + scy_ds_5 floor) |
| 3 | 4/4 | 1 (scy_ds_5 floor) |
| **2** | **4/4** | **1 (scy_ds_5 floor)** |
| 1 | 4/4 | 1 (scy_ds_5 floor) |
| 0 | 4/4 | 1 (scy_ds_5 floor) |

0/1/3 also clear the 4 targets on pixel tolerance, but **2 is the physically-
correct value** — the only debt that lands the eager commit on the EXACT OFF/tier2
commit dot (93/99), matching the ground-truth trace above. Chosen 2.

## The sole at-risk sibling: `scx_0060c0/scx_during_m3_ds_1` — safe

`_ds_1` is the only DS scx row that passes BOTH OFF and EV. Its write-A is
**pre-match** (`hunt_done=false`, line-start) → keeps debt 4, untouched. Its
write-B is post-match (eager commit 244 vs OFF 243, +1 late) but passes EV anyway
on tolerance; debt 2 moves it to 243 = exact OFF → still passes. No risk.

## Gates (all hold)

* **golden_fingerprint byte-identical** (render-path change, `eager_value`-scoped).
* Pixel two-bin EV (scx/scy, 150 rows): 4 targets recovered, `pass OFF, fail EV`
  set = `{scy_during_m3_ds_5}` (the untouched floor) both before and after ⇒ ZERO
  OFF-passing regressions.
* **OCR flagon_probe EV two-bin (cgb_rowlist, 3422 rows): 287/287 IDENTICAL** — my
  CGB-DS render change perturbs no OCR verdict (zero recovered, zero new). DMG OCR
  structurally untouched (the arm is `is_cgb && ds`-only; DMG has no double-speed).
* mooneye **93/93 × 3** (OFF / `SLOPGB_MOONEYE_EAGER` / `SLOPGB_MOONEYE_RECLOCK`).
* clippy `-D warnings` clean; `stage.rs` 313 / `eager_web.rs` 464 (both <1000).
* Red-before-green pin `eager_cgb_m3_render_scx_ds_passes` (`gambatte/eager_web.rs`):
  FAILS 8px with the arm's `2`→`4` (or removed), PASSES restored.

## Lesson

Same as #11el/#11em, one speed grid over: the uniform CGB-DS debt of 4 shifted ALL
DS SCX writes equally, so the 4 post-match targets could never separate from the
uniform — a false weld. The fine-scroll comparator lock (`hunt_done`, guarded by
`dot > hunt_match_dot`) is the representable discriminator on the DS grid too; debt
2 is the exact eager-cc+0-to-OFF alignment (half the SS +1-dot debt because the DS
M-cycle is 2 dots, not 4).
