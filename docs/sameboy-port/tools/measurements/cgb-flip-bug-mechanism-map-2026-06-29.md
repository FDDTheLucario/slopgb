# CGB flip-BUG mechanism map — the 31 non-DS in-scope rows (2026-06-29 #11ae)

The goal's deliverable: re-survey the post-#11ad CGB flip-BUG rows with the 4-step
method (observation trace → disassemble → SameBoy ground truth → whole-dot vs
atomic) **before** flooring, on the premise (from #11ad) that "several of 146 are
the same shape" — a wrong-dot dispatch that CGB-gates cleanly. This file is the
classification; the verdict for the in-scope set is **0 whole-dot CGB-gatable
slices** (the #11ad win was specific, not a pattern), with each row's mechanism
identified and grounded in build measurement.

## Scope

149 BUG rows (`cgb-flip-bug-classification-2026-06-28.md`); #11ad fixed 3
(enable_display glitch family) → 146 remain. The goal scopes the **non-DS**
dispatch-observation / read-frame / halt families and puts window 39 / cgbpal 7 /
DS S6-S7 ~37 OUT. That leaves **31 in-scope rows**: halt 12, lycEnable 10,
m2enable 3, ly0 2, miscmstatirq 2, irq_precedence 1, m2int_m3stat 1.

## Method / tooling (validated this session)

- **3-mode probe** (`gambatte_flagon_probe`, `SLOPGB_ROWLIST`): OFF (production) /
  LE (`SLOPGB_PROBE_LE`, leading-edge engine only, no tier2 render-frame) / ON
  (full `boot_with_reclock`). The **LE-vs-ON split is the primary mechanism
  discriminator**: LE-pass→ON-fail = a tier2 RENDER-FRAME bug; LE-fail = an
  ENGINE-DISPATCH bug (the `stat_update_tick` rising edge / IF lifecycle).
  OFF=31/31 pass, ON=0/31, LE=8/31 → **8 render-frame, 23 engine-dispatch**.
- **slopgb dual trace** (`run_gambatte` + `SLOPGB_TIER2/LE` + `SLOPGB_S5DBG`):
  `SLOPGB dispatch ly/dot/mfi` + `SLOPGB ff41/ff0f ly/dot/{mode,if}`.
- **SameBoy ground truth** (`sameboy_tester --cgb --length 4`, `SB_TRACE`):
  `SBTRACE STAT_IRQ ly/cfl/mfi` + `SBREAD ff41/ff0f`. (CGB needs `--length 4`,
  #11u.)

## Per-row table (build-measured)

`on`/`le` = the OCR'd flag-on / leading-edge result; `want` = SameBoy = the BUG
target. `class` from the LE split.

```
family          want  on   le    class   row
halt            0     2    2     ENGINE  late_m0int_halt_m0stat_scx2_3a
halt            0     2    2     ENGINE  late_m0int_halt_m0stat_scx3_3a
halt            0     2    pass  RENDER  late_m0int_halt_m0stat_scx2_1a
halt            0     2    pass  RENDER  late_m0int_halt_m0stat_scx2_2a
halt            0     2    pass  RENDER  late_m0int_halt_m0stat_scx2_4a
halt            0     2    pass  RENDER  late_m0irq_halt_m0stat_scx2_1a
halt            0     2    pass  RENDER  late_m0irq_halt_m0stat_scx2_2a
halt            0     2    pass  RENDER  m0int_m0stat_scx2_1
halt            0     2    pass  RENDER  m0irq_m0stat_scx2_1
halt            2     0    0     ENGINE  late_m0irq_halt_m0stat_scx3_3b
halt            6     7    7     ENGINE  late_m0irq_halt_dec_scx2_2
halt            6     7    7     ENGINE  late_m0irq_halt_dec_scx3_2
irq_precedence  E2    E0   pass  RENDER  late_m0irq_retrigger_scx1_1
ly0             E0    E2   E2    ENGINE  lycint152_lyc153irq_ifw_2
ly0             E2    E0   E0    ENGINE  lycint152_lyc153irq_2
lycEnable       0     2    2     ENGINE  late_ff41_enable_2
lycEnable       1     3    3     ENGINE  late_ff45_enable_2
lycEnable       1     3    3     ENGINE  late_ff45_enable_3
lycEnable       2     0    0     ENGINE  ff41_disable_2
lycEnable       3     1    1     ENGINE  ff45_disable_2
lycEnable       3     1    1     ENGINE  lyc_ff45_disable2_2
lycEnable       E0    E2   E2    ENGINE  lyc0_late_ff45_enable_2
lycEnable       E0    E2   E2    ENGINE  lyc153_late_ff41_enable_2
lycEnable       E2    E0   E0    ENGINE  lyc0_ff41_disable_2
lycEnable       E2    E0   E0    ENGINE  lyc0_ff45_disable_2
m2enable        0     2    2     ENGINE  lyc0_late_m2enable_lycdisable_2
m2enable        0     2    2     ENGINE  lyc1_m2irq_late_lyc255_2
m2enable        2     0    0     ENGINE  late_enable_ly0_lcdoffset2_1
m2int_m3stat    0     3    3     ENGINE  late_scx4_2
miscmstatirq    E0    E2   E2    ENGINE  lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_4
miscmstatirq    E0    E2   E2    ENGINE  lycwirq_trigger_m0_late_ly44_4
```

## The four mechanism classes

### 1. S7 sub-cc halt wake-clock floor — `halt` (12 rows) — ATOMIC, re-confirmed CGB

The mode-0-halt-wake FF41 read (`*_m0stat_*`, want 0/2) and the halt DEC counter
(`*_dec_*`, want 6). Measured: the want=0 (scx2) AND the want=2 (scx3/4/5) reads
ALL land at slopgb **`ly2 dot4 mode2`** under tier2 — identical state, opposite
expected modes. SameBoy reads scx2 inside the new line's dots-0-3 mode-0
line-start hold (`ly2 cfl0` → mode 0) and scx3/4/5 at the mode-2 commit (mode 2);
the distinguisher is the sub-T-cycle wake phase (#11m: want-0 flush ends `ly2
dc2`, want-2 at `ly2 dc8`) that slopgb's M-cycle-quantized deferred clock
collapses.

**Attempted + reverted this session** (`halt_ff41_mode0`, the FF41-mode analogue
of `halt_ly_phase`: CGB-gated one-shot armed on the mode-0 wake, the first
post-wake line-start (`dot<8`) FF41 read showing mode 2 forced to mode 0).
Two-bin over the full 3422-row gambatte CGB set: **+7 / −15** — the 7 want=0
scx2 rows fixed, but the 15 want=2 floors (`*_m0stat_scx3/4/5_2`, `_3xb`,
`_scx{2,3}_ds_2`, all `cgb04c_out2`, SameBoy-passing) clobbered, since they read
the SAME `ly2 dot4 mode2` and any force-mode-0 inverts them. This is the #11e DMG
"net A/B" (`scx5_2`/`scx3_2b` drop) reproduced on CGB — the cc-grid genuinely
cannot separate them. **The goal predicted this ("halt → expect S7"); reverted,
byte-identical.** Lift condition unchanged: a sub-M-cycle wake clock (record the
IRQ rise at its T-phase, not the M-cycle boundary).

The 5 LE-fail engine-halt rows (scx2_3a, scx3_3a, scx3_3b, dec×2) are the same
wake-clock class observed before the render-frame even applies (LE already
mis-reads); `scx3_3b` (want 2, reads 0) is the opposite-direction collapse.

### 2. mech-3 LYC-engine IF-delivery atomic — `lycEnable`/`ly0`/`m2enable`/`miscmstatirq` (17 rows) — ATOMIC

`on == le` (engine, not render-frame), and **`got` differs from `want` by exactly
the STAT bit (±0x02)**: the engine sets/clears the STAT IF bit opposite to
SameBoy at the LYC enable/disable/write-trigger edge. Decisive grounding —
`miscmstatirq/lycwirq_trigger_m0_late_ly44_4` (want E0, got E2): slopgb dispatches
the ly44 mode-0 STAT at **`dot 254` = SameBoy `cfl 257` (the correct dot)**, yet
SameBoy delivers `if=E0` (bit cleared by the LYC-write-trigger precedence) where
slopgb leaves `if=E2` (bit pending). **The dispatch dot is already right; the
divergence is the IF-bit lifecycle** (edge presence + blocking level), i.e. NOT a
#11ad-shaped wrong-dot. This is the #11h–#11z mech-3 core (LYC re-arm / line-start
carry / late-write trigger). The prior DMG slices (#11j vblank re-arm, #11k line-0
carry, #11l late-FF45) do not extend here: CGB already carries the line-0 vblank
window (`stat_irq.rs:247`) and the dispatch dot is correct, so the residual is the
sub-edge blocking-level precedence — the documented atomic, not a missing
whole-dot feature.

### 3. read-frame↔boundary atomic — `m2int_m3stat/late_scx4_2` (1 row) — ATOMIC

want 0, got 3: the SCX-extended bare-line m3stat read. slopgb's deferred read
lands BEFORE the `SCX&7`-extended mode-3 boundary → mode 3; SameBoy's later read
→ mode 0. The documented atomic read-frame↔boundary core (#11e FF41-READ ground
truth: the read frame and the boundary are each self-consistent ~4 dots apart, and
shifting either alone breaks the `scx0` kernel pin). No whole-dot lever.

### 4. retrigger-dispatch atomic — `irq_precedence/late_m0irq_retrigger_scx1_1` (1 row) — ATOMIC

LE-pass, ON E0 (want E2). The tier2 frame mis-places the IF poll relative to the
STAT re-raise; the observed read is not even on the deferred trace path (serviced
through the retrigger re-arm), so it is the dispatch-retime↔read-frame phase, not
a render-frame slice. No opposing sibling, but no whole-dot lever either (the dot
relationship is the atomic dispatch-retime).

## Conclusion

**0 whole-dot CGB-gatable slices in the 31 in-scope rows.** The goal's hypothesis
("several are #11ad-shaped") is REFUTED for the non-DS in-scope set: #11ad was a
wrong-dot dispatch on the LCD-enable glitch line with no opposing pin, a unique
shape. The in-scope rows are instead (a) halt = the S7 sub-cc wake-clock floor
(want-0/want-2 collapse at `ly2 dot4`, +7/−15 confirmed), (b) mech-3 LYC-engine
IF-delivery atomics (dispatch dot correct, IF-bit lifecycle wrong), (c) one
read-frame↔boundary atomic, (d) one retrigger-dispatch atomic. All four classes
need a sub-M-cycle lever (wake-clock T-phase / engine IF-lifecycle / read-frame
reclock), the atomic C-stage core — none is a localizable whole-dot CGB gate.

**LESSON (the #11ad re-survey, inverted):** a flag-OFF-passing want=2 sibling
(`*_m0stat_scx3/4/5_2`) sitting at the IDENTICAL slopgb state as a want=0 BUG row
is the floor's signature — "fixing" the BUG inverts the sibling. Always two-bin
over the full family before believing a halt/lyc slice; the cc-grid collapse is
the wall. The dispatch-dot-correct / IF-delivery-wrong finding (class 2) is the
key map result: these are NOT dispatch reclocks, so they cannot be CGB-gated like
#11ad — they are the engine IF-lifecycle, the atomic flip core.

Gate: no code shipped (the halt attempt reverted); gbtr/mooneye OFF byte-identical
by construction. The map is the product.
