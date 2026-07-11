# HALFDOT wall-1 FF41-ENABLE bar — REFUTED: the 4 DMG FF41-enable rows are NOT the `eng_stat_half` write-commit substrate. Two are the eager interrupt-service/READ clock (dispatch byte-identical to the passing tier2 frame — no PPU edge to move); two fail on BOTH frames (a missing mode-2/LYC pulse-suppression law, no correct reference frame + no held-source coincidence). The broadened write-commit arm SHUFFLES EV DMG 52→57 (+5), reproducing #11du/#11dv (2026-07-11, #11dy)

Base: `finish-port-halfdot @ 2943fa7` (has the #11dw substrate: `stat_update_half`,
`eng_stat_half`). NO code shipped; tree byte-identical @ 2943fa7. This is a measure-only
REFUTE (the #11br / #11bu / #11bw pattern).

## TL;DR — REFUTE, with trace

The task's 4 DMG FF41-enable bar rows are NOT the FF41-write-commit sub-dot the #11dw
`eng_stat_half` substrate resolves. The write-commit lever recovered the #11dw m1disable
pair because there the eager frame produced a **spurious PPU edge** that a deferral to a
**held-source join** (the line-153 LYC re-latch, dot 6) removed, WITH the passing
tier2/SameBoy frame as the correct reference. None of these 4 rows is that pattern:

| # | row (`_2` variant) | want | EV | tier2 | dispatch EV vs tier2 | class |
|---|---|---|---|---|---|---|
| 1 | `m2enable/late_enable_2` | 0 | **2** | **0** (pass) | **byte-IDENTICAL** | eager READ clock |
| 2 | `m2enable/late_enable_after_lycint_disable_2` | 0 | **2** | **0** (pass) | **byte-IDENTICAL** | eager READ clock |
| 3 | `m2enable/late_enable_m0disable_2` | 0 | **2** | **2** (fail) | 1-dot shift (ly2 dot1 vs dot2) | both-fail pulse law |
| 4 | `lycEnable/lycwirq_trigger_ly00_stat50_2` | E0 | **E2** | **E2** (fail) | +1 EV halfdot dispatch (existing #11dw arm) | both-fail pulse law |

Empirical confirmation: broadening the `eng_stat_half` arm from `line==153` to
`line==153 || line<=2` measured **EV DMG 52 → 57 (+5 net WORSE)** — a shuffle, exactly
the #11du "uniform write-borrow is a strict pair-shuffle" / #11dv "+N family cost"
outcome. Reverted; tree byte-identical.

## Baselines reproduced (exact, at 2943fa7)

| metric | value | gate |
|---|---:|---|
| `golden_fingerprint` | 1 pass (byte-identical) | THE gate ✓ |
| EV DMG (`dmg_rowlist.txt`) | **52** | steady-state floor |
| EV CGB | 295 (untouched, DMG-scoped work) | ✓ |
| tier2 CGB / DMG | 291 / 116 | ✓ |

## Method — the dispatch-identity discriminator (`run_gambatte`, `port_probe`)

Per row, ran `SLOPGB_EAGER=1` vs `SLOPGB_TIER2=1` with `SLOPGB_S5DBG=1` and compared:
(a) the last-frame OCR digit, (b) the in-order `SLOPGB dispatch` sequence (the PPU
`GB_STAT_update` IF-generation trace), (c) a temporary FF41/FF0F read-value probe on
`Bus::read`'s return (reverted).

The discriminator: **is the PPU STAT dispatch (IF generation) byte-identical to the
frame that PASSES?** If yes → the PPU is already correct; the divergence is CPU-side
(read/interrupt-service clock) → `eng_stat_half` (which resolves a PPU write-commit
edge) is structurally inapplicable. If the eager frame produces a spurious PPU edge that
tier2/SameBoy does not → the write-commit lever is in play (the #11dw pattern).

## The evidence per class

### Rows 1 & 2 — the eager interrupt-service / READ clock (dispatch byte-identical)

`late_enable_2`: EV outputs 2 (fail), tier2 outputs 0 (pass). The **in-order dispatch
sequence is byte-identical** between EV and tier2 (`diff -q` → identical; the aggregate
`14 dispatch ly=0 dot=4 mfi=2 lycln=1` matches exactly). So the PPU STAT engine emits
the IDENTICAL IF stream under EV as under the PASSING tier2 frame — there is **no
spurious PPU edge to move**.

The divergence is entirely CPU-side. Under EV the ROM reads FF41 only **5 times total**
(mode bits 0x85/86/86/86/87 = mode 1/2/2/2/3) where the tier2 ISR trace (`SLOPGB_ISRTRACE`)
shows a **tight per-frame FF41 poll** (`rd a=ff41` at ly=0 dot=4,32,60,88…) — the eager
cc+0 FF41/FF0F read (via `leading_edge_sample`/`vis_mode_read`, `Bus::read` bus.rs:16-60)
returns a mode/IF value 4 dots off the SameBoy cc+4 view, so the CPU **branches
differently and takes a wholly different code path** → the `2` result. The FF41
architectural read returns `stat_en`, NOT `eng_stat` — so deferring the engine view
(`eng_stat_half`) provably cannot change what the CPU reads. This is the read-frame
residual (#11bw→#11cb line), not a write-commit.

Corroboration in-tree: the existing EXPERIMENT `ff0f_le` (`interconnect/cycle.rs:22-29`)
already documents exactly this: "the eager FF0F read trails at cc+4 … the
dispatch-cluster ISR reads see the STAT bit a dot early (got E2 want E0) … test whether
the 'dispatch-coupled' rows are really a read-frame miss." Same class.

### Rows 3 & 4 — a missing mode-2/LYC pulse-suppression law (both frames fail)

`late_enable_m0disable_2` (want 0): **both EV and tier2 output 2** — neither frame passes.
The only dispatch difference is a 1-dot position shift (EV `ly=2 dot=1 mfi=2 lycln=0` vs
tier2 `ly=2 dot=2 mfi=2 lycln=0`); both fire the mode-2 dispatch on line 2 and both read
`ff0f ly=2 dot=20 ret=e2`. SameBoy suppresses this mode-2 pulse entirely (want 0). The
suppression law (a late m2-enable relative to the mode-2 window blocking the pulse) is
**unported on BOTH the tier2 AND eager engines** — there is no correct reference frame to
copy, and no held-source join for a deferral to land against (it is a pulse BLOCK, not an
edge-vs-held-source coincidence). Not the `eng_stat_half` pattern.

`lycwirq_trigger_ly00_stat50_2` (want E0): both frames output E2. EV additionally emits an
extra `ly=153 dot=82 mfi=1 (halfdot)` dispatch from the **existing** line-153 `eng_stat_half`
arm — but tier2 reaches the same E2 without it, so that is not the cause. Same both-fail
class.

## Why `eng_stat_half` cannot apply (the structural refutation)

`eng_stat_half` defers the engine-view (`eng_stat`) write commit to a later odd half-dot
so a coincident level re-eval lands the disable/enable AT a held-source join (the line-153
LYC re-latch), removing a spurious 0→1 edge. It requires: (i) a spurious PPU edge under
the eager frame, (ii) a held source to land coincident with, (iii) a correct reference
frame (SameBoy/tier2 passing). Rows 1&2 lack (i) — the PPU dispatch already equals the
passing frame; the failure is the CPU FF41/FF0F read value (which reads `stat_en`, not
`eng_stat`). Rows 3&4 lack (ii) and (iii) — a missing pulse-block law, failing on both
frames. The broadened-arm build-measure (+5 EV DMG) is the empirical confirmation.

## Gates (all hold; NO code shipped, tree byte-identical @ 2943fa7)

1. `golden_fingerprint` byte-identical — 1 pass (no code change to crates/).
2. EV DMG 52 unchanged; EV CGB 295, tier2 291/116 unchanged (no code touched).
3. Zero regression (nothing shipped).
4. All probe edits (a temporary FF41/FF0F read trace on `bus.rs` + `cycle.rs`, and the
   broadened-arm experiment on `regs.rs`) REVERTED; `git diff HEAD crates/` empty.

## Do-not-re-chase ledger (add)

- The DMG FF41-**enable** bar rows (`m2enable/late_enable*`, `lycEnable/lycwirq_*`) are
  NOT the `eng_stat_half` write-commit substrate. Two (`late_enable_2`,
  `late_enable_after_lycint_disable_2`) are the eager interrupt-service/READ clock —
  PPU dispatch byte-identical to the passing tier2; the eager cc+0 FF41/FF0F read
  (`leading_edge_sample`/`vis_mode_read`) diverges the CPU control flow. FF41 read =
  `stat_en`, never `eng_stat`; the engine deferral cannot touch it. These land with the
  read-frame convergence (or the dispatch C3-flip), not a wall-1 STAT-engine arm.
- Two (`late_enable_m0disable_2`, `lycwirq_trigger_ly00_stat50_2`) fail on BOTH the tier2
  AND eager frames — a missing mode-2/LYC pulse-SUPPRESSION law (late-enable blocks the
  pulse), not an eager-specific write-commit edge. No held-source join → no deferral
  target. Needs the pulse-block law ported to BOTH engines first (a tier2/S5 task), then
  re-measured on eager — not an odd-half `eng_stat_half` arm.
- CONFIRMS #11du/#11dv/#11dw: a write-commit deferral broadened past the line-153 quirk
  scope is a strict shuffle (measured EV DMG 52→57, +5). The line-153 scope is load-
  bearing; there is no wider FF41-write-commit vein on the EV DMG frame.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd5
# dispatch-identity discriminator (per row):
for R in m2enable/late_enable_2_dmg08_cgb04c_out0 \
         m2enable/late_enable_after_lycint_disable_2_dmg08_out0_cgb04c_out2 \
         m2enable/late_enable_m0disable_2_dmg08_out0_cgb04c_out2 \
         lycEnable/lycwirq_trigger_ly00_stat50_2_dmg08_outE0_cgb04c_outE2; do
  ROM=test-roms/game-boy-test-roms-v7.0/gambatte/$R.gbc
  SLOPGB_EAGER=1 SLOPGB_S5DBG=1 cargo run -q -p slopgb-core --example run_gambatte \
    --release --features port_probe -- $ROM dmg 2>d_ev.txt >scr_ev.txt
  SLOPGB_TIER2=1 SLOPGB_S5DBG=1 cargo run -q -p slopgb-core --example run_gambatte \
    --release --features port_probe -- $ROM dmg 2>d_t2.txt >scr_t2.txt
  echo "$R EV=$(tail -1 scr_ev.txt|cut -c1-4) T2=$(tail -1 scr_t2.txt|cut -c1-4)"
  diff -q <(grep dispatch d_ev.txt) <(grep dispatch d_t2.txt)
done
# broadened-arm shuffle (regs.rs: line==153 -> line==153 || line<=2): EV DMG 52 -> 57
```
