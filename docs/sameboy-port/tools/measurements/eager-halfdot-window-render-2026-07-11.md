# HALFDOT wall-2 window-RENDER bar — REFUTED at premise: the 7 DMG window-bar rows do NOT diverge on the window-recorder dots. The eager render records `wx_match_dot`/`win_reenable_dot`/`win_predraw_abort_dot`/`wy_trig_sb_dot` at the IDENTICAL whole dot the PASSING tier2 frame uses (byte-identical render trace, all 7), and the mode-3 exit model (`vis_exit_hd`) is identical too — the sole EV-vs-tier2 divergence is `read_pos_hd` (EV +8hd ahead), the L2 read-frame re-host (#11dh §4/§6-2), NOT the odd-half render-recorder substrate wall-2 named (2026-07-11, #11dz)

Base: `finish-port-halfdot @ 5ac45f4` (= #11dy; has the #11dw `stat_update_half`/
`eng_stat_half` substrate). **NO CODE SHIPPED — tree byte-identical @ 5ac45f4.**
All experiments were env-gated / reverted (`git diff HEAD crates/` empty). This is a
measure-only REFUTE (the #11br / #11bu / #11bw / #11dy pattern).

## TL;DR — REFUTE, with trace

The task hypothesis was: *"the 7 DMG window-bar rows fail because the eager render
records the window abort/reenable/WY-trigger dot at the whole-dot commit; SameBoy/
tier2 land it at the sub-dot — so an odd-half RENDER-recorder advance (the render
analogue of #11dw's `stat_update_half`) recovers them."* **Two independent
measurements refute the premise:**

1. **tier2 PASSES all 7 with the SAME whole-dot render.** The DEFAULT
   `ProbeMode::Reclock` (tier2 deferred clock, identical `render_step` /
   `window.rs` / recorder-dot code as eager) passes **7/7**. The whole-dot render
   is already correct; the recorder dots land at the right whole dot.

2. **The render-recorder events are byte-IDENTICAL between EV and tier2 on all 7
   rows.** `diff` of the `winmatch` / `wlcdc` (LCDC.5 clear+re-enable) / `wytrigset`
   trace (the events that set `wx_match_dot`/`win_reenable_dot`/
   `win_predraw_abort_dot`/`wy_trig_sb_dot`, each at `self.dot`) is **empty** for
   every one of the 7 rows. There is no armed coincident window-recorder commit that
   differs between the failing and passing frames at an odd half-dot — so the odd-half
   render-recorder advance (which fires only on such a differing armed commit, the
   #11dw idempotency gate) has literally nothing to move.

The actual divergence, isolated to the half-dot in a shared `vis_mode_read` probe:
the CPU-visible mode-3→0 exit differs ONLY because `read_pos_hd` (the eager read
position) runs **+8 half-dots (4 dots) ahead** of the tier2 frame, while the exit
model (`vis_exit_hd`) and every recorder dot are identical. This is the L2 DMG-window
**read-frame** re-host (#11dh §4 "L2 DMG window", §6 step 2), a DIFFERENT substrate
from wall-2's render odd-half.

## Baselines reproduced (exact, at 5ac45f4)

| metric | value | gate |
|---|---:|---|
| `golden_fingerprint` | 1 pass (byte-identical, 42.21s) | THE gate ✓ |
| EV CGB (`cgb_rowlist.txt`) | **295** | steady-state floor ✓ |
| EV DMG (`dmg_rowlist.txt`) | **52** | steady-state floor ✓ |
| OFF CGB | 486 | ✓ |
| tier2 CGB / DMG | 291 / 116 | inherited (byte-identical tree) ✓ |

## The 7 target rows (all FAIL EV DMG, all PASS tier2)

| row | want | EV got | tier2 | class |
|---|:--:|:--:|:--:|---|
| `window/arg/late_wy_FFto2_ly2_scx2_1` | 3 | **0** | 3 (pass) | WY-trigger read-frame |
| `window/arg/late_wy_FFto2_ly2_scx3_1` | 3 | **0** | 3 (pass) | WY-trigger read-frame |
| `window/late_disable_early_scx03_wx11_2` | 3 | **0** | 3 (pass) | pre-draw-abort read-frame |
| `window/late_disable_late_scx03_wx11_2` | 3 | **0** | 3 (pass) | pre-draw-abort read-frame |
| `window/late_reenable_2` | 0 | **3** | 0 (pass) | re-enable read-frame |
| `window/late_reenable_wx0f_2` | 0 | **3** | 0 (pass) | re-enable read-frame |
| `window/late_scx_late_disable_0` | 0 | **3** | 0 (pass) | disable read-frame |

`want=3 got=0` = eager reads BARE where the extended window should read mode 3;
`want=0 got=3` = eager reads mode 3 where the bare/re-enabled line should read 0. Both
directions are a read-position miss, not a render-length miss (the render length is
correct — tier2 passes with the identical render).

## The render-identity discriminator (own probe, reverted)

Per row, `run_gambatte --features port_probe`, `SLOPGB_EAGER=1` vs `SLOPGB_TIER2=1`,
`SLOPGB_S5DBG=1`. Compared the RENDER-recorder events only
(`winmatch`/`wlcdc`/`wytrigset` — the events that assign the four recorder dots):

```
for all 7 rows:  diff <(EV render events) <(tier2 render events)  ->  IDENTICAL
```

The recorder dots themselves (confirmed in a temporary `vis_mode_read` dump,
reverted): `wx_match_dot`, `win_reenable_dot`, `win_predraw_abort_dot`,
`wy_trig_sb_dot` are equal EV vs tier2 to the dot. E.g. `late_wy_FFto2_ly2_scx2_1`:
`wytrigset ly=2 dot=94 wy2=2` IDENTICAL on both frames.

## The decisive read — `read_pos_hd` is the ONLY difference (own probe, reverted)

A temporary dump in `vis_mode_read` right before its `read_pos_hd() < exit` verdict
(`m`, `read_pos_hd`, the fired `exit` arm, verdict, and the recorder dots), reverted
after. `late_wy_FFto2_ly2_scx2_1`, ly=2 (the decisive window line):

```
        EV (want 3, got 0)                       tier2 (pass, 3)
dot=250 m=3 rphd=508 exit=510 verdict=3 flip=0   dot=250 m=3 rphd=500 exit=510 verdict=3
dot=251 m=0 rphd=510 exit=510 verdict=0 flip=0   dot=251 m=3 rphd=502 exit=510 verdict=3
dot=253 m=0 rphd=514 exit=510 verdict=0          dot=253 m=3 rphd=506 exit=510 verdict=3
dot=254 m=0 rphd=516 exit=510 verdict=0 flip=254 dot=254 m=0 rphd=508 exit=510 verdict=3
dot=255 m=0 rphd=518 exit=510 verdict=0          dot=255 m=0 rphd=510 exit=510 verdict=0
```

`exit=510`, `wxm=90`, `flip=254` are **byte-identical** on both frames. The verdict
flips 3→0 when `read_pos_hd` reaches `exit=510`: tier2 crosses at dot 255, EV at
dot 251 — **exactly 4 dots (+8hd) early**. The ROM polls FF41 at a fixed CPU cycle;
EV's cc+0 read returns the mode value +8hd too far along the line → it reads mode 0
where the extended window should still read mode 3 → digit 0 (fail). `late_reenable_2`
is the mirror (EV rphd +8hd ahead makes the re-enabled line read 3 past the bare exit;
`exit`/`wxm`/`flip` identical).

## Why the odd-half render-recorder substrate cannot apply (structural refutation)

The #11dw `stat_update_half` substrate defers an ARMED engine commit to a later odd
half-dot so a coincident level re-eval lands at a held-source join, and it is GATED to
fire ONLY when such an armed commit differs from the whole-dot landing (a bare per-dot
re-eval is non-idempotent → +N shuffle). The render analogue requires the same: a
window-recorder commit whose true half-dot differs from its whole-dot landing on the
failing frame relative to the passing frame. **None of the 7 rows has that:**

- The recorder dots are IDENTICAL EV vs tier2 (render-identity discriminator) — there
  is no sub-dot phase; SameBoy/tier2 land them at the SAME whole dot, and tier2 passes.
- The mode-3 exit model (`vis_exit_hd`, `exit=510`) and the recorded `flip_dot` (254)
  are identical too — the #11dh §2 "flip_dot == projected_flip_dot, no sub-dot flip
  granularity gap" holds here to the dot.
- The sole divergence is `read_pos_hd`, a READ quantity (already half-dot-resolved:
  `2·dot + dhalf + eager_debt`), not a render-recorder quantity. An odd-half render
  advance cannot move `read_pos_hd`.

This is the #11dy rows-1&2 pattern reproduced for the window family: the PPU render
(here the window recorder + exit) is byte-identical to the PASSING frame; the failure
is the eager read frame.

## Residual (the real path — the L2 DMG-window read-frame re-host)

These 7 land with the eager read-FRAME calibration for the DMG window mode-3-exit
reads (#11dh §4 "L2 DMG window", §6 step 2), NOT an odd-half render arm. The measured
signature: on these rows the eager `read_pos_hd` sits **+8hd** above the passing tier2
frame, so the +8hd read-debt (#11by/#11cb, calibrated for the CGB/DMG extend rows it
recovered 172→147) is the WRONG offset for this DMG window-exit subclass — they want
the read to sample ~8hd EARLIER, i.e. a read-debt of ~0 for this arm, not +8. That is a
read-frame arm-calibration slice (measure the per-arm debt, gate `eager_value && !ds`
+ DMG + the window-exit arm, zero-drop A/B), on the #11by→#11cb read-frame lever — the
`vis_mode_read` / `vis_exit_hd` / `read_pos_hd` web, not `render.rs`/`window.rs`.
Do NOT re-chase an odd-half render-recorder advance for these rows.

## Gates (all hold — NO code shipped, tree byte-identical @ 5ac45f4)

1. `golden_fingerprint` byte-identical — 1 pass (42.21s; no code change to crates/).
2. EV DMG **52** unchanged; EV CGB **295**, tier2 **291/116** unchanged (nothing shipped).
3. Zero regression (nothing shipped).
4. All probe edits (a temporary `vis_mode_read` dump on `read_laws.rs`) REVERTED;
   `git diff HEAD crates/` empty.
5. No file grew → no split needed (`read_laws.rs` 999, `render.rs` 784,
   `render/mode0.rs` 530, `window.rs` 223 — untouched).

## Do-not-re-chase ledger (add)

- The 7 DMG window-bar rows (`late_wy_FFto2_*`, `late_disable_*_scx03_wx11_2`,
  `late_reenable_*`, `late_scx_late_disable_0`) are NOT the odd-half RENDER-recorder
  substrate. Their `wx_match_dot`/`win_reenable_dot`/`win_predraw_abort_dot`/
  `wy_trig_sb_dot` and their `vis_exit_hd` exit model + recorded `flip_dot` are
  byte-identical to the PASSING tier2 frame; the sole divergence is the eager
  `read_pos_hd` (+8hd). They land with the L2 DMG-window read-frame re-host
  (per-arm read-debt calibration on the `vis_mode_read` web), not a render half-dot arm.
- CONFIRMS #11dh §4 (these are "L2 DMG window", read-frame class) and §2 (no sub-dot
  flip granularity gap — `flip_dot`/exit identical to the passing frame here).
- CONFIRMS the #11dy structural rule: when the PPU render trace is byte-identical to
  the passing tier2/SameBoy frame, the failure is the eager READ FRAME; no PPU-side
  (engine or render) odd-half arm can move a read-position miss.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd6
cargo test -p slopgb-core --test gbtr --release --features port_probe --no-run
BIN=$(ls -t target/hd6/release/deps/gbtr-* | grep -v '\.d$' | head -1)
grep -E 'late_wy_FFto2_ly2_scx[23]_1|late_disable_(early|late)_scx03_wx11_2|/late_reenable(_wx0f)?_2|late_scx_late_disable_0' \
  scratchpad/dmg_rowlist.txt > scratchpad/wall2_targets.txt   # 7 rows
# EV DMG (all 7 FAIL): 4 want=3 got=0, 3 want=0 got=3
SLOPGB_ROWLIST=$PWD/scratchpad/wall2_targets.txt SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture | grep -E 'FAIL|flagon_probe\['
# tier2 (whole-dot render): all 7 PASS
SLOPGB_ROWLIST=$PWD/scratchpad/wall2_targets.txt SLOPGB_REQUIRE_ROMS=1 \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['  # pass=7
# render-identity discriminator (run_gambatte, port_probe): render events IDENTICAL
cargo build -p slopgb-core --example run_gambatte --release --features port_probe
G=target/hd6/release/examples/run_gambatte
ROM=test-roms/game-boy-test-roms-v7.0/gambatte/window/arg/late_wy_FFto2_ly2_scx2_1_dmg08_cgb04c_out3.gbc
diff <(SLOPGB_EAGER=1 SLOPGB_S5DBG=1 $G $ROM dmg 2>&1 | grep -E 'winmatch|wlcdc|wytrigset') \
     <(SLOPGB_TIER2=1 SLOPGB_S5DBG=1 $G $ROM dmg 2>&1 | grep -E 'winmatch|wlcdc|wytrigset')  # empty
# decisive read: re-add a dump in vis_mode_read (read_laws.rs, before the read_pos_hd()<exit
#   verdict) printing m/read_pos_hd/exit/verdict/wx_match_dot/flip_dot; SLOPGB_S5DBG=1 ly=2:
#   EV rphd +8hd vs tier2, exit=510 / wxm=90 / flip=254 IDENTICAL.
golden: SLOPGB_REQUIRE_ROMS=1 cargo test -p slopgb-core --test gbtr --release golden_fingerprint  # byte-identical
```
