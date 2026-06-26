# m1 / lycEnable IF-delivery family — FF0F ground truth (S5 engine-dispatch)

2026-06-25 (#11h). The diagnostic the #11g handoff scoped: the m1/lycEnable
"want=3↔1 / want=E0↔E2" rows observe the STAT-vs-vblank IRQ delivery by reading
**FF0F (IF)**, not FF41 — so the committed FF41/`SLOPGB ff41` tracer is blind to
them. Built the matching **FF0F read tracer on both emulators**, swept the whole
DMG family dual-emulator, and pinned the mechanism. **No fix shipped** (this is
the atomic S5 engine core, all-or-nothing). Tracers byte-identical OFF.

## Tracers added (this session)

- **slopgb** `interconnect/cycle.rs::read_deferred`: `SLOPGB ff0f ly/dot/if`
  alongside the existing `ff41` block, gated `s5dbg_on()`, **NOT** gated to
  `ly<144` (the IF reads that matter land at ly143–153). Byte-identical OFF.
- **SameBoy** `Core/memory.c` `read_high_memory` `case GB_IO_IF`: `SBREAD ff0f
  ly/cfl/dc/if`, `SB_TRACE`-gated, mirroring the `case GB_IO_STAT` (`ff41`)
  patch. `/tmp/sbbuild` tester rebuilt (`make tester`).
- Run: slopgb `SLOPGB_ROWLIST=row SLOPGB_S5DBG=1 <gbtr-bin> --ignored
  flagon_probe --nocapture 2>&1 >/dev/null`. SameBoy `SB_TRACE=1
  sameboy_tester --dmg --length 2 ROM 2>&1 >/dev/null`. The slopgb probe runs
  exactly the gambatte protocol → the single non-`if=00` read **is** the
  measurement read (no rare-count isolation needed); SameBoy's `--length 2`
  loops, so its measurement read is the **count-1** `if=` value (setup frames
  repeat a different one). Sweep script left at `/tmp/sweep.sh`.

## The two ground-truth rows (decisive)

### A — `m1/lycint143_m1irq_2` [Dmg] (want=3 got=1) — MISSING re-arm
LYC=143, mode-1(vblank) STAT enabled. SameBoy IF read **ly=144 cfl=0 if=03**
(vblank bit0 + STAT bit1, count-1 measurement; setup frames read if=01). slopgb
IF read **ly=144 dot=4 if=01** (STAT bit MISSING).

- SameBoy STAT IRQ set: **ly143 cfl0 mfi=2** (mode-2 ∧ LYC=143) **and ly144 cfl0
  mfi=1** (mode-1). Two rising edges; bit1 set, never auto-cleared.
- slopgb STAT dispatch set: **ly143 dot4 only** (mfi=255/LYC). **No ly144 fire.**
- **Mechanism (airtight):** IME is on; the CPU **services** the ly143 LYC-STAT
  IRQ (vectors $48, clears IF bit1). SameBoy then **re-raises** bit1 at ly144
  cfl0 via the mode-1 line rise, so the post-service read sees bit1 again →
  if=03. slopgb produces **no ly144 mode-1 edge**, so after the service nothing
  restores bit1 → if=01. The 4-dot read offset (slopgb dot4 vs SameBoy cfl0) is
  NOT the cause — a ±4-dot read shift cannot restore a bit that was never
  re-raised a full line earlier. **Engine, not read-frame.**

### B — `lycEnable/lycwirq_trigger_ly00_stat50_2` [Dmg] (want=E0 got=E2) — SPURIOUS re-arm
STAT=0x50 (LYC int + OAM int), LYC=0. SameBoy STAT IRQ set: **ly144/151/153
mfi=1; NO ly0**. slopgb dispatch: **ly1 dot0** (spurious). SameBoy's STAT line
stays high across the ly153→ly0 LYC handoff (the internal ly=0 window during
ly153) → no fresh 0→1 edge at ly0; slopgb **re-arms** at the ly0/ly1 wrap →
spurious STAT bit → got E2.

## Full DMG family sweep (17 regr rows)

`OFF` = production verdict; `LE` = leading-edge engine verdict (the flip
switches `stat_events_tick`→`stat_update_tick`). **Every row PASSES OFF.**

| row | want/got | OFF | LE | slopgb vs SameBoy dispatch | class |
|---|---|---|---|---|---|
| `m1/lycint143_m1irq_2` | 3/1 | ✓ | ✗ | ly143 only / ly143+**ly144·m1** | **MISSING m1 re-arm** |
| `m1/lycint143_m1irq_ifw_1` | 3/1 | ✓ | ✗ | ly143 / ly143+**ly144·m1** | MISSING m1 re-arm |
| `m1/lycint143_m1irq_late_retrigger_1` | 3/1 | ✓ | ✗ | ly143 / ly143+**ly144·m1** | MISSING m1 re-arm |
| `m1/m1irq_m2enable_lyc_3` | 3/1 | ✓ | ✗ | ly0-143 / ly1-143+**ly144·m1** | MISSING m1 re-arm (+ly0 extra) |
| `lycEnable/lycwirq_trigger_ly00_stat50_1` | E0/E2 | ✓ | ✗ | **ly0+ly1** / ly144,151,153 (no ly0) | SPURIOUS wrap |
| `lycEnable/lycwirq_trigger_ly00_stat50_2` | E0/E2 | ✓ | ✗ | **ly1** / ly144,151,153 (no ly0) | SPURIOUS wrap |
| `lycEnable/lyc0_late_ff45_enable_3` | E0/E2 | ✓ | ✗ | **ly1** / ly0(·-1),150,152,153 | SPURIOUS wrap |
| `lycEnable/late_ff45_enable_3` | 1/3 | ✓ | ✗ | ly5,6,**7** / ly5,6 | SPURIOUS (extra ly7) |
| `m1/m2m1irq_ifw_2` | 1/3 | ✓ | ✗ | ly0-143 / ly1-143+ly144·m1 | SPURIOUS (extra ly0) |
| `m2enable/late_enable_m0disable_2` | 0/2 | ✓ | ✗ | ly0-143 / ly0·m2+m3,ly1·m0+m2,… | SPURIOUS (late-disable) |
| `m2enable/late_m1disable_ly0_3` | 0/2 | ✓ | ✗ | ly0-143 / ly0·m2,ly1-143·m2,150/152·m1 | SPURIOUS (late-disable) |
| `miscmstatirq/lycstatwirq…ly44_lyc44` | E0/E2 | ✓ | ✗ | ly68,69 / ly68·m0+m2,ly69·m2 | SPURIOUS/delivery |
| `miscmstatirq/lycwirq…m0_late_ly44` | E0/E2 | ✓ | ✗ | ly0-143 / ly0-143·m0+**ly68·m2** | SPURIOUS/delivery |
| `lycEnable/ff45_enable_weirdpoint_3` | 1/3 | ✓ | ✗ | ly5,6 / ly5,6 (**SAME set**) | DELIVERY (same dispatch, diff result) |
| `m1/lycint_m1intirq_1` | 3/**∅** | ✓ | ✗ | ly143 / ly143+ly144·m1 | MISSING + **blank OCR** |
| `m1/lycint_m1intirq_2` | 1/**∅** | ✓ | ✗ | ly143 / ly143+ly144·m1 | MISSING + **blank OCR** |
| `lycEnable/ff40_disable_2` | 2/0 | ✓ | **✓** | (no dispatch) / ly145,147,148·m1 | **tier2-only** (NOT engine) |

## The two engine-dispatch roots (the atomic S5 work — concrete sub-targets)

1. **MISSING m1 re-arm — the vblank-entry mode-1 line rise.** SameBoy raises a
   mode-1 STAT edge at **ly144 cfl0** (`SBTRACE STAT_IRQ ly=144 cfl=0 mfi=1`).
   slopgb's `update_mode_for_interrupt` (stat_irq.rs:805-815) sets vblank `mfi =
   vis_mode()`, and `vis_mode` (stat_irq.rs:14-15) holds **mode 0 across ly144
   dot0-3**, mode 1 only from dot4 (`display.c:2178` ~dot4). Combined with the
   LYC-143 carry holding the line high through ly143, slopgb's line never
   dips-and-rises at vblank entry the way SameBoy's does → **no ly144 edge**.
   After the CPU services the ly143 LYC-STAT, bit1 is never restored. Fix lives
   in the vblank-entry `mode_for_interrupt`/LYC-latch-drop phase (atomic engine).

2. **SPURIOUS re-arm — the ly153→ly0 LYC wrap + late-disable level.** slopgb
   fires a fresh STAT edge at the ly153→ly0/ly1 wrap where SameBoy's line was
   held high across the internal-ly=0 window (no edge), and on late FF45/m1/m2
   disable where SameBoy suppresses the already-armed source. Both are the
   `lyc_interrupt_line` wrap re-evaluation + the level-carry across source
   handoffs in the `stat_update_tick` driver.

## Verdict

The whole family is **engine-dispatch core** (16/17 fail LE-only; only
`ff40_disable_2` is tier2/read-frame). **No clean read-frame slice exists** —
re-confirms #11g with direct FF0F evidence and SHARPENS it: the family splits
into exactly two driver bugs (missing vblank-entry m1 edge; spurious wrap/
late-disable edge), both in how `stat_update_tick` is **driven**
(`mode_for_interrupt` vblank phase + `lyc_interrupt_line` wrap), NOT in the
`StatUpdate::level` OR-model (which faithfully matches `display.c:545-556`) and
NOT in the FF41/FF0F read frame. These land with the atomic reclock — touching
the vblank-entry mfi phase or the LYC wrap in isolation moves SameBoy-passing
rows. 2 rows (`lycint_m1intirq_{1,2}`) additionally render a **blank** result
under LE (got=∅) — a separate render/OCR effect to isolate next.

## #11j (2026-06-25) — mech 3 root 1 SHIPPED (the vblank-entry LYC-latch drop)

Implemented + landed flag-gated (byte-identical OFF). Root 1 ("MISSING m1
re-arm") is **not** a missing mfi edge — it is a missing LYC-latch DIP. Direct
SameBoy `SBLEVEL` (the rising/falling level engine ground truth, two transitions
logged at line-144 entry):

| ROM | SameBoy at ly144 cfl0 | VBlank en | re-arms? |
|---|---|---|---|
| `lycint143_m1irq_2` (want3) | `1->0 mfi=0 lyc_line=0 stat=d0` then `0->1 mfi=1` IF\|=2 | yes (d0 bit4=1) | YES |
| `m1irq_m2enable_lyc_1` (want1) | `1->0 mfi=0` then `0->1 mfi=1 dc=6` IF\|=2 | yes (f0 bit4=1) | YES |
| `m1irq_m2disable_lycdisable_3` (want1) | `0->1 mfi=1 dc=6` IF\|=2 | yes (91 bit4=1) | YES |
| `lyc143_late_m0enable_lycdisable_2` (want1) | `1->0 mfi=1 lyc_line=0 stat=89` **no re-rise** | **no** (89 bit4=0) | NO |

**Mechanism.** At line-144 entry SameBoy releases the held visible-line LYC match
(`lyc_line 1->0`); the STAT line dips, then the decoupled mode-1 source re-rises
(`0->1 mfi=1`) → a fresh `IF |= 2` edge, restoring the bit the CPU cleared
servicing the ly143 LYC-STAT. slopgb held the ly143 match latched across line
144's `ly_for_comparison == -1` line-start gap (`stat_update_tick`: the latch is
re-evaluated only when `ly != -1`), so the line never dipped and the natural
dot-4 mode-1 rise fused into the LYC fall → no edge → `if=01`.

**Fix** (`stat_irq.rs::stat_update_tick`, LE/Tier-2 only): at `line==144 dot==0`,
drop a held-true LYC match that no longer applies (`lyc != 144`) **iff VBlank
(mode-1) is armed** (`stat_en & STAT_SRC_VBLANK`). The VBlank gate is the
measured discriminator: with mode 1 disabled SameBoy's line dips and stays low
(no IF, last row above) — a whole-dot drop there only mis-frames the deferred
read (`lyc143_late_m0enable_lycdisable_*`, VBlank off). Never force-set a match
(LYC=144 rows re-arm via the natural dot-4 re-eval; front-running breaks
`m1irq_enable_after_lyc144_*`).

**Result** (gambatte m1/lycEnable/lycm2int/miscmstatirq/m2enable/ly0 family probe,
1092 rows, flag-on): **774→783 pass (+9): 16 fixed, 7 moved.** The 16 fixed are
the MISSING-m1-rearm rows (`lycint143_m1irq_*`, `m1irq_m2enable_lyc_{2,3}`, both
models) **including the two `lycint_m1intirq_{1,2}` BLANK-OCR rows** (the #11h
"render BLANK" loose end — they were blank precisely because the missing re-arm
left the result line empty; with the edge restored they render 3/1). The 7 moved
are all `want=1 got=3`: SameBoy fires the **same** ly144 mode-1 edge (verified for
3 of them), so the dispatch is now correct — the `got=3`-vs-`want=1` is the
deferred-read placement (mech 1 read-frame), the all-or-nothing convergence trap.
Pinned by gbtr `tier2_m1_vblank_rearm_passes` (both models). mooneye flag-on
91/91, 7→8 tier2 pins, gbtr+mooneye OFF byte-identical. **Root 2 (SPURIOUS
ly153→ly0 wrap / late-disable; the lone family target `m2m1irq_ifw_2` want1 got3
stays) + the 7 read-frame rows remain for mech 1 / mech 3 root 2.**

## #11k (2026-06-25) — mech 3 root 2 (SUB-CASE) SHIPPED: the line-0 VBlank carry

Implemented + landed flag-gated (byte-identical OFF). The first **SPURIOUS**-side
fix (#11h root 2). Root 2 splits into ≥2 sub-cases by *what held SameBoy's line
high across the wrap*; this session ships the **VBlank-overlap** sub-case.

**Ground truth (SBLEVEL/SBTRACE, DMG).** SameBoy's `stat_interrupt_line`
(`display.c:546-556`) is NOT a wired-OR — it is the SINGLE mode source selected
by `mode_for_interrupt` (case 0/1/2), OR the LYC source. And SameBoy **never
re-sets `mode_for_interrupt`** between the line-144 entry (`:2215`, `= 1`) and
line 0's `GB_SLEEP 7,1` OAM step (`:1828`, `= 2`). So with VBlank enabled the
line stays **continuously HIGH** from line 144 through vblank AND through line
0's OAM rise — the dot-4 OAM pulse joins an already-high line → **no fresh 0→1
edge on line 0**.

| ROM (DMG) | SameBoy STAT_IRQ | slopgb (before) | class |
|---|---|---|---|
| `m1/m2m1irq_ifw_2` (want1) | ly1-143 (cfl0 mfi2) + ly144·m1; **NO ly0** | spurious `ly0 dot4 mfi2` | VBlank-overlap |
| `lycEnable/lycwirq_trigger_ly00_stat50_1` (E0) | ly144/151/153 m1; **NO ly0** | spurious `ly0 dot1 mfi0` (HBlank) | VBlank-overlap |

slopgb's `update_mode_for_interrupt` read `vis_mode` (mode **0** for DMG) across
line 0 dots 0-3, dropping the line at dot 0 and re-raising it at the dot-4 OAM
pulse (or a held HBlank) → a spurious line-0 STAT edge.

**Fix** (`stat_irq.rs::update_mode_for_interrupt`, line-0 `dot < 4` arm, LE/Tier-2
only): return **1** (VBlank source), not `self.vis_mode()`. CGB already read 1
there (`vis_mode` CGB line-0 dot<4 = 1) → **DMG-only change, CGB byte-identical**.
Decoupled from the visible FF41 mode (still 0 for DMG). With VBlank disabled the
carried mode-1 source contributes nothing → line low into dot 4 → the OAM pulse
fires its real edge (matches SameBoy's vblank-off rows).

**Result** (measured flag-on, gambatte): **+4 / −0**, zero SameBoy-passing rows
dropped.
- 6-family probe (1092 rows): 783→786 (+3): `ly0/lycint152_m0irq_1`,
  `lycEnable/lycwirq_trigger_ly00_stat50_1`, `m2enable/late_m1disable_ly0_3` (all
  [Dmg]); 0 newly broken.
- Non-family DMG diff (2876 rows): +1 `lcdirq_precedence/m2irq_ly00_lcdstat30`
  [Dmg]; 0 newly broken. CGB byte-identical (no change).

Pinned `tier2_line0_vblank_carry_passes` (DMG `lycwirq_trigger_ly00_stat50_1`
outE0). mooneye flag-on 91/91; 9 tier2 pins; gbtr+mooneye OFF byte-identical;
lib 658; clippy -D clean. Unit test `mode_for_interrupt_has_no_mode2_lead_on_
line_0` re-pinned `(vis,mfi)=(0,1)` at line 0 dot 3.

**Named target `m2m1irq_ifw_2` (want1 got3): DISPATCH now correct** (spurious ly0
gone, fires ly1-143 like SameBoy) but OCR still `got=3` — the residual is the
deferred-read placement (**mech 1 read-frame**), the convergence trap, NOT root 2.

**Root 2 RESIDUALS (banked, NOT this session's lever):**
- **LYC-source / late-write sub-case** (`lycwirq_trigger_ly00_stat50_2`,
  `lyc0_late_ff45_enable_3`, both still E2). Here SameBoy's **LYC=0 source** (not
  VBlank) holds the line high across ly153→ly0→ly1cfl0; slopgb fires a spurious
  `ly1 dot0 mfi2` because the **FF45=0 write timing** vs `ly_for_comparison` at
  the line-1 carryover differs (slopgb line-1 dots 0-2 lyfc=0 matches; SameBoy's
  line-1 setup sets lyfc=-1 then 1, and the write lands differently). This is the
  write-trigger / `ly_for_comparison`-wrap lever — a separate mechanism.
- **CGB side** of the lycwirq E2 rows (byte-identical here; own residual).
- **Late-disable** (`m2enable/late_enable_m0disable_2`) — suppress an already-
  armed source on late FF45/m1/m2 disable; untouched.

## #11l (2026-06-25) — mech 3 root 2 LYC-WRITE sub-case SHIPPED: the line-start LYC-carryover hold

Implemented + landed flag-gated (byte-identical OFF, gate green, defaults NOT
flipped). The follow-on to #11k's VBlank-overlap sub-case: the OTHER spurious-wrap
sub-case, where SameBoy's **LYC source** (not VBlank) determines the wrap edge.

**Ground truth (new `SBWRITE ff45` tracer + `SBLEVEL`, DMG).** SameBoy's per-line
start sets `ly_for_comparison` in two `GB_SLEEP` steps (`display.c:1805-1830`):
state-6 (`:1811`, `= current_line ? -1 : 0`) then state-7 (`:1830`,
`= current_line`). `GB_STAT_update` (which re-evaluates `lyc_interrupt_line`,
`display.c:533-544`) runs at those two steps and on CPU register writes — but
NOT during the **held carryover** before state-6, where `ly_for_comparison`
still names the *previous* line. The late FF45=0 writes land at the state-7 step:

| ROM (DMG) | SBWRITE | SBLEVEL at the wrap | dispatch |
|---|---|---|---|
| `lyc0_late_ff45_enable_3` | `ly=1 cfl=0 lyfc=-1 val=0` | rise `ly153 cfl0` (LYC), fall `ly1 cfl0` | NO ly0/ly1 edge |
| `lycwirq_trigger_ly00_stat50_2` | `ly=0 cfl=0 lyfc=0 val=0` | held high (VBlank), fall `ly1 cfl0` | NO ly0/ly1 edge |

So the write at `lyfc=-1` (r3) latches no match, and at `lyfc=0` (r2) joins an
already-high line — **no fresh LYC edge** either way. slopgb's per-dot
`stat_update_tick` re-latched `lyc_interrupt_line` against the line-start
carryover (`ly_for_comparison = line - 1 = 0` at the ly0→ly1 wrap, dots 0-2) the
moment LYC became 0 → a spurious `ly1 dot0 mfi2` rise (`SLOPGB lvl ly=0 dot=4
1->0` then `ly=1 dot=0 0->1 lyc_line=1`) → `got=E2`, want E0.

**Fix** (`stat_irq.rs::stat_update_tick`, LE/Tier-2 only): HOLD the latch across
the line-start carryover dots (lines 1-143, `dot <= 2`) like the `-1` gap — only
re-latch at the dot-4 (`=N`) step and the `-1` hold. A legitimate LYC=N-1 *tail*
was already latched true at line N-1, so holding preserves it; only the spurious
fresh match (latch low coming in, carryover number == freshly-written LYC) is
suppressed — SameBoy's "no re-latch during the held carryover".

**DMG-family only.** On CGB the LCD-offset rows shift SameBoy's whole grid: the
offset-shifted LYC edge SameBoy raises one line *earlier*
(`late_ff45_enable_lcdoffset1_1`: SameBoy `ly6 cfl0`, slopgb `ly7 dot0`) lands on
slopgb's carryover dot as a **mis-dotted but REAL** edge, so the hold drops a STAT
SameBoy delivers (out2→0). slopgb models no `lcd_offset`, so the spurious wrap and
the offset-shifted real edge are indistinguishable here without porting the CGB
write-state / lcd-offset timing — banked (the goal scopes CGB lycwirq as its own
residual). DMG is the clean single-speed case.

**Result** (full gambatte BEFORE/AFTER, both models, two gbtr bins — fix in
`target/gbtr`, reverted in `target/lint`): with the DMG-family gate, **+4 DMG / −0
SameBoy-passing dropped**: `lyc0_late_ff45_enable_3`, `late_ff45_enable_3`,
`ff45_enable_weirdpoint_3`, `miscmstatirq/lycwirq_trigger_m0_late_ly44_4` (all
[Dmg]). (The UNGATED change was +9/−2; the 2 breaks were the CGB `lcdoffset1`
rows above — hence the gate. The 5 CGB fixes it also gave up — incl. CGB `r3`/`r2`
and 2 DS rows — are real but ride with the CGB lcd-offset port.) Pinned
`tier2_lyc_carryover_late_ff45_passes` (DMG `lyc0_late_ff45_enable_3` outE0) +
unit test `lyc_latch_holds_across_line_start_carryover_flag_on` (fails without the
hold). mooneye flag-on 91/91; 10 tier2 pins; gbtr+mooneye OFF byte-identical; lib
661; clippy -D clean.

**Named target `lycwirq_trigger_ly00_stat50_2` [Dmg] (want E0): the spurious `ly1`
edge is GONE** (carryover hold), but it still fails — its residual is a SEPARATE
mechanism: slopgb fires the VBlank STAT IRQ at `ly144 dot4`, SameBoy at `ly144
cfl0` (read-frame Δ4), and the HALT-wake/read diverges (slopgb reads `ly0 dot20
if=02`, SameBoy `ly144 cfl0 if=01`) — **mech 1 read-frame / mech 2 wake-clock**,
not the carryover lever.

**Tracer added (kept, documented in `tools/stat-irq-trace.md`):** SameBoy
`memory.c` `case GB_IO_LYC` → `SBWRITE ff45 ly/cfl/dc/val/lyfc/ds`, `SB_TRACE`-
gated. The FF45-write-timing ground truth (when + at what `ly_for_comparison` the
late LYC write lands). slopgb temp `SLOPGB_LVL` level-transition tracer in
`stat_update_tick` (reverted; recipe in the handoff).

**Remaining root 2:** the CGB lcd-offset carryover (the 5 given-up CGB fixes +
the 2 lcdoffset breaks — needs the CGB write-state/lcd_offset port);
`lycwirq_trigger_ly00_stat50_2` [Dmg] vblank-delivery residual (mech 1/2);
late-disable (`m2enable/late_enable_m0disable_2`).
