# EAGER non-gambatte suite re-host — the coherent-flip real-regression list; simple `|| eager_value` vein DRAINED (2026-07-11, #11dt)

Base: `finish-port-halfdot @ 74d67cb` (#11ds construction fix + `GameBoy::new_with_eager`).
Task: identify + re-host the non-gambatte (gbmicrotest / mealybug / age / wilbertpol)
eager C3-flip regressions — the `tier2_*` laws that fire under tier2 but not eager.

## TL;DR

- **15 real regressions** under the COHERENT flip, measured two independent ways
  that AGREE EXACTLY (`GameBoy::new_with_eager` == the temp-flipped
  `interconnect.rs` struct-literal default — coherence proven, no incoherent-flip
  artifact).
- **Zero clean `|| eager_value` re-hosts remain at this base.** Every one of the 15
  is a counter-pinned **dispatch-frame (Part-A)** row, a **CGB accessibility /
  mode-3-start render frame (Part-B)** row, or a **documented A/B-swap refutation**
  (SCX-DMG). The simple re-host vein the task targeted was already fully drained by
  #11cm / #11dj / #11dp (the earlier "~4-6 laws" estimate came from a pre-fix /
  incoherent-flip read; the coherent measurement shows the residual is all deep).
- **Shipped:** the tier2-pin HYBRID hygiene fix (`set_tier2_reclock(true)` now clears
  `eager_value`) + a measurement env-gate (`SLOPGB_GBTR_EAGER` / `SLOPGB_GBTR_TIER2`
  in `harness::boot`). Both inert in production (defaults false) — golden
  byte-identical, all steady-state two-bins unchanged.

## Measurement method

`tests/gbtr/harness.rs::boot` gained two env gates (unset ⇒ production frame,
byte-identical):

- `SLOPGB_GBTR_EAGER=1` → `GameBoy::new_with_eager` (the coherent #11ds C3-flip).
- `SLOPGB_GBTR_TIER2=1` → `GameBoy::new_with_reclock` (pure deferred tier2).

Run the four matrices under each; a case "failing but not in the OFF baseline" is a
regression, "now passing" (baselined OFF-fail now green) is a flip GAIN (drift). The
baseline-ratchet panic message IS the per-suite diff.

### Coherence cross-check (the task's explicit warning)

`new_with_eager` and the real temp-flip (`interconnect.rs:546/548` → `true`, reverted)
produce the **IDENTICAL 15-row regression set** — `diff` empty. The #11ds propagation
fix makes the struct-literal flip coherent; no incoherent-flip mismeasurement.

## Category separation

| category | count | what | regression? |
|---|---:|---|---|
| 1. DRIFT (flip gains) | 74 | baselined OFF-fails now GREEN under eager (age 5 / gbmicro 24 / mealybug 3 / wilbertpol 42) | NO — improvements + floor rebaselines |
| 2. tier2-PIN HYBRID | — | `tier2_*_passes` pins run tier2∧eager under the temp-flip (`set_tier2_reclock` didn't clear `eager_value`) | NO — test-harness artifact (FIXED below) |
| 3. REAL regressions | 15 | OFF passes, coherent eager fails | YES |

Category 2 does not appear in the matrix measurement at all: the matrices run only
the `_matrix` tests via pure `new_with_eager` (no tier2). It manifests ONLY when the
interconnect default is temp-flipped AND a pin calls `set_tier2_reclock` — verified:
without the hygiene fix, `tier2_dmg_hblank_if_passes` FAILS under the temp-flip (1/3),
with it all 3 pass.

## The 15 real regressions — every one refuted for a simple re-host

| # | case | group | tier2? | verdict |
|---|---|---|:--:|---|
| 1 | `age-test-roms/m3-bg-bgp` [Cgb] | A CGB render | PASS | Part-B (accessibility / mode-3-start) |
| 2 | `mealybug ppu/m3_bgp_change` [Cgb] | A | PASS | Part-B |
| 3 | `mealybug ppu/m3_window_timing` [Cgb] | A | PASS | Part-B |
| 4 | `mealybug ppu/m3_window_timing_wx_0` [Cgb] | A | PASS | Part-B |
| 5 | `mealybug ppu/m3_wx_4_change_sprites` [Cgb] | A | PASS | Part-B |
| 6 | `mealybug ppu/m3_scx_high_5_bits` [Dmg] | B SCX-DMG | PASS | REFUTED #11cm (length-coupled A/B swap) |
| 7 | `mealybug ppu/m3_scx_low_3_bits` [Dmg] | B | PASS | REFUTED #11cm |
| 8 | `gbmicrotest ppu_sprite0_scx2_b` [Dmg] | C sprite0 | PASS | REFUTED here (grid-snap breaks 21 EV DMG) |
| 9 | `gbmicrotest ppu_sprite0_scx6_b` [Dmg] | C | PASS | REFUTED here |
| 10–15 | `wilbertpol ly_lyc_153_write-{GS[Dmg,Mgb,Sgb,Sgb2],C[Cgb,Agb]}` | D line-153 | FAIL | Part-A (B=48 dispatch-frame; SameBoy-pass) |

"tier2?" = does the pure-tier2 clock (`new_with_reclock`) pass the row. Group D is the
only set tier2 ALSO fails — i.e. no tier2 law exists to re-host; it is the deferred /
eager dispatch frame itself.

## Group-by-group evidence

### A — CGB mode-3 render (5): Part-B, NOT a write-commit law (empirically confirmed)

`pixel_probe[ON] 4/4`, `[EV] 0/4` (age skipped by the probe's known-set). The eager
render commits ~12px late (`m3_bgp_change`: the `#7BFF31` block that belongs at x=2
appears at x=14). The CGB write-commit render debt (`regs.rs::stage_write`, +8hd SS)
is ALREADY applied and matches tier2. Swept `SLOPGB_WCOMMIT` (the ×2 debt knob) over
{−24…+8}: negative = no change, +4/+8 recovers only 1/4 (uniform ⇒ would shift DMG
too). **The +12px shift is the mode-3-START / accessibility frame, not the palette
commit** — exactly #11cm's Part-B classification. No discrete `tier2_reclock`-gated
render law governs it; the difference is the eager clock's inherent render-frame
offset. Left for the coherent per-T retime (HALFDOT Part A/B).

### B — DMG mode-3 SCX (2): REFUTED (#11cm, `eager-dmg-render-rehost-2026-07-09.md`)

Adding the FF43 render debt recovers both SCX pixel rows but is a forbidden one-sided
A/B swap: it drops the SameBoy-PASS `late_scx_late_disable_1` + `ly0_late_scx7_scx0_2`
OCR rows. `eff.scx`'s fine-scroll discard IS the mode-3 length, so no dot carries the
render debt without shifting the FF41 length verdict. The #11bq render/read split does
not save it. Needs the coherent length reclock.

### C — DMG sprite0 (2): REFUTED here (measured A/B)

`ppu_sprite0_scx2_b/scx6_b` read `$FF80=0x83`, want `0x80` (mode-3 sprite penalty 3
dots long under eager). The only tier2 lever is the sprite-line grid-snap
(`mode0.rs::early_lead`/`snap_ok`, `tier2_reclock`-gated). A/B: extending it to
`eager_value && !is_cgb && has_sprites` — **did NOT fix sprite0 (still 0x83) AND
regressed EV DMG 54→75 (+21) + broke 12 gbmicrotest rows**. Reverted. This is the
coupled render∧dispatch (Part-A) the docs warn moves the sprite-line dispatch and
breaks `intr_2_*_sprites`.

### D — wilbertpol ly_lyc_153_write (6): Part-A dispatch-frame

All six models break with `B=48` (not the Fibonacci `03`) — the classic dispatch
shift. Prior census (`c3-flip-census-2026-07-04.md`) classifies these SameBoy-PASS
(`--dmg` 4 + `--cgb` 2), and `dispatch-retime-plan-2026-07-04.md` names them
"line-153 LYC dispatch-frame". tier2 ALSO fails them (no ported law); a read-law
`|| eager_value` cannot move a counter-pinned dispatch count. Lands with the C3-flip
per-T retime (HALFDOT Part A). (Sibling `timer_if` ×4 breaks under tier2 but PASSES
under the DMG-count-safe eager clock — not an eager regression.)

## The task's named cases — all already fixed / hybrid, NOT current regressions

- `mealybug m3_bgp_change [Dmg]` — fixed by #11cm slice 1 (the [Cgb] leg is the
  residual, group A).
- `age halt-m0 / m3-bg` — halt-m0 passes eager; `m3-bg-bgp` is the [Cgb] group-A row.
- `gbmicrotest int_hblank_halt_scx1` / `hblank_int` — PASS under eager (the DMG-safe
  clock recovers them); only fail under pure tier2, covered by the `tier2_*` pins
  (category 2 hybrid), never a matrix regression.
- `wilbertpol intr_0_timing` — passes under eager (not in the regression set).

## Shipped changes (both production-inert)

1. `interconnect/cycle.rs::set_tier2_reclock(true)` now clears `eager_value`
   (+ `ppu.set_eager_value(false)`). The two clocks are mutually exclusive; this makes
   a `set_tier2_reclock`-built machine run PURE tier2 even under the temp-flip.
   Red-before-green: without it, `tier2_dmg_hblank_if_passes` fails under the temp-flip.
   Inert in production (eager already off → no-op).
2. `tests/gbtr/harness.rs::boot` env gates `SLOPGB_GBTR_EAGER` / `SLOPGB_GBTR_TIER2`
   (the whole-suite OFF-vs-flip diff harness). Unset ⇒ byte-identical.

## Gates (all green)

- `golden_fingerprint` byte-identical.
- EV CGB **295**, EV DMG **54**, tier2 CGB **291**, tier2 DMG **116** — all unchanged.
- mooneye 3-clock **93 / 93 / 93** (production / `MOONEYE_RECLOCK` / `MOONEYE_EAGER`).
- 4 matrices green OFF; clippy `-D warnings` clean; `cycle.rs` 543 / `harness.rs` 406.

## Endgame

The non-gambatte simple-re-host vein is drained. The 15 residuals are the same three
walls the gambatte-OCR work hit: counter-pinned dispatch (ly_lyc_153, sprite0), the
CGB mode-3-start/accessibility frame (5 render rows), and the SCX length coupling.
All land with the coherent per-T dispatch/read retime (HALFDOT Part A), not a
flag-gated law port.
