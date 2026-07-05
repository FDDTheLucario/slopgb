# S6 timer/serial completion frame — flag-gated deferred-completion advance BUILD-MEASURED as READ-FRAME-WELDED (2026-07-04, #11bu)

Attack on the **S6 TIMER/SERIAL COMPLETION FAMILY** — the last un-attempted
flag-gated slice before the C3-flip architectural wall. The #11bt census
(`c3-flip-census-2026-07-04.md`) counted this family as ~52 of the 98 DMG
SameBoy-pass flip-blockers (tima 45 + serial 1 gambatte-OCR + gbmicro
`int_timer_halt` ×2 + wilbertpol `timer_if` ×4), and separated it from the
refuted dispatch reclock as a distinct, orthogonal, "still-unbuilt" lever.

## VERDICT: **REFUTED as a flag-gated slice.** The timer/serial completion is
welded to the read frame by TWO mechanisms with NO read-time discriminator.
Tree left **byte-identical @ `d3d7d40`** (`git diff --quiet` clean; all
measurement scaffolding reverted). Every gate stays at baseline. The
flag-gated approach for this family is **EXHAUSTED**; the fix needs the same
coherent retime (write-commit + read frame co-move) the dispatch reclock
needs.

## 0. Base + method

- Worktree `phase-b-s7` @ `d3d7d40`, `git diff --quiet` before and after.
- Target rowlist = the 46 DMG gambatte-OCR tima/serial SameBoy-pass blockers
  (`c3_dmg_sbpass_blockers.txt` grep `tima|serial`, `[Dmg]` tag). All 46 pass
  flag-OFF (production), all 46 fail flag-ON — true flip-blockers.
- Probe: `gambatte::flagon_probe::flagon_probe` ON (`boot_with_reclock`) vs the
  full DMG rowlist ON baseline (116 fails). A row is a REGRESSION iff it passes
  baseline-ON and fails with the peek; a FIX iff it fails baseline-ON and passes.
- Scaffolding (all reverted): `Timer::dbg_state` + `#[derive(Clone)]`,
  `Serial` `#[derive(Clone)]`, an `SLOPGB_TIMDBG` timer-state tracer and an
  `SLOPGB_S6PEEK` completion-peek in `interconnect/cycle.rs::read_deferred`.

## 1. The mechanism (dual-traced, confirmed against the code)

The tier2 deferred FF0F/FF05/FF06 read (`read_deferred`) advances the machine
to the M-cycle **leading edge (cc+0)** and samples the timer, then parks this
M-cycle's 4 T. Production (eager) ticks the full M-cycle first and reads at
cc+4. So the deferred read samples the timer/serial **one M-cycle (4 T) early**:
TIMA is one increment behind, an overflow/reload pending in the parked window
has not yet fired, and IF_TIMER/IF_SERIAL for a completion in the window is not
yet set. A clone-advanced peek (`pending` T forward on a Timer/Serial clone)
reconstructs the cc+4 view for the **clean** reload reads
(`tc00_ff_tma_3`: peek TIMA=fe = want FE; `tc00_irq_2`: peek IF=04 → E0→E4 = want E4).

## 2. Build-measure — four peek variants, all net-negative or dropping a SameBoy-pass

`SLOPGB_S6PEEK` reconstructs production's cc+4 timer view at the deferred read:

| variant | what it does | fix / 46 | full-DMG regressions |
|---|---|---:|---|
| `full` | replace FF05←tima', FF06←tma', FF0F\|=timer+serial IF' | 21 | **−28** (9 serial `_1` + 19 div-write tima) |
| `reload` | reload-window FF05←TMA + serial-always FF0F OR | 11 | (serial `_1` welded) |
| `tif5` | FF0F\|=timer-IF' + FF05←TMA on reload | 11 | **−4** (`late_div_write_1b`/`late_tc01_5`/`late_div_write_if_1a` want 00/E0) |
| `tif` | FF0F\|=timer-IF' ONLY (OR, verdict-only, safest) | 4 | **−1** (`tc00_late_div_write_if_1a`, want E0) |

Even the minimal, safest possible peek — an OR-only fold of the imminent timer
completion into the FF0F verdict (the exact `ff0f_stat_peek` shape) — fixes 4
`irq_2` rows but **drops `tc00_late_div_write_if_1a`** (a currently-green,
production-passing, SameBoy-pass row wanting E0). That is a forbidden drop. No
variant is a clean +N/−0.

## 3. The decisive co-temporal proof — identical timer read-state, opposite wants

Three pairs, dual-traced (`SLOPGB_TIMDBG`): each pair has the **identical
timer read-state** (`rin`, `tima`, `tma`, `pend`, and the clone-advance
`ahead_if`) yet **opposite `want`**. No bus-/timer-observable field separates
them:

| row A (peek FIXES) | state | row B (peek DROPS) | state | discriminator |
|---|---|---|---|---|
| `tc00_irq_2` want **E4** | ff0f rin=4 tima=00 tma=fe pend=4 ahead_if=04 | `tc00_late_div_write_if_1a` want **E0** | ff0f rin=4 tima=00 tma=fe pend=4 ahead_if=04 | only `div` (bc00 vs **0400**) |
| `tc00_ff_tma_3` want **FE** | ff05 rin=4 tima=00 tma=fe pend=4 | `tc00_late_div_write_1b` want **00** | ff05 rin=4 tima=00 tma=fe pend=4 | only `div` (bc00 vs **0400**) |
| `tc00_ff_tma_3` want **FE** | ff05 rin=4 tima=00 tma=fe div=**bc00** | `tc00_late_tc01_5` want **00** | ff05 rin=4 tima=00 tma=fe div=**b610** | **NONE** (both large div) |

The first two pairs differ only in the `div` VALUE (0400 = a `late_div_write`
just reset the counter). The third pair kills even a `div`-recency heuristic:
`late_tc01_5` has a LARGE div (b610, no recent reset) identical in timer-read
state to `ff_tma_3` (bc00), opposite want. **There is no read-time
discriminator — timer state OR div magnitude — that separates the fix rows
from the drop rows.**

## 4. Why it is welded (two mechanisms, code-grounded)

The `reload_in` state (the completion the read misses) is itself **desynced**
between the flag-on and production frames:

1. **C0 DIV +4** (`interconnect/boot.rs`, the reclock's construction-time
   `div += 4`). TIMA increments and the overflow that *triggers* the reload are
   DIV-falling-edge events, so the +4 shifts them ~4 T under flag-on. The
   reload countdown (`reload_in`, an absolute-T pipeline) is NOT shifted, so a
   completion the reload frame lands at a position that does not line up with
   either a cc+0 or a cc+4 read. Confirmed dead by #11ai's C0-DIV sweep
   `{−4..12}` (zero effect) — the +4 is load-bearing for `boot_div`/DIV reads
   and cannot be removed to "un-shift" the timer.
2. **Deferred write-commit desync** (the new finding). A `late_div_write` /
   TAC-write that *triggers* the overflow commits at the M-cycle leading edge
   (cc+0) under the deferred frame vs a later cc under production's
   write-conflict deferral. So flag-on's overflow → `reload_in=4` fires ~1
   M-cycle EARLIER than production's: at the read, flag-on has `rin=4` while
   production has not yet overflowed. Same `rin=4` observable, opposite
   physical reality. This is the write-frame analogue of the dispatch weld
   (#11bs) — the completion's *trigger* moved with the write frame, and the
   read frame cannot compensate.

## 5. Serial is separately welded

The serial `_1`/`_2` legs are a co-temporal A/B: `_2` wants the completion bit
(SameBoy-pass blocker), `_1` wants no bit (currently green). The completion is
imminent within BOTH reads' cc+0→cc+4 windows on the flag-on frame (the +4
shifts the DIV-bit-7 falling edge that clocks the serial), so ORing the
imminent serial IF fixes `_2` and drops all **9** `_1` legs
(`serial/start_wait_read_if_1`, `nopx1/2_*`, `div_write_*`, `start_late_div_write_*`,
`start_wait_restart_*`). Same #11ah finding, now on DMG: SameBoy reads the same
IF bit for both legs a few dots apart; slopgb's cc+0 frame collapses them.

## 6. Conclusion

The S6 timer/serial completion is the third independently-refuted face of the
same architectural weld (dispatch #11ai/#11br/#11bs; now the timer completion).
The completion's IF-set and TIMA-reload are triggered by DIV-edge and write-
commit events that the reclock has already shifted; the deferred read then
samples one M-cycle early into that shifted frame. A read-value peek can
reconstruct flag-on's OWN cc+4 view, but that is NOT production's cc+4 view
(the +4 and the write-commit desync diverge them), so every peek that fixes a
`_2`/reload row drops the co-temporal `_1`/div-write sibling. **No flag-gated
slice exists.** The clean fix requires the timer's completion frame (its DIV
phase AND its write-commit point) to co-move with the read frame — i.e. the
same coherent per-T retime (HALFDOT Part A: eager PPU/timer, deferred CPU
clock) the dispatch core needs. The flag-gated attack surface for the C3 flip
is now confirmed exhausted across all three families (render DONE, dispatch
atomic, S6 completion welded); the flip is gated solely on the coroutine
rewrite.

## 7. Reproduction

- Target rows: `grep -E 'tima|serial' scratchpad/c3_dmg_sbpass_blockers.txt | sed 's/$/ [Dmg]/'`.
- Peek variants sat behind `SLOPGB_S6PEEK={full,tif,tif5,reload}` in
  `read_deferred` (reverted; the clone-advance shape is in this session's
  transcript). Dual-trace via a temp `SLOPGB_TIMDBG` line dumping
  `rin/tima/tma/div/pend` + a `pending`-T clone-advance peek (`ahead_if`,
  `a_tima`). All reverted; tree byte-identical.
