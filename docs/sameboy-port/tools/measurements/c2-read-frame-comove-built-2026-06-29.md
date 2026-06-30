# C2 #11ai — the read-frame co-move BUILT + measured: option A & the serial lever REFUTED, the dispatch-coupling NAILED

2026-06-29. The goal's C2 START: build the ISR-frame read-frame co-land (the
#11z' "+4 PPU-advance lag, NOT bus.tick"), converge the read-frame flip-BUG class
flag-on at mooneye 91/91. **Result: ESCAPE — no byte-identical-OFF lever converges
the read-frame beyond #11z while holding mooneye 91/91.** Every realization was
BUILT and MEASURED (not reasoned): the cost is exact, two prior diagnoses are
corrected, and the refined co-move spec is sharpened to three distinct atomic
levers. Defaults NOT flipped; HEAD byte-identical (all experiments env-gated,
reverted). The map advances C2 even at 0 net rows (the goal's ESCAPE deliverable).

## Baselines (this session, fresh HEAD binaries)

| set | flag-on (tier2) fail | note |
|---|---|---|
| READ-FRAME class (20) | 20/20 | the #11ah READ-FRAME subset |
| LE-pass (44, `SLOPGB_PROBE_LE` passers) | 44/44 | the "primary convergence target" |
| window family (255 testable) | 59 | |
| mooneye flag-on (`SLOPGB_MOONEYE_RECLOCK`) | **91/91** | the convergence GATE |

## LEVER 1 — option A: PPU +4 machine advance at dispatch (the goal's primary) — REFUTED, 3 ways

`interconnect.rs::dispatch_retime`, env-gated `SLOPGB_C2ADV`: after the vector
retime, `advance_machine_t(clock.now(), clock.now()+4)` (SS) — advance the PPU +
timer + serial 4 dots WITHOUT moving the CPU `clock` (the goal's "PPU-only nudge,
no CPU cycle"). Measured:

| measurement | C2ADV ON | control | verdict |
|---|---|---|---|
| READ-FRAME (20) | 8 pass / 12 fail | 0 pass | "8 fixed" — but see below |
| window (255) | **90 fail** | 59 fail | **+31 REGRESSION (drift)** |
| mooneye flag-on | **89/91** | 91/91 | acceptance_ppu + acceptance_root FAIL |

**(a) It shifts the counter-pinned IRQ DISPATCH (the fatal flaw).** The 2 broken
mooneye groups are the interrupt-TIMING tests that HANG (`B=C=…=42`, never reach
the Fibonacci): `intr_2_0_timing`, `intr_2_mode0_timing`, `intr_2_mode0_timing_sprites`,
`intr_2_mode3_timing`, `intr_2_oam_ok_timing`, `hblank_ly_scx_timing-GS` — all
models. These pin the mode-0/mode-2 IRQ **dispatch dot** to HARDWARE. Advancing
the PPU +4 fires the dispatch 4 dots early → the tests' NOP-counted measurement is
off → they never complete. **So the +4 is READ-position-only; the dispatch must
NOT move. Option A moves the whole PPU, which couples read+dispatch — the exact
coupling SameBoy DECOUPLES.** (This is NOT the bus.tick di_timing break of #11z' —
option A adds no CPU cycle; it is a distinct, PPU-side dispatch shift. Same
mooneye score 89, different mechanism.)

**(b) It drifts cumulatively.** The +4/dispatch advance is never reabsorbed, so
`cycles` inflates by 4 × (dispatches) and the OCR capture frame mis-aligns →
window +31.

**(c) The "8 READ-FRAME fixed" are DRIFT ARTIFACTS, not read+4 fixes.** The fixed
set includes `serial _2` flipping **E8 → E0** under "advance the machine MORE" —
anti-physical (advancing cannot CLEAR a set bit); it is the OCR frame shifting.
The real read+4 (option B, a pure read-position offset with NO machine advance) is
the already-shipped #11z window law for windows and a NO-OP for bare lines (the
bare boundary is in slopgb's own frame, so read+4 ∧ boundary+4 cancel) — confirmed
algebraically + by the prior session's empirical `in_isr` no-op (window 79 = #11z).

**Conclusion:** the post-dispatch ISR read lands +4 vs SameBoy ONLY as a read
POSITION; in slopgb the read and the dispatch are RIGIDLY COUPLED (both driven off
the same whole-dot PPU clock), so no whole-PPU lever can move one without the
other. SameBoy decouples them via the T-granular `read_high_memory` sub-M-cycle
sample (read at cfl+4, dispatch at cfl+0). **The genuine lever is the S7
sub-M-cycle read clock (architectural, multi-session), exactly as #11v/#11z'/#11ah
concluded — now build-confirmed by option A's dispatch-shift failure.**

## LEVER 2 — serial/tima `_1`: the C0-DIV diagnosis (#11ah) REFUTED; it is the deferred COMPLETION frame

The serial/tima `_1` legs read CLEAR (E0) where BOTH oracles read SET
(SameBoy + gambatte want E8/E4); the `_2` legs are gambatte-reference floors
(SameBoy reads E8/E4, gambatte wants E0 — the C2 flip rebaselines, NOT a drop).
So `_1` is a genuine SameBoy-pass the tier2 read-frame regresses.

**Trace (slopgb deferred, `SLOPGB_S5DBG`):** serial `_1` reads FF0F at ly8 **dot28
if=00**; `_2` at ly8 **dot32 if=08**. The serial IF bit (0x08) is set between dot28
and dot32. SameBoy reads SET at BOTH cfl168/cfl172 (#11ah) → SameBoy's serial
completes ≤cfl168, i.e. ~4 dots EARLIER than slopgb's ~dot29-32. slopgb's serial
completes 1 M-cycle LATE relative to the read.

Two levers tried and BUILD-REFUTED:
- **FF0F read at cc+4** (`SLOPGB_C2IF`, route the IF read to the trailing edge):
  moved `_1`'s read dot28 → dot32 but it STILL read **if=00** — slopgb's serial in
  `_1` completes even later than dot32 (the `_1`/`_2` ROMs differ in completion
  time, not just read time). 0/8 fixed. The read frame is not the lever.
- **C0 boot-DIV offset sweep** (`SLOPGB_C0DIV` ∈ {−4, 0, 4, 8, 12}): **ZERO effect**
  on serial/tima `_1` (4 fail at every offset). **This REFUTES #11ah's "C0 +4 DIV
  is the serial/tima read-frame lever."** The boot-DIV phase does not move the
  completion-vs-read relationship.

**Corrected diagnosis:** the deferred-commit leading-edge (cc+0) read samples IF
as of the PREVIOUS M-cycle's tail; the serial/timer completion landing in the
CURRENT M-cycle's tail is invisible. The eager (production, cc+4) read ticks the
current M-cycle first and catches it — which is why OFF passes `_1`. The lever is
the **deferred serial/timer COMPLETION advance** (fire the IF-set at the leading
edge instead of 1 M-cycle deferred), i.e. the S6 machine-advance reconciliation —
NOT C0-DIV, NOT the FF0F read frame. Atomic (the timer is a free-running counter;
a per-dispatch advance drifts; a phase shift breaks `boot_div`/timer mooneye).

## LEVER 3 — the SS read-frame rows (display_startstate/irq_precedence/m2int_m2irq): RENDER-FRAME

The 6 non-DS, non-serial/tima READ-FRAME rows: **5 of 6 PASS leading-edge-only**
(`SLOPGB_PROBE_LE`) — pure RENDER-FRAME regressions (the engine + LE-read are
correct; only the full tier2 deferred-frame + C0 breaks them). `display_startstate
stat*` want 84 (mode 0), tier2 got 87 (mode 3): the deferred read lands at ly0
**dot252 on a NORMAL line** (`glitch=0 ve=0 lrd=0`) where SameBoy reads the
GLITCH-line mode 0 (SBMODE ly0 cfl76 vis=0). The C0-DIV-shifted read POSITION
lands the program's read on a different line than SameBoy. Atomic with the global
deferred frame.

## The refined C2 co-move spec (the deliverable)

The READ-FRAME class (20) is THREE mechanically distinct atomic levers, **none a
clean byte-identical-OFF slice**:

| sub-class | rows | lever (build-measured) |
|---|---|---|
| serial/tima `_1` | 4 (+ 4 `_2` gambatte-ref) | the deferred serial/timer COMPLETION advance (S6 machine-advance reconciliation). NOT C0-DIV (sweep-refuted), NOT the read frame (cc+4-refuted). |
| DS m2int (m0irq/m2stat/m0stat `_ds_2`) | 6 | the DS read grid (S7, the +2 DS read offset, deferred with the DS frame) |
| SS m2int / display_startstate / irq_precedence | 6 (5 RENDER-FRAME) | the global deferred read POSITION (couples to the C-stage frame; the read lands on the wrong line/dot) |

**The unifying barrier (build-confirmed this session):** the ISR read's +4 frame
is rigidly coupled to the IRQ dispatch in slopgb's whole-dot model. SameBoy
decouples them (read cfl+4, dispatch cfl+0) only via the T-granular
`read_high_memory` sub-M-cycle sample. So the read-frame co-land is NOT a flag-gated
nudge — it is the **S7 sub-M-cycle read clock + the S6 deferred-completion
reconciliation**, landing TOGETHER with the C-stage dispatch rebaseline
(intermediate states RED). This is the PORT-PLAN S6/S7 stage, not a slice.

## What this session adds over #11ah / the prior C2 sessions

1. **Option A (PPU-only +4) BUILT + measured** (the goal's primary, never built):
   refuted via the **dispatch-shift** mechanism (intr_2_* hang) — distinct from the
   #11z' bus.tick di_timing break — proving read+dispatch are coupled.
2. **#11ah's "C0 +4 DIV is the serial/tima lever" REFUTED** by the C0DIV sweep
   (0 effect); corrected to the deferred-completion frame.
3. **The FF0F-read-frame serial lever REFUTED** (cc+4 read still misses the bit).
4. **The SS read-frame rows characterized** (5/6 RENDER-FRAME; display_startstate =
   wrong-line read position).

## Method / tooling
- `flagon_probe` binary (`CARGO_TARGET_DIR=target/gbtr ... --no-run`; `gbtr-<hash>
  --ignored flagon_probe`), `SLOPGB_ROWLIST` / `SLOPGB_PROBE_OFF` / `SLOPGB_PROBE_LE`
  / `SLOPGB_S5DBG`. Env experiment gates (all reverted, byte-identical OFF):
  `SLOPGB_C2ADV` (PPU advance), `SLOPGB_C2IF` (FF0F cc+4), `SLOPGB_C0DIV` (boot-DIV).
- SameBoy `--cgb` SBREAD/SBMODE (the tester loops on polled reads → cap with a
  short `--length` + `head`; the serial ROM's FF0F poll floods, use #11ah's
  captured cfl168/172 ground truth).
- mooneye gate: `SLOPGB_REQUIRE_ROMS=1 SLOPGB_MOONEYE_RECLOCK=1 cargo test --test
  mooneye --release` → 91/91 the flag-on convergence gate.
- Rowlists in `scratchpad/`: `read_frame_rows.txt` (20), `le_pass_rows.txt` (44),
  `window_rows.txt`, `st_rows.txt`/`st1_rows.txt` (serial/tima).
