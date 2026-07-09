# Eager DMG-safety — scope-to-is_cgb REFUTED; the DMG regression is the frame-80 entry, and the web HELPS DMG (2026-07-08, #11ch)

Task: the eager-value (EV) clock REGRESSES DMG-OCR (measured this session: DMG OFF
103 fail → EV **145**, **107 flip-BUGs** = OFF-pass ∩ EV-fail). Since `eager_value`'s
purpose is the CGB read-frame port, the hypothesis was a clean **scope-to-is_cgb**
fix: under EV, make DMG (`!is_cgb()`) return to production (native `vis_mode`,
frame 84), removing 107 flip-BUGs golden-safe.

**Result: REFUTED. The DMG regression is NOT a scoping leak — it is the
`mode3_entry_dot` frame-80 shift baked into native `vis_mode`, and frame-80 is
LOAD-BEARING for mooneye DMG intr_2 (cannot revert to 84). The read-law web does
NOT leak onto DMG destructively — it HELPS (bypassing it made DMG WORSE, 145→170).
Tree byte-identical @ `28f1d94` (experiment reverted); this map is the deliverable.**

## Baselines (branch `finish-port-halfdot` @ `28f1d94`, `CARGO_TARGET_DIR=target/ev`)

| bin | rowlist | fail | note |
|---|---|---:|---|
| DMG OFF (production) | `dmg_rowlist.txt` | 103 | reference |
| DMG EV (`SLOPGB_PROBE_EV=1`) | `dmg_rowlist.txt` | **145** | +42 vs OFF; 107 flip-BUGs |
| CGB EV | `cgb_rowlist.txt` | 400 | must not move |
| tier2 CGB | `cgb_rowlist.txt` | 291 | must not move |

107 DMG flip-BUG families: window 30, halt 28, enable_display 9, ly0 8, m1 5,
lycEnable 5, m2enable 3, … (`scratchpad/dmg_flipbugs.txt`).

## The decisive experiment — bypass the read-law web for DMG under EV

Edit (`read_laws.rs`, after the `vis_mode_read` production gate): under
`eager_value && !tier2_reclock && !is_cgb()`, return native `vis_mode()` (bypass
the entire web for DMG). Golden-safe by construction (production + tier2 return
paths untouched). Left `mode3_entry_dot` (frame-80) intact, so this ISOLATES the
web (leak B) from the entry-frame shift (leak A):

| config | DMG EV fail | vs 145 | reading |
|---|---:|---:|---|
| control (web ON for DMG) | 145 | 0 | the web fires on DMG under EV |
| **web BYPASSED for DMG** | **170** | **+25** | the web was NET-HELPING DMG |
| CGB EV (both) | 400 | 0 | CGB unaffected either way |

**The web is not the leak.** Its DMG arms (arm D1, the DMG window family, gated
`tier2_reclock || eager_value`) already partially re-host the §3b DMG length laws
onto the eager clock — bypassing them REMOVES 25 DMG fixes. So the 145-vs-103
regression lives in **native `vis_mode`** itself, i.e. **leak A: `mode3_entry_dot`
= 80** (`stat_irq.rs:94`, `if leading_edge_reads && !tier2_reclock && !ds { 80 }`),
NOT `is_cgb()`-scoped.

## Why leak A cannot be scoped away — frame-80 is load-bearing for DMG mooneye

`mode3_entry_dot` = 80 back-dates the CPU-visible mode-2→3 entry by the eager
read's 4-dot debt so the cc+0 read is observationally neutral at the ENTRY
(`stat_irq.rs:73-79` doc: "moving the boundary the same 4 dots makes that read
observationally neutral for the mode-2→3 entry — mooneye `intr_2_mode3_timing`
passes LE-only"). The eager clock IS leading-edge, so DMG under EV needs 80 for
`intr_2_mode3_timing` / `intr_2_mode0_timing`. Reverting DMG to 84 (the naive
scope-to-is_cgb) would re-break mooneye DMG intr_2 (the cc+0 read sees mode 2
where cc+4 sees mode 3, miscounts — the exact failure 80 fixes). **frame-80 is
correct for the ENTRY (mooneye) but shifts every DMG read near the boundary that
production reads correctly at frame-84 (the 107 OCR rows).** Same class as the
#11cg CGB slices (sub-field / line-boundary reads that don't take the +4
back-date) — but on DMG, and entangled with the §3b DMG length laws the web
carries.

## Verdict — eager DMG-safety = re-host the DMG §3b onto the eager frame (a large piece)

Not a scoping shortcut. The eager clock's DMG frame IS 80 (mooneye-correct); the
107 OCR regressions are DMG reads that need the same eager re-host the CGB side
got (#11by–#11cg): the sub-field / boundary +4 back-dates (#11cg's coincidence /
VBlank-entry / line-0-entry, applied to DMG) PLUS the §3b DMG window/hblank/poweron
length laws re-hosted `|| eager_value` (the web already does part of this — extend
it, don't bypass it). This is a large multi-session piece **parallel to the CGB
HALFDOT Part A**, both gating the eager C3 flip.

Do NOT re-attempt: scope-to-is_cgb on `mode3_entry_dot` (breaks mooneye DMG intr_2)
or bypassing the read-law web for DMG (removes 25 §3b DMG fixes, +25 worse).

## Gate state (unchanged — experiment reverted, tree @ `28f1d94`)

EV CGB 400 / DMG 145; tier2 291; flip bar CGB 70 SameBoy-PASS blockers + DMG 107
flip-BUGs (SameBoy classification of the 107 pending — the DMG classifier run was
not reached before the weekly agent limit). golden_fingerprint PASS; defaults NOT
flipped.

## Reproduction

```
CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_EV=1  $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 145
SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_OFF=1 $BIN ... | grep pass=   # 103
# web-bypass experiment (revert after): read_laws.rs vis_mode_read, add after the
# production gate: `if self.eager_value && !self.tier2_reclock && !self.model.is_cgb() { return m; }`
# → DMG EV 170 (WORSE); CGB EV 400 (unchanged).
```
