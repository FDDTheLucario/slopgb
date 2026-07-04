# The dispatch-atomic core — fresh characterization + coherent-retime BUILD PLAN (2026-07-04, #11br)

Task #4 attack on the §3b ENGINE DISPATCH-ATOMIC CORE — the rows that fail
flag-on because slopgb's interrupt DISPATCH samples at its own frame, not
SameBoy's. **Result: the primary dispatch lever (an imminent-rise dispatch
fold) was BUILT and build-measured to be ATOMIC — it fixes 22 dispatch-presence
rows but breaks `mooneye intr_2_0_timing` (B=42, all 4 DMG-family models) AND
drops 9 gbmicro count/timing SameBoy-passes. NO clean flag-gated slice exists;
the dispatch must co-move with the read frame (one coherent retime). Tree left
byte-identical (fold reverted); every gate green.** This map is the code-grounded
build plan for the coherent retime + the exact measurements pinning it.

## 1. The core, freshly measured (worktree `phase-b-s7` @ d3d7d40)

Method: the banked `gbmicro_flagon_probe` / `wilbertpol_flagon_probe`
(`boot_with_reclock`, FF82/fib verdict) ON vs OFF (`SLOPGB_PROBE_OFF`), the
flip-blockers = ON-fail ∩ OFF-pass. gbmicro flag-on **445/68** (matches #11bm);
the 68 minus the 30 no-verdict testbenches minus the production baseline = **31
true flip-blockers**, plus wilbertpol 7 + age 1 = **≈39 dispatch-atomic rows**
(the "43" of prior sessions, re-censused; the S6/read-frame rows split out below).

| family | rows | got→want | class |
|---|---|---|---|
| `hblank_int_scx{0-7}_if_b` | 8 | E0 → FF | **DISPATCH LOST** |
| `hblank_int_scx{1-7}_nops_a` | 7 | 00 → FF | **DISPATCH LOST** (ISR reads DIV) |
| `hblank_int_scx{1-7}_nops_b` | 7 | 00 → FF | **DISPATCH LOST** |
| `hblank_int_scx7` | 1 | 2E → 2F | dispatch COUNT (INC-A one short) |
| `hblank_scx3_if_a` / `_int_a` | 2 | E2/00 → FF | dispatch-frame |
| `hblank_scx3_if_b` / `_if_c` | 2 | 00 → E0/E2 | read-frame gap (scx3 not covered by #11bk) |
| `int_timer_halt` / `_div_b` | 2 | 0E/02 → 0F/03 | **S6 timer-completion** (not PPU; #11ai DO-NOT-RETRY) |
| `stat_write_glitch_l1_a` / `_l143_a` | 2 | E2 → E0 | engine glitch-IF (spurious STAT bit) |
| wilbertpol `ly_lyc_153_write` GS×3 / C×1 | 4 | B=48 | line-153 LYC dispatch-frame |
| wilbertpol `timer_if` Dmg/Mgb/Sgb | 3 | B=48 | **S6 timer-completion** |
| age `halt-m0-interrupt` | 1 | — | mode-0 halt-wake dispatch |

The pure DISPATCH-frame subset (must move the dispatch dot) = the `if_b` (8) +
`nops` (14) + `hblank_int_scx7` (1) + `hblank_scx3 a/int_a` (2) + `ly_lyc_153`
(4) + age (1) = **30**. The remaining 9 are S6-completion (5) / engine-glitch
(2) / scx3 read-frame (2) — orthogonal levers, not the dispatch dot.

## 2. The mechanism, code-grounded (why the dispatch is LOST)

`hblank_int_scx0_if_b`: `EI; …NOP sled…; ldh a,(FF0F); DI; <verdict>` with the
mode-0 STAT interrupt armed (`FF41=0x08`, `IE=0x02`). The `ldh` reads FF0F = E0
(correct); the mode-0 rise `R = 254 + SCX&7` then fires; the interrupt must
DISPATCH before the following `DI` clears IME. On hardware/SameBoy it does; the
reclock loses it.

The reclock's deferred-commit model (`interconnect/cycle.rs::read_deferred`,
`tick.rs::advance_machine_t`): a read pays the PREVIOUS M-cycle's parked debt,
samples at the M-cycle **leading edge (cc+0)**, then parks this M-cycle's 4 T.
`flush_pending` catches the machine up to the instruction boundary. So at the
running CPU's dispatch check (`cpu/execute.rs:130`
`if cpu.ime && bus.pending_dispatch() != 0`), the machine sits at the current
fetch's cc+0 — **4 dots (one M-cycle) behind** where production/SameBoy have
advanced the PPU (through the fetch to cc+4). For `if_b` the DI fetch's cc+0 is
dot 252, R = 254: `pending()` at 252 has no `IF_STAT` → no dispatch → DI runs,
clears IME → the rise fires during the following `flush_pending` with IME=0 →
dispatch LOST. `Interconnect::dispatch_pending_impl` (`interconnect/speed.rs`,
#11bf) already handles the OPPOSITE case (a rise slopgb committed too early is
masked off via the `stat_vis_from_t` deadline) but has no forward fold for a
rise imminent WITHIN the fetch M-cycle.

## 3. The lever tried this session — imminent-rise dispatch FOLD (BUILT, ATOMIC, reverted)

Built `Ppu::dmg_m0_dispatch_imminent()` (`R > dot && R <= dot+4` via the #11bk
`dmg_m0_if_rise` anchor, tier2+`!is_cgb`+SS+non-glitch scoped) and folded it
into `dispatch_pending_impl`: when the flushed `pending()` lacks `IF_STAT`, the
STAT source is IE-enabled, the rise has NOT yet fired (`!stat_rise_m0`), and a
mode-0 rise is imminent within the fetch M-cycle, return `w | IF_STAT`. A PEEK
(no machine advance — a flush there breaks 8 pins, #11bf); the rise fires for
real during the dispatch's own ticks/pushes, so the post-high-push re-eval
picks + acks the real bit.

**Build-measured verdict (the reason it cannot ship):**

- gbmicro flag-on **445→458** (+13 net): FIXED all 22 `if_b` + `nops` rows.
- **DROPPED 9 gbmicro SameBoy-passes**: `hblank_int_scx{0,1,2,4,5,6}` (bare,
  INC-A dispatch COUNT, got 2C want 2D), `hblank_int_l1`/`l2` (got 31 want 32),
  `hblank_int_di_timing_b` (got 02 want FF). All pass OFF (hardware-correct).
- **mooneye flag-on 91→90**: `acceptance/ppu/intr_2_0_timing.gb` HANGS at
  **B=42** on `[Dmg]` `[Mgb]` `[Sgb]` `[Sgb2]` — the counter-pinned dispatch
  test the docs warned of (#11ai, HALFDOT §3 Part-C).

**Why it is atomic (the decisive diagnosis):** the fold makes the dispatch
sample at cc+4 (SameBoy's frame) while the READS still sample at cc+0. That is
an **incoherent frame** — reads and dispatch in DIFFERENT frames. `intr_2_0_timing`
(which reads registers to time the dispatch) detects the mismatch and hangs; the
gbmicro COUNT rows (which count dispatches via the same mode-0 rise) mis-count
because the dispatch moved but the read frame they compare against did not. The
`if_b`/`nops` PRESENCE rows are the only ones insensitive to the mismatch (they
only test "did it dispatch at all"). There is NO bus-observable discriminator
between a presence test and a count test using the same rise — so no tighter
scope separates the +22 from the −9. The dispatch dot cannot move alone.

This is the direct, this-session confirmation of the multi-session verdict
(#11ae/#11ah/#11ai/#11al/#11bj, HALFDOT §5): the reclock moves ALL reads to
SameBoy's frame at once; the dispatch must co-move; intermediate (dispatch-only
or read-only) states are RED.

## 4. The coherent retime — what must move, why mooneye holds, staging

SameBoy advances the PPU EAGERLY per-T as the CPU consumes cycles; both reads
and the interrupt check sample the PPU at the exact current T. For `if_b`,
SameBoy's read of FF0F samples the read M-cycle's cc+0 (E0, correct) AND the
interrupt check after the DI fetch samples the PPU at the fetch's cc+4 (the PPU
eagerly advanced through the fetch → R fired → dispatch). Both coherent because
the PPU is always at the exact T.

slopgb must reproduce this WITHOUT the two failed shortcuts (fold = incoherent
frame; flush = shifts every following operand read, #11bf 8-pin break). The
solution is HALFDOT-BUILD-PLAN Part A: **advance the PPU per-T (half-dot)
continuously (eager), decoupled from the CPU deferred-commit clock.** Then:

1. **PPU eager, CPU deferred (`interconnect/tick.rs::advance_machine_t` +
   `ppu/mod.rs::tick`).** The half-dot grain (already landed, #11ba) advances
   the PPU per T; the CPU's `pending_cycles` clock (`cycle_clock.rs`) keeps
   deferring the CPU-side bookkeeping ONLY. A read samples the PPU at the read's
   exact T (= today's cc+0 leading edge — unchanged); the interrupt check after
   a fetch samples the PPU at the fetch's cc+4 (the PPU has eagerly advanced
   through the fetch) — the change from today's cc+0 boundary view.
2. **`pending_dispatch` / `pending_halt_wake` read the eager PPU.** No fold, no
   `stat_vis_from_t` deadline table — the PPU is genuinely at cc+4 at the check,
   so `intf` genuinely has `IF_STAT` iff R fired within the fetch (the `if_b`
   fix) AND genuinely lacks it when R is later (the count/`intr_2` non-break).
   The #11bf `stat_vis_from_t` mask + `if_late`/`m0_halt_hold` masks retire
   (they approximate the missing eager advance).
3. **The reads stay in SameBoy's frame.** The deferred CPU clock still parks the
   operand-read debt, so the following instruction's operand reads land at their
   cc+0 (the #11bf 8-pin invariant holds — the CPU clock is untouched; only the
   PPU advance is made eager). This is the coherence the fold lacked.

**Why mooneye holds at the new frame:** production (fully eager PPU) passes BOTH
`intr_2_0_timing` and `if_b`. The retime makes slopgb's PPU eager for the
dispatch check (= production's dispatch M-cycle) while keeping reads at cc+0 (=
SameBoy's read frame). The dispatch M-cycle matches production → `intr_2_0_timing`
+ `int_hblank` + `di_timing` hold (B=42 avoided); the read frame matches SameBoy
→ the read-frame rows resolve. The two frames are now ONE (PPU eager, CPU
deferred) — exactly SameBoy's `GB_advance_cycles` + `pending_cycles` split.

**Staging (HALFDOT §6, refined by this session):**
- **A-render/A-infra:** wire the half-dot PPU advance into `advance_machine_t`
  so the PPU state is exact-T at every read AND every dispatch/halt check (the
  grain exists; the consumers — `read_deferred`, `pending_dispatch`,
  `pending_halt_wake` — must read the exact-T PPU, not the deferred boundary).
- **Retire the approximations:** the `stat_vis_from_t` deadline
  (`dispatch_pending_impl`/`halt_entry_impl`), `if_late`/`m0_halt_hold`
  (`tick.rs`), and the #11bk read-frame DELIVER/SERVICE-CLEAR laws all collapse
  into the eager-PPU exact-T sample (do NOT keep both — they are the
  whole-dot patch for the missing per-T advance).
- **Gate:** mooneye flag-on 91/91 (the B=42 quartet is the make-or-break) AND
  flag-off 91/91; gbmicro no drop (the 9 count/`di_timing` rows + the 22
  presence rows converge TOGETHER, not one side); CGB two-bin zero new
  regression; pixel two-bin 100/100 held. Intermediate states are RED — converge
  the eager PPU advance across reads∧dispatch∧wake, then measure clean.

## 5. The orthogonal residual (NOT the dispatch dot)

- **S6 timer-completion (5):** `int_timer_halt{,_div_b}` + `timer_if` (Dmg/Mgb/
  Sgb). The leading-edge FF0F/IF read samples the timer IF one M-cycle before the
  completion lands. Lever = the S6 deferred-completion advance (a timer-domain
  event); C0-DIV sweep `{−4..12}` has ZERO effect (#11ai DO-NOT-RETRY). Lands
  with S6, not the PPU dispatch retime.
- **`stat_write_glitch_l1_a`/`_l143_a` (2):** got E2 want E0 — slopgb fires a
  mid-mode STAT-write glitch IF SameBoy lacks. Engine-side (spurious edge), a
  separate glitch-suppression lever; not the dispatch dot.
- **`hblank_scx3_if_b`/`_if_c` (2):** got 00 want E0/E2 — the #11bk DELIVER/
  SERVICE-CLEAR window does not cover the `scx3` (no-`int`) family's read dots;
  a read-frame scope gap entangled with the `scx3` dispatch legs (`if_a`/`int_a`).

## 6. Tooling / reproduction (banked)

- gbmicro flag-on: `SLOPGB_ROWLIST=scratchpad/gbm_all_rows.txt` (all 513 rows,
  `[Dmg]`) → the `gbmicro_flagon_probe` (`#[ignore]`, `boot_with_reclock`);
  `SLOPGB_PROBE_OFF=1` for the OFF bin; flip-blockers = `comm -23 on off`.
- wilbertpol flag-on: `wilbertpol_flagon_probe` with the `ly_lyc_153_write`/
  `timer_if` rows (fib verdict, B=48 = dispatch-frame).
- mooneye flag-on gate: `SLOPGB_MOONEYE_RECLOCK=1 --test mooneye` (the B=42
  quartet). Build the worktree crate with an explicit `--manifest-path
  <worktree>/Cargo.toml` (a git worktree + a bare `cargo` can resolve the wrong
  workspace root).

## 8. REFUTED — the eager-PPU/deferred-CPU split reproduces the incoherent-frame drop (2026-07-04, #11bs)

The §4 build plan was **IMPLEMENTED and BUILD-MEASURED. It does not converge:
the eager split fixes the 22 `if_b`/`nops` presence rows but DROPS 53
count/timing rows that pass on production AND the deferred reclock AND
SameBoy, and breaks mooneye (88/91). The §4 claim "the two frames are now ONE
(PPU eager, CPU deferred)" is false for the dispatch-COUNT tests — dispatch at
cc+4 with ISR reads at cc+0 is INCOHERENT. Tree reverted to `d3d7d40`
byte-identical; every gate back to baseline (mooneye ON 91/91, gbmicro ON 445,
pixel 123/123).** This is the DMG-single-speed, full-split confirmation of the
#11ai C2ADV / #11br fold atomicity — a third independent refutation.

### What was built (clean, byte-identical OFF, then reverted)

- `Interconnect::machine_t` + `advance_machine_to(target)` — a never-rewind
  choke point every deferred advance routes through (`machine_t == clock.now()`
  after every op → byte-identical to the old `advance_machine_t(before,
  clock.now())`; verified mooneye 91/91 ON+OFF, gbmicro ON 445 with the wrapper
  in and `eager_ppu` off).
- `eager_ppu_presample()` — advances the machine to `clock.now() + pending`
  (the fetch M-cycle's cc+4) at `dispatch_pending_impl`, so an imminent STAT
  rise inside the fetch is GENUINELY folded (not a peek); the `stat_vis_from_t`
  deadline mask bypassed under the flag. Gated behind `SLOPGB_EAGER`.
- KEY property confirmed: the eager pre-advance changes ONLY the dispatch-check
  view. A non-dispatching instruction is byte-identical (its operand reads still
  sample at the same cc+0 — the wrapper's no-rewind absorbs the pre-advance).
  So the read frame is provably untouched; only the dispatch check moves +4.

### The measurement (running-dispatch eager only, `SLOPGB_EAGER=1`)

- gbmicro flag-on **445 → 416** (+24 fixed / **−53 dropped**).
- mooneye flag-on **91 → 88**: `acceptance/ppu` 10 fails, `acceptance/serial` 2,
  `acceptance` root 8 (`intr_2`/`di_timing`/`hblank`-family — the counter-pinned
  dispatch, B=42).
- **+24 fixed** = all 22 `hblank_int_scx*_{if_b,nops_a,nops_b}` + `line_144_oam_int_b/d`.
- **−53 dropped** = `int_hblank_incs/nops_scx0-7`, `int_oam/lyc/timer/vblank{1,2}_
  incs/nops`, `hblank_int_scx0-6` (bare INC-A count), `hblank_int_l0/l1/l2`,
  `hblank_int_di_timing_b`, `lcdon_to_{lyc1-3,oam}_int`, `*_int_inc_sled`,
  `*_int_nops_b`. **All 53 verified SameBoy-PASS** (patched `sameboy_tester`
  FF82 dump, `--dmg --length 2`: 53/53 `ff82=01`) AND production-pass AND
  deferred-reclock-pass. So the split regresses 53 rows correct on every oracle
  to recover 24 tight-dispatch rows.

### The root — why cc+4 dispatch ∧ cc+0 reads is INCOHERENT (measured, not argued)

The dropped rows are dispatch-**COUNT** tests: the ISR reads a counter/DIV/STAT
in a loop and the verdict is the dispatch instant measured RELATIVE to those
reads. The deferred reclock keeps dispatch AND ISR reads both on the cc+0 frame
(dispatch "one M-cycle late", reads "4 dots early" — internally coherent → the
counts land) and passes them. The eager split moves the dispatch to cc+4
(production's frame) but leaves the ISR reads at cc+0 → the dispatch-to-read
offset shifts 4 dots → the count is off by ~1 → drop. There is NO PPU-observable
discriminator between a PRESENCE row (`if_b`, wants the fold) and a COUNT row
(`int_hblank_incs`, wants no fold) using the same mode-0 rise `R` — exactly §3's
"no bus-observable discriminator". Production passes both ONLY because its READS
are also cc+4 (coherent). To make the counts coherent under the eager split the
ISR reads must ALSO move to cc+4 — i.e. production, which drops the CGB
leading-edge read-frame rows the whole reclock exists to fix. No middle ground.

### The target set was mis-scoped — `if_b`/`nops` are NOT achievable, and are SameBoy-FAILS

Patched-`sameboy_tester` FF82 (DMG, length 2, stable at length ≥2):

| rows | SameBoy-emu | production/HW | deferred reclock | class |
|---|---|---|---|---|
| `hblank_int_scx*_if_b` (8) | **FAIL** (ff82=ff, main path e0) | PASS | FAIL | NOT a target |
| `hblank_int_scx*_nops_a/b` (14) | **FAIL** | PASS | FAIL | NOT a target |
| `int_hblank_incs/nops`, `int_oam/lyc/timer/vblank`, bare `hblank_int_scxN`, l0-l2 (53) | PASS | PASS | PASS | eager DROPS these |
| shipped `if_c`/`if_d`/`poweron_*` | **FAIL** (ff82=ff) | PASS | PASS (shipped #11bk/#11bl) | reclock matches **HW**, not SameBoy-emu |

Two corrections to the §1 map:
1. The gbmicro ground truth is **HARDWARE** (the ROM's FF82), NOT SameBoy-emu —
   SameBoy-emu FAILS the shipped `if_c`/`if_d`/`poweron` that slopgb-tier2 makes
   PASS. #11bk/#11bl match hardware via read-VALUE laws (no dispatch move), which
   is achievable; `if_b` needs the dispatch to MOVE, which is not.
2. The §1 "30 pure dispatch-frame" set is 22 `if_b`/`nops` (unachievable — the
   dispatch cannot move off the leading-edge frame without dropping the 53
   coherent counts + hanging `intr_2`) + only ~3 gbmicro genuinely-open
   (`hblank_int_scx7` count, `hblank_scx3_if_a`/`int_a`) + wilbertpol/age. The
   `if_b`/`nops` **BASELINE at C3** — the flip installs the leading-edge frame,
   which fails them exactly as SameBoy-emu does. They are not convergence work.

### Verdict / refined next lever

The eager-PPU/deferred-CPU split is REFUTED as a lever for the DMG dispatch
rows: the dispatch dot is welded to the read frame by the count tests, so ANY
dispatch-frame move (fold #11br, C2ADV #11ai, or the genuine eager advance
#11bs) drops the coherent counts. The `machine_t`/`advance_machine_to`
never-rewind substrate is proven and byte-identical — bank it — but it has no
consumer that converges. Do NOT re-attempt a DMG dispatch-frame retime. The
real flip-blockers are the **CGB two-bin** render/read-frame/wake/S6 residual
(the ~291 rows), which do NOT touch the DMG dispatch dot; that is where the
SameBoy-pass blockers actually live. Reproduction: patched tester (add
`fprintf(stderr,"FF82RESULT %s ff82=%02x ...", filename, GB_read_memory(&gb,
0xFF82), ...)` before `GB_free` in `Tester/main.c`, `make tester`); the eager
build sits behind `SLOPGB_EAGER` (reverted, but the shape is in this session's
transcript).
