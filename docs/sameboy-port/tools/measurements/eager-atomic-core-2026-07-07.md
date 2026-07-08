# The EAGER-VALUE atomic core — the read-frame is HALF-DOT-blocked (2026-07-07, #11bw)

Task: attack the eager-value (EV) reclock atomic core — get the EV CGB two-bin
from 578 toward tier2's 291 by re-hosting the tier2 read/render laws onto the
eager clock (dispatch cc+4, cc+0 value peeks). **Result: NO whole-dot slice
improves EV. The read-frame family (the biggest CGB block) is walled by a
SUB-DOT (half-dot) mismatch — measured as a monotone shift curve that never dips
below baseline. The next lever is the half-dot read on the eager `Bus::read`
(HALFDOT Part B), the multi-session piece.** Tree reverted byte-identical to
`ace4d31` (slice #2a); this map is the whole deliverable.

## Baselines (branch `finish-port-halfdot` @ `ace4d31`, `CARGO_TARGET_DIR=target/ev`)

Probe: `SLOPGB_ROWLIST=scratchpad/{cgb,dmg}_rowlist.txt [SLOPGB_PROBE_EV=1] $BIN
--ignored gambatte::flagon_probe::flagon_probe --nocapture` (3422 rows, 402 skip).

| bin | fail | note |
|---|---:|---|
| tier2 (ON, default) | **291** | the target |
| EV CGB (`SLOPGB_PROBE_EV=1`) | **578** | == LE; the start |
| EV DMG (dmg_rowlist) | **172** | the "must not worsen" floor |
| CGB OFF (production) | 486 | reference |

Slice #2a (`stage_write_dots` on the eager `Bus::write`) is ACTIVE under EV
(`tier2_reclock || eager_value`) but **net-zero on EV CGB** (578 with or without
it) — the write-commit render-frame deferral needs the render-LENGTH + read-frame
laws to become OCR-observable; it does not move the two-bin alone. Confirms the
atomicity thesis (write ∧ render ∧ read must land together).

## The architecture under EV, precisely (why 578 == LE)

Under `eager_value`: `leading_edge_reads = true`, `tier2_reclock = false`. The
read path is the NON-deferred `Bus::read` (`interconnect.rs:754`):
`leading_edge_sample` samples **FF41 only**, cc+0 (before `tick_machine`);
everything else (OAM/VRAM/palette/FF0F/LY) returns the production cc+4 trailing
value. And FF41 goes through `vis_mode_read` (`regs.rs:184`) whose first line is
`if !self.tier2_reclock { return native vis_mode }` — so under EV the **entire
tier2 read-law web is bypassed**: no window-length law, no `vis_exit_hd`, no
accessibility back-date, no FF0F peek. EV FF41 = native `vis_mode` (frame-80:
`mode3_entry_dot() == 80`, glitch-74, `vis_hold_until == 0`). That is exactly the
LE base → 578.

Render feeders that ARE live under EV (real render flags, not tier2-gated):
`render.win_active` (render.rs:414, window.rs:95/104), DMG `render.win_aborted`
(window.rs:191). Tier2-gated (OFF under EV): the CGB pre-draw abort
(`win_predraw_abort`, window.rs:186), `wx_match_dot` (window.rs:47),
`vis_hold_until` (mode0.rs:256), `read_carried` (armed only in `read_deferred`),
all `blocking.rs` accessibility laws, `lcd_shift_dots`.

## Position analysis (the key finding, then the measurement that refined it)

Traced both clocks to the FF41 sample point with a clock counter: the deferred
read pays the previous M-cycle's parked 4T (`read_deferred`: `before =
clock.now(); clock.read(); advance_machine_t(before, now)`) landing at this
M-cycle's leading edge; the eager read samples BEFORE this M-cycle's
`tick_machine`, also at the previous M-cycle's end. **On a whole-dot counter they
land at the same `self.dot`.** But the tier2 laws use `mode3_entry_dot == 84`
while LE/EV use `80` — a 4-dot (8-hd) frame difference that is NOT a whole-dot
position difference but a calibration choice reflecting where SameBoy's read
genuinely sits (sub-dot). This is the 80-vs-84 conflict from the foundation map.

## The decisive experiment — enable `vis_mode_read` under EV, sweep the read frame

Edit A: `read_laws.rs:65` `if !self.tier2_reclock` → `if !(self.tier2_reclock ||
self.eager_value)` (apply the FF41 read laws under EV).
Edit B: `engine.rs read_pos_hd` += `if self.eager_value { N } else { 0 }` (shift
the eager read position N hd to test frame alignment; tier2 untouched — `eager_value`
is false there → byte-identical).

**EV CGB two-bin vs read-frame shift N (hd):**

| N (hd) | N (dots) | EV CGB fail | vs 578 |
|---:|---:|---:|---:|
| — (laws OFF, baseline) | — | 578 | 0 |
| +0 | +0 | **601** | +23 |
| +4 | +2 | **585** | +7 |
| +8 | +4 | **578** | +0 (== native verdict) |

**Monotone toward baseline; no even-hd shift dips below 578.** At +8hd (+4 dots =
the tier2 84-frame) the deferred exit constants exactly reproduce native
`vis_mode` for CGB → no change. At +0 the read laws fire in the wrong frame and
net +23.

Per-family delta of the +0 enable (comm of fail lists, `base_fails.txt` vs
`s1_fails.txt`): **broke 43** (speedchange 11, window 10, window/arg 9, sprites 6,
lcd_offset 2, m2int 3, oamdma 1) / **fixed 19** (window 10, sprites 4, window/arg
3, m0int 1, lcd_offset 1). The window family is a near-WASH (fixes ~13, breaks
~19) — the signature of a sub-dot straddle: some `_2` legs align at the eager
frame, their `_1` siblings do not, and no whole-dot shift separates the pair.

**mooneye EAGER gate at +8hd (`SLOPGB_MOONEYE_EAGER=1 … acceptance_ppu`):** only
`lcdon_timing-GS` fails (4 model combos — PRE-EXISTING, the task's known
exemption); **`intr_2_mode0_timing` / `intr_2_mode3_timing` PASS.** So the read
laws + a +8hd shift are intr_2-safe (entry stays frame-80 in `vis_mode`; the
exit shift does not break intr_2's mode0/mode3 counts). Not committable anyway —
+8hd == baseline (zero gain).

## The wall, stated exactly

The read-frame family (window-length + bare-exit + aborts, ~the largest CGB
block) cannot converge on the eager clock with a whole-dot read. The window
`vis_exit_hd` arms use FIXED deferred-frame constants (`259 + SCX&7`, arm 1) while
the bare arm (arm 8) is EMERGENT eager-frame (`2*flip + 2`, using the eager
`flip_dot`). A uniform read-position shift that aligns the fixed window arms
(+8hd) de-aligns the emergent bare arm, and vice-versa — they need DIFFERENT frame
treatment, i.e. the read must resolve to its true HALF-DOT (odd-hd = `dhalf == 1`)
per config. Under EV `dhalf` is always 0 (the eager clock never runs the half-dot
machine on the read path), so the sub-dot position the `_1`/`_2` pairs straddle is
UNREACHABLE. This is exactly HALFDOT Part B, un-hosted on the eager clock.

## The exact next lever (single, precise)

**Wire the half-dot read on the eager `Bus::read`** so the FF41 peek is sub-dot
precise (the doc's approach step 2). Concretely:
1. On the eager read path (`interconnect.rs:759`, `leading_edge_sample`), resolve
   the PPU to the read's exact half-dot before sampling FF41 — the eager analogue
   of `read_deferred`'s `advance_machine_t` half-dot drain (`Ppu::tick_half` /
   `dhalf`), WITHOUT moving the dispatch (dispatch stays cc+4). This gives EV reads
   an odd-hd `dhalf` when the CPU-T lands mid-dot, which `read_pos_hd` already
   consumes (`2*dot + dhalf`).
2. THEN enable `vis_mode_read` under `eager_value` (Edit A) — with the half-dot
   read position, the window arms and the bare arm each resolve at their own true
   half-dot, separating the `_1`/`_2` pairs (the doc's §5 atomicity: render ∧ read
   at half-dot together).
3. Re-measure the same shift sweep; expect the curve to now dip BELOW 578 (the
   pairs separate) instead of the flat 578→601 whole-dot curve measured here.

Only after the read-frame converges do the position-based families (accessibility
`blocking.rs` gated `tier2 || eager_value` + a cc+0 OAM/VRAM/palette value peek
routed through `leading_edge_sample`; the render-length `vis_hold_until` /
`win_predraw_abort` feeders) become worth porting — they were UNTESTED here
because the read-frame gates whether their effects reach an OCR verdict.

## What did NOT work (the honest signal)

- **Enabling `vis_mode_read` under EV (whole-dot):** +23 (601). Frame-mismatched.
- **+8hd read shift to the tier2 frame:** returns to 578 (native verdict) — zero
  gain, NOT the 291 the deferred clock gets, because the deferred clock ALSO has
  the half-dot machine (`advance_machine_t`) that the eager read lacks.
- **+4hd (half-dot midpoint, but still even):** 585 — between, still > 578. The
  minimum on the even-hd curve is 578 at +8hd. A real sub-578 point requires odd-hd
  (`dhalf`), only reachable via the half-dot read machine.
- Slice #2a write-commit staging: net-zero on EV CGB alone (needs render+read).

## Reproduction

`CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release
--no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head -1)`.
Two-bin: `SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN
--ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=` (ALWAYS
the exact test path — `--ignored flagon_probe` races 3 tests). Fail-list delta:
`scratchpad/{base,s1}_fails.txt`. mooneye gate: `SLOPGB_MOONEYE_EAGER=1
CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test mooneye acceptance_ppu`.
