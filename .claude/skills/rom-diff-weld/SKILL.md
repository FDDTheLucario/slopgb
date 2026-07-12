---
name: rom-diff-weld
description: Crack a "structural floor" / "sub-M-cycle weld" / "welded pair" verdict on a gambatte or mooneye `_1`/`_2` (or `_0`/`_2`) sibling test-ROM pair in the slopgb SameBoy eager-clock port. Use when one sibling PASSES and the want-opposite sibling FAILS on the eager clock and a prior investigation (or you) concluded it is unfixable / a dispatch floor / a read-frame weld — ESPECIALLY if that verdict came from sweeping read-debt. The method (`cmp -l` the ROM binaries → full-trace diff to the first WRITE divergence → find the representable latch → recalibrate the eager threshold arm) recovered ~24 rows that four separate investigations had declared a structural floor. Invoke on "crack this weld", "re-examine this floor row", "rom-diff this pair", "/rom-diff-weld", or before ever accepting a `_1`/`_2` "floor" verdict.
---

# rom-diff-weld — crack a "welded" sibling test-ROM pair

The load-bearing lesson of the slopgb eager-clock C3-flip endgame (#11ec–#11ef): **almost every `_1`/`_2` sibling pair declared a "sub-M-cycle poll-phase weld / dispatch floor / structural floor / un-buildable" was NONE of those.** Those verdicts came from sweeping the READ-DEBT lever, which shifts *both* siblings equally, so the pair never separates and everything looks welded. The truth: the `_1`/`_2` ROMs differ by a **whole-M-cycle** NOP that shifts a **WRITE**, which slopgb *does* represent as a latched dot. Calibrate the threshold arm that consumes that latch for the eager cc+0 frame and the pair separates, golden-safe, zero-shuffle.

**Never trust a "floor"/"weld" verdict that only swept one lever class (read-debt/dispatch). Run this method first.**

## When it applies

A gambatte (or mooneye) OCR row where:
- OFF (production) and tier2 PASS it, the eager clock (`SLOPGB_PROBE_EV` / `GameBoy::new_with_eager`) FAILS it, and SameBoy passes it (a true flip-bar row, not a FLOOR); AND
- it has a want-opposite sibling (`_1` vs `_2`, or `_0`/`_1`/`_2`, or `scxN` variants) — one passing, one failing; AND
- a prior map called it "sub-M-cycle weld", "poll-phase", "counter-pinned dispatch", "read-frame unmovable", "needs a T-exact CPU / half-dot read", or "structural floor."

If there is genuinely no sibling and the full trace is bit-identical to the OCR digit, it may be a render-LENGTH mismatch (the render's own mode-3 length differs) — that is the one real exception; refute it with the trace, do not force it.

## The method (4 steps — do them in order, do not skip to a lever)

### 1. `cmp -l` the sibling ROM binaries
```sh
cmp -l test-roms/game-boy-test-roms-v7.0/gambatte/<dir>/<rom>_1*.gb \
       test-roms/game-boy-test-roms-v7.0/gambatte/<dir>/<rom>_2*.gb
```
Expect a short diff: a **single NOP (`00`) inserted / removed**, shifting a run of bytes (`3E xx E0 41` = `LD A,$xx; LDH ($41),A`, an FF41/FF45/FF40 write). One inserted byte = a **whole M-cycle (4 T at single speed, 2 dots × the write; at double speed 2 dots)**. That is REPRESENTABLE — NOT the "sub-M-cycle 1-T shift" the floor verdicts assume. If `cmp -l` shows a whole-instruction shift, the discriminator exists; proceed.

### 2. Full-trace diff both siblings to the FIRST divergence
Add a temporary full-CPU-state probe (revert it after — Part-C convention, never merge probe code) that dumps `{pc, opcode, addr, val, clk, pending, ly, dot, dhalf}` on **every** `Bus::read` / `Bus::write` / exec, under the eager clock:
```sh
# build the trace binary (port_probe feature exposes run_gambatte / new_with_eager)
SLOPGB_EAGER=1 SLOPGB_S5DBG=1 cargo run -p slopgb-core --example run_gambatte --features port_probe -- <rom>
```
Diff the two siblings' COMPLETE access traces (not just the FF41 read stream — #11eb's fatal error was diffing only reads). The FIRST divergence is almost always a **WRITE** (the window/LCDC/LYC/SCX write the NOP shifted), landing a few dots apart (`_1` dot104 vs `_2` dot108). The decisive later READ is usually byte-identical (the CPU re-syncs) — so `clock.now()` at the read being identical is TRUE BUT IRRELEVANT.

### 3. Find the representable latch that write sets
The shifted write sets a slopgb-tracked dot latch. Known ones (grep for the field + where it's assigned `= self.dot`):
- `win_predraw_abort_dot`, `win_reenable_dot`, `wx_match_dot`, `wy_trig_sb_dot`, `wy_xline_trig` (window family, `ppu/render.rs`/`window.rs`)
- the `eng_stat` / `eng_stat_pending` commit dot (FF41 STAT engine, `ppu/regs.rs` / `stat_irq/`)
- `scx_write_dot`, `wx_match_scx` (SCX fine-scroll)
- the mode-0 emission dot / `m0_flip_events` `flip_dot` (`ppu/render/mode0.rs`)
- the FF0F glitch mode-0 mask / ack-squash window (`stat_irq/ff0f.rs`, `interconnect/speed.rs`)
Confirm the latch differs between `_1`/`_2` (e.g. abort 102 vs 106) — THAT is the representable discriminator.

### 4. Recalibrate the threshold arm consuming the latch, for the eager frame
The eager cc+0 write records the latch ~1 M-cycle (**+4 dots single speed, +4 hd / half-dot at double speed**) BEFORE the tier2 cc+4 read the whole-dot threshold was tuned against. So the arm's constant is off by that debt. Fix pattern:
```rust
// arm consuming `abd` (win_predraw_abort_dot):
let extend = abd + if self.eager_value { 4 } else { 3 } >= wxm && ...;
//                  ^^^^^^^^^^^^^^^^^^^^^^^^^ +1 (the cc+0→cc+4 read-debt), eager-scoped
```
`eager_value`-gated (and `!is_cgb` / `is_cgb && ds` where the row is model/speed-specific) → production + tier2 byte-identical by construction. **Sweep the delta for the UNIQUE-optimal** (e.g. `+3 → n fails / +4 → 0 fails / +5 → n fails`) — a wrong delta shuffles siblings. Double-speed rows use the `EAGER_READ_DEBT_HD_DS = 4` half-dot frame.

## Gate contract (every fix)

- **`golden_fingerprint` byte-identical** — the change is `eager_value`-gated so production never enters it; run golden FIRST and it must pass unchanged. If golden drifts, the branch isn't gated — fix that.
- **EV two-bin DOWN, zero drops both models.** `flagon_probe` A/B: revert the change, capture the before fail-set, `comm -23 before after` = your targets, `comm -13 before after` = **∅** (no new fails) on BOTH `cgb_rowlist.txt` and `dmg_rowlist.txt`.
- **tier2 CGB 291 / DMG 116 unchanged; mooneye 93×3** (OFF / `SLOPGB_MOONEYE_EAGER=1` / `SLOPGB_MOONEYE_RECLOCK=1`).
- **Classify recovered rows SameBoy-pass** (BUG, must-fix), not FLOOR: `python3 docs/sameboy-port/tools/classify_{cgb_regr,dmg}.py`. Never drop a SameBoy-pass row.
- **Red-before-green pin** in `tests/gbtr/gambatte/` — fails with the arm reverted, passes with it.
- **File-cap:** every `.rs` < 1000 lines; split (`foo.rs` + `foo/` second `impl` via `use super::*`) as a separate byte-identical commit BEFORE adding if a file would breach.

## flagon_probe invocation (the measurement loop)

```sh
# ROWLIST path MUST be absolute — the gbtr test cwd is the crate root, a repo-relative path fails NotFound.
export CARGO_TARGET_DIR=target/<name>   # avoid lock contention with parallel runs
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/cgb_rowlist.txt \
  cargo test -p slopgb-core --test gbtr --release -- --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture 2>&1 | grep -E "flagon_probe\["
# SLOPGB_PROBE_{OFF,RECLOCK}=1 for the OFF / tier2 frames; dmg_rowlist.txt for DMG.
```

## Trap ledger (each bit us)

- **A read-debt sweep gives a FALSE floor.** It shifts both siblings equally. If a verdict only tried read-debt / a dispatch move / a single scalar, it is unproven. Re-run this method.
- **`clock.now()` identical at the read ≠ welded.** The discriminator is upstream at the WRITE. Diff ALL accesses, not the read stream.
- **A `FAIL gambatte/X` line's rel path is field `$2`, not `$1`** (`awk '{print $2}'`). Feeding `$1` to a classifier yields all-UNK / a vacuous "bar 0."
- **A timed-out probe truncates its `tee`'d file** → a bogus "everything recovered." Re-run + `wc -l` before diffing.
- **Measure the eager flip COHERENTLY** — via `GameBoy::new_with_eager` or the `#11ds`-fixed default-flip, NOT a raw `interconnect.rs` struct-literal flip (that leaves `ppu.eager_value` un-propagated → incoherent → phantom intr_2 failures).
- **The one real exception is render-LENGTH** (the render's own mode-3 pixel length differs, e.g. mealybug `m3_*` tile-output rows). If the full trace is bit-identical to the OCR digit and there is no write-dot latch, it is a render lever — refute with the trace, do not force a shuffle.

## Provenance

Method: `docs/sameboy-port/tools/measurements/eager-floor-adversarial-audit-2026-07-11.md` (#11ec) and the recalibration runs #11ed/#11ee/#11ef — which cleared the gambatte-OCR eager flip bar from a "proven 34-regression structural floor" to 0.
