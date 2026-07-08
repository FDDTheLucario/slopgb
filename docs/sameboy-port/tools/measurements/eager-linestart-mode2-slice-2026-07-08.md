# The EAGER line-start mode-2 back-date — the read-frame completed (2026-07-08, #11cb)

Task: continue the eager-value re-host from #11ca (EV CGB 462). Port the
now-dominant SS read-frame families, each `|| eager_value` per-family, measured
for clean net gain. **Result: EV CGB 462 → 428 (−34), one clean read-frame
mechanism shipped flag-gated (SS + DS), one candidate (vis_early accessibility)
REVERTED as a shuffle; all hard gates hold (golden byte-identical, tier2 CGB
two-bin 291 unchanged, mooneye 91/91 OFF+tier2, eager intr_2 PASS, EV DMG 147).**

## The lever: the line-start mode-0 window is the ONE boundary never back-dated

The eager cc+0 FF41 read samples the PPU ahead of the tier2 deferred cc+4 read by
its speed's read-debt (SS +4 dots / DS +2 dots — a DS M-cycle is 2 dots). The
eager frame back-dates every OTHER mode boundary by that debt to stay
observationally neutral:

| boundary | back-date | site |
|---|---|---|
| mode-2→3 entry | 84 → 80 (SS) | `mode3_entry_dot` (`stat_irq.rs`) |
| mode-3→0 exit | `read_pos_hd` +8 hd SS / +4 hd DS | `vis_exit_hd` (`read_laws.rs`) |
| glitch mode-3 entry | −4 | `vis_mode` glitch arm |
| **line-start mode-0 `[0,4)`** | **NONE (the bug)** | `vis_mode` `dot < 4 → 0` |

So a mode-0-ISR handler's FF41 read landing at the next line's start read 0 where
SameBoy's cc+4 view reads mode 2 (the OAM scan) — the `m0stat`/
`late_m0int_halt_m0stat`/`m0irq_m0stat` cluster (`want=2 got=0`, 70 rows). The fix
back-dates the line-start window by the read-debt: a visible-line read whose
debt-shifted dot reaches the OAM scan (≥ 4) returns mode 2. **CGB-scoped** (CGB
reads 2 at true-dot 0-3, DMG reads 0 — the tests split `dmg08_out0`/`cgb04c_out2`);
**`eager_value`-only** (tier2's `read_deferred` already advances `self.dot` to
cc+4, reading mode 2 natively — no double-shift).

```
vis_mode_read (read_laws.rs:146), eager_value && cgb && line 1-143 && !glitch
  && !line_render_done && m==0 && dot<4 && dot + (ds?2:4) >= 4  →  return 2
```

- **SS** (`6666d9d`): the whole `[0,4)` window → mode 2. CLEAN **31 fixed / 0
  broke** (462 → 431).
- **DS** (`bc68a24`): the `[2,4)` sub-window → mode 2, `[0,2)` stays mode 0 — this
  separates the DS m0stat `_ds_1`/`_ds_2` pair (`_1` reads dots 0-1, keeps its
  mode-0 pass; `_2` reads dots 2-3, reads mode 2). CLEAN **3 fixed / 0 broke**
  (431 → 428): `m0int_m0stat_ds_2` / `lycint_m0stat_ds_2` /
  `lyc_ff45_trigger_delay_ds_2`. The scx3/scx5 DS siblings land on the DS mid-dot
  the whole-dot eager clock can't split (the documented sub-dot floor).

This is not curve-fit: it is the SAME −4 read-debt back-date the other three
boundaries already take, completing an internally-consistent eager read frame.

## REVERTED: the `vis_early` accessibility release (a shuffle, not a lever)

Flipping the three `blocking.rs` `vis_early` gates (OAM/VRAM read + write unblock,
lines 57/181/190) from `tier2_reclock` to `tier2_reclock || eager_value` measured
**11 fixed / 9 broke (net WORSE 428 → 429)** — a classic A/B swap: the
`postread_scx3_2`/`vramw_m3end_scx3_3` `_2/_3` siblings fix while the `postread_1`/
`postwrite_1`/`10spritesprline_postread_1` `_1` siblings break. Under eager
`vis_early` fires at the LE-frame dot (3-4 dots before `line_render_done`), NOT the
tier2 reclocked dot the accessibility verdict is calibrated to — so the release
lands at the wrong dot and shuffles the `_1`/`_2` accessibility pairs. REVERTED
(`git checkout blocking.rs`); needs the reclocked `vis_early` dot, i.e. the
half-dot clock, not a gate flip.

## The 428 residuals — the mechanism classes (all parked, not read-frame)

| class | count | why parked |
|---|---:|---|
| STAT/IF dispatch reads (`E0/E2`, `C1/C5`, `8x`, `9x` hi-nibble) | 129 | the counter-pinned IRQ **dispatch** (reclock.rs STAT engine) — lands with the C3 flip, NOT a read-verdict gate; moving it breaks intr_2 |
| DS mode-bit reads | 94 | the DS **mid-dot** (`sb_dsa8`/`cfl D+3`) the whole-dot eager clock can't represent (the HALFDOT floor, #11ca item 1) |
| SS mode-bit reads | 123 | window 33 (sub-dot `_1`/`_2` exit pairs), dma 18 (HDMA/DMA-service timing), **halt 13 + the wider halt dir 21** (unported wake-clock: `stat_vis_from_t`/`m0_halt_hold`/`if_late`/LY-phase carry), m1 10, sprites 9, m0enable 9 |
| pixel/data reads | 47 | HDMA DMA-service (`defer_steal` lives in the tier2 `write_deferred` path, absent under eager's `Bus::write`) + mid-mode-3 render |

The remaining SS `want=2 got=0` (39) are dominated by the **halt dir** (the ISR read
lands at the wrong PPU dot because the eager halt-wake timing is unported, not a
line-start frame miss) + a diverse wxA6/disable-edge tail (11, no single lever;
the determining ISR read is buried under a 1 M-read FF41 poll loop, un-isolable by
trace).

## The exact next lever (priority order)

1. **The halt-wake clock port** (halt dir ≈ 21, the biggest non-floor SS block).
   The tier2 wake laws — `stat_vis_from_t` (tick.rs:131), `m0_halt_hold`
   (tick.rs:338), `wake_skew`/`repay_wake_skew` (cycle.rs:236, tick.rs:436),
   `halt_ly_phase` (memory.rs:108) — are all `tier2_reclock`-gated and re-host the
   mode-0-rise halt-wake visibility + the post-wake first-read LY phase. This is a
   **multi-mechanism** port (the map #11ca flagged it), not a gate flip; do it as
   one coherent wake retime + measure per-sub-law for shuffle.
2. **The HDMA DMA-service timing** (dma dir ≈ 28: `hdma_start`/`hdma_cycles`/
   `hdma_late_disable`/`hdma_late_enable`). The register-write-race `defer_steal`
   + post-store `service_vram_dma` live ONLY in the tier2 `write_deferred` path
   (cycle.rs:298-334); eager writes go through `Bus::write` and never defer the
   steal. Replicating the scoped defer (FF51-55/FF70/FF4F) in the eager write path
   is a targeted port — measure against the base-passing `hdma_vs_m0_scx2_halt`
   guardrail #11ca notes the general post-store service broke.
3. **The DS mid-dot + the counter-pinned dispatch** — the two hard floors (223 of
   the 428): both need the per-T half-dot clock (HALFDOT Part A) and the coherent
   dispatch retime respectively. Not gate-flippable.

## Gate state (ALL hold, verified this run)

- golden_fingerprint (`--release`) PASS — production byte-identical (`eager_value`
  off → the arm never fires).
- mooneye OFF 91/91 (unchanged); mooneye tier2 (`SLOPGB_MOONEYE_RECLOCK=1`) via
  the two-bin: **tier2 CGB two-bin 291 (unchanged — `eager_value` off under
  tier2)**.
- mooneye EAGER (`SLOPGB_MOONEYE_EAGER=1` acceptance_ppu): only `lcdon_timing-GS`
  ×4 (DMG/Mgb/Sgb, pre-existing exemption); **`intr_2_mode0/mode3/sprites` PASS**
  (dispatch stays cc+4 — no dispatch-moving law enabled; the arm is a read
  verdict).
- EV DMG (dmg_rowlist): **147 (unchanged — the arm is `is_cgb`-scoped)**.
- clippy `-D warnings` clean (default + `port_probe`); `read_laws.rs` 882 < 1000.

## Reproduction

`CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr --release
--no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v '\.d$' | head -1)`;
`SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored
gambatte::flagon_probe::flagon_probe --nocapture | grep pass=` (exact test path).
tier2 two-bin: same WITHOUT `SLOPGB_PROBE_EV` (→ 291). EV DMG: `SLOPGB_PROBE_EV=1`
with `scratchpad/dmg_rowlist.txt` (→ 147). Commits: `6666d9d` (SS), `bc68a24`
(DS).
