# EAGER halt-fold — REFUTED: the STAT-IF FOLD is byte-identical OFF/EV; the halt `m0stat` rows fail on the mode-2 OAM **setup-ISR dispatch** landing one M-cycle early (the eager path omits production's `stat_late` running-CPU mask), and that mask is WELDED to the eager cc+0 read peek — delaying it recovers the halt rows but breaks 110 CGB / 56 DMG read-anchored rows (2026-07-09, #11cp)

Task (#11co redirect): find the dot at which the eager engine folds the mode-0
STAT bit into `intf` at HALT entry, compare to OFF/tier2, and correct it — the
#11cm-class engine fix ("the eager glitch-line IRQ rose 4 dots early"). Trace
first; a crisp refutation is a first-class result.

## Answer — the premise is WRONG (trust-the-trace): the fold is byte-identical; the divergence is the OAM SETUP-ISR dispatch, and it is read-frame-welded

- **The STAT-IF fold dot is IDENTICAL under OFF and EV.** On
  `late_m0int_halt_m0stat_scx3_3a` [Cgb] every STAT bit folds into `intf` at
  the SAME `(ly,dot)` in both clocks (the setup OAM pulse at ly=1 dot 0 cc=4;
  the ly=1 mode-0 rise at dot 257 cc=1). There is NO engine fold-dot shift to
  correct — the #11cm analogue does not exist here.
- **The real divergence: the mode-2 OAM *setup* ISR dispatches one M-cycle (4
  dots) EARLIER under EV** (ack ly=1 dot **12** EV vs dot **16** OFF), because
  the eager `stat_update_halt_masks` sets only `stat_halt_late` (the halt-exit
  mask) for the OAM line-start pulse, NOT `stat_late` (the running-CPU dispatch
  mask that production's `stat_events_tick` sets for every lines-1-143 pulse).
  The pulse folds on the M-cycle's last T (cc=4); without `stat_late` the eager
  cc+4 dispatch check sees it in its OWN M-cycle instead of one later.
- **That 4-dot head start cascades to an 80-dot HALT-position shift.** The setup
  ISR runs 4 dots early → the whole downstream alignment runs 4 dots early →
  `op_halt` lands at ly=1 dot **256** (EV) vs dot **336** (OFF), one dot BEFORE
  the ly=1 mode-0 STAT rise (dot 257). OFF reaches HALT *after* dispatching
  ly=1's STAT inline and then waits for ly=2; EV halts one dot early and its
  first idle wake-check (dot 260) catches ly=1's STAT — a one-line-early wake →
  reads mode 2 where SameBoy/OFF read mode 0.
- **The fix works but is WELDED.** Adding `stat_late` for the eager OAM pulse
  makes the CPU HALT at OFF's dot 336 and wake on ly=2 (the target row PASSES,
  eager CPU halts at production's dot: **YES**). But the `stat_late`→
  `if_stat_late` mask hides IF from `pending()` for one M-cycle, shifting the
  ISR one M-cycle later — and the eager clock reads FF41/palette via the cc+0
  PEEK (`vis_mode_read`) whose position is a FIXED offset from the dispatch dot,
  so the shifted dispatch drags every read-anchored OAM ISR's read to the wrong
  dot: **EV CGB 361→488 (+127), EV DMG 92→148 (+56).** cc-gating the mask to the
  last-T fold (cc==4) recovers 17 (488→471) but 110 remain welded — most OAM
  pulses fold at cc=4.
- **Nothing shipped. Tree byte-identical @ `c0b082d`** (empty `git diff`). EV CGB
  **361** / EV DMG **92** / tier2 CGB **291** (env unset). TRUE flip bar
  unchanged: **49 CGB BUG + 46 DMG BUG**; the 5 CGB + 6 DMG halt `m0stat`/`dec`
  rows sit inside it, welded.

## The dual-trace (single-row `late_m0int_halt_m0stat_scx3_3a`, Cgb, EV vs OFF)

Session-local `SLOPGB_HALTDBG` probes (all reverted) at: the `intf|=IF_STAT`
fold (`fold_ppu_events`), the running-CPU dispatch sample (`dispatch_pending_
impl`), the STAT ack (`ack_impl`), `set_cpu_halted`, the OAM pulse fold cc
(the `m0_rise`/`stat_late` sites), and the FF41/all-IO read/write path
(`Bus::read`/`Bus::write` with `cycles`).

| event | OFF (pass) | EV (fail) |
|---|---|---|
| setup OAM pulse folds `intf|=STAT` | ly=1 dot **0** cc=4 | ly=1 dot **0** cc=4 (IDENTICAL) |
| CPU dispatch-check first sees pending | dot 4, **cyc 4856** | dot 0, **cyc 4852** (4 T ahead) |
| setup OAM ISR ack | ly=1 dot **16** | ly=1 dot **12** |
| self-modifying align byte written to `$FFFC` | **0xb2** @ cyc 4868 | **0xb1** @ cyc 4864 |
| `op_halt` executes (entry pending=00, halts) | ly=1 dot **336** | ly=1 dot **256** |
| ly=1 mode-0 STAT folds | dot 257 cc=1 | dot 257 cc=1 (IDENTICAL) |
| HALT resolution | inline-dispatch ly=1 STAT, then **halt dot 336**, wake **ly=2** dot 260 → mode 0 ✓ | **halt dot 256**, first-check wake on ly=1 STAT dot 260 → mode 2 ✗ |

The instruction streams are byte-aligned (same cycles, same read values —
`$0168..$01b0` NOP sled, FF41 sync reads all matching) up to the setup OAM
ISR; the 4-T split is born ENTIRELY at that ISR's dispatch and propagates.

## Why it is welded (the read-frame coupling, same class as #11co/#11cn/#11bw)

The regression rows are exactly the read-anchored OAM/mode-2 ISRs: CGB
`cgbpal_m3` (palette read at m3-end), `dma` gdma/hdma `_cycles`,
`display_startstate`; DMG `m2int_m0irq`, `m2int_m3stat/scx` (the very
`m2int_m3stat_1` "reverts 3→0" the `stat_update_halt_masks` comment already
warned of), `window`, `sprites`, `oam_access`. Their ISR reads FF41/palette via
the eager cc+0 `vis_mode_read` peek, whose dot is a FIXED offset from the
dispatch dot. Delaying the dispatch one M-cycle (to the SameBoy-correct late
position the halt rows need) drags that peek one M-cycle past its calibrated
dot. The eager clock's correctness is the #11co two-frame decomposition:
early-dispatch + early-cc+0-read-peek CANCEL for a read-anchored ISR, so the
early OAM dispatch is *required* to keep those reads right. The halt setup ISR
does NOT read FF41 for its verdict (it computes an alignment byte), so it needs
the TRUE (late) dispatch — the one case the cancellation does not serve. The
two demands are mutually exclusive on this clock; the OAM-dispatch mask cannot
separate them. (Compensating the read peek by the same M-cycle to keep the
read fixed is the coupled #11co "tower" edit, which #11co measured strictly
worse — the peek offset also frames the m2stat/m0stat/enable ISR reads.)

## Corrections to the prior two maps (both partly wrong; the fold trace supersedes)

- **#11cn ("the eager HALT lands ~80 dots earlier via the cc+0 FF41 poll peek
  tipping the halt race")** — the 80-dot HALT-position shift is REAL and
  re-confirmed, but its cause is NOT the FF41 poll peek. It is the mode-2 OAM
  *setup* ISR dispatching one M-cycle early (missing `stat_late`); the FF41
  sync reads are byte-identical (all 0x81/0x85/0x86/0x87/0xa2 at matching
  cycles). The wake mask #11cn ported targets the wrong stage (the wake, not
  the setup dispatch) and is welded regardless.
- **#11co ("the eager CPU NEVER enters HALT — ly=1's mode-0 STAT is already
  folded into `intf` at the HALT instruction, dispatches inline")** — WRONG on
  both clauses. The eager CPU DOES `set_cpu_halted(true)` (traced), and at the
  `op_halt` entry `pending == 00` (the ly=1 STAT has not yet folded — it folds
  at dot 257, one dot AFTER `op_halt` at dot 256). The `set_cpu_halted`-count-0
  #11co saw is the *first-idle-check* wake firing immediately (the CPU halts,
  then wakes on its very first idle sample before the "staying-halted"
  `set_halted(true)` runs). The STAT-IF fold is byte-identical, not early.

## What refused, and why not to re-chase

- **Uniform eager OAM `stat_late`** → +127 CGB / +56 DMG (over-delays every
  read-anchored OAM ISR). REFUTED.
- **cc==4-gated eager OAM `stat_late`** (mask only the genuine last-T same-cycle
  race) → 488→471 CGB (recovers 17), 148 DMG. Most OAM pulses fold at cc=4, so
  the gate does not separate halt-wanted-late from anchor-wanted-early. REFUTED.
- **NOT re-attempted** (documented dead ends): the tier2 wake-mask port (#11cn,
  structurally unportable — eager `clock.now()` non-monotonic, `machine_now==0`);
  the true-T read retime (#11co, strictly worse, the cc+0 tower is load-bearing);
  the dispatch retime (#11cl/#11br/#11bs, thrice-refuted). A read-carry
  compensation to hold the anchor reads fixed while delaying the OAM dispatch is
  the coupled #11co "tower" rewrite — parked with the coherent eager half-dot
  read frame (HALFDOT Part B), which subsumes the OAM-dispatch/read coupling.

## The real lever (for the next session)

The halt `m0stat`/`dec` rows need the eager OAM setup-ISR dispatch at SameBoy's
true (late) position AND the read-anchored OAM ISR reads to stay fixed — i.e.
the dispatch and the cc+0 read peek DECOUPLED per-ISR. That is the coherent
eager half-dot read frame on `Bus::read` (HALFDOT Part B), the same un-hosted
lever #11bw/#11cl/#11cn/#11co all point to. Do NOT re-attempt the OAM-dispatch
mask, the wake-mask port, or the true-T read on the current clock — all three
are welded/refuted.

## Gate state (all HARD invariants green; nothing shipped)

Tree byte-identical @ `c0b082d` (empty `git diff`). EV CGB **361** / EV DMG
**92** / tier2 CGB two-bin **291** (env unset). golden_fingerprint / mooneye 92
flag-off / clippy `-D warnings` — unchanged (no `.rs` touched; the mechanism was
built, measured, and fully reverted). Eager tripwires unaffected. TRUE flip bar
**49 CGB BUG + 46 DMG BUG**, the 5 CGB + 6 DMG halt rows inside it, welded.

## Reproduction

```
git checkout eager-halt-fold        # byte-identical @ c0b082d
CARGO_TARGET_DIR=target/agF2 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agF2/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# baselines (env unset = byte-identical):
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 361
SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 92
# the refuted slice: in Ppu::stat_update_halt_masks (reclock.rs), for the mode-2
# OAM pulse (line != 0) add `if self.eager_value { self.stat_late = true; }` →
# EV CGB 361→488, DMG 92→148. cc==4-gate the if_stat_late apply in
# fold_ppu_events (tick.rs) under eager → 471. Both drop SameBoy-pass rows.
# HALT dual-trace: re-add the SLOPGB_HALTDBG eprintln probes (all reverted) at
# the fold / dispatch_pending / ack / set_cpu_halted / Bus::read sites.
```
