# eager `m1statwirq_3` — the ISR-context adversarial re-attack: TRULY FLOORED (two want-0 ISR-internal siblings defeat BOTH representable levers) (2026-07-12)

Base: `finish-port-halfdot @ e3febd1` (isolated worktree; no push, no default
flip, tree byte-identical after this session — only this map added).

## Task

Adversarially re-attack the `#11dt` FLOORED verdict on
`gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb` (DMG; OFF/SameBoy `2` PASS,
eager `0` DROP). Primary hypothesis (per the rom-diff-weld method + the
`eager-write-halfdot` §3 want-opposite framing): the decisive `FF41=00` write is
the glitch write inside a just-dispatched STAT ISR, while the want-0
`late_enable`/`m2enable` siblings write FF41 **directly**; a DMG-STAT-glitch
back-date GATED on a **representable CPU-state flag** ("am I in the STAT ISR I
vectored to") would classify the target's write as-of its true dot-0 while
leaving the direct-write siblings at dot-4, separating the weld the prior
uniform `−4` write-debt (`+1/−12` shuffle) could not.

## Verdict: TRULY FLOORED

The premise is **factually false**, and BOTH candidate representable
discriminators — the CPU-context "in STAT ISR" flag AND a genuinely new
lever the prior agents never tried (the STAT rise's intra-M-cycle lateness δ)
— are each defeated by an explicit want-0 ISR-internal sibling that presents at
the identical value. The residual is the accumulated CPU↔PPU phase, resolvable
only by a per-T CPU IF sample (moving the dispatch recognition → the
`intr_2`/`di_timing`/`int_hblank` tripwire). `#11dt` stands; two more levers now
refuted.

## The decisive write (rom-diff-weld step 1b/2, full CPU-state trace)

Instrumented (all reverted; tree byte-identical): a `Bus::write` FF41 probe
dumping `{ly, dot(leading-edge), val, cyc, cycles-since-last-STAT-ack, PC, IME,
ISR-depth, last-vector}`; a `stat_write_trigger_dmg` `fire`/branch probe
(`SLFIRE`, glitch-eval position = post-`tick_machine` dot); a STAT-rise stamp
(`SLRISE`, `intf` STAT 0→1 fold dot) and a STAT-dispatch stamp (`SLACK`); an
ISR-depth counter (`+1` on real `dispatch_interrupt` ack, `−1` on `RETI 0xD9`).

Target `m1statwirq_3`, decisive (verdict-fixing) FF41 write:

| clock | write leading-edge | glitch-eval dot | old | fire | verdict |
|---|---|---:|---|---|---|
| OFF   | ly153 d452 (→wrap) | **ly0 d0** (dot<4 ⇒ hblank, old&HBLANK==0) | 00 | **1** | **2** ✓ |
| EAGER | ly0 d0             | **ly0 d4** (dot≥4 ⇒ mode-2/3, !lyc_high)   | 00 | **0** | **0** ✗ |

The eager write's glitch is evaluated **4 dots late** (d4 vs d0), because the
STAT dispatch that entered its ISR was recognized late and the ISR's fixed-cycle
wait loop preserves that offset across the ly153→ly0 wrap. The write is
ISR-internal: `IME=0, PC=0x1060, ISR-depth=1, last-vector=0x48`.

## Lever 1 — the CPU-context "in STAT ISR" flag: REFUTED

The premise ("want-0 siblings write FF41 directly") is FALSE. Every decisive
want-0 write is **inside the STAT ISR** at the identical CPU context:

| ROM | want | glitch-eval | old | IME | PC | ISR-depth | last-vec |
|---|---|---|---|---|---|---|---|
| `m1statwirq_3`           | **2** | ly0 **d4** | 00 | 0 | 1060 | **1** | **48** |
| `m2enable/late_enable_ly0_2` | **0** | ly0 **d4** | 00 | 0 | 10d7 | **1** | **48** |
| `miscmstatirq/m0statwirq_2`  | **0** | ly6 d4 | 00 | 0 | 1061 | **1** | **48** |
| `miscmstatirq/lycflag_statwirq_4` | **0** | ly6 d4 | 00 | 0 | 1061 | **1** | **48** |
| `m2enable/late_enable_2`     | **0** | ly2 d4 | 00 | 0 | 1066 | **1** | **48** |

**`late_enable_ly0_2` is the exhibit.** A want-0 row, ISR-internal
(`ISR-depth=1`, `last-vec=0x48`, `IME=0`), whose decisive write is at the
**same PPU line AND dot AND old** as the target (ly0, glitch-eval d4, old00) —
opposite want. Any arm gated on "in STAT ISR" (or PC-in-handler, or
`ISR-depth>0`, or `last-vec==0x48`) fires for BOTH → back-dates BOTH into the
hblank branch → BREAKS `late_enable_ly0_2`. The CPU-state flag is TRUE for both
wants; it cannot separate them. (`cycles-since-STAT-ack` also fails to separate:
target 436, `late_enable_ly0_2` 896, `m0statwirq_2` 440, `late_enable_2` 444 — a
continuous ROM-structural value, a uniform-threshold shuffle.)

## Lever 2 — the STAT-rise intra-M-cycle lateness δ (NEW, prior-agents-untried): REFUTED

The genuine physical difference: the target's write is **late** (eager d4 vs OFF
d0) because its ISR-entry STAT dispatch was recognized at the M-cycle boundary
**after** a mid-M-cycle rise; the want-0 rows are **on-time** (eager == OFF).
The recognition lateness δ = (M-cycle-boundary sample dot − rise dot) is a
per-dispatch quantity — measurable as `rise_dot & 3` at the `intf` STAT 0→1 fold,
NOT the CPU dispatch-M-cycle-move the prior agents refuted:

| ROM | want | STAT-rise ly.dot (ISR entry) | δ = rise_dot&3 |
|---|---|---|---:|
| `m1statwirq_3`               | 2 | ly153 **d6** | **2** |
| `m2enable/late_enable_ly0_2` | 0 | ly152 d4 | 0 |
| `m1/m1irq_late_enable_3`     | 0 | ly152 d4 | 0 |
| `m2enable/late_enable_2`     | 0 | ly1 d0 | 0 |
| `m2enable/late_enable_after_lycint_3` | 0 | ly1 d0 | 0 |
| `m2enable/late_enable_after_lycint_disable_3` | 0 | ly1 d0 | 0 |
| `m2enable/late_enable_m1disable_ly0_3` | 0 | ly152 d4 | 0 |
| `m2enable/late_m1disable_ly0_3` | 0 | ly152 d4 | 0 |
| `m2enable/lyc1_late_m2enable_lycdisable_2` | 0 | ly1 d4 | 0 |
| `lycEnable/late_ff41_enable_3` | 0 | ly5 d4 | 0 |
| `miscmstatirq/lycflag_statwirq_4` | 0 | ly5 d4 | 0 |
| `miscmstatirq/m0statwirq_2`  | 0 | ly5 d4 | 0 |
| **`m0enable/late_enable_3`** | **0** | **ly0 d254** | **2** |

δ separates the target (2) from 11 of the 12 broken rows (0) — but
**`m0enable/late_enable_3` (want-0) has δ=2, identical to the target.** Its ISR
was entered on a mid-M-cycle rise (ly0 d254, `254&3=2`), yet its decisive write
is **on-time** — eager AND OFF both glitch-eval at ly2 **d4**, `fire=0` (correct,
want-0). A δ-gated back-date fires it (d4→d2, dot<4 ⇒ hblank) → BREAKS it.

`late_enable_3` is on-time despite δ=2 because its intervening ISR code re-syncs
to the PPU (absorbing the offset) where the target's fixed-cycle wait preserves
it — a distinction **not observable at the write**. δ present ≠ write late; the
lever shuffles.

## The two exhibits pin every representable write-side field

| field @ decisive write | target (want 2) | `late_enable_ly0_2` (want 0) | `late_enable_3` (want 0) |
|---|---|---|---|
| glitch-eval line | 0 | **0** (=) | 2 |
| glitch-eval dot | 4 | **4** (=) | **4** (=) |
| old | 00 | **00** (=) | **00** (=) |
| m0_src | 0 | **0** (=) | **0** (=) |
| IME / ISR-depth / last-vec | 0 / 1 / 48 | **=** | **=** |
| δ (rise&3) | 2 | 0 | **2** (=) |

No single representable, hardware-principled field differs from BOTH
counterexamples. `late_enable_ly0_2` matches the target on `{line, dot, old,
ISR-context}`; `late_enable_3` matches it on `{dot, old, ISR-context, δ}`.
(The only fields that differ across both are `PC` and the written `data` — PC is
ROM-layout special-casing, forbidden; the DMG glitch is documented
value-INDEPENDENT and `m0statwirq_2` uses `data=00` too, so `data` is not a
discriminator.)

## Root cause + why it is genuinely unrepresentable

The eager clock recognizes an interrupt only at the CPU M-cycle boundary. When a
STAT source rises mid-M-cycle, SameBoy (T-exact) dispatches at the rise T; the
eager clock waits to the boundary, running the whole ISR 2–4 dots late. Whether
that offset reaches the decisive glitch write depends on the intervening ISR
code (a fixed-cycle wait **preserves** it → the write is late → the glitch flips;
an LY-poll **re-syncs** it → on-time). "Was I dispatched late" is a per-dispatch
latch (δ), but "did the offset survive to this write" is the accumulated CPU↔PPU
phase — only recoverable by catching the mid-M-cycle rise at its true T, i.e. a
per-T CPU IF sample. Moving the dispatch recognition dot is the refuted
`intr_2`/`di_timing`/`int_hblank` tripwire (`#11br`/`#11dq`/`#11dt`).

## Gates

- `git diff e3febd1 -- crates/` **empty** — all probes reverted; defaults NOT
  flipped; no push. `golden_fingerprint` + mooneye 93×3 unchanged **by
  construction** (tree byte-identical). Base reproduced: `m1statwirq_3` OFF `2` /
  EAGER `0`.

## Reproduction

```sh
git checkout finish-port-halfdot   # @ e3febd1 (isolated worktree)
export CARGO_TARGET_DIR=target/adv
cargo build -p slopgb-core --example run_gambatte --release --features port_probe
BIN=target/adv/release/examples/run_gambatte
R=test-roms/game-boy-test-roms-v7.0/gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb
$BIN $R dmg                    # OFF  -> 2 (pass)
SLOPGB_EAGER=1 $BIN $R dmg     # EAGER -> 0 (fail)
# The ISR-depth / rise-δ / fire A/B tables were taken with temporary probes
# (Part-C convention, all reverted): DBG_STAT_DISP + FF41-write dump in bus.rs;
# DBG_ISR_DEPTH/DBG_LAST_VEC/DBG_PC/DBG_IME (+RETI/dispatch hooks) in cpu/execute.rs
# (mod execute made pub(crate)); SLFIRE in ppu/regs.rs; SLRISE in tick.rs;
# SLACK in speed.rs::ack_impl. Exhibits:
#   want-0 late_enable_ly0_2 = ISR-internal, ly0 d4 old00 (== target) → defeats CPU-context flag
#   want-0 late_enable_3     = ISR-internal, δ=2 (== target), write on-time ly2 d4 → defeats δ-lever
```
