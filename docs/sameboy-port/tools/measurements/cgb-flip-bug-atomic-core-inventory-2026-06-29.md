# The DEFINITIVE CGB flip-BUG atomic-core inventory — the C2-reclock lever spec (2026-06-29 #11ah)

The goal's primary deliverable: the complete, build-measured census of **every**
CGB flip-BUG (OFF-pass ∧ ON-fail) tagged with its atomic class and the lever the
C2 atomic reclock must move to flip the C3 default. Plus the DS S6 batch survey
(speedchange / dma / serial / tima / lcd_offset_ds) for clean slices — **0 found,
build-measured** (it is the S6 read-grid, as the goal predicted). The map is the
product; this scopes C2/C3.

## The census (full 3422-row CGB gambatte set, `flagon_probe` ON vs OFF, fresh binaries)

```
ON  (boot_with_reclock):  pass=2541 fail=479
OFF (production):         pass=2534 fail=486
flip-BUGs (comm -23 ON-fail OFF-fail = OFF-PASS ∧ ON-FAIL):  185
```

**185 flip-BUGs.** (479 ON-fails − 294 baselined-floors-also-failing-ON = 185
production-correct rows the tier2 leading-edge breaks. The 486 OFF-fails are the
baselined floor; 7 of them tier2 actually FIXES, hence ON<OFF.) Of the 185:
**112 single-speed · 73 double-speed (`_ds`)**.

**LE-split (the class discriminator — `SLOPGB_PROBE_LE`, engine + leading-edge
reads but NOT the full tier2 render-frame / C0 DIV):**

```
141 ENGINE-DISPATCH (LE-fail) — the stat_update_tick rising edge / IF-lifecycle /
                               leading-edge read-collapse is ALREADY wrong at LE
 44 RENDER-FRAME    (LE-pass) — engine+LE-read correct; only the FULL tier2 frame
                               (C0 +4 DIV + deferred render-frame) breaks it
```

## DS S6 BATCH SURVEY — 0 clean slices (build-measured, the goal's bonus target)

The 32 S6-batch flip-BUGs (speedchange 8 · dma 7 · serial 4 · tima 4 · lcd_offset 9),
each traced (slopgb 3-mode + SameBoy `--cgb --length 4` ground truth). **None is a
whole-dot CGB+tier2 slice** (no accessibility-window-shaped lever — the only DS
clean-slice shape, #11s; these are all read-frame / S6-conflict / dispatch).

| family | rows | want/got | LE | build-measured verdict |
|---|---|---|---|---|
| **speedchange** | 8 | `m2int_m3stat*` 0/3 | ENGINE | The FF41 mode-3 read after a STOP speed-switch. SBMODE: the visible mode-3 EXIT (vis 3→0) lands `ly1 cfl257`; slopgb's deferred read samples mode 3 before the SCX-extended boundary. = the m2int_m3stat read-frame↔boundary collapse (#11ae class 3) + the STOP speed-switch S6 `ly_for_comparison` mid-frame phase recalibration. ATOMIC. |
| **dma** | 7 | hdma 0/1, 1/7 · gdma 3/0 | RENDER | 3× `hdma_late_{destl,length,wrambank}_1` (SS, 0/1): HDMA register readbacks — the deferred cc+0 read samples the mid-transfer register 1 M-cycle off = the C0-DIV/read-frame on an HDMA-staged register. 2× `hdma_late_disable_{ds,scx5_ds}_2` (1/7): HDMA-disable + a co-located FF41 m3stat read-collapse (render mode-3 length). 2× `gdma_cycles_{short,2xshort}_scx5_ds_1` (3/0): GDMA stalls the CPU N cycles then reads FF41; the DS GDMA cycle count lands the read past the mode-3 exit. All = **S6 cycle_write conflict table / GDMA cycle**. The SS siblings PASS or are OFFLOOR — no portable lever. ATOMIC. |
| **serial** | 4 | `int8_read_if` E8/E0↔E0/E8 | RENDER | See the gambatte-ref finding below. The deferred FF0F (IF) read samples the serial-complete bit 1 M-cycle (4 dots) off SameBoy. **C0-DIV-phase read-frame.** ATOMIC. |
| **tima** | 4 | `tc00_irq_late_retrigger` E4/E0↔E0/E4 | RENDER | Same as serial for the timer IF bit (0x04). **C0-DIV-phase read-frame.** ATOMIC. |
| **lcd_offset** | 9 | `offset*_lyc99int_count` 90/00, 91/90, 98/00 | 7 ENG / 2 REN | The lcd-offset dispatch / lyc-wrap COUNT rows (the #11q/#11r class re-surveyed — distinct from the #11s oam_access/m0enable lcd-offset rows). The lcd-offset shifts the LYC=99 dispatch dot; the per-frame mode-0/2/IRQ count diverges and the loop reads FF44 a line off. The 7 m0irq/m2irq LE-fail = dispatch; the 2 m0stat_ds LE-pass = read-frame. The DS m0stat rows have PASSING SS siblings (`scx2_1`) but are render-frame regressions, not halvable windows (#11s flavour-1 floor). ATOMIC. |

### The serial/tima gambatte-reference finding (NEW — settles the flip-pair shape)

The serial/tima flip-BUGs look like "can't-fix-both" flip pairs (`_1` wants the IF
bit set, `_2` wants it clear; slopgb has them INVERTED). **SBREAD ff0f ground truth
refutes that read** — SameBoy reads the SAME IF value for BOTH legs (4 dots apart):

```
serial _1 (want E8): SBREAD ff0f ly10 cfl168 if=08      tima _1 (want E4): ly1 cfl180 if=04
serial _2 (want E0): SBREAD ff0f ly10 cfl172 if=08      tima _2 (want E0): ly1 cfl184 if=04
(DS legs mirror: serial _ds_1/_2 if=08 dc-632/-628; tima _ds_1/_2 if=04 dc-160/-156)
```

SameBoy renders **E8/E4 for both `_1` and `_2`** (the bit stays set across the
4-dot read gap). So:
- The **`_1` legs** (E8/E4) = **SameBoy-passes** that tier2's read-frame currently
  REGRESSES (slopgb misses the bit → E0). The C2 read-frame reclock FIXES them.
- The **`_2` legs** (gambatte wants E0) = **gambatte-reference** rows SameBoy does
  NOT pass (it renders E8/E4). tier2 already renders the SameBoy value; the C2
  flip correctly REBASELINES them (the #11s/#11t gambatte-reference-floor pattern).

So serial+tima (8 flip-BUGs) split **4 SameBoy-pass fixes (`_1`) + 4 gambatte-ref
rebaselines (`_2`)** at the C2 flip — NOT an irreducible collapse.

## THE FULL ATOMIC-CORE INVENTORY (185 rows, 5 classes)

Per-family, with the LE-split (ENG = engine-dispatch / REN = render-frame) and the
C2 lever each class needs. Sorted by class.

```
CLASS          family             flip  ENG  REN
WAKE-CLOCK     halt                 12    5    7
ENGINE-IF      lycEnable            19   19    0
ENGINE-IF      m1                   14   12    2
ENGINE-IF      m0enable              9    9    0
ENGINE-IF      m2enable              6    6    0
ENGINE-IF      miscmstatirq          6    6    0
ENGINE-IF      ly0                   5    4    1
ENGINE-IF      lyc153int_m2irq       5    4    1
RENDER-LENGTH  window               34   28    6
RENDER-LENGTH  cgbpal_m3             8    5    3
RENDER-LENGTH  m2int_m3stat          7    7    0
RENDER-LENGTH  enable_display        6    4    2
RENDER-LENGTH  vram_m3               5    5    0
RENDER-LENGTH  oam_access            4    4    0
RENDER-LENGTH  scx_during_m3         1    1    0
READ-FRAME     serial                4    0    4
READ-FRAME     tima                  4    0    4
READ-FRAME     m2int_m0irq           3    3    0
READ-FRAME     display_startstate    3    1    2
READ-FRAME     m2int_m2stat          2    2    0
READ-FRAME     irq_precedence        2    0    2
READ-FRAME     m2int_m2irq           1    0    1
READ-FRAME     m2int_m0stat          1    1    0
S6-DS          speedchange           8    8    0
S6-DS          dma                   7    0    7
S6-DS          lcd_offset            9    7    2
```

### Class totals + the C2-reclock lever each needs

| class | rows | ENG/REN | the C2 lever (what the atomic reclock must move) | grounding |
|---|---|---|---|---|
| **RENDER-LENGTH** | **65** | 54 / 11 | The production-shared render mode-3 LENGTH reclock — the visible mode-3→0 boundary placed at SameBoy's `display.c:1493` `167 + SCX&7 + penalties`, in BOTH the EXTEND (window low-WX / `_1` scx5, sprite-line) and SHORTEN (late_disable/1toFF abort) directions, plus the cc-exact OAM/VRAM/palette accessibility read-phase (the `_2` read-collapse: cgbpal_m3end framebuffer-confirmed #11ag, vram_m3/oam_access). Breaks byte-identical OFF (touches the render). | #11af/#11ag (window), #11ag (cgbpal framebuffer), #11u class 2, #11ad (glitch) |
| **ENGINE-IF** | **64** | 60 / 4 | The STAT engine IF-bit LIFECYCLE: the dispatch DOT is already correct (`miscmstatirq` fires ly44 mode-0 at dot254 = SameBoy cfl257) — the divergence is the IF-bit edge-presence / blocking-level PRECEDENCE at the LYC re-arm / line-start carry / late-FF41/FF45-write-trigger sub-edge. Needs the `stat_update_tick` rising-edge to match SameBoy `GB_STAT_update`'s `mode_for_interrupt | LYC` continuity (the line-boundary HIGH-hold). | #11ae class 2, #11u (A)+(B), #11j-#11k (the DMG roots shipped; CGB residual) |
| **RENDER-LENGTH** \ (see above) | | | | |
| **READ-FRAME** | **20** | 7 / 13 | The deferred cc+0 read FRAME POSITION: the post-dispatch ISR read lands 1 M-cycle (≈4 dots) early vs SameBoy (the #11z' interrupt-service-frame +4, a PPU-ADVANCE LAG not a CPU deficit), AND the C0 +4 DIV phase shifts the DIV-driven serial/timer IF-set dot vs the read (serial/tima). The dispatch-retime + read-frame co-land. | #11z'/#11u class 1 (ISR +4), this session (serial/tima C0-DIV) |
| **S6-DS** | **24** | 15 / 9 | The double-speed S6 read-grid: the DS `ly_for_comparison` / `display_cycles` recalibration (lcd_offset_ds, speedchange STOP phase) + the `cycle_write` conflict-staging table (HDMA byte-transfer, GDMA cycle count). The PORT-PLAN S6 stage. | #11s (lcd_offset DS), #11t (sprite DS), this session (speedchange/dma) |
| **WAKE-CLOCK** | **12** | 5 / 7 | The sub-M-cycle CPU WAKE T-phase: the mode-0-halt-wake records the IRQ rise at the M-cycle boundary; want-0 (scx2) and want-2 (scx3/4/5) reads COLLAPSE at `ly2 dot4 mode2` and only the sub-T-cycle wake phase separates them (the +7/−15 force-mode-0 inverts the want-2 siblings). The PORT-PLAN S7 sub-M-cycle IF-raise/wake clock. | #11ae class 1 (halt +7/−15 build-measured), #11m wake-clock |

**Partition: RENDER-LENGTH 65 · ENGINE-IF 64 · S6-DS 24 · READ-FRAME 20 ·
WAKE-CLOCK 12 = 185.**

The LE-split cross-cut: **141 ENGINE-DISPATCH** are dominated by ENGINE-IF (60) +
RENDER-LENGTH read-collapse-at-LE (54, the `_2` mode-3 reads that mis-frame even
without C0) + S6 dispatch (15) + WAKE-CLOCK LE-fails (5). **44 RENDER-FRAME** are
the pure C0-DIV/deferred-render-frame regressions: READ-FRAME (13, incl all 8
serial/tima), RENDER-LENGTH window/cgbpal (11), S6 dma/lcd_offset (9), WAKE-CLOCK
halt LE-pass (7), ENGINE m1/ly0 (4).

## THE C3 FLIP ACCOUNTING (what the atomic C2 reclock buys)

When the C2 reclock (dispatch-retime + read-frame co-land + render mode-3 length +
engine IF-lifecycle + S6 DS-grid + S7 wake-clock — all land TOGETHER, intermediate
states RED) flips the C3 default, the 185 flip-BUGs resolve as:

- **FIXED (SameBoy-pass rows the read-frame currently regresses):** the READ-FRAME
  `_1` legs (serial/tima 4, m2int_* read-collapse), the RENDER-LENGTH SameBoy-pass
  reads (window m2int_wx<0xA0 already shipped; the cc-exact boundary lands the
  rest), the ENGINE-IF rows once `stat_update_tick` matches the IF lifecycle.
- **REBASELINED (gambatte-reference, SameBoy renders ≠ gambatte):** the serial/tima
  `_2` legs (4 — SBREAD-confirmed E8/E4), the dma `_ds_1` GDMA + lcd153 lines, the
  cgbpal A/B-pinned m3start window (#11u class 5), the DMG glitch `frame0_m0irq`
  (#11ad). These join `baselines/gambatte.txt` as genuine-floor.
- **NET:** the flip is NOT a clean +185; it is ~`fix(SameBoy-pass)` − `rebaseline
  (gambatte-ref)` − the ~294 already-baselined floors that stay floored. The exact
  N/M needs the C2 build (the global ~7000-row rebaseline) — this inventory scopes
  WHICH rows move and by WHICH lever, the convergence spec.

## Why 0 clean slices remain (the slice-by-slice phase IS exhausted)

The clean-slice vein required a LOCAL tier2 ADD lever (an accessibility window or a
dispatch dot a CGB gate can place) where the SS sibling already passes. After
#11j-#11ag every such lever is shipped. The 185 residual all need a lever that is
GLOBAL (the deferred read frame / the C0 DIV phase / the render mode-3 length) or
SUB-M-CYCLE (the wake clock / the IF-lifecycle sub-edge) — both atomic by
construction (moving them shifts the counter-pinned dispatch tests
`intr_2_mode0`/`int_hblank`/`di_timing` that pin the current cc+4 frame; #11z'
proved the ISR-frame nudge converges window −11 but costs mooneye 91→89). The S6
batch confirmed the pattern one last family-cluster: speedchange/dma/serial/tima/
lcd_offset_ds are uniformly read-frame / S6-conflict / dispatch, no halvable window.

**Next is C2 (the atomic reclock build), not another slice.** The lever spec above
is the convergence target.

## Method / tooling (this session)

- Census: `flagon_probe` binary (`target/gbtr/release/deps/gbtr-<hash>`),
  `SLOPGB_ROWLIST` = the 3422-row CGB list (`find gambatte -name '*.gbc'` +
  `[Cgb]`), ON + OFF + LE (`SLOPGB_PROBE_LE`), `comm -23` for the flip subset.
- Per-row ground truth: SameBoy `sameboy_tester --cgb --length 4` `SB_TRACE`
  (`SBREAD ff0f`/`SBMODE`); slopgb `SLOPGB_S5DBG` (`SLOPGB ff0f`/`ff41`/`dispatch`).
- Data files (this session): `scratchpad/flipbugs.txt` (the 185),
  `inventory_rows.tsv` (family·row·want·got·LE), `s6_paths.txt` (the 32 S6 batch).
- 0 code shipped (0 clean slices); gbtr + mooneye OFF byte-identical by
  construction; 26 tier2 pins held; defaults NOT flipped.
</content>
</invoke>
