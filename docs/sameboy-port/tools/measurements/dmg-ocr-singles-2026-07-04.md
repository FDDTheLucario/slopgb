# DMG-OCR non-window singles — read-frame characterization (2026-07-04, #11bm)

The third read-frame pass (after #11bk hblank +16 / #11bl poweron +20) over the
C3-CHECKLIST §3b "gambatte DMG-OCR non-window singles". The #11bi dry-run count
of **8** singles was an UNDER-count (the same want-regex miss that under-counted
the DMG window blockers 29→62): a fresh flag-on-vs-off census + `classify_dmg`
SameBoy-pass classification finds **61** SameBoy-PASS flip-blockers across the 7
named categories. Each is characterized as a READ-frame leg (ships the
cc+4-restore fold) or a DISPATCH/RENDER/COMPLETION-frame leg (park, classified).

**Result: +1 shipped read-frame leg (clean full-DMG two-bin), 60 measured
parks.** The read-frame vein for the non-window DMG singles is nearly drained —
unlike hblank (#11bk +16) and poweron (#11bl +20), only ONE clean read-frame
leg (the glitch-line mode-0 co-instant read mask) remained; the rest are
timer/serial-completion (S6), dispatch-count, render-length, or co-temporal
dispatch legs whose read frame cannot separate the A/B.

## Census (flag-on-vs-off, then classify_dmg SameBoy-pass)

Method: `flagon_probe` ON vs OFF over the 7 categories → ON-fail ∩ OFF-pass =
flip-blockers; `classify_dmg.py` → SameBoy-pass (BUG, must-fix) vs SameBoy-fail
(rebaseline). Timeout/skip class dropped (`grep -v 'no undefined-opcode'`).

| category | SameBoy-pass flip-blockers | class |
|---|---|---|
| tima | 45 | S6 timer-completion (park) |
| serial | 1 | S6 serial-completion (park) |
| enable_display | 9 | mixed (see below) |
| sprites | 2 | inverted IF lifecycle (park) |
| lycEnable | 2 | line-start service / LCD-disable (park) |
| miscmstatirq | 1 | glitch mfi=3 dispatch (park) |
| m2enable | 1 | co-temporal line-start service (park) |
| **total** | **61** | **1 ship + 60 park** |

## SHIPPED — `ly0_m0irq_scx1_1` (glitch-line mode-0 co-instant read mask)

`enable_display/ly0_m0irq_scx1_1` (want E0). Dual-traced (`SLOPGB_S5DBG` ↔
SameBoy `SB_TRACE`), the A/B family fully resolved:

| row | want | slopgb read dot | flip_dot | slopgb got | pass |
|---|---|---|---|---|---|
| scx0_1 | E0 | 249 | (pre-flip, intf=00) | E0 | ✓ |
| scx0_2 | E2 | 253 | 252 (read > flip) | E2 | ✓ |
| **scx1_1** | **E0** | **253** | **253 (read == flip)** | **E2 → E0** | **FIX** |
| scx1_2 | E2 | 257 | 253 (read > flip) | E2 | ✓ |

The invariant: **the read wants the STAT bit SET iff `read_dot > flip_dot`;
AT `read_dot == flip_dot` it reads CLEAR.** On the glitch line (first line after
an LCD enable) `scx1_1` reads EXACTLY on the recorded mode-0 flip dot 253 —
which equals SameBoy's dispatch cfl257 (frame map slopgb dot D ↔ SameBoy cfl
D+4). SameBoy's `read_high_memory` orders the CPU read BEFORE the STAT rise at
that shared instant → E0; slopgb's whole-dot frame folds the rise first and
commits the set bit → E2. This is NOT a service-clear — the poll is `DI`,
`IE=0` (`intf & ie & STAT` gate FALSE); it is the read-before-rise complement
of the #11bk `intf & ie`-gated SERVICE-CLEAR (which fires only for a SERVICED
read). The fix (`Ppu::ff0f_dmg_m0_coincident_mask`, `ppu/stat_irq/ff0f.rs`)
masks `IF_STAT` off the read verdict EXACTLY at `dot == flip_dot` — never a
window (the `_2`/`scx0_2` siblings read past the flip and keep E2).

**Verdict-only** — the rise/dispatch never moves. This is why it decouples: the
#11ad `tier2_glitch_m0irq_dispatch_passes` doc parked the DMG side as "a genuine
multi-mechanism atomic (the same glitch-line rise drives the poll path AND the
`int_hblank_halt` halt-wake grid, which want the rise at conflicting dots)" —
but that conflict is only about MOVING the rise. The co-instant mask changes the
READ value alone, so `int_hblank_halt` (which needs the rise at its dispatch
dot) is untouched — the exact #11bk/#11bl read-frame decoupling. Scoped `tier2`
+ `!is_cgb` + `glitch_line` + SS → production and CGB byte-identical. Pin
`tier2_dmg_m0_coincident_passes`.

## PARKED (60 rows, measured dispatch/render/completion-frame)

### tima (45) + serial (1) — S6 timer/serial-completion frame
`tc00_*`/`tc01_*` read TIMA (FF05) / TMA (FF06) / the timer IF bit, and
`serial/start_wait_trigger_int8_read_if_2` reads the serial IF bit. Dual-traced:
the tier2 deferred cc+0 read samples IF/TIMA one M-cycle BEFORE the timer/serial
completion lands (`tc01_irq_2` reads IF at ly1 dot96 = 00, wants the timer bit
E4; serial reads IF = 00, wants the serial bit E8). The lever is the **S6
deferred-completion advance** (a timer/serial-domain event, NOT a PPU mode
transition), refuted for C0-DIV (#11ai: the `{−4..12}` DIV sweep has ZERO
effect) and the goal DO-NOT-RETRY. Not a PPU read-frame; land with S6.

### enable_display `frame*_m0irq_count` (6) — dispatch-COUNT
`frame{0,1,2}_m0irq_count_scx{2,3}_1` poll FF0F at ~dot252 EACH line and count
the mode-0 IRQs (want 90 = 144). got 00/01: the reclock's cc+0 read-frame does
not DELIVER the mode-0 dispatch to the poll (the poll at dot252 ≠ the flip dot,
so the co-instant mask does not apply, and the count needs the dispatch to fire,
which the cc+0 frame loses). The #11bk `if_b`/`nops` dispatch-frame analogue —
a COUNT cannot be restored by a read-value fold.

### sprites (2) — inverted IF lifecycle
`10spritesPrLine_10xposA6_m0irq_1/_2`. The A/B reads straddle a 10-sprite-line
mode-0 rise: `_1` reads ly1 dot305 = set (E2, want 0), `_2` reads dot309 = clear
(0, want 2). slopgb transitions set→clear across (305,309]; the truth is
clear→set. An INVERTED IF lifecycle (not a read-frame shift, which preserves
direction) — the sprite-extended mode-3 length is render-reclock atomic (the
pixel-classify "mode-3 RENDER-RECLOCK" class).

### miscmstatirq `lycwirq_trigger_m0_early_ly44_2` (1) — glitch mfi=3 dispatch
Read ly68 dot256, want E2. SameBoy dispatches a STAT-WRITE-GLITCH interrupt
(`mfi=3`, mode 3) at cfl252 that slopgb does not replicate; slopgb's mode-0 rise
R=254 mis-anchors it, so the #11bk service-clear even MIS-FIRES here (`sc=true`,
gate=true → returns 00, want E2). Dispatch-frame — the edge itself differs.

### m2enable `late_enable_m0disable_2` (1) — co-temporal, read-frame ambiguous
`_1` (want E2) and `_2` (want 0) BOTH read ly2 dot20 with `intf=02`, `ie=02`
(IDENTICAL slopgb read state), OPPOSITE wants. The discriminator (whether the
mode-2 STAT was serviced) is NOT observable in the read frame — a read-value
fold cannot separate them. IME cannot help: it is cleared on dispatch, so the
serviced `_2` has IME=false at its ISR read, same as the `_1` DI poll.
Dispatch-frame.

### lycEnable `lycwirq_trigger_ly00_stat50_2` (1) — line-start LYC service
Read ly0 dot20, gate=true, want E0 (serviced). `_1` reads dot16 (gate=false,
bit not yet risen, E0). The LYC-STAT rise sits between; `_2` should read the
SERVICED value. The anchor is the line-start LYC rise, not the mode-0 flip — a
line-start service-clear would need the LYC-rise dot and shares the m2enable
line-start-service ambiguity (a gate-based clear breaks the mode-2 `m2enable_1`
poll that wants E2 at the same dot20/gate=true). Dispatch-coupled; parked with
m2enable as the line-start-STAT-service class.

### lycEnable `ff40_disable_2` (1) — LCD-disable timing
Read ly0 dot0, `intf=00`; the OCR verdict is not the mode-0/FF0F read the fold
touches — an LCD-disable (FF40) timing case. Not a mode-0 read-frame.

### enable_display `ly0_late_scx7_m3stat` (2) — render-length atomic
`scx0_2` (want 87 = mode3) and `scx0_3` (want 84 = mode0) read the IDENTICAL
slopgb state (ly0 dot253, `flip_dot=252`, mode 0), OPPOSITE wants — the late
SCX7 write changes SameBoy's actual glitch-line mode-3 LENGTH, which slopgb's
render collapses to one flip; the FF41 read frame is identical (co-temporal).
Render-length atomic (the same class the 100 pixel-reference legs park under).

## Gates (all green)

- **full-DMG two-bin (probe2 vs frozen base `27d8dba8`, 3422 rows): +1
  (`ly0_m0irq_scx1_1`) / 0 regressed.**
- gbmicro DMG flag-on 445/68, 0 regressed (the #11bk hblank + #11bl poweron
  families untouched — the glitch-line scope avoids the non-glitch rows).
- full-CGB two-bin: 0-new (`!is_cgb` → the arm returns 0; CGB byte-identical).
- mooneye 91/91 flag-on (`SLOPGB_MOONEYE_RECLOCK`) AND flag-off (the B=42
  counter-pin gate held — the mask is verdict-only, dispatch untouched).
- `tier2_boot_div_passes` + all 54→55 tier2 pins (new
  `tier2_dmg_m0_coincident_passes`); lib 660; clippy `-D warnings` clean;
  full gbtr OFF byte-identical.
- `reclock.rs` split (1074 → 848) with the FF0F read-view/squash family moved to
  `ppu/stat_irq/ff0f.rs` (239) for the CLAUDE.md <1000-line cap.

## Method / tooling (banked)

- `scratchpad/dmg2bin.sh` (DMG-ON two-bin, my probe2 vs frozen probe_base;
  `gambatte::flagon_probe::flagon_probe --exact` to skip the slow gbmicro/
  wilbertpol probes), `scratchpad/gtrace.sh` (dual-emulator gambatte trace),
  `scratchpad/slmeas.sh` (slopgb measurement-read isolate), `classify_dmg.py`.
- Decisive traces via a temp `SCDBG` FF0F line (intf/ie/gate/sc/flip_dot,
  reverted) + the FF41 `FD`/`GL` additions (reverted) — the read state at
  `dot == flip_dot` is the whole diagnosis.
