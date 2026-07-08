# The EAGER coupled render-length ∧ read-exit slice — SS convergence STARTED (2026-07-07, #11by)

Task: test THE untested atomic pair of the eager-value re-host — the COUPLED
render-length + read-frame EXIT laws under `eager_value`, entry kept at 80,
exit at the tier2 frame. Prior attempts each tested ONE half (read-law alone
+23 / #11bw; read-position alone inert / #11bx; EV-v0 all-frame-84 → intr_2
hang / #11bv). **Result: coupling render-length ∧ read-exit SEPARATES the SS
window `_1`/`_2` pairs WHOLE-DOT on the eager clock — EV CGB two-bin 578 → 553
(clean +25 / −0), all hard gates hold. Convergence STARTED. SHIPPED flag-gated,
production byte-identical.**

## What was enabled (all `|| eager_value`, single-speed-scoped)

The read-exit web + the render-length laws it consumes, coupled:

| site | law | scope |
|---|---|---|
| `stat_irq/read_laws.rs:65` | enable `vis_exit_hd` (the whole exit table) | `eager_value && !ds` |
| `render/mode0.rs:256` | `vis_hold_until = 263 + SCX&7` (triggering-window vis-hold) | `eager_value && !ds` |
| `render/mode0.rs:280` | `access_lead = -8` (boundary-coincident OAM/VRAM release) | `eager_value && !ds` (the `!ds` was already there) |
| `render/window.rs:47` | `wx_match_dot`/`wx_match_scx` latch (abort/shadow deadline) | `eager_value` (consumed only by ds-gated arms → inert DS) |
| `render/window.rs:186` | `win_predraw_abort` latch (pre-draw LCDC.5 clear) | `eager_value` (idem) |
| `engine.rs:427` | `wy_trig_sb`/`_raw`/`_xline` latch block (WY-trigger shadow) | `eager_value` (idem) |
| `engine.rs read_pos_hd` | `+8 hd` eager read-debt (cc+0 → cc+4) | `eager_value` (inert DS) |

**Entry stays 80 / glitch 74** — `mode3_entry_dot` + the glitch back-date keep
their `leading_edge && !tier2_reclock` branch (true under EV), so EV keeps the
frame-80 entry `intr_2_mode3` needs. Tier2 sites are `tier2 || eager_value` →
no-op under tier2 (`tier2_reclock` already true) → **tier2 two-bin unchanged
291, mooneye 91/91 flag-on unchanged.**

## The +8hd read-debt is the PRINCIPLED frame (not a swept constant)

The eager `Bus::read` samples FF41 at cc+0 (this M-cycle's leading edge); the
deferred read (`read_deferred`) pays the previous M-cycle's parked 4T and lands
at cc+4. The tier2 `vis_exit_hd` exit constants (`259 + SCX&7` etc.) are
calibrated to that **deferred cc+4 read position**. So the eager read must
advance `+4 dots = +8 hd` to resolve them at the same frame. Hardcoded
`EAGER_READ_DEBT_HD = 8` under `eager_value`; NOT an env-swept overfit — it is
the architectural cc+0→cc+4 debt. Confirmed optimal: SS-scoped sweep
{6→562, 8→**553**, 10→(DS-noise)}; +8 is both the derived and the measured
minimum. It is also intr_2-safe (the frame-80 entry the `+8` exit pairs with).

## Two-bin (branch `finish-port-halfdot`, hardcoded +8, no env)

| bin | fail | vs baseline |
|---|---:|---|
| EV CGB baseline (#11bw) | 578 | — |
| **EV CGB coupled (this)** | **553** | **−25** |
| EV DMG baseline | 172 | — |
| **EV DMG coupled** | **147** | **−25** (bonus; the DMG window arms D1/D6) |
| tier2 CGB (unchanged) | 291 | 0 |

**Per-family CGB recovery (fixed 25 / broke 0 — a CLEAN convergence):**

| family | fixed | note |
|---|---:|---|
| window | 21 | `m2int_wx*_m3stat_2/_3` (length arm 1/D1), `late_wy_*_1` (shadow arm 2), `late_disable_early_scx03_wx*_1` (pre-draw abort D3), `late_wy_1toFF/2toFF_1` (un-trigger D6), `wxA5/wxA6` off-screen |
| vram_m3 / oam_access | 2 / 2 | `postread_scx2/5_2` (the `access_lead=-8` boundary release) |

The window recovery is genuine `_1`/`_2` **pair separation**: e.g.
`m2int_wx03_m3stat_2` (want 0) now reads 0 while its `_1` (want 3) stays 3 —
the mode-3 LENGTH (arm 1 `259+SCX&7`) resolves the read against the eager `+8`
frame. This is exactly the coupling #11bw's read-law-ALONE lacked (that was
578/zero-gain, window near-wash at +8hd — the fixed window arms need the
render-length latches armed AND the vis-hold `m`-gate to fire, which the
coupling supplies).

## Why SINGLE-SPEED only (the DS is the explicit next lever)

Enabling the web for DS too (`eager_value` un-`!ds`) reaches EV CGB **539**
(−39) BUT via a **34-fixed/34-broke DS pair SHUFFLE** — the DS read sits at a
different sub-M-cycle offset (the "+3 not +4" ISR offset) whose alignment lives
in the tier2-gated `lcd_shift_dots`/`sb_dsa8` shadow, un-ported here. A uniform
`+8` DS shift only shuffles the DS `_1`/`_2` pairs (breaks 34 previously-green
DS rows to fix 34 others) — the exact overfit signature #11bx flagged. So the
web is `!ds`-scoped: DS returns native `vis_mode` (byte-identical to the EV
baseline, measured 0 DS breaks). **The DS half-dot alignment is the next
sub-lever; porting it recovers the DS pairs cleanly (est. +14 net beyond 553).**

## Gate state (ALL hold)

- golden_fingerprint (`--release`) PASS — production byte-identical (`eager_value` off).
- mooneye OFF 91/91; mooneye tier2 (`SLOPGB_MOONEYE_RECLOCK=1`) 91/91; tier2 CGB two-bin 291 (unchanged).
- mooneye EAGER (`SLOPGB_MOONEYE_EAGER=1` acceptance_ppu): only `lcdon_timing-GS` ×4 fail (pre-existing exemption); **`intr_2_mode0/mode3/sprites` PASS** (dispatch cc+4 never moves — the `early_lead`/`snap_ok` DISPATCH-moving arms were deliberately NOT enabled under EV; they broke `intr_2_*_sprites`).
- core lib 754/754; clippy `-D warnings` clean (default + `port_probe`); every `.rs` < 1000.

## What did NOT enable (measured, not guesses)

- **`early_lead` (mode0.rs:184) + `snap_ok` (mode0.rs:216):** these move the
  sprite-line DISPATCH (`line_render_done`/`flip_dot`) to a grid calibrated for
  the deferred frame → break `intr_2_mode0_timing_sprites` on the eager clock
  (dispatch must stay cc+4). REVERTED. For a bare-line read arm 8 masks
  `vis_early` anyway, so `early_lead` is unneeded for the read verdict.
- **DS arms / `lcd_shift_dots` / STOP alignment / `blocking.rs`:** out of scope
  (the DS sub-lever + accessibility verdicts gate on `tier2_reclock`, left OFF).

## Reproduction

`CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release
--no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head
-1)`; `SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN
--ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=` (exact
test path). mooneye eager gate: `SLOPGB_MOONEYE_EAGER=1 CARGO_TARGET_DIR=target/evm
cargo test -p slopgb-core --test mooneye acceptance_ppu`.
