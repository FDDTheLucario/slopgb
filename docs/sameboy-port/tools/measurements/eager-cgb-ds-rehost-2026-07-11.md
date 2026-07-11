# EAGER CGB double-speed re-host attempt — the 8 candidate DS bar rows are ALL Part-A / pre-refuted; ZERO shipped, tree byte-identical (2026-07-11, #11dk)

Task (#11dk): port up to 8 CGB double-speed bar rows onto the eager-value (EV)
clock — EV-CGB-fail ∩ tier2-CGB-pass ∩ SameBoy-pass DS rows an SS/other sibling
already passes under EV. **Result: 0 rows shipped. All 8 candidates dual-traced
and REFUTED — each is a dispatch/emission-frame (Part A) divergence or a
prior-session pair-shuffle, NOT a portable golden-safe read-frame peek. Tree
byte-identical @ a866b0b (no code, no doc-only-affecting change to source).**

Per the task's own method ("ship ONLY zero-drop recoveries; refute the rest with
a trace; a small/zero honest yield beats a forced net-negative") this is the
honest outcome — every candidate was pre-flagged likely-Part-A or already
refuted, and the traces confirm it.

## Baselines reproduced (exact, at `a866b0b`)

`flagon_probe` two-bin on `scratchpad/{cgb,dmg}_rowlist.txt`:

| frame | CGB | DMG |
|---|---:|---:|
| OFF (`SLOPGB_PROBE_OFF`) | 486 | — |
| EV (`SLOPGB_PROBE_EV`) | **318** | **66** |
| tier2 (`SLOPGB_PROBE_RECLOCK`) | **291** | 116 |

golden_fingerprint: 9020 cases match HEAD (byte-identical).

## Method

Dual-trace one row per family under OFF / EV (`SLOPGB_EAGER=1`) / tier2
(`SLOPGB_TIER2=1`) on `examples/run_gambatte.rs --features port_probe` with
`SLOPGB_S5DBG=1`, temporary probes on the eager FF0F read (`Bus::read`), the
per-dot STAT fold (`fold_ppu_events`), the IF ack (`ack_impl`), the FF40
enable write, and the mode-0 flip (`m0_flip_events`). All probes reverted; the
tree is byte-identical to base (`git diff a866b0b -- crates/` empty).

## Per-row verdict

### 1–2. `enable_display/ly0_m0irq_scx{0,1}_ds_1` (want E0, EV got E2) — REFUTED

The task's "best odds (m0irq read-frame DS)". **The read-frame hypothesis is
REFUTED by the trace: it is a spurious mode-0 STAT EMISSION at line-0 dot 19 on
the glitch line, present ONLY under EV+DS.**

Dual-trace (glitch line, ly=0), first STAT fold into `intf`:

| config | first fold dot | FF0F read @252 | verdict |
|---|---:|---|---|
| production/OFF DS | 254 | (n/a) | — |
| tier2 DS `_1` | 254 | `if=00` | E0 ✓ |
| EV SS `_1` | 253 | `raw=00` | E0 ✓ |
| **EV DS `_1`** | **19** (+ 254) | `raw=02` | **E2 ✗** |
| EV DS `_2` (passes) | **19** (+ 254) | `raw=02` @254 | E2 ✓ |

Key findings:

- The FF0F read is NOT a value-peek miss — `raw intf` genuinely carries bit 1 at
  the read (set at dot 19), so `ff0f_stat_peek` (glitch-line-gated to 0 anyway)
  is irrelevant. tier2 passes because its finer deferred machine advance simply
  never emits the dot-19 STAT (first fold 254, read@252 sees it clear).
- The dot-19 STAT fires with `render.active=0`, `line_render_done=0`, `flip=0`,
  and NO `m0_flip_events` firing (`m0flip` trace empty for dot<100) — it is a
  stale-`m0_rise_dot` consumption on the EV+DS glitch-line re-enable, whose exact
  setter is in the eager DS glitch-line render/enable geometry (the "452-dot /
  dot-82-pipe" glitch frame), NOT any read law.
- The spurious emission is present in BOTH `_1` and `_2` (benign for the
  want-E2 `_2`, fatal for the want-E0 `_1`).

This is a dispatch/enable-write-commit-frame divergence (Part A class) — the same
"9-row dispatch/IF web" #11da left to the write-commit machinery, of which #11df
recovered only the FF0F-WRITE (`ifw`) variants via the squash arm. The plain-READ
`ly0_m0irq_*_ds_1` rows want the enable/emission frame retimed, which the golden
law forbids (never move dispatch). No golden-safe verdict mask can clear the
dot-19 bit without also clearing the legitimate dot-254 bit the `_2` sibling and
other glitch-line rows read. **REFUTED.**

### 3–4. `enable_display/frame0_m0irq_count_scx{2,3}_ds_1` (want 90) — REFUTED

Mode-2/mode-0 STAT interrupt COUNTER rows. OCR output:

| config | got |
|---|---|
| OFF (production) | **90** ✓ |
| tier2 DS | **90** ✓ |
| EV DS | **00** ✗ |

The counter reads 00 under EV = the STAT interrupts do not dispatch/count on the
enable frame. This is exactly #11dj's DMG `frame1_m2stat_count` finding (the
counter never increments on the enable frame = dispatch-frame, not
read-peek-portable). Production PASSES these — the eager clock REGRESSES them (an
EV-specific dispatch-frame miss on the glitch/enable frame). Not a read law.
**REFUTED (dispatch-frame, Part A).**

### 5. `irq_precedence/late_m0irq_retrigger_scx1_ds_2` (want E0, EV got E2) — REFUTED

Pre-refuted in #11de (`eager-ack-squash-port-2026-07-10.md`): the DS ack-squash
window is 3 (`2 + 1`); this row's +1-dot retrigger needs window 4, which
over-squashes six other-family DS `_1` retriggers (ly0/m1/m2int/lyc153int) that
sit one dot inside window 4 → net −6. No new lever found this session (the retime
would need the DS half-dot the eager whole-dot clock cannot represent —
`sb_dsa8`/`read_carried` unreconstructed, #11by/#11cb parked floor). **REFUTED
(pre-refuted; no new evidence).**

### 6. `lcd_offset/offset1_lyc99int_m0stat_count_scx2_ds_1` (want 90) — REFUTED

Same class as rows 3–4: an m0stat STAT COUNTER on the enable/offset frame,
compounded by the STOP-shift `lcd_shift_dots` frame that is unported on the eager
clock (#11bz parked lever, called out in #11da §"STOP-shift lcd-offset"). Counter
= dispatch-frame, not a read peek. **REFUTED (dispatch/STOP-shift frame, Part A).**

### 7. `m2int_m0stat/m2int_m0stat_ds_2` (want 2, EV got 0) — REFUTED

Pre-refuted in #11da (`eager-ds-rehost-2026-07-10.md` §parked): the DS line-start
m2int_m0stat lever is a PAIR-SHUFFLE — forcing the line-start arm's `+2` DS debt
to the `_2` leading dot recovers 5 halt/m0int rows but breaks 6 `_1` siblings
(`lycint_m0stat_ds_1`, `m0int_m0stat_ds_1`, `lycint_lycflag_ds_1/3`,
`lyc0flag_ds_3`, `lyc_ff45_trigger_delay_ds_1`) — the DS mid-dot floor
(#11cb/#11by) that needs the half-dot read. No genuinely different lever found.
**REFUTED (pre-refuted; DS mid-dot floor).**

### 8. `sprites/space/10spritesPrLine_wx7_m3stat_ds_2` (want 0, EV got 3) — REFUTED

Classed genuine Part-A in #11dg/#11dh. OCR: EV=3, tier2=0, want=0. The FF41 m3stat
read is decided by `read_pos_hd < vis_exit_hd` on the DS sprite arm; tier2
recovers it via the production `stat_mode_edge` whole-M-cycle stamp that the
eager FF41 leading peek bypasses. The eager `vis_exit_hd` DS sprite arm is the
parked DS mid-dot floor (needs the half-dot read + the `!ds`-scoped `early_lead`/
`snap_ok` which move the sprite-line dispatch and break `intr_2_*_sprites`).
**REFUTED (DS sprite mid-dot floor, Part A).**

## Result

| | before | after |
|---|---:|---:|
| EV CGB fails | 318 | **318** (no change) |
| rows recovered | — | **0** |
| tier2 CGB | 291 | **291** |
| EV DMG | 66 | **66** |

**0 rows shipped, 0 regressions (no code).** All 8 DS bar candidates are either
dispatch/emission-frame (Part A — rows 1–4, 6, 8) or prior-session pair-shuffle
floors (rows 5, 7). The residual DS bar is now conclusively the counter-pinned
dispatch + DS half-dot floor that lands with the C3 flip / a coupled per-T retime,
NOT the read-frame vein this session probed.

## Gates

1. golden_fingerprint — byte-identical (9020 cases match HEAD, 41.9s).
2. EV CGB 318 (unchanged); tier2 CGB 291; EV DMG 66 — all reproduced on the
   reverted tree.
3. Zero-regression — trivially (tree byte-identical to `a866b0b`).
4. mooneye / clippy / file-caps — unaffected (no source change; `git diff
   a866b0b -- crates/` empty).
5. interconnect/engine reclock defaults NOT flipped.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/cds
BIN=$(ls -t target/cds/release/deps/gbtr-* | grep -v '\.d$' | head -1)
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/cgb_rowlist.txt \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['  # 318
```

Single-ROM verdict: `cargo run -p slopgb-core --example run_gambatte --features
port_probe -- <rom.gbc> cgb` under `SLOPGB_EAGER=1` vs `SLOPGB_TIER2=1`.
