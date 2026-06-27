# CGB lcd-offset — the DOUBLE-SPEED (`_ds_`) batch (2026-06-26, #11s)

The `_ds_` siblings of the single-speed lcd-offset class (#11q dispatch + #11r
lyc-wrap). Goal: build-measure each before flooring — does the DS row ride the
single-speed lever on the DS clock (a halved window / a `self.ds` arm), or need
the S6 `ly_for_comparison` DS recalibration?

**Result: TWO clean Tier-2 DS slices shipped, +5/−0 (full 3524-CGB two-bin
flag-on). Every other `_ds_lcdoffset` fail is a build-measure-CONFIRMED floor
(read-frame / render / HDMA-S6 / line-153-S6 / gambatte-reference where SameBoy
renders the same "wrong" value slopgb does).** The lesson held both ways: the
two slices were clean DS-clock ports of the SS levers; the floors were each
proven a floor by direct SameBoy ground truth (framebuffer + SB_TRACE), not by
reasoning.

## SHIPPED — line-start OAM-read window, DS (+2/−0)

`oam_access/preread_ds_1_cgb04c_out0` + `preread_ds_lcdoffset1_1_cgb04c_out0`,
both want0 (OAM accessible). slopgb flag-on read OAM `ly2 dot0` blocked (v=ff →
digit 3); SameBoy reads `ly2 cfl0 dc=-2 rdblk=0` accessible.

The SS lever (`cgb_linestart_oam_open`, #11q) was `!self.ds`-gated. Under DS the
deferred cc+0 read lands **2 dots earlier** in the dot grid (the CPU runs at 2×),
so the read shifts SS `dot2` → DS `dot0`, and the slopgb-side window shifts down
with it: SS `< 4` → **DS `< 2`** (`CGB_LINESTART_OAM_OPEN_DS = 2`, `blocking.rs`).

**The render-floor `_2` sibling is the discriminator that fixes the window
length.** `preread_ds_2` / `preread_ds_lcdoffset1_2` (out**3**) read `ly_ dot2`;
SameBoy reads them `cfl0 dc0 rdblk=0` ACCESSIBLE too — but renders **3**, because
the `_2` digit is the lcd-offset RENDER shift, NOT the OAM read (SameBoy keeps
`oam_read_blocked = false` to ~dot8 in DS: `display.c:1789` `!cgb_double_speed`
is false, lock only at the unconditional `:1804`). slopgb reproduces the
SameBoy-passing `_2` digit via its mode-3 OAM block at dot2 — so the window must
stop at `< 2` (cover dot0 `_1`, exclude dot2 `_2`). A `+ ds` (wider) window was
measured first → +2/−2 (broke both `_2`). The `_2` legs are pinned as regression
guards. Pin `tier2_oam_preread_ds_lcdoffset1_passes`.

## SHIPPED — dispatch HBlank carryover window, DS (+3/−0)

`m0enable/late_enable_ds_lcdoffset1_2` (out0) + base `late_enable_ds_2` (out1) +
`m1/lyc143_late_m0enable_lycdisable_ds_1` (out1), all want NO STAT fire; slopgb
flag-on fired (got 2/3/3).

The #11q HBlank carryover lever fires a fresh mode-0 enable in the line-start
`vis_mode==0` carryover tail (`tail = dot < 4`). The DS `_2` rows enable HBlank
at **`dot2`** where SameBoy's fire is *early* (cleared by the test's IF-clear) so
it must NOT be delivered; the passing `_1` siblings enable at **`dot0`**. Halve
the tier2 carryover window in DS: a separate `carryover_tail = dot < if ds {2}
else {4}` (NOT the shared production `tail`), `stat_irq.rs::stat_write_trigger_cgb`.
Mirrors the existing DS halving (`m2` window `2+2*ds`, `stage_stat_copies` k=2).
Pin `tier2_m0enable_late_ds_lcdoffset_passes`. The lyc_carryover halve was tried
too (`< 4`→`< 2`) and measured **INERT** (the lyc `_2` rows fire via the engine,
not the write-trigger) → reverted.

## FLOORED — build-measure-confirmed (no clean tier2 lever)

Workflow fan-out (16 agents, slopgb flag-on/off + SameBoy framebuffer/SB_TRACE
per row). NONE is a clean tier2 ADD lever:

| row [Cgb] | want/got | floor class | proof |
|---|---|---|---|
| `m2enable/late_enable_ly0_ds` | 2/0 | tier2 read-frame regr | flag-**OFF passes** (out2 == SameBoy `ly0 dc-2 if=02`); flag-ON cc+0 read at `ly0 dot10` samples if=00. Production already correct → not a missing fire, the leading-edge read mis-frames it = C2 global reclock. |
| `lycEnable/late_ff41_enable_ds_lcdoffset1_2` | 2/0 | tier2 read-frame regr | flag-OFF passes (0); flag-ON carryover/engine fires at `ly7 dot2` where SameBoy's `ly7 cfl0` fire is test-cleared. |
| `lycEnable/late_ff45_enable_ds_lcdoffset1_2` | 2/0 | tier2 read-frame regr | flag-OFF passes (0); same line-6→7 sub-dot LYC-match-fall. |
| `m1/ly143_late_m0enable_ds_lcdoffset1_2` | 1/3 | gambatte-reference | SameBoy reads `if=03` at the measurement (renders 3) — slopgb flag-ON (3) is ALREADY SameBoy-correct; gambatte's out1 is the non-SameBoy reference. |
| `m1/ly143_late_m2enable_ds` | 3/1 | read-frame | both dispatch the SAME LYC=143 STAT (`ly143 cfl0`/`dot4`); the out3 is the read-frame, flag-OFF==flag-ON. |
| `lycEnable/ff45_enable_weirdpoint_ds_lcdoffset1_2,3` | 0/2 | gambatte-reference | SameBoy fires the identical LYC=6 STAT at `ly6 cfl0` every frame; flag-OFF==flag-ON==2 → SameBoy does NOT pass out0. |
| `lycEnable/lyc153_late_ff41,ff45_enable_ds_lcdoffset1_2` | E0/E2 | S6 line-153 + gambatte-reference | line-153 `ly_for_comparison` is the documented DS placeholder; AND SameBoy's own framebuffer renders **E2** (its `.bmp`/`.log` sidecar), not the gambatte outE0. |
| `lycEnable/lycwirq_trigger_ly00_stat50_ds` | E0/E2 | read-frame | the #11r SS-floor analogue (legit LYC-153 dispatch dot + read position); flag-OFF==flag-ON. |
| `window/late_enable_afterVblank_ds`, `late_wy_ds_2`, `late_wy_1toFF_ds_2` | 3/0 | render C2 | digit is rendered window content / a co-located FF41 m3stat read (slopgb `dot262 mode0` vs SameBoy `cfl261 mode3` — same dot, different mode = render-grid mode-3 length, not read-frame). |
| `oam_access/prewrite_ds_lcdoffset1_1` | 1/0 | write floor | CGB blocks OAM writes from line-start (`display.c:1802`), both emulators drop (like the SS `prewrite` floor #11q). |
| `dma/hdma_late_m0halt_ds` | 00/FF | HDMA S6 | digit is an HDMA-transferred byte readback; flag-OFF==flag-ON. |

**Two distinct floor flavours worth naming:** (1) **tier2 read-frame regressions**
— rows that PASS flag-OFF and the leading-edge path REGRESSES (m2en_ly0,
late_ff41/ff45_ds_2). Not baselined (green in production); blocked on the C2
global read-frame reclock. (2) **gambatte-reference floors** — SameBoy renders
the same digit slopgb does, ≠ the gambatte filename expectation (m1m0en_2,
weirdpoint, lyc153). Correctly baselined; never SameBoy-passing, so not droppable
and not fixable.

## Tooling

- SameBoy 1.0.2 tester + the SB_TRACE patch set (SBLEVEL/SBTRACE STAT_IRQ/SBMODE/
  SBREAD ff0f+ff41/SBWRITE ff45). This session ADDED a temp `SBOAM`/`SBVRAM`
  read-block tracer to `memory.c` (rdblk/wrblk at the OAM/VRAM read path) — `/tmp`
  is cold, re-add if wiped. `--cgb --length 4` = double speed.
- slopgb temp `wr41/wr45` write tracer in `cycle.rs::write_deferred` (reverted
  after) + the committed `ff41/ff0f/oam/vram/dispatch` tracers (`SLOPGB_S5DBG`).
- Two-bin: full 3524-CGB rowlist, `flagon_probe` (target/gbtr) vs stashed-revert
  (target/lint), `comm` the FAIL row-ids.
