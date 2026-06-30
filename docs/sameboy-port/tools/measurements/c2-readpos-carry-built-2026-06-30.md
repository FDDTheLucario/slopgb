# C2 #11aq — the per-ISR read-POSITION CARRY BUILT (decoupled from the IF dispatch); the #11ap "co-temporal/unseparable" verdict is REVERSED, the (a)+(b) co-land PROVEN at family level; ESCAPE on the global atomic consistency

2026-06-30. Executed the goal's single sharpest lever: **build the per-ISR
deferred FF41-read POSITION decoupled from the IF dispatch** — carry the
IRQ-source's read-frame offset into the ISR-handler reads *after* the IF-ack
latch, so the read moves to SameBoy's absolute cfl while the counter-pinned
dispatch dot and IF delivery stay put. Result = **ESCAPE**, but with the lever
**built, validated, and the model substantially deepened**: the read CAN be
separated from the dispatch without DSM2DELAY's IF-delivery drag, the per-source
read-frame offsets are MEASURED, and the `(a)`render-length + `(b)`read-position
co-land is PROVEN to converge the m0int **and** m2int families. The single
residual barrier is now precisely characterized: the exit law and the read frame
must be **globally** consistent (carry EVERY deferred read, one SBex law), so a
flag-gated subset is an A/B swap. Defaults NOT flipped; `pixel-pipe-reclock` core
byte-identical; the scaffold + tracers on `phase-b-s7`.

## The lever (the mechanism — distinct from DSM2DELAY)

`Interconnect::dispatch_retime` (`interconnect.rs`): after
`dispatch_vector_retime` flushes the clock to the IF-ack latch (the
counter-pinned dispatch dot) and reparks `pending=2`, **add `carry` T-cycles of
read debt** (`CycleClock::carry_read`). That debt is paid by the *next* bus op
(the vector fetch) **before** it samples, so the vector fetch + every subsequent
handler read shift `carry` T later — WITHOUT moving the IF ack (already
committed) or the dispatch dot. The decoupling DSM2DELAY's *dispatch* delay could
not give: DSM2DELAY moved the whole timeline (IF delivery included → forbidden
m2irq/m2enable drops); the carry moves ONLY the post-ack reads.

Keyed on the IRQ SOURCE (`Ppu::stat_rise_oam` / `stat_rise_m0`, sticky levels set
on each STAT 0→1 edge in `stat_update_halt_masks`): mode-2 OAM ISR vs mode-0
HBlank ISR. DS-only; the dispatched bit is STAT iff it is the lowest pending bit
(VBlank out-prioritizes STAT). Env-gated `SLOPGB_M2CARRY` (+ `_T` magnitude /
`SLOPGB_M0CARRY_T`); byte-identical OFF.

## The decisive measurement — the reads were NEVER collapsed; the offset is +4 (mode-2) / +2 (mode-0)

`m2int_m3stat_ds` (scx0), slopgb base (no carry) vs SameBoy (true half-dot pos =
`cfl*2+dc`, `/2` to dots):

```
            slopgb base read   SameBoy read   SameBoy bare exit (SBMODE)
_ds_1 want3  ly135 dot252      cfl256 (256)   cfl257 dc2 = 258 dots
_ds_2 want0  ly135 dot254      cfl258 (259/-2)
```

- slopgb reads `_1`/`_2` **2 dots apart** (252, 254) — they are NOT collapsed
  (the #11ap "co-temporal, both dot254" was the cc+4-vs-carried confound). The
  handler NOP-delta IS in slopgb's clock.
- slopgb's whole frame is a **uniform +4 dots EARLIER** than SameBoy (252→256,
  254→258). `carry_t=8` (= +4 dots DS) lands them at SameBoy's EXACT cfl
  (verified: `_1`→dot256, `_2`→dot258).
- The ONLY error is the EXIT position relative to the reads: slopgb's native
  `vis_mode` exit `255+SCX&7` sits ABOVE both reads (both mode 3); SameBoy's exit
  `257+SCX&7(+ds)` = 258 sits BETWEEN them (`_1` mode3, `_2` mode0).
- **mode-0 (m0int) reads are +2 dots early, not +4** (`m0int_m3stat_ds_1` slopgb
  dot254 ↔ SameBoy cfl256). The read-frame offset is **per-IRQ-source**.

## The (a)+(b) co-land — PROVEN at family level

With the per-source carry (mode-2 `+4`/`carry_t 8`, mode-0 `+2`/`carry_t 4`)
landing every STAT-ISR read at SameBoy's cfl, a SINGLE SBex exit-hold
(`SLOPGB_M2HOLD`: hold mode 3 to `257+SCX&7+ds`, the read_offset-0 BARELAW) gives
SameBoy's verdict. The **m0int AND m2int m3stat DS families both converge
21/22** (only `late_scx4_ds_2`, a separate late-write mechanism, remains). The
recipe is the goal's thesis, realized: read → SameBoy's per-ISR position +
SameBoy's exit.

## Why it ESCAPES — the global consistency barrier (the full two-bin)

| variant (flag-on, full-CGB two-bin vs base) | +fixed (SB-pass) | −dropped (SB-pass) | mooneye |
|---|---|---|---|
| carry mode-2 `+2 dot` (`T=4`) | 28 | 22 | 91/91 |
| carry mode-2 `+1 dot` (`T=2`) | 22 | **7** | 91/91 |
| dual-carry `+4/+2` + `M2HOLD` (full co-land) | 22 | 50 | 91/91 |

- **mooneye 91/91 in EVERY variant** — the dispatch dot is NOT moved (the
  counter-pinned `intr_2_*`/`int_hblank`/`di_timing` tests intact). The carry is
  read-position-only, as designed. This alone REVERSES #11ap/#11ai: the read DOES
  move without the dispatch.
- The `M2HOLD` blanket exit law (hold ALL bare-DS reads to SBex) drops **50**
  SameBoy-passing rows because only the STAT-ISR reads are CARRIED; the
  non-carried bare-DS reads (polled, other ISRs) stay at slopgb's lower frame and
  the SBex hold mis-frames them. The read frame and the exit law must be
  **globally** consistent.
- carry-only (no hold) at `T=2` is the least-bad (`+22/−7`): the 7 drops are
  mostly scx-ODD (`scx5`/`scx1`) — the carry's whole-dot grid vs the half-dot
  exit parity (the HDEXIT `scx&1` term) — + 2 window-wx. Still an A/B swap: the
  `_ds_1` siblings whose reads land closer to the (config-dependent) boundary are
  pushed over.

The over-shoots are NOT the carry being wrong — they are the carry magnitude
being a UNIFORM whole-M-cycle where the per-config read-frame offset + exit are
sub-M-cycle and config-dependent. The clean fix carries EVERY read by its exact
per-ISR offset to SameBoy's cfl AND applies ONE SBex exit law — the atomic
reclock, now reduced to this concrete, measured recipe.

## The model, sharpened (what #11aq adds over #11ap)

| | #11ap (prior) | #11aq (this session) |
|---|---|---|
| separability | "co-temporal, unseparable by any read clock" | the post-ack CARRY separates the reads (the plain m2int pair, both families) |
| IF-delivery | "only DSM2DELAY separates, drags IF" | the carry moves the read WITHOUT the IF ack (mooneye 91/91, no m2irq/m2enable IF drag) |
| read-frame offset | unmeasured | **MEASURED: DS mode-2 +4 dots, DS mode-0 +2 dots** |
| exit | half-dot parity solved (HDEXIT) | + the carried-frame SBex hold (read_offset 0) converges both families |
| barrier | "the read POSITION" (vague) | the GLOBAL read↔exit consistency: carry EVERY read + one SBex law (atomic) |

## The single sharpest lever (the ESCAPE deliverable — refined)

**The full per-ISR deferred-read POSITION reclock: carry EVERY deferred read by
its IRQ-source read-frame offset to SameBoy's absolute cfl, then apply ONE SBex
(`257+SCX&7`) exit law.** The CARRY mechanism is BUILT and validated (decoupled
from IF, mooneye-neutral); the per-source offsets are measured (mode-2 +4 / mode-0
+2 DS; SS + the LYC/wake/engine-if sources are the remaining per-source
measurements); the exit law is the proven render-length port (#11am `vis_mode_read`
template). The barrier is purely that all of it must land ATOMICALLY — a
flag-gated subset breaks the global consistency. This is the same atomic reclock
the port has named, now with a concrete, family-verified recipe instead of an
open question.

## Scaffold (env-gated, byte-identical OFF; `phase-b-s7`)

`cycle_clock.rs::carry_read` · `interconnect.rs::dispatch_retime` per-source
carry · `ppu/mod.rs` `m2carry_on`/`m2carry_t`/`m0carry_t`/`m2hold_on` gates +
`stat_rise_oam`/`stat_rise_m0` fields · `ppu/stat_irq.rs` getters + the M2HOLD
SBex branch in `vis_mode_read` · `ppu/stat_irq/reclock.rs` source tagging. Tracers
(`SLOPGB_S5DBG`): `SLOPGB ff41 … clk=` (carried read dot), `SLOPGB vec` (the
dispatch/IF-ack latch dot), SameBoy `SBDISP`/`SBWAKE` (re-added to
`build_sameboy_tracers.sh`).

## Gate (END CLEAN — no production change)

mooneye flag-on 91/91 (all variants); gbtr OFF byte-identical (the scaffold is
inert OFF — every gate defaults off, the fields set only on the LE/tier2 path);
clippy clean; `pixel-pipe-reclock` core byte-identical.
