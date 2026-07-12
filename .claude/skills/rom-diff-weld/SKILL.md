---
name: rom-diff-weld
description: Crack a "structural floor" / "sub-M-cycle weld" / "welded" / "counter-pinned dispatch" / "unfixable" verdict on ANY failing test ROM in the slopgb SameBoy eager-clock port — any suite (gambatte, mooneye, gbmicrotest, wilbertpol, mealybug, age, blargg, acid) and any ROM (a `_1`/`_2`/`_a`/`_b`/`scxN` sibling pair OR a lone ROM with no sibling). Use whenever a ROM PASSES on OFF/tier2 (or a passing sibling exists) but FAILS on the eager clock, SameBoy passes it, and a prior investigation (or you) concluded it is unfixable / a dispatch floor / a read-frame weld / a shuffle — ESPECIALLY if that verdict came from sweeping a read-frame lever (read-debt / a uniform exit-bias / arm-8-bias). The method (find the discriminator via ROM `cmp -l` OR an eager-vs-passing-frame full-trace diff → the first WRITE/dispatch divergence → the representable latch/render-state term → a DISCRIMINATED eager arm) recovered ~26 rows that FIVE separate investigations had each declared a structural floor. Invoke on "crack this weld", "re-examine this floor row", "rom-diff this pair", "/rom-diff-weld", or before EVER accepting an eager "floor"/"weld"/"unfixable" verdict on any ROM.
---

# rom-diff-weld — crack an eager-clock "floor" / "weld" verdict on ANY ROM

The load-bearing lesson of the slopgb eager-clock C3-flip endgame (#11ec–#11eh): **almost every ROM declared a "sub-M-cycle poll-phase weld / dispatch floor / structural floor / un-buildable / rphd-N weld / +N shuffle" was NONE of those.** Those verdicts came from sweeping a READ-FRAME lever (read-debt, a uniform exit-bias, `ARM8BIAS`, a uniform LYC back-date) — which shifts *all* affected ROMs EQUALLY, so want-opposite rows never separate and everything looks welded/shuffled. The truth: the passing and failing frames differ by a **whole-M-cycle** shift of a **WRITE** (or dispatch), which slopgb *does* represent as a latched dot or a render-FSM state term (`read_carried`, `n_sprites`, `win_active`, a write-dot). Find that DISCRIMINATOR and gate a per-frame arm on it — the rows separate, golden-safe, zero-shuffle.

**Never trust a "floor"/"weld"/"shuffle" verdict that only swept ONE uniform lever class (read-debt, exit-bias, back-date, dispatch move). Run this method first. Five such verdicts were overturned this way.**

## When it applies

ANY failing test ROM (any suite: gambatte / mooneye / gbmicrotest / wilbertpol / mealybug / age / blargg / acid) where:
- OFF (production) PASSES it AND SameBoy passes it (a true flip regression, not a FLOOR — check with the suite classifier / SameBoy tester), the eager clock (`SLOPGB_PROBE_EV` / `GameBoy::new_with_eager` / the coherent temp-flip / `SLOPGB_GBTR_EAGER=1`) FAILS it; AND
- a prior map called it "sub-M-cycle weld", "poll-phase", "counter-pinned dispatch", "read-frame unmovable", "rphd-N weld", "+N shuffle", "needs a T-exact CPU / half-dot read", "un-buildable", or "structural floor" — **especially if that came from a uniform read-frame sweep.**

Two shapes, both cracked by the same method:
- **Sibling pair** (`_1`/`_2`, `_0`/`_2`, `_a`/`_b`, `scxN`): one passes, the want-opposite fails → `cmp -l` the two ROM binaries (step 1a).
- **Lone ROM** (no sibling, or a whole-suite matrix row): passes OFF/tier2, fails eager → diff the ROM's OWN eager trace against its passing (OFF or tier2) trace (step 1b).

The ONE real exception is a render-LENGTH mismatch: if the OCR compares rendered FRAMEBUFFER PIXELS (mealybug/age `m3_*` tile output) and the full trace is bit-identical except the render's mode-3 pixel length, a read-verdict law cannot move a pixel → it's a render-side retime, not this method. Refute it with the pixel-diff trace; do not force it. (A row that fails PURE tier2 too has no existing arm to recalibrate — it needs a NEW SameBoy law ported, a harder but still-tractable variant: find the discriminator here, then BUILD the arm rather than recalibrate one.)

## The method (find the discriminator, then gate a per-frame arm on it)

### 1a. Sibling ROMs — `cmp -l` the binaries
```sh
cmp -l test-roms/game-boy-test-roms-v7.0/<suite>/<dir>/<rom>_1*.gb \
       test-roms/game-boy-test-roms-v7.0/<suite>/<dir>/<rom>_2*.gb   # or _a/_b, or the scxN pair
```
Expect a short diff: a **single NOP (`00`) inserted / removed**, shifting a run of bytes (`3E xx E0 4N` = `LD A,$xx; LDH ($4N),A`, an FF40/41/43/45 write). One inserted byte = a **whole M-cycle** (4 T single speed / the DS equivalent) — REPRESENTABLE, NOT a "sub-M-cycle 1-T shift." Note WHICH write it shifts and by how much.

### 1b. Lone ROM — trace the eager frame against its own passing frame
No sibling? The discriminator is the eager frame's own error. Trace the SAME ROM under the eager clock AND under the passing clock (OFF, or tier2 if tier2 passes) and diff — the eager frame's write-commit / dispatch / read lands at a different dot than the passing frame (the cc+0-vs-cc+4 read-debt, the DS half-dot, the dispatch M-cycle). That offset IS the discriminator to calibrate against.

### 2. Full-trace diff to the FIRST divergence
Add a temporary full-CPU-state probe (revert after — Part-C convention, never merge probe code) dumping `{pc, opcode, addr, val, clk, pending, ly, dot, dhalf}` on **every** `Bus::read` / `Bus::write` / exec:
```sh
# port_probe feature exposes run_gambatte / run_mooneye / new_with_eager. Pick the runner for the suite.
SLOPGB_EAGER=1 SLOPGB_S5DBG=1 cargo run -p slopgb-core --example run_gambatte --features port_probe -- <rom>
# gbmicrotest / mooneye ROMs: use run_mooneye; suite-matrix rows: new_with_eager / SLOPGB_GBTR_EAGER=1.
```
Diff the COMPLETE access traces (siblings, or eager-vs-passing for a lone ROM) — NOT just the FF41 read stream (#11eb's fatal error was diffing only reads; #11eg's was sweeping a read-frame exit-bias). The FIRST divergence is almost always a **WRITE** (the window/LCDC/LYC/SCX write shifted) or a **dispatch** M-cycle, landing a few dots apart. The decisive later READ is often byte-identical (the CPU re-syncs) — so `clock.now()`/`rphd` at the read being identical between two ROMs is TRUE BUT IRRELEVANT; the discriminator is upstream or in the render-FSM state (see step 3).

### 3. Find the representable DISCRIMINATOR
The failing frame's shifted write/dispatch sets — or the read lands against — a slopgb-tracked quantity that DIFFERS between the passing and failing case. Two kinds; find whichever separates them:

**(a) a write-dot latch** (grep the field + where it's assigned `= self.dot`):
- `win_predraw_abort_dot`, `win_reenable_dot`, `wx_match_dot`, `wy_trig_sb_dot`, `wy_xline_trig` (window family, `ppu/render.rs`/`window.rs`)
- the `eng_stat` / `eng_stat_pending` commit dot (FF41 STAT engine, `ppu/regs.rs` / `stat_irq/`)
- `scx_write_dot`, `wx_match_scx` (SCX fine-scroll)
- the mode-0 emission dot / `m0_flip_events` `flip_dot` (`ppu/render/mode0.rs`)
- the FF0F glitch mode-0 mask / ack-squash window (`stat_irq/ff0f.rs`, `interconnect/speed.rs`)

**(b) a render-FSM / read-context state term** — when two ROMs read at the SAME dot/rphd but want opposite outcomes, the discriminator is NOT the read position; it's the state the read carries:
- `read_carried` — polled read (`false`) vs a carried mode-2-ISR read (`true`); the exit's `- carry` term already owns carried reads, so a `!read_carried`-gated arm separates a polled read from a carried one at the same rphd (**this cracked sprite0, #11eh — the sole discriminator where #11eg's uniform `ARM8BIAS` sweep saw a false "rphd-512 weld"**).
- `render.n_sprites`, `render.win_active`/`win_stalled`, `eff.scx`/`eff.wx`, `projected_flip_dot()` — the live render state at the read.

**CRITICAL:** if a prior verdict claimed two ROMs are "welded at identical rphd/dot", TRACE BOTH and check EVERY render-FSM + read-context field — the "identical" claim is often factually wrong (#11eg claimed rphd 512 for a ROM actually reading 504) or ignores a differing state term (`read_carried`, `n_sprites`). A genuine weld requires bit-identical render-FSM state AND read context AND opposite wants — verify all of it before believing it.

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
- **EV eager DOWN, zero drops.** A/B: revert the change, capture the before eager fail-set, `comm -23 before after` = your targets, `comm -13 before after` = **∅** (no new fails). For a gambatte row use `flagon_probe` on BOTH `cgb_rowlist.txt` and `dmg_rowlist.txt`; for a **non-gambatte** row (gbmicro/mooneye/wilbertpol/mealybug/age) run that suite's coherent-eager runner (`GameBoy::new_with_eager` / `SLOPGB_GBTR_EAGER=1` / the suite matrix test) — AND still run the gambatte `flagon_probe` A/B to prove no gambatte-OCR row drops (a non-gambatte fix must not shift `EV CGB`/`EV DMG`).
- **tier2 unchanged (CGB 291 / DMG 116); mooneye 93×3** (OFF / `SLOPGB_MOONEYE_EAGER=1` / `SLOPGB_MOONEYE_RECLOCK=1`).
- **Classify recovered rows SameBoy-pass** (BUG, must-fix), not FLOOR: `python3 docs/sameboy-port/tools/classify_{cgb_regr,dmg}.py` (gambatte) or the SameBoy tester at `~/.cache/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester` (any suite). Never drop a SameBoy-pass row.
- **Red-before-green pin** in the matching test module (`tests/gbtr/gambatte/` for gambatte, `tests/gbtr/gbmicrotest.rs`/`mooneye.rs`/etc. for other suites) — fails with the arm reverted, passes with it.
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

- **Any UNIFORM-lever sweep gives a FALSE floor/weld.** Read-debt, a uniform exit-bias, `ARM8BIAS`, a uniform LYC back-date, a dispatch move — each shifts ALL affected ROMs equally, so want-opposite rows can never separate (they "shuffle"). If a verdict only tried ONE uniform scalar, it is UNPROVEN. Re-run this method with a DISCRIMINATED arm.
- **`clock.now()` / `rphd` identical at the read ≠ welded.** The discriminator is upstream (the WRITE) or in the render-FSM/read-context state (`read_carried`, `n_sprites`). Diff ALL accesses + ALL render-FSM fields, not the read stream.
- **"Two ROMs welded at identical rphd/dot" is usually FALSE or incomplete** (#11eg): it claimed rphd 512 for a ROM actually reading 504, and ignored `read_carried` differing. TRACE BOTH ROMs and verify EVERY field before believing a cross-ROM weld.
- **A lone ROM with no sibling is NOT automatically render-length.** Trace its eager frame vs its own OFF/tier2 frame (step 1b) — the eager write/dispatch offset is the discriminator. Only conclude render-length after the full-trace shows bit-identical everything except rendered PIXELS.
- **A `FAIL gambatte/X` line's rel path is field `$2`, not `$1`** (`awk '{print $2}'`). Feeding `$1` to a classifier yields all-UNK / a vacuous "bar 0."
- **A timed-out probe truncates its `tee`'d file** → a bogus "everything recovered." Re-run + `wc -l` before diffing.
- **Measure the eager flip COHERENTLY** — via `GameBoy::new_with_eager` or the `#11ds`-fixed default-flip, NOT a raw `interconnect.rs` struct-literal flip (that leaves `ppu.eager_value` un-propagated → incoherent → phantom intr_2 failures).
- **The one real exception is render-LENGTH** (the render's own mode-3 pixel length differs, e.g. mealybug/age `m3_*` tile-output rows whose OCR reads framebuffer PIXELS). A read-verdict law cannot move a pixel → it's a render-side retime, refute with the pixel-diff trace, do not force a shuffle.
- **A row that fails PURE tier2 too has no arm to recalibrate** — it needs a NEW SameBoy law ported (find the discriminator here, then BUILD the arm), not a recalibration. Still tractable, just more work.

## Provenance

Method born in `docs/sameboy-port/tools/measurements/eager-floor-adversarial-audit-2026-07-11.md` (#11ec, the adversarial audit that cracked a "proven absolute floor"). Recalibration + generalization runs #11ed (window), #11ee (DMG STAT), #11ef (CGB double-speed), #11eh (gbmicro sprite0 via `read_carried`) — together they cleared the gambatte-OCR eager flip bar from a "proven 34-regression structural floor" to **0**, and recovered ~26 rows that FIVE separate investigations had each declared unfixable. The recurring root cause every time: a uniform read-frame sweep mistaken for a structural weld.
