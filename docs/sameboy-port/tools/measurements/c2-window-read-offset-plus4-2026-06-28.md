# C2 #11z — the window FF41-read offset is +4 (NOT +3); exit `259+SCX&7` ships +2/−0

2026-06-28. The window-length law's read half, re-derived from the **measured read
offset** after a tooling-frame bug was found and fixed. Shipped clean (+2/−0
full-CGB), byte-identical OFF, defaults NOT flipped.

## Two tooling bugs found first (both cost prior measurements)

1. **`run_gambatte` was mis-framed.** The example did `GameBoy::new(model, rom)`
   then `gb.set_tier2_reclock(true)` *after* boot — which SKIPS the C0 `div += 4`
   that `new_with_reclock` applies at construction (the post-boot toggle is too late;
   handoff "the set-after-boot path skipped the +4 and mis-validated"). So every
   single-ROM window/late_wy trace taken via `run_gambatte` ran in the WRONG DIV
   frame. Concretely: `m2int_wx03_scx2_m3stat_1` traced mode 3 @ dot260 (looks
   passing) under the mis-frame, but the real reclock (`flagon_probe` /
   `boot_with_reclock`) reads mode 0 @ dot260 (fails). FIX: `run_gambatte` now uses
   `GameBoy::new_with_reclock` under `SLOPGB_TIER2=1`. The kernel m2int trace
   (dot252) was INSENSITIVE to the +4 (its sync is interrupt-driven off line start),
   which is why prior kernel numbers held; the window reads are sensitive.

2. **Stale experimental binary as baseline.** The prior session left a built
   `gbtr-…` test binary (19:40) whose source differed from committed HEAD (it
   carried an experimental law: scx2_1 fail / scx5_2 pass). A two-bin against it is
   invalid. LESSON: rebuild BOTH bins from known source; never trust a pre-existing
   `deps/` binary. The true HEAD window-family baseline is **81 fail** (full-CGB
   503), re-established with a fresh build.

## The reliable method

Run the `flagon_probe` binary itself with `SLOPGB_S5DBG=1` — the committed
`read_deferred` FF41 tracer fires from the EXACT probe frame (same construction +
run length as the gbtr tests), so the read dot is ground-truth. Identify the
OCR-relevant read by mode: slopgb's read mode == `got`, SameBoy's == `want`.

## The finding — read offset +4, not +3

The #11y law set the CPU-visible window mode-3 exit to `260 + SCX&7 = SBex(263) −
3`, using the **dispatch** offset (slopgb dot254 ≡ SameBoy cfl257). But this is a
READ comparison, and the deferred FF41 read samples the PPU **+4** dots before
SameBoy reads the same `ldh a,(FF41)`:

| row (want) | slopgb read | SameBoy read | SameBoy SBex | offset |
|---|---|---|---|---|
| `m2int_wx03_scx5_m3stat_2` (0) | ly1 dot **264** mode3 | ly1 cfl **268** mode0 | cfl 268 (=263+5) | **+4** |
| `m2int_wx03_scx2_m3stat_1` (3) | ly1 dot **260** mode3 | ly1 cfl **264** mode3 | cfl 265 (=263+2) | **+4** |
| `m2int_wx03_m3stat_2` scx0 (0) | ly1 dot **260** mode0 | ly1 cfl **265** mode0 | cfl 263 | +5 (robust) |

So the read-law exit is `SBex − read_offset = 263 − 4 + SCX&7 = 259 + SCX&7`. The
scx5 `_2` rows read dot264: at exit 265 (260+5) → 264<265 → mode 3 (WRONG, was the
floor); at exit 264 (259+5) → 264≥264 → mode 0 (RIGHT). The scx0 `_2` rows read
robustly past the exit (SameBoy cfl265 > SBex263) so 259 vs 260 is invisible to
them — the #11y +7 hold.

## Two-bin (full-CGB flag-on, fresh binaries both bins)

- `260+SCX&7` (HEAD) → 503 fail. `259+SCX&7` (#11z) → **501** = **+2/−0**.
- FIXED: `m2int_wx03_scx5_m3stat_2`, `m2int_wx07_scx5_m3stat_2` (both want 0).
- REGRESSED: none.
- 23 tier2 pins green (the window pin now includes the scx5 `_2` leg, pinning 259 vs
  260); mooneye flag-on 91/91; gbtr+mooneye OFF byte-identical (`is_cgb`/`tier2`
  gated). clippy/fmt clean.

## What did NOT work (build-measured negatives, same session)

- **Two-sided absolute law** (`m != 2 → (dot < exit)*3`, EXTEND mode0→3 +
  SHORTEN mode3→0): vs the TRUE HEAD baseline = **net 0, identical row-for-row**.
  The native `vis_mode` already OVER-extends these reads (line_render_done is past
  the read), so there is no under-extend for the extend-half to fix; the residual is
  pure shorten-direction. (My first +3/−3 reading was against the stale 19:40 binary
  — invalid; the corrected baseline shows no-op.)

## The residual (still the atomic reclock)

The remaining 79 window fails are the read-collapse: slopgb's M-cycle-quantized
deferred read lands the `_1`/`_2` variants of a config at nearly the same dot while
SameBoy resolves them apart by the per-config CPU↔PPU phase (the read offset is +4
at the scx2/scx5 boundary but the late_wy/late_disable/wxA6 families carry the #11g
length terms AND a non-uniform offset). No single `vis_mode_read` boundary separates
them. That is the global read-frame reclock (C2 atomic), not a length-law tweak.
#11z is the one clean dot the corrected read offset buys.

## The read offset is INTERRUPT-DRIVEN (+4) vs POLLED (+0) — the interrupt-service frame, NOT an lcd-offset

Followed the collapse to the late_wy family (probe-internal slopgb + SameBoy, correct
frame) and checked for a dispatch on the read line:

| ROM (want) | slopgb read | SameBoy read | SameBoy exit | offset | read kind |
|---|---|---|---|---|---|
| `m2int_wx03_scx5_m3stat_2` (0) | ly1 dot264 | ly1 cfl268 | cfl268 | **+4** | INT (mode-2 dispatch ly1 dot0) |
| `m2int_wx03_m3stat_2` scx0 (0) | ly1 dot260 | ly1 cfl265 | cfl263 | **+5** | INT |
| kernel `m2int_m3stat_1` (3) | ly1 dot252 | ly1 cfl256 | — | **+4** | INT (dispatch ly1 dot0) |
| `late_wy_10to0_ly1_1` (3) | ly1 dot260 | ly1 cfl260 | cfl263 | **+0** | POLLED (NO dispatch ly1-2) |

The concrete COLLAPSE: `late_wy_10to0_ly1_1` (want 3) and `late_wy_1toFF_1` (want 0)
BOTH read slopgb **ly1 dot260**, opposite wanted modes. The discriminator is NOT
window state (both are steady `wy_latch`, `wy2 != ly`) and NOT a per-ROM lcd-offset —
it is **whether the read is INTERRUPT-DRIVEN or POLLED**: the m2int reads follow a
mode-2 STAT dispatch (the FF41 read is in the ISR), the late_wy reads are a direct
poll with no dispatch on the line. The PPU EVENT (the dispatch / IF-raise) ALIGNS in
both emulators (slopgb dot0 ≡ SameBoy cfl0); only the CPU-side ISR read diverges +4.

**So the +4 is the INTERRUPT-SERVICE FRAME: slopgb's post-dispatch ISR reads land 1
M-cycle (4 dots) EARLIER than SameBoy's** (the dispatch→read latency: kernel SameBoy
cfl0→cfl256 = 256 dots, slopgb dot0→dot252 = 252). The deferred-clock dispatch retime
(`dispatch_vector_retime`: `pending -= 2; flush; pending = 2`, `cycle_clock.rs:187`)
re-parks 2 after the vector latch, sampling the post-dispatch reads early; the net
ISR-frame divergence is one M-cycle. The POLLED reads (no dispatch) are already
ALIGNED (+0).

**This relocates the per-read frame-offset model from a global lcd-offset to a bounded
CPU-timing seam: the interrupt-service / post-dispatch read frame.** Implication for
the window family: the m2int reads (+4, the #11y/#11z `260`/`259` law families) and
the late_wy POLLED reads (+0, want exit 263) need DIFFERENT exits ONLY because the
interrupt reads are mis-framed by the ISR +4. Fixing the interrupt-service frame
(post-dispatch reads land at SameBoy's +0, like polled) would let a UNIFORM exit
(263 = +0) serve BOTH families — but it shifts the counter-pinned interrupt tests
(`intr_2_mode0_timing`, `int_hblank_halt`) that currently pass at the +4 frame, so it
is the atomic reclock (the dispatch retime + boundary co-move), NOT a flag-gated
read-only nudge. Still, the lever is now LOCALIZED: `dispatch_interrupt` /
`dispatch_vector_retime` (the ISR read frame), not a global per-ROM lcd phase. The
`vis_mode_read` boundary-law approach is exhausted (it can't see CPU read-kind);
#11y (+7) + #11z (+2) shipped the interrupt-read families whose +4/+5 is uniform.

## Next-target probe (late_disable, build-measured — NOT a read-law slice)

The `late_disable` want0-got3 rows (slopgb over-extends, reads mode 3): probe-internal
reads — `late_disable_scx2_1`/`scx5_1` ly1 dot260 mode3, `late_disable_early_scx03_wx0f_1`
ly1 dot256 mode3 — all want mode 0. They are NOT covered by the #11z law: the disable
sets `win_aborted` (the law's gate excludes it) and, more fundamentally, slopgb's
NATIVE `vis_mode` (= `line_render_done`, the dispatch) STILL reports mode 3 at dot260
for a disabled window — i.e. the window abort does not shorten `line_render_done`.
SameBoy's window mode-3 aborts early on disable (exit ≈ the bare-line position), so its
read at cfl260+ lands mode 0. So this is the #11g **render-coupled** `late_disable`
term (the abort must shorten the visible mode-3), not a read-law exit tweak — a
production-render change (breaks byte-identical OFF) on the C2 render-model path, NOT
a `vis_mode_read` one-liner. Floored for #11z; carried to the render model.
