# The EAGER DS half-dot READ reconstruction — PROVABLY INERT; the mid-dot lives in the EXIT, not the read (2026-07-08, #11ce)

Task (HALFDOT Part A, the "most tractable piece"): on the eager DS read path,
reconstruct the true DS sub-dot half-dot from `sb_dsa8` (the SameBoy
`double_speed_alignment` shadow, maintained under eager via `engine.rs:206`) —
plus the read M-cycle's CPU-T parity — and add it to [`Ppu::read_pos_hd`] under
`eager_value && ds`, so the DS mid-dot `_ds_1`/`_ds_2` blocker pairs separate.
Sweep the reconstruction term empirically; keep the clean separator, or (if
inert/shuffle) document that the DS mid-dot needs the full per-T PPU advance.

**Result: the read-position reconstruction is INERT — provably, not just
measured. EV CGB two-bin UNCHANGED (421 → 421); DS blockers cleared = 0. Every
`sb_dsa8`/paid-debt variant is BYTE-IDENTICAL to the control fail set. Tree
reverted byte-identical to `3c02495`; this map is the whole deliverable.** The
DS mid-dot the `_1`/`_2` pairs need is NOT in the read position — it is in the
mode-3 EXIT (`flip_dot`/`win_predraw_abort_dot`), which must land on the ODD
half-dot. Reconstructing only the read's `dhalf` cannot separate the pairs
against the EVEN-grid exit. The next lever is the coupled per-T (`tick_half`)
PPU **render** advance on the eager read/write paths (dispatch held cc+4) — the
HALFDOT Part A core, not a read-position term.

## The sweep (all `eager_value && ds`-scoped; `SLOPGB_EVDSHALF=<v>` knob)

Added a DS-only reconstructed half-dot term to `read_pos_hd` and a paid-debt
parity plumb (`before = clock.now()` around the eager `Bus::read`/`read_inc`
`clock.read()`), then swept variants against the 3422-row cgb_rowlist
(`SLOPGB_PROBE_EV=1`). Control = knob unset (byte-identical to `3c02495`).

| variant | term added to `read_pos_hd` (DS only) | EV CGB fail | Δ rows vs control |
|---|---|---:|---:|
| control | 0 | **421** | — |
| `dsa0` | `sb_dsa8 & 1` | 421 | **0 (identical)** |
| `dsa1` | `(sb_dsa8 >> 1) & 1` | 421 | **0 (identical)** |
| `dsa2` | `(sb_dsa8 >> 2) & 1` | 421 | **0 (identical)** |
| `one` | `1` (uniform +1 hd, every DS read) | 421 | **0 (identical)** |
| `paid` | `paid_debt & 1` (odd conflict class of prev access) | 421 | **0 (identical)** |
| `big` | `+4000` (control — must move) | 530 | 111 |
| `bign` | `−4000` (control — must move) | 472 | 61 |

The `big`/`bign` controls prove the term REACHES `read_pos_hd` for DS reads (a
large EVEN shift moves ~100 rows). Yet every physically-motivated ±1-scale
variant — including the naive uniform `+1` — is **byte-identically inert**. Not
a shuffle (net 0): the fail SET is unchanged (md5-identical) for all five.

## Why it is inert — the even-grid parity proof

Under the eager whole-dot clock, at an FF41 read:

- **`read_pos_hd` is EVEN.** `base = 2*dot + dhalf`, and `dhalf == 0` always
  (the eager `tick_machine` ticks the PPU whole-dot; #11bx proved `dhalf` never
  becomes 1 — the read re-syncs to an even 4·M grid). The read-debt is `+8` (SS)
  / `+4` (DS), both even. So `read_pos_hd ∈ 2·ℤ`.
- **The mode-3 exits are EVEN.** Every `vis_exit_hd` arm is a `2*(…)` form
  (arm 1 `2*(259+SCX&7+ds)`, arm 8 DS `2*flip − 2 + 2*(SCX&1)`, …) because
  `flip_dot`/`win_predraw_abort_dot` are stamped at the WHOLE `self.dot`
  (`mode0.rs:52,244`, `window.rs:188`). `isr_read_carry_hd` DS is `−4`/`0`
  (even). So `exit ∈ 2·ℤ`.
- **`sb_dsa8` is itself always EVEN.** It is `0` at enable, `+2` per dot
  (`engine.rs:206`), `+4` per STOP pause (`dsa_pause_correction`) — all even, so
  `sb_dsa8 ∈ {0,2,4,6}` and `sb_dsa8 & 1 ≡ 0`. Its higher bits
  (`>>1`, `>>2`) are just the whole-dot-parity, a deterministic function of
  `dot` (line = 456 dots, `912 mod 8 = 0`, so `sb_dsa8 = 2·dot mod 8`) — no
  NEW sub-dot information beyond `dot`.

With `read ∈ 2ℤ` and `exit ∈ 2ℤ`, adding an ODD `d = 1` to the read can never
change `read < exit`: `read < exit ⇔ read ≤ exit−2 ⇒ read+1 ≤ exit−1 < exit`
(still <), and `read ≥ exit ⇒ read+1 > exit` (still ≥). **An odd half-dot on
the read alone cannot cross an even exit boundary.** Only even shifts
(`big`/`bign`) move rows. QED — the sweep is a confirmation, not a discovery.

## Where the DS mid-dot actually lives (the pair-separation model)

The `_ds_1`/`_ds_2` pairs differ by ONE setup CPU-T = one 8 MHz half-dot. Say
`_1` reads at whole dot `D` (hd `2D`) and `_2` a half-dot later (hd `2D+1`, same
whole dot `D`), against exit `E`:

- **If `E` is EVEN** (the eager whole-dot render): `2D < E ⇔ 2D+1 < E` (no even
  `E` sits at `2D+1`), so `_1` and `_2` get the SAME verdict — they CANNOT
  separate, whatever the read term. ← the current state.
- **If `E` is ODD** (`E = 2D+1`, a mid-dot flip): `_1` (`2D < 2D+1`) → mode 3,
  `_2` (`2D+1 < 2D+1` false) → mode 0. ← the pair separates.

So separation requires the **exit** (the render's `flip_dot`/abort boundary) to
land on the odd half-dot — i.e. the render flip resolving mid-dot. The read's
own `dhalf` is a red herring: for POLLED (`!read_carried`) DS reads it is 0 in
**tier2 too** (the deferred read samples after `advance_machine_t` completes to
`clock.now()`, which conserves to a multiple of the M-cycle → even → `dhalf 0`;
the paid odd conflict-class debt leaves the clock ODD only transiently at the
WRITE's commit, and the following read pays it back to even). The already-ported
ISR sub-M-cycle carry (`read_carried`/`isr_read_carry_hd`, #11cd) is the ONLY
genuinely-odd read position, and it is even (`−4`/`0`) on the half-dot grid.

## The next lever (single, precise)

Port the coupled per-T (`tick_half`) PPU **render** advance to the eager
read/write paths — advance the PPU T-granularly across the read/write's leading
edge (the `advance_machine_t` structure, DS = 1 half-dot/T) so `flip_dot` and
`win_predraw_abort_dot` are stamped at the true HALF-dot, WITHOUT moving the
dispatch (stays cc+4; `intr_2` + DMG safe). Then the DS mode-3 exit lands on the
odd half-dot for the `_2` sibling and `read_pos_hd`'s `dhalf` (already even, no
term needed) separates the pair against the now-odd exit. This is the coupled
Part-A core the #11bx/#11ca/#11cb/census maps park; the read-position
reconstruction is a downstream consumer of the render half-dot, not a source —
which is why every read-only variant here is inert. Do NOT re-attempt a
read-position `sb_dsa8`/paid-debt term (proven inert above) or a swept
`read_pos_hd` constant (an even shift is a whole-dot A/B overfit, #11bx).

## Gate state / flip bar (unchanged — doc-only, tree at `3c02495`)

- No source change committed (all five variants inert → nothing to keep). Tree
  byte-identical to `3c02495`; golden/mooneye OFF+tier2/tier2-291/clippy
  inherit that commit's verified-green state.
- EV CGB two-bin **421** (unchanged). DS blockers cleared **0**. The C3-flip bar
  stays **97 SameBoy-PASS blockers** (census `eager-flip-census-2026-07-08.md`),
  of which the ~34 DS mid-dot legs are now shown to be **unreachable by any
  read-position term** — they gate on the render half-dot (Part A core).

## Reproduction

Re-apply the 2-minute scaffold (probe knob + paid-debt plumb) then:
`CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release
--no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head
-1)`; `SLOPGB_EVDSHALF=<variant> SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt
SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe
--nocapture | grep pass=` (exact test path). `big`/`bign` = ±4000 sanity that
the term reaches `read_pos_hd`.
