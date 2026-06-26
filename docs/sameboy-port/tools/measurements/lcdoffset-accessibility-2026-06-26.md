# CGB lcd-offset batch — accessibility ground truth (2026-06-26, #11q)

The C-stage mech-3 CGB lcd-offset batch. **Three clean tier2 slices shipped,
+7/−0 total:** (1) OAM line-start read window +1, (2) palette m3-start read+write
window +2, (3) the **dispatch-class HBlank write-trigger +4** (the lever the
goal's "build a tier2-gated dispatch reclock" called for — a live build-measure
REFUTED the premature "floored to C2" verdict). The remaining dispatch rows
(m1/lycEnable sub-families) + window / pure-`_ds` / HDMA are floored to the
delicate line-0/lyc engine (#11k/#11l) or render / S6.

## What lcd-offset does (CORRECTED — it is NOT a mode-boundary shift)

**Decisive measurement (do not re-chase the wrong framing):** SameBoy's
per-line PPU mode timeline is **IDENTICAL** between the non-offset base ROM and
the lcd-offset ROM — both show mode-3 entry **cfl84**, mode-2 **cfl0**, mode-0
**cfl257** (SBMODE `late_enable_1` vs `late_enable_lcdoffset1_1`, 33118 counts
each). cfl84 (= `MODE2_LENGTH + 4`) / cfl257 are SameBoy's NORMAL mode timing,
NOT an offset effect. So the lcd-offset does **not** shift the cfl-referenced
mode boundaries.

What the lcd-offset actually does: the LCD was turned on at a different CPU
cycle, shifting the whole frame's **CPU-instruction ↔ PPU-phase alignment**. The
test's measurement access (designed to hit a specific PPU phase, e.g. line-start
`cfl0`) still lands at that phase on SameBoy in both base and offset variants;
but slopgb — whose PPU-vs-CPU alignment after enable does not reproduce
SameBoy's sub-dot enable phase (slopgb tracks no `display_cycles`/enable-phase
offset) — lands the deferred access at a DIFFERENT PPU dot in the offset variant
(e.g. OAM read base `ly1 dot452` → offset `ly2 dot2`). The "windows" SameBoy
exposes (line-start OAM, m3-start palette) are UNIVERSAL CGB mode-boundary
accessible windows present on every line in BOTH variants; the offset merely
moves slopgb's deferred access INTO them, exposing that slopgb lacked the
windows. (The `cfl=0 dc=2 vis=0 → dc=8 vis=2` line-start transition IS the
universal mode-0-tail OAM window, not an offset artifact.)

## SHIPPED — OAM line-start read window (clean +1/−0)

`oam_access/preread_lcdoffset1_1_cgb04c_out0` [Cgb], want0 (OAM accessible) got3
(blocked). The clean isolation: the non-offset base `preread_1` [Cgb] PASSES
flag-on (slopgb reads `ly1 dot452`, real mode-0 → accessible); only the offset
variant fails.

| source | OAM read | accessible? |
|---|---|---|
| SameBoy (base `preread_1`) | ly1 cfl0 | blk=0 (accessible) |
| slopgb flag-on (base) | ly1 dot452 | v=00 (accessible) — passes |
| SameBoy (offset `preread_lcdoffset1_1`) | ly2 cfl0 | blk=0 (accessible) |
| slopgb flag-on (offset) | ly2 dot2 | v=ff (blocked) — FAILS |

Root: SameBoy keeps `oam_read_blocked = false` for the first ~3 T-cycles of each
visible line on CGB single-speed — `display.c:1805-1810`, the mode-0/HBlank tail
runs 2+1 cycles (`GB_SLEEP` state 35 ×2, state 6 ×1) before `oam_read_blocked =
!cgb_double_speed` engages at the mode-2 lock (state 7). The lcd-offset shifts
slopgb's read 6 dots later (line1 dot452 → line2 dot2), across the line boundary
into mode-2, where slopgb (locking OAM from dot 0) blocks. SameBoy reads in the
line-start window → accessible.

Fix (`ppu/blocking.rs::cgb_linestart_oam_open`, tier2-gated): release
`oam_read_blocked` for dots `0..CGB_LINESTART_OAM_OPEN` (=4) on CGB
single-speed, line != 0. **Two-bin (654 CGB baseline rows, flag-on): +1/−0**
(fixed `oam_access/preread_lcdoffset1_1` [Cgb], zero SameBoy-passing dropped).
Pin `tier2_oam_preread_lcdoffset1_passes`. Byte-identical OFF (window never open
in production). Commit `457955e`.

## SHIPPED — m3-start palette-RAM window (clean +2/−0)

`cgbpal_m3/cgbpal_read_m3start_lcdoffset1_1_cgb04c_out00` [Cgb] (want00 got=FF,
read blocked) + `cgbpal_write_m3start_lcdoffset1_1_cgb04c_out01` [Cgb] (want01
got00, write dropped). Same clean isolation: the non-offset base
`cgbpal_read/write_m3start_1` PASS flag-on (slopgb accesses `ly1 dot80`,
pre-lock); only the offset variants fail.

| source | palette read | accessible? |
|---|---|---|
| slopgb flag-on (base read) | ly1 dot80 | v=00 — passes |
| slopgb flag-on (offset read) | ly1 dot86 | v=ff (blocked) — FAILS |

Root: SameBoy keeps `cgb_palettes_blocked = false` for 3 T-cycles INTO mode 3
(`display.c:1867` false → 3-cycle `GB_SLEEP` → `:1877` true) — palette RAM stays
accessible at the mode-3 entry before the lock engages, even though the visible
mode is already 3. The lcd-offset shifts slopgb's access from dot80 to dot86
(+6, same shift as OAM), past slopgb's sharp dot-84 mode-3 palette anchor.
SameBoy's lock is at ~cfl87 (mode-3 entry cfl84 + 3).

Fix (`ppu/blocking.rs::pal_ram_blocked`, tier2-gated): extend the mode-3 lock by
`PAL_M3START_OPEN`(=3) → dot 87 on CGB single-speed (`pal_ram_blocked` gates both
read and write, so both legs land). **Two-bin (654 CGB baseline rows, flag-on):
+2/−0.** Pin `tier2_cgbpal_m3start_lcdoffset1_passes`. Commit `e8c1257`.

NOTE the asymmetry with the OAM/VRAM WRITE floor: palette RAM has a mode-3-entry
accessible window for BOTH read and write (the single `cgb_palettes_blocked`
flag), so the offset write lands; OAM/VRAM writes are blocked from line-start
(`display.c:1802`, no read-style window) so the offset OAM write is a genuine
floor.

## FLOORED (measured, not a clean tier2 lever)

- **`vram_m3/preread_lcdoffset2_1` [Cgb] (want0 got3) — render/readback floor.**
  **flag-OFF OCR == flag-ON OCR == `3…`** (tier2 read phase irrelevant, like
  `scx_m3_extend`). slopgb reads VRAM at `ly1 dot84` (mode-3, blocked); SameBoy's
  matching read is `ly1 cfl87 blk=1` (also blocked). Both block → the read is not
  the discriminator; the out0 expectation is render/readback-level. Not a
  line-start window (the read is at the mode-2→3 boundary, not line start). C2.
- **`oam_access/prewrite_lcdoffset1_1` [Cgb] (want1 got0) — genuine floor (both
  block).** Calibrated: out1 ⟺ write LANDS (base `prewrite_1` slopgb writes
  `ly1 dot452 blk=false` → lands → out1, passes). Offset: slopgb writes
  `ly2 dot2 blk=true` → drops → out0. SameBoy SBOAMW also blocks (`ly2 cfl0
  blk=1`). Per the FSM, CGB writes ARE blocked from line-start (`display.c:1802`
  `oam_write_blocked = is_cgb && !cgb_double_speed` = true), unlike the 3-cycle
  read window — so slopgb CORRECTLY blocks; both emulators drop the write; the
  want1 is real-hardware behavior SameBoy misses too → NOT SameBoy-passing,
  genuine floor. Leave baselined. (Same for `vram_m3/prewrite_lcdoffset2_1`.)

## DISPATCH-CLASS — the HBlank sub-family BUILT (+4/−0); the prior "floor" was WRONG

`m0enable/late_enable_lcdoffset1_1` (want2 got0), `m1/m1irq_late_enable_lcdoffset1_1`
(want2 got0), `lycEnable/late_ff41_enable_lcdoffset1_1` (want2 got0),
`m1/ly143_late_m0enable_lcdoffset1_1` (want3 got1). Clean offset-isolation. These
enable a STAT source late (FF41 write) and read FF0F to check the IRQ; slopgb
delivered `if=00` where SameBoy fires.

**CORRECTION — the dispatch class is NOT one atomic-reclock floor; it splits by
SOURCE, and the HBlank sub-family is a CLEAN tier2 write-trigger lever (`06f369b`,
+4/−0).** The earlier "floored to C2 / read-frame atomic reclock" verdict was a
premature inference; a live build-measure refuted it. Hard trace:
`late_enable_lcdoffset1_1` writes FF41 (enabling the HBlank/mode-0 source) at
**`ly130 dot3`** — inside the line-start hblank carryover (dots 0-3, `vis_mode==0`,
the PREVIOUS line's mode-0 already passed) — then reads FF0F at `ly130 dot14`. In
`stat_write_trigger_cgb` the `tail = dot < 4` branch defers a fresh m0 enable to
the scheduled m0irq event; but in the carryover that event points at the NEXT
line's mode-0 (beyond the LY increment, dot254 ≫ read dot14), so the deferral
**loses the IRQ**. SameBoy raises IF AT the write. **Fix:** under tier2, a fresh
m0 enable in the carryover tail (`vis_mode==0`, glitch excluded) raises IF
immediately. Two-bin (654 CGB rows, flag-on): **+4/−0** —
`m0enable/late_enable_lcdoffset1_1`, `m1/ly143_late_m0enable_lcdoffset1_1`, plus
double-speed `late_enable_ds_1` and `late_enable_ds_lcdoffset1_1` riding the same
lever. Pin `tier2_m0enable_late_lcdoffset_passes`. The discriminator that makes it
clean (not the per-config offset I wrongly assumed unresolvable): `vis_mode==0`
separates the post-m0 carryover (fire) from the pre-m0 line-start tail (defer) —
a real slopgb state difference, like the #11n eighth-grid / #11o accessibility
levers.

**Remaining dispatch rows (m1 + lycEnable sub-families) — harder, entangled,
floored for now.** Both ALSO land at `dot3` (the same carryover tail) but hit
DIFFERENT source suppressions with no clean `vis_mode` discriminator:
`m1/m1irq_late_enable_lcdoffset1_1` enables the VBlank source at `ly0 dot3`, hitting
the `m1_tail = line==0 && dot<4` suppression (`stat_irq.rs:523`) that #11k's
line-0 VBlank-carry work pins (`lycstatwirq_trigger_ly00` reads E0); `lycEnable/
late_ff41_enable_lcdoffset1_1` enables a non-HBlank (LYC) source whose `cmp_cgb`
compare at the `dot3` tail is the #11l lyc-carryover territory. Lifting those tail
suppressions risks the pinned #11k/#11l line-0/lyc rows and has no measured
discriminator yet → ground-truth-first + anti-thrash: floored, the next dispatch
slices. The `window / late_wy / dma / pure-_ds` lcdoffset rows remain render / S6.

## Full-sweep triage (38 baselined lcdoffset [Cgb] rows, flag-on after both fixes)

14 pass (incl. the 3 fixes), 24 fail. The 24 residual, all needing the full
lcd_offset port / out-of-scope infra:

- **engine-dispatch (~13):** `m0enable/late_enable`, `m1/{m1irq_late_enable,
  ly143_late_m0enable, ly143_late_m2enable}`, `lycEnable/{late_ff41, lyc153_late_ff41,
  lyc153_late_ff45, lycwirq_trigger_ly00, ff45_enable_weirdpoint}`,
  `miscmstatirq/lycstatwirq` — want2/E0/E2/3 IRQ-delivery swaps; slopgb's un-offset
  `stat_update_tick` mis-frames the offset-shifted mode edge. = C2/engine port.
- **render/window (4):** `window/{late_enable_afterVblank, late_wy, late_wy_1toFF}`
  — render-level WY-latch/abort (the #11g/#11p window floor), shifted. = C2.
- **double-speed `_ds` (~4):** `oam_access/preread_ds_lcdoffset1`,
  `dma/hdma_late_m0halt_ds`, `m0enable/late_enable_ds`, `m1/ly143_late_m2enable_ds`
  — the same levers but on the double-speed clock = S6/S7.
- **HDMA / render floors (3):** `dma/hdma_late_enable_lcdoffset3`,
  `vram_m3/{preread, prewrite}_lcdoffset2`, `oam_access/prewrite_lcdoffset1` — S6
  HDMA / render-readback / genuine write floor (above).

## Verdict

The clean local accessibility levers in the lcd-offset batch are the two
sub-dot-window slices: the **line-start OAM-read window** (+1) and the
**m3-start palette-RAM read+write window** (+2) — **+3/−0 total, shipped +
pinned**. Both are SameBoy mode-boundary accessible windows (OAM: 3 cycles before
the mode-2 lock; palette: 3 cycles into mode 3) that slopgb lacked; the lcd-offset
moves the deferred access into them. Everything else in the batch needs the full
lcd_offset PPU-phase model in the shared engine/render grid (engine-dispatch,
window) or the double-speed/HDMA clock (S6) → C2. This matches #11p's prediction:
mech-3 lcd-offset is the largest batch but mostly needs the lcd_offset port; the
extractable slices that don't are the two mode-boundary accessibility windows.
Defaults NOT flipped.
