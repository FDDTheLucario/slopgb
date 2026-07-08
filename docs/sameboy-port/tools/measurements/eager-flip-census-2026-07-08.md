# The EAGER-VALUE flip census — the true C3-flip distance (2026-07-08, #11cc)

Read-only measurement, no source edits (`crates/` untouched). Branch
`finish-port-halfdot` @ `a3ed24d`, tracked tree clean. Question: of the eager-value
(EV) re-host's remaining CGB-OCR fails, how many are **SameBoy-PASS flip-BLOCKERS**
(forbidden drops — must be recovered before the C3 flip) vs **SameBoy-FAIL**
(rebaseline-OK, like tier2's own 291 floor)? This decides whether the flip is NEAR
or needs the full per-T half-dot clock.

## Method (C3-FLIP-CHECKLIST §3 + census #11bt, mirrored)

Same binary (`target/ev/release/deps/gbtr-*`) for all three probes, so the fail
lists are self-consistent for `a3ed24d`:

- OFF (production) fail = `SLOPGB_PROBE_OFF=1` → **486** (regenerated this run,
  byte-identical to the committed `scratchpad/fail_off.txt` — current for `a3ed24d`).
- EV fail = `SLOPGB_PROBE_EV=1` → **428** (matches the #11cb map).
- **flip-BUGs = OFF-pass ∩ EV-fail** = `comm -13 fail_off ev_fail` → the rows the EV
  flip would BREAK vs production (the only rows that matter for the flip bar).
- Each flip-BUG classified against SameBoy 1.0.2 (`--cgb --length 4` OCR vs the
  `_out<hex>` tag) via `docs/sameboy-port/tools/classify_cgb_regr.py`. SameBoy-PASS
  (sb==want) = BLOCKER; SameBoy-FAIL = rebaseline-OK.

## Headline: the flip bar is 97, not 291

| set | count |
|---|---:|
| EV CGB residuals (all fails) | 428 |
| **flip-BUGs** (OFF-pass ∩ EV-fail) | **141** |
| — SameBoy-PASS **BLOCKERS** (must reach 0) | **97** |
| — SameBoy-FAIL rebaseline (drop into baseline at flip) | 44 |
| flip-FIXes (OFF-fail ∩ EV-pass, the flip's benefit) | 199 |
| already-floored EV residuals (OFF-fail ∩ EV-fail, NOT flip-relevant) | 287 |

Sanity: 199 − 141 = 58 = 486 − 428 ✔. 97 + 44 = 141, no UNK ✔.

**The 428 EV residuals are NOT the flip bar.** 287 of them are rows production (OFF)
ALSO fails — already floored/exempt in the baseline, so the flip introduces no new
drop for them. Only the **141 flip-BUGs** matter, and of those only the **97
SameBoy-PASS** must be recovered (the 44 SameBoy-FAIL rebaseline like the existing
class-F exemptions).

## The 97 blockers by family + per-family verdict

`ds` = double-speed (`_ds`) legs (DS mid-dot floor); `disp` = STAT/IF dispatch
signature (got hi-nibble `E0/E2`, `C1..C6`, `8x`, `9x` — the counter-pinned dispatch
frame); `cand` = SS non-dispatch (recoverable-candidate).

| family | blk | ds | disp | cand | verdict |
|---|---:|---:|---:|---:|---|
| window | 17 | 3 | 0 | 14 | **clean-lever candidate** (mode-3 length / WX-defer / late-WY render read-frame); 3 DS legs = mid-dot floor; `_2` sub-dot shuffle risk |
| enable_display | 12 | 4 | 12 | 0 | **half-dot / dispatch floor** (E0/E2 mode-0-IRQ ISR reads + 90/00 mode-2 count — counter-pinned) |
| ly0 | 10 | 5 | 10 | 0 | **dispatch floor** (lycint152 LYC/STAT composite C1/C5/E0/E2) |
| m2int_m3stat | 5 | 1 | 0 | 4 | **clean-lever candidate** (mode-2→3 entry STAT read-frame, `want=0 got=3`) |
| m2int_m0irq | 5 | 3 | 0 | 2 | clean candidate (2) + DS mid-dot floor (3) |
| lycEnable | 5 | 0 | 3 | 2 | dispatch floor (3) + clean candidate (2, ff41 enable/disable) |
| lcd_offset | 5 | 2 | 5 | 0 | **dispatch floor** (STOP-shift residual, C1/C4 + 90/00 count) |
| halt | 5 | 0 | 0 | 5 | **multi-mechanism port** (halt-wake clock, #11cb lever 1) |
| vram_m3 | 4 | 2 | 0 | 2 | **half-dot floor** (vis_early accessibility — #11cb REFUTED the gate-flip as a shuffle) |
| irq_precedence | 4 | 2 | 4 | 0 | **dispatch floor** (late_m0irq_retrigger E0/E2) |
| cgbpal_m3 | 4 | 2 | 0 | 2 | render read-frame; scx `_2` legs = sub-dot floor |
| speedchange | 3 | 0 | 0 | 3 | **sub-dot floor** (post-STOP `_2` poll-phase, #11ca residual) |
| oam_access | 3 | 2 | 0 | 1 | **half-dot floor** (vis_early accessibility) |
| lycint_lycflag | 3 | 2 | 0 | 1 | clean candidate (1) + DS floor (2) |
| m2int_m2stat | 2 | 2 | 0 | 0 | DS mid-dot floor |
| m0enable | 2 | 0 | 0 | 2 | clean candidate (lycdisable ff41/ff45 STAT read-frame) |
| m2enable | 1 | 0 | 0 | 1 | clean candidate |
| m1 | 1 | 0 | 0 | 1 | clean candidate (lycint_m1stat) |
| lyc153int_m2irq | 1 | 0 | 0 | 1 | clean candidate |
| miscmstatirq | 1 | 0 | 1 | 0 | dispatch floor |
| sprites / scx_during_m3 / m2int_m0stat / m0int_m3stat | 1 ea | 1 ea | 0 | 0 | DS mid-dot floor |

### The three-bucket tally (the 97 blockers)

| bucket | count | what it needs |
|---|---:|---|
| **half-dot-clock / dispatch-retime FLOOR** | **56** | DS mid-dot (34) ∪ dispatch-signature (35), union 56 — HALFDOT Part A (per-T half-dot clock) + the coherent dispatch retime; **not** gate-flippable (moving dispatch as a read gate breaks `intr_2`, #11br) |
| — plus SS sub-dot / vis_early floor already REFUTED as gate-flips | ~8 | accessibility vram_m3+oam_access (3), speedchange post-STOP `_2` (3), cgbpal_m3 scx `_2` (2) — need the reclocked dot (half-dot), #11ca/#11cb measured |
| **multi-mechanism port** | **5** | halt-wake clock (`stat_vis_from_t`/`m0_halt_hold`/`wake_skew`/`halt_ly_phase`) re-hosted `\|\| eager_value` as one coherent wake retime, #11cb lever 1 |
| **clean read-verdict / render-length lever candidates** | **~28** | the `\|\| eager_value` per-family pattern (#11by–#11cb wins): window-length 14, mode-2→3 entry 6, LYC/enable STAT read-frame 8 — with the honest caveat that the `_2` sub-dot legs carry the #11cb `vis_early` shuffle risk, so a fraction will fall to the floor when attempted |

## Cross-check of the #11cb "parked floors (223: dispatch 129 + DS mid-dot 94)"

Reconstructed the parked-floor universe over all 428 EV residuals (dispatch-sig by
got hi-nibble, DS by `_ds`): **dispatch-sig 105 ∪ DS 157 = 222 rows ≈ the map's
"223".** The mechanism-class description is REAL. But as a **flip bar it is a ~4×
overcount:**

| of the 222 parked-floor rows | count | flip meaning |
|---|---:|---|
| already-floored (OFF-fail — production ALSO fails) | 147 | **NOT flip-relevant** — no new drop; stay as baselined |
| flip-BUG + SameBoy-FAIL | 19 | rebaseline-OK |
| **flip-BUG + SameBoy-PASS (genuine hard-floor blocker)** | **56** | the real half-dot/dispatch floor bar |

So the answer to "are the 223 SameBoy-PASS blockers or SameBoy-FAIL?" is **neither,
mostly**: 147 are already-floored (production-fail, the flip never touches them) and
19 rebaseline; only **56 are genuine SameBoy-PASS flip-blockers**. The parked floors
do NOT set a 223/291 flip bar. (For completeness, of all 428 EV residuals 279 are
SameBoy-PASS and 149 SameBoy-FAIL — but 182 of the SameBoy-PASS are already-floored
production drops, irrelevant to the flip.)

## The VERDICT

**The flip is much nearer than the 291 tier2 floor — the bar is 97 SameBoy-PASS
blockers — but it is NOT reachable without the half-dot clock.**

- **~33 blockers are recoverable WITHOUT HALFDOT Part A:** ~28 clean read-verdict /
  render-length levers (window-length, mode-2→3 entry, LYC/enable STAT — the
  established `|| eager_value` per-family pattern) + 5 halt-wake via one
  multi-mechanism port. Peeling these takes the bar **97 → ~64**.
- **≥56 blockers are genuine half-dot-clock / dispatch-retime FLOOR** (DS mid-dot 34
  ∪ counter-pinned dispatch 35, union 56; +~8 sub-dot/vis_early SS legs already
  REFUTED as gate-flippable). These need the per-T half-dot clock (HALFDOT Part A)
  and the coherent dispatch retime — the multi-session rewrite the #11ca/#11cb/#11br
  maps park. **K ≈ 56–64.**

Path: **(b) flip needs the half-dot clock.** Concretely — clear the ~28 clean +
5 halt-wake first (97 → ~64), rebaseline the 44 SameBoy-FAIL flip-BUGs, then the
residual ~56–64 hard-floor blockers gate the flip on HALFDOT Part A. The 291-floor
fear is unfounded (the flip bar is 97, not 291) but the "recover a few families then
rebaseline" hope is also unfounded — the majority of the flip bar is the same
DS-mid-dot + dispatch-retime floor the port has parked all along.

## Also measured

- flip-FIXes (OFF-fail ∩ EV-pass) = **199** (the flip's benefit — 199 production
  fails that EV passes, incl. many previously-floored SameBoy-PASS rows recovered).
- EV DMG two-bin (`dmg_rowlist`, `SLOPGB_PROBE_EV=1`) = **147** (≤147 ✔, unchanged —
  the CGB read-laws are `is_cgb`-scoped).

## Reproduction

```
CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1  $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture 2>&1 | grep '^FAIL' | awk '{print $2}' | sort -u > /tmp/ev_fail.txt   # 428
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_OFF=1 $BIN ... | ... > fail_off  # 486
comm -13 <(sort -u scratchpad/fail_off.txt) /tmp/ev_fail.txt > flipbugs   # 141
docs/sameboy-port/tools/classify_cgb_regr.py flipbugs   # BUG(SameBoy-PASS)=97  FLOOR=44  UNK=0
```
