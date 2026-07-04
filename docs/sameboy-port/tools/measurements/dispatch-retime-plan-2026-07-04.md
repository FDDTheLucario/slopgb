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
