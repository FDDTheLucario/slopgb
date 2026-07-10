# EAGER halt-wake port — REFUTED: the tier2 T-deadline wake mask is structurally incompatible with the eager clock; the halt `m0stat` rows are a READ-FRAME (early-HALT + collapsed read-position) miss, not a wake-clock miss (2026-07-09, #11cn)

Task (the named next lever, #11cm): port the tier2 halt-wake clock
(`stat_vis_from_t` T-deadline + `mask_hidden_m0_stat` + the mid-cycle wake grid
+ the re-fetch carry) so it fires under `eager_value`, recovering the ~5 CGB +
~6 DMG `halt/*_m0stat`/`halt/*_dec` flip-bar rows. Build-measure, not argument.
A crisp refutation is a valid result.

## Answer — NO. The tier2 wake mask does not port; every debt is net-negative; the rows are read-frame-blocked

- **Nothing shipped. Tree byte-identical @ `9e66ae8`.** EV CGB **361** / EV DMG
  **92** / tier2 CGB **291** (env unset). golden PASS; mooneye 92 flag-off.
- **TRUE flip bar unchanged: 49 CGB BUG + 46 DMG BUG** (`OFF-pass ∩ EV-fail ∩
  SameBoy-pass`, classified). The halt subset is 5 CGB + 6 DMG of these, all BUG.
- The faithful mask port **regresses**: EV CGB 361→371, DMG 92→102 (−20 sum) at
  the true deadline (debt 0); a uniform debt only shuffles the read-position A/B
  (best net = −20, all others worse; +4 → 372/128, +6 → 400/160).

## The flip-bar halt rows (the exact targets, re-derived)

`OFF-pass ∩ EV-fail`, both models, classified BUG (SameBoy `--cgb`/`--dmg
--length 4` == want) — the recoverable set:

| CGB (5) | DMG (6) |
|---|---|
| `late_m0int_halt_m0stat_scx2_3a` (want 0) | `late_m0int_halt_m0stat_scx2_3a` (0) |
| `late_m0int_halt_m0stat_scx3_3a` (want 0) | `late_m0int_halt_m0stat_scx3_3a` (0) |
| `late_m0irq_halt_dec_scx2_2` (want 6) | `late_m0int_halt_m0stat_scx3_3b` (0) |
| `late_m0irq_halt_dec_scx3_2` (want 6) | `late_m0irq_halt_dec_scx2_2` (6) |
| `late_m0irq_halt_m0stat_scx3_3b` (want 2) | `late_m0irq_halt_dec_scx3_2` (6) |
| | `late_m0irq_halt_m0stat_scx3_3b` (2) |

All are `late_*` legs. tier2 passes all 64 `halt/*m0stat` rows; production (OFF)
already passes these 11 and fails the *floored* siblings (`_1b`/`_2b`/etc,
OFF-fail — do not "recover" them, they are not regressions).

## The dual-trace (single-ROM, `late_m0int_halt_m0stat_scx3_3a`, CGB, EV vs OFF vs tier2)

Tracer: `flagon_probe` binary (`port_probe` on) + `SLOPGB_S5DBG`/`SLOPGB_ISRTRACE`,
one-row rowlist, with `clk`/`intf`/`srm0`/`halted` added to the m0-rise + a new
per-sample `hsample` + `ack` + eager-read (`SLE`) probe.

The measurement halt+wake fires **once** per run. The disagreeing dot:

| config | HALT lands (first idle sample) | wakes on | measurement FF41 read | got |
|---|---|---|---|---|
| **OFF** (pass) | ly=1 dot **336** (`wake[plain]`) | ly=**2** dot260 STAT | ly=2 dot **452** = mode 0 | 0 ✓ |
| **tier2** (pass) | ly=1 dot **336** | ly=**2** dot260 | ly=3 dot 0 = mode 0 | 0 ✓ |
| **EV** (fail) | ly=1 dot **260** (`wake[first]`, immediate) | ly=**1** dot260 STAT | ly=2 dot **68** = mode 2 | 2 ✗ |

**The eager HALT lands ~80 dots (≈1 line worth of stream skew) EARLIER than
OFF/tier2.** Under OFF/tier2 the CPU is still running its alignment loop when the
ly=1 mode-0 STAT rises (dot 257): it **dispatches that STAT inline** (`SLACK
bit=1` at dot 272, no wake), the ISR returns, and the CPU *then* HALTs (dot 336),
waiting for ly=2's rise. Under EV the CPU has already reached HALT before dot 257,
so its **first idle sample catches ly=1's STAT via the halt-wake path**
(`wake[first]` dot 260) — one line early. Same STAT, same dot; the difference is
whether the CPU was *running* (dispatch) or *just-halted* (wake) at the rise.

The eager early-HALT is driven by the **eager read frame**, not the wake: the
setup poll reads FF41 through the cc+0 leading-edge peek (`leading_edge_sample`),
4 T ahead of production's cc+4 read (`SLE ff41 v=a2 ly=1 dot=40` EV vs `dot=44`
OFF), and the accumulated stream skew tips the halt-vs-interrupt race by one M.
The `_a`/`_b` (`_1`/`_2`) legs are **read-POSITION** variants (the `_b` handler
reads FF41 one M later, straddling the mode-0→2 boundary); the eager clock
**collapses them to one value**, so no single wake retime satisfies both
`_3a` (want 0) and `_3b` (want 2).

## Why the tier2 wake mask cannot port (the structural refutation)

`mask_hidden_m0_stat` hides the m0-origin STAT from the halt sampler while
`clock.now() < stat_vis_from_t`, with `stat_vis_from_t = machine_now + gl` set at
the rise. It works under **tier2** because the DEFERRED clock samples reads at
cc+0 → `clock.now()` **lags** the machine, so a just-halted sample legitimately
has `clock.now() < machine_now` → hidden.

Under the **eager** clock the CPU clock and the PPU advance in lockstep
(`tick_machine`), so:

1. **`machine_now` is 0 under eager** (only `advance_machine_t`, the deferred
   per-T advance, maintains it). The only available base is `clock.now()` at the
   rise. Measured: eager `clock.now()@rise` (5108) == tier2 `machine_now@rise`
   (5108) — the base is right.
2. **But eager `clock.now()` is non-monotonic** across the read/wake sub-points
   (it carries the cc+0/cc+4 read-park skew). So `stat_vis_from_t` captured at
   the rise can *exceed* `clock.now()` at a *later* halt sample → the mask
   **mis-fires** even at the faithful deadline (debt 0). Measured: EV CGB
   361→**371**, DMG 92→**102** (−20 sum), with the glitch-line `gl` term forced
   off — pure mis-fire.
3. **The eager first-idle (halt-entry) sample is at cc+4** (dispatch position),
   *after* `tick_machine` folded the rise into `intf` — so it structurally SEES
   the just-risen STAT. The deferred sample is at cc+0, *before* the fold — it
   misses it. Hiding it needs a +4 debt on the *entry* sample only; but that just
   defers the wake +4 dots (still same line), never the *full line* `_3a` needs.

**A full-line delay is what `_3a` requires** (dispatch ly=1's STAT inline, or wait
for ly=2). The mask can only hide a bit for a few clks at the boundary — it
cannot make the CPU un-halt and dispatch inline. That is an early-HALT
(read-frame) fix, out of the wake clock's reach.

## What each debt does (the A/B shuffle, measured)

Widening `mask_hidden_m0_stat`'s gate to `(tier2 || eager_value)` + eager
`stat_vis_from_t = clock.now() + debt`, swept (`SLOPGB_HWDEBT`):

| debt | EV CGB | EV DMG | sum (base 453) |
|---:|---:|---:|---:|
| baseline (mask off) | 361 | 92 | **453** |
| −8 / −4 / −2 | 371 | 102 | 473 |
| 0 | 371 | 102 | 473 |
| +2 | 372 | 128 | 500 |
| +4 | 372 | 128 | 500 |
| +6 | 400 | 160 | 560 |

`+4` fixes the read-position `_b`/`_2` legs (want 2) by shifting the post-wake
read +4, and **breaks** the `_a`/`_1` legs (want 0) plus OFF-pass `m2int_m0irq`,
`m0enable`, `dma/hdma_vs_m0int` rows by the same shift — a pure boundary shuffle,
never net-positive. The re-fetch `carry_read(4)` (plain path, `cgb_any`/
`dmg_first`) is inert/net-negative under eager. The DMG **mid-cycle wake grid**
(`halt_wake_mid_impl`, the `Na`/`Nb` SameBoy-exact 4k+2 sampler) is **unportable
in principle**: it is built on `advance_machine_t`/`advance_pending`/`forgive` —
the deferred half-M-cycle machine — which the eager clock (driven whole-dot by
`tick_machine`) does not have.

## Verdict — this is the same weld as #11bw / #11cl §5

The halt `m0stat`/`dec` rows are **read-frame-blocked**, matching #11bw ("EV
half-dot-blocked read frame") and #11cl §5 ("route eager reads through the
deferred machine breaks the +16 ISR reads — the atomic weld"). The eager clock:
(a) lands the HALT ~1 M early because the cc+0 FF41 poll peek tips the
halt-vs-interrupt race, and (b) collapses the read-position `_a`/`_b` legs. Both
need the coherent eager **half-dot read frame** (HALFDOT Part B on `Bus::read`) —
the same un-hosted lever the dispatch-retime and FF41-ISR work already point to —
NOT a wake-clock port. The tier2 wake mask has no eager analogue: its correctness
depends on the deferred cc+0 read-lag that the eager lockstep clock does not
produce.

## Gate state (all HARD invariants green; nothing shipped)

golden_fingerprint PASS (73.9 s); tier2 CGB two-bin **291**; EV CGB **361** / EV
DMG **92** (byte-identical, env unset); `cargo test --test mooneye` **92 passed**
flag-off; clippy clean (no code change); no `.rs` touched.

## Recomputed TRUE flip bar (unchanged)

`OFF-pass ∩ EV-fail`: CGB 91 / DMG 55. Classified SameBoy-pass (BUG, must-fix):
**CGB 49 BUG + 42 FLOOR; DMG 46 BUG + 9 FLOOR** — identical to the #11cm / task
baseline. The 5 CGB + 6 DMG halt rows sit inside the 49/46 BUG bar and stay there
(refuted this session). Next lever: the coherent eager half-dot read frame
(Part B), which subsumes the halt-race and the read-position collapse; do NOT
re-attempt the tier2 wake-mask port on the eager clock.

## Reproduction

```
git checkout eager-halt-wake      # this session's tip (byte-identical @ 9e66ae8)
CARGO_TARGET_DIR=target/agH2 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agH2/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# baselines (env unset = byte-identical):
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # fail=361
SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # fail=92
# flip-bar + classify:
#   OFF two-bin (SLOPGB_PROBE_OFF=1) minus EV two-bin → OFF-pass ∩ EV-fail
#   python3 docs/sameboy-port/tools/classify_cgb_regr.py <cgb_flipbar.txt>   # BUG=49 FLOOR=42
#   python3 docs/sameboy-port/tools/classify_dmg.py <dmg_flipbar.txt> <pfx>  # BUG=46 FLOOR=9
```

The mask/debt probes of this session were reverted (net-negative, structural);
the trace-only probes (`hsample`/`SLE`/rise-`clk`) that found the disagreeing dot
were reverted too — re-add them locally to re-run the dual-trace.
