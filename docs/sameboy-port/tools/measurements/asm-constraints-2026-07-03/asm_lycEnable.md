# lycEnable failing-row constraint tables (static analysis)

Sources: `/tmp/sbbuild/gambatte-src/test/hwtests/lycEnable/*.asm` + `*.txt`,
SameBoy 1.0.2 `Core/display.c` (`GB_STAT_update` :523, normal-line lyfc :1784-1815, VBlank-line lyfc :2166-2184, line-153 :2232-2256), `Core/memory.c` (FF41 write :1560, FF45 write :1455), `Core/sm83_cpu.c` (write-conflict maps :31-83, `GB_CONFLICT_STAT_DMG` :149-166, `GB_CONFLICT_STAT_CGB` :168-177, `GB_CONFLICT_STAT_CGB_DOUBLE` :180-187).

All analysis is static (instruction counting + SameBoy code reading). No emulator was run.

---

## 0. Shared machinery

### 0.1 Test skeleton

Every test in this family:

1. `LYC=0xFF` (never matches), `lwaitly_b` polls FF44 until `LY==B`.
   Loop body = `ldff a,(c)`[2M] + `cp b`[1M] + `jrnz`[3M taken] = **6 M quantum**
   (24 dots SS, 12 dots DS). The LY==B-observing FF44 read therefore lands in
   `[dot 3, dot 3+24)` of line B (LY register commits at dot 3 on render lines,
   dot 2 on VBlank lines — SameBoy display.c:1788 / :2174).
2. Arms `STAT=0x40` (bit6 = LYC source only; **no mode sources in any of these
   five rows until row 5's 0x50**), `IE=0x02`, clears IF, `ei`, then writes the
   trigger `LYC` (B+2) and enters a **1-byte nop slide** (0x00 padding from the
   end of `.text@150` to `.text@1000`) so the STAT IRQ dispatch is quantized to
   1 M = 4 dots (2 dots DS).
3. The LYC=B+2 match raises STAT IRQ; dispatch = 5 M; vector `.text@48` runs a
   fixed prologue; then a second nop slide (padding after `lstatint`) positions
   the measured write; legs differ by the `.text@` address of the measure block
   (= ±1 nop = ±1 M).
4. Measurement = `ldff a,(c)` with `c=0x0F` → **reads FF0F (IF)**, either
   masked `and a,b` (b=3 → prints 0/1/2/3) or unmasked (prints 0xE0/0xE2:
   IF bits 7-5 read as 1 = 0xE0 base; bit1 = STAT IRQ latched; bit0 = VBlank).

### 0.2 The exact-M anchor

Because the pre-slide code is whole-M and the nop slide re-quantizes, every
event is exact relative to the IRQ rise **R** (the dot where the PPU sets
IF|=2 for the trigger LYC match) up to a per-model sub-M residue
**δ_model ∈ [0,4)** dots (CPU M-grid phase vs the PPU line grid, fixed at boot).
Vector entry = R + 20 dots + δ (5 M dispatch). `ldff(nn),a` = 3 M, bus write in
M3 (leading edge T0 = M3 start + δ); `ldff(c),a` = 2 M, write in M2;
`ldff a,(c)` = 2 M, read at M2 leading edge.

**Solving all 16 SS legs of the five families simultaneously yields one
coherent assignment** (cross-checked below, no exceptions):

- **δ_dmg ≈ 2..3 dots** — dmg08 frame phase,
- **δ_cgb ≈ 6..7 dots** (i.e. δ≈2-3 **plus one whole M-cycle**) — cgb04c frame
  phase. The dmg08/cgb04c expectation splits in these tests are exactly this
  4-dot post-boot frame-alignment offset, NOT different PPU laws (SameBoy's
  lyfc schedule is byte-identical for DMG and CGB-C at single speed).
- **δ'_ds ≈ 2..3 dots** for the DS tests (post-STOP grid).

### 0.3 SameBoy STAT semantics (the laws the wants pin)

**`GB_STAT_update` (display.c:523)** — single wired line, edge-triggered:
`stat_interrupt_line = (mode_for_interrupt-selected source bit) OR (STAT.6 AND lyc_interrupt_line)`;
`IF |= 2` **only on the 0→1 edge** of that line ("STAT blocking"). Disabling a
source while the line is high just drops the line (no IF change); enabling a
source whose condition is already high **raises the line immediately → IF|=2**;
a source-to-source handoff with no gap produces **no edge → no IF**.

**LYC latch (`lyc_interrupt_line`)**: recomputed only when
`ly_for_comparison != -1` (or model≤CGB-C && !DS, where the compare runs but the
**latch is NOT cleared while lyfc==-1** — display.c:536-547). So on DMG/CGB-C
SS the previous line's LYC match is **held across the lyfc=-1 gap** at the next
line's start (STAT bit2 clears, the IRQ latch does not).

**lyfc schedule** (dots from line start):

| line type | events |
|---|---|
| render lines 1-143 (:1784-1815) | dot 3: `LY=N`, `lyfc=-1` (latch held), mfi=2 pulse; dot 4: `lyfc=N` → **match rises dot 4**, old match latch **falls dot 4** |
| VBlank lines 144-152 (:2166-2184) | dot 0: `lyfc=-1` (latch held); dot 2: `LY=N`; dot 4: `lyfc=N` → **match rises dot 4** |
| line 153, SS, model≤CGB-C (:2232-2256) | dot 0: lyfc=-1 (held); dot 2: LY=153; **dot 6: LY=0 (early wrap) + lyfc=153 → match(153) rises dot 6**; dot 8: lyfc=-1 (**latch held [8,12)**); **dot 12: lyfc=0 → match(153) falls / match(0) rises at dot 12** |
| line 153, DS, CGB-C | dot 2: LY=153; dot 6: lyfc=153 (**rise dot 6**); dot 8: LY=0, lyfc **stays 153 (no -1 gap)**; dot 12: lyfc=0 (**fall dot 12**) |
| `mode_for_interrupt` in VBlank | =1 from line-144 entry, **never touched through line 153** → the mode-1 condition is continuously high lines 144-153 |

**FF41 write conflicts (sm83_cpu.c)**:

- **DMG (`GB_CONFLICT_STAT_DMG` :149)**: at T0 the register is written **0xFF
  for one T-cycle** (all sources glitch-enabled; special-case state-7 writes
  ~0x20), then the real value at T0+1. ⇒ *any* DMG FF41 write while an
  enabled-by-FF condition is high and the line is low **raises IF** — in
  VBlank (mfi=1) a DMG FF41 write with line low **always** sets IF bit1.
- **CGB SS (`GB_CONFLICT_STAT_CGB` :168)**: at T0 writes `(old&0x40)|(value&~0x40)`
  — bits 3-5 commit at T0, **bit6 (LYC enable) holds the OLD value one extra
  T** and commits at T0+1. No FF glitch.
- **CGB DS (`GB_CONFLICT_STAT_CGB_DOUBLE` :180)**: at T0 writes
  `(value&~8)|(old&8)` — everything **including bit6 and bit4** commits at T0,
  only bit3 (mode0-enable) is held one T.

**FF45 write**: DMG `READ_OLD` (commit T0); CGB SS `WRITE_CPU` (commit T0+1);
CGB DS `READ_OLD`. memory.c:1455 additionally has display-state-14/29 hacks
(LYC writes during line-153 dots ~12-24 have side effects) — none of these five
rows writes LYC in that window.

**FF0F write**: `WRITE_CPU` (T0+1, CPU value wins over a simultaneous HW set).

---## 1. `ff41_disable_2` (dmg08_out0, cgb04c_out2)

### SETUP
`LYC=0xFF`; wait `LY==3`; `STAT=0x40` (LYC source only); `IE=0x02`; `ei`;
`LYC=5` (a=b=3, inc×2); `c=0x0F`; nop slide. Vector @48: `inc a` → `LYC=6`,
`jp lstatint`. Both models, single speed.

### TIMELINE
From wait exit (the FF44 read observing LY==3, quantized mod 6 M):
`cp b` +1, `jrnz` +2..3, `ret` +4..7, `STAT=0x40` write at +12 (M3 of
`ldff(41),a`), `IE=2` +17, `ei` +18, `LYC=5` write +24, `ld c,0f` +25..26,
slide from +27. All still inside line 3/4.

Exact frame, anchored on **R = L5+4** (LYC=5 match rise, VBlank... render-line
law dot 4; IF|=2; dispatch begins next M edge, +δ):

| event (leg 2, 90 slide nops) | M after R | dots after L5 start (+δ) |
|---|---|---|
| dispatch | +0..4 | 4..24 |
| `inc a` (a=5→6) | +5 | |
| `LYC=6` write (M3) | +8 | [36,40) — kills the match(5) line |
| `jp lstatint` | +9..12 | |
| slide (0x1000..0x1059 = 90 nops; leg1=89, leg3=91) | +13..102 | |
| `ld a,48` | +103..104 | |
| **`STAT=0x48` write (M3)** | +107 | [432,436) — line-5 HBlank |
| `xor a,a` | +108 | |
| **`IF=0` write (M2)** | +110 | [444,448) |
| **`STAT=0x00` write (M3)** | +113 | **[456,460) = line-6 dots [0,4)** |
| **IF read (M2 leading edge)** | +115 | [464,468) = line-6 dots [8,12) |

(leg 1: every row −1 M; leg 3: +1 M.)

### MEASUREMENT
FF0F & 0x03. `2` = IF.1 set = the line-6 LYC=6 rise latched before the LYC
source died. `0` = the STAT=0 disable killed bit6 before the rise. (bit0 is 0:
IF was cleared at +110 and no VBlank occurs before the read.)

### LEG DIFF
`.text@1059/105a/105b` → 89/90/91 slide nops. The **entire
0x48-write / IF-clear / disable / read tail shifts +1 M per leg**; the PPU
events stay fixed. Discriminating event: the `STAT=0x00` write's bit6-off
instant vs the **LYC=6 match rise at line-6 dot 4** (lyfc=6 commit).

### CONSTRAINT
- Rise law pinned: **match(N) raises the STAT line and IF at dot 4 of line N**
  (both models); once IF is latched a later disable must NOT clear it.
- leg2 DMG out0: bit6 dead by dot 4 — DMG disable T0 at L6 dots [2,3], FF
  glitch masks nothing (line already high via 0x48's bit3·mfi=0 through dots
  [0,3) — the momentary FF produces **no edge**, incl. across the dot-3 mfi
  0→2 pulse), value 0 commits T0+1 ≈ dot 3-4 → rise suppressed → 0.
- leg2 CGB out2: same instruction lands 4 dots later (δ_cgb=δ_dmg+4): bit6-off
  at ≈ dots 7-8 **after** the dot-4 rise → IF already latched → 2.
- Brackets: leg1 (both 0) → disable ≤ dot 4 for both models; leg3 (both 2) →
  disable > dot 4 for both. slopgb must reproduce: disable committing in
  (L6+4, …] leaves IF set; committing ≤ L6+4 yields 0 — with the CGB write
  landing one M later than DMG in the same leg.

DS siblings (`ds_1` out1 / `ds_2` out3, IF&3 with stale VBlank bit0=1): same
bracket at 2-dot granularity around L6+4; measure blocks 0x10d3/0x10d4.

---

## 2. `late_ff41_enable_2` (dmg08_out2, cgb04c_out0)

### SETUP
`LYC=0xFF`; wait `LY==3`; wait mode0 (`waitm0`: FF41&3==0); CGB palette writes
(FF68/FF69, ~26 M, no-op on DMG); `STAT=0x40`; `IF=0`; `IE=0x02`; `ei`;
`LYC=5`; slide. Vector @48: `inc a`→`LYC=6`; `xor`→**`STAT=0x00`** (disable);
`IF=0` (`ldff(c),a`, c=0F); `jp l1000`. Both models, SS.

So during line 6 the LYC=6 compare is TRUE the whole line but the source is
OFF; the test re-enables bit6 "late" and asks whether the match is still there.

### TIMELINE (anchor R = L5+4; vector = 5+14 M → l1000 at R+19 M)

| event (leg 1, 203 slide nops) | M after R | dots after L5 (+δ) |
|---|---|---|
| `LYC=6` write (M3 of vector) | +8 | [36,40) |
| `STAT=0x00` write | +12 | [52,56) |
| `IF=0` write | +14 | [60,64) |
| slide 0x1000..0x10ca (leg2 +1, leg3 +2) | +19..221 | |
| `ld a,40` | +222..223 | |
| **`STAT=0x40` write (M3)** | +226 | **[908,912) = line-6 dots [452,456)** |
| **IF read (M2)** | +228 | [916,920) |

(leg 2: +1 M → enable in [912,916) = line-7 dots [0,4); leg 3: [916,920) =
line-7 dots [4,8).)

### MEASUREMENT
FF0F & 3. `2` = enabling bit6 found the LYC=6 condition (or a glitch condition)
high → line 0→1 → IF.1. `0` = by commit time the match was gone → no edge.

### LEG DIFF
`.text@10cb/10cc/10cd` = 203/204/205 slide nops; the **enable write and the
read shift together +1 M per leg**. Discriminating event: the bit6-enable
commit vs the **fall of the line-6 LYC match**, which on hardware is **line-7
dot 4** (not dot 0!): the lyfc=-1 gap [L7+0, L7+4) *holds* the match latch
(display.c:542-546), the latch clears only at L7+4 when lyfc=7 commits.

### CONSTRAINT
- **Enable-while-condition-high must fire immediately** (leg1, both models:
  enable at L6 dots ~454-459 with match(6) high → IF).
- **The match survives into line 7 dots [0,4)** (latch hold). leg2 DMG out2:
  DMG enable T0 at L7 dots [2,3] — two sufficient mechanisms, both must not be
  broken: (a) latch-held match + bit6 commit at T0+1 ≤ dot 4; (b) the DMG FF
  glitch at T0 sees mfi=0 still held from line-6 HBlank (mfi flips to the
  mode-2 pulse only at dot 3) → rise.
- leg2 CGB out0: same write 4 dots later — bit6 commits at T0+1 ≈ L7 dots 7-8,
  after the latch cleared at dot 4 (and CGB has no FF glitch; value 0x40 has
  no mode bits) → no edge → 0.
- leg3 both out0: L7 dots [4,8)+δ — latch cleared, mfi=-1 (OAM pulse over,
  and even the DMG glitch finds no active condition) → 0. This leg **excludes**
  any model where the mode-2 condition stays high through mode 2 (it must be
  the 1-dot pulse) and any "enable always re-fires" shortcut.
- Bracket: match-fall = L7+4 exactly; enable commit < L7+4 → 2, ≥ L7+4 → 0.

---

## 3. `lyc0_ff41_disable_2` (dmg08+cgb04c outE2)

### SETUP
`LYC=0xFF`; wait `LY==0x96` (150, VBlank); `STAT=0x40`; `IE=0x02`; `IF=0`;
`ei`; `LYC=0x98` (152); slide. Vector @48: `jp lstatint`. lstatint:
`xor` → **`LYC=0`**; slide; measure. Both models, SS.

The probe: line 153's **LYC=0 match at dot 12** (lyfc=0 commit — the LY 153→0
early wrap) vs a `STAT=0x00` disable.

### TIMELINE (anchor R = L152+4, the LYC=152 rise; vector = 5+4 M)

| event (leg 1, 98 slide nops) | M after R | dots after L152 (+δ) |
|---|---|---|
| `jp lstatint` | +5..8 | |
| `LYC=0` write (M3) | +12 | [52,56) — drops match(152), line falls |
| slide 0x1003..0x1064 (leg2: 99) | +13..110 | |
| `xor a,a` | +111 | |
| **`STAT=0x00` write (M3)** | +114 | **[460,464) = line-153 dots [4,8)** |
| 5 nops (leg2: 4) | +115..119 | |
| **IF read (M2)** | +121 | [488,492) = L153 dots [32,36) — **same dot both legs** |

(leg 2: disable at +115 → L153 dots [8,12); the tail nop count compensates so
the read is fixed.)

### MEASUREMENT
FF0F unmasked. `E2` = STAT IRQ latched (bits7-5 float high, bit0 VBlank was
cleared back on line 150 and again never set). `E0` = never raised.

### LEG DIFF
Slide 98→99 nops AND tail 5→4 nops: **only the disable write moves +1 M; the
read dot is identical**. Discriminating event: bit6-off commit vs the
**LYC=0 rise at line-153 dot 12**.

### CONSTRAINT
- CGB (no glitch): leg1 disable T0 ≈ L153 dots 10-11, bit6 dead by dot 11-12 ≤
  rise 12 → **E0**; leg2 T0 ≈ dots 14-15 → bit6 alive at dot 12 → rise fires,
  IF latched, later commit can't clear it → **E2** (the wanted value).
  Bracket: the CGB LYC=0-on-line-153 IF-set instant ∈ (leg1 commit, leg2
  commit] = **exactly the lyfc 153→0 handoff at dot 12**.
- DMG: **E2 in BOTH legs — this is the DMG STAT-write FF glitch, not the
  rise**: the disable write's momentary STAT=0xFF at T0 (L153 dots 6-7 / 10-11)
  enables bit4 while mfi=1 (VBlank) with the line low → 0→1 edge → IF at the
  write itself. A model without the DMG FF41-write glitch reads E0 here and
  fails the dmg08 leg. (The genuine rise at dot 12 is *after* both DMG disable
  positions — the glitch is the only path to E2.)
- Also pinned en passant: LY=0/lyfc=0 from dot 12 stays matched through line 0
  (line-0 law: lyfc=0 at dot 3, no -1 gap) → no second edge; IF read at dot ~34
  is stable.

DS siblings (`ds_1` E0 / `ds_2` E2, measure 0x10d9/0x10da, separate IF-clear at
0x10c0): same dot-12 bracket at 2-dot granularity, no glitch (CGB).

---

## 4. `lyc153_late_ff41_enable_2` (dmg08_outE2, cgb04c_outE0)

### SETUP
`LYC=0xFF`; wait `LY==0x96`; palette writes; `STAT=0x40`; `IE=0x02`; `IF=0`;
`ei`; `LYC=0x98` (152); slide. Vector: `jp lstatint`. lstatint:
**`STAT=0x00`** (disable, line falls); **`LYC=0x99`** (153); **`IF=0`**;
slide; measure = re-enable `STAT=0x40`, fixed-position IF read. Both models, SS.

The probe: the **LYC=153 match window on line 153** = dots **[6,12)** —
lyfc=153 commits at dot 6 (simultaneous with the LY 153→0 early wrap), the
latch is *held* across the lyfc=-1 gap [8,12), and dies at dot 12 (lyfc=0).

### TIMELINE (anchor R = L152+4)

| event (leg 1, 88 slide nops) | M after R | dots after L152 (+δ) |
|---|---|---|
| `STAT=0x00` write | +13 | [56,60) — line falls (match(152) was high) |
| `LYC=153` write | +18 | [76,80) |
| `IF=0` write | +21 | [88,92) |
| slide 0x100a..0x1061 (leg2: 89) | +22..109 | |
| `ld a,40` | +110..111 | |
| **`STAT=0x40` write (M3)** | +114 | **[460,464) = line-153 dots [4,8)** |
| 10 nops (leg2: 9) | +115..124 | |
| **IF read (M2)** | +126 | [508,512) = L153 dots [52,56) — same both legs |

(leg 2: enable at +115 → L153 dots [8,12).)

### MEASUREMENT
FF0F unmasked; E2 = bit1 latched, E0 = not. bit0 clear (IF=0 at +21, no
VBlank-set before the read).

### LEG DIFF
Slide 88→89, tail 10→9: **only the bit6-enable moves +1 M; read fixed**.
Discriminating event: bit6-enable commit vs the **end of the lyc==153 window
at dot 12**.

### CONSTRAINT
- CGB leg1 E2: enable commit (bit6 at T0+1, T0 ≈ dots 10-11) lands **inside
  the lyfc=-1 hold window [8,12)** where the latch still remembers the dot-6
  match → enabling bit6 against the held latch raises the line → IF. This leg
  REQUIRES the latch-hold across [8,12): a model that drops the match when
  lyfc goes -1 (or that ends the 153 window at dot 8) reads E0 and fails.
- CGB leg2 E0 (the wanted value slopgb misses): enable commit ≈ dots 14-15 —
  at dot 12 lyfc=0 ≠ 153 cleared the latch → **enabling bit6 after dot 12
  must NOT fire** (no condition high; no edge) → E0.
- DMG both legs E2: T0 ≈ dots 6-7 (leg1) / 10-11 (leg2); the FF-glitch at T0
  sees mfi=1 (VBlank) with the line low → immediate rise → IF, independent of
  the LYC state; additionally leg1/leg2 commits ≤ dot 11 also hit the live/held
  match. dmg08 E2 is thus doubly protected — but only the glitch covers a
  δ_dmg=3 landing (commit exactly at 12).
- Bracket: L153 lyc-153 window = [6, 12) dots; enable inside → E2, at/after 12
  → E0; the -1 gap [8,12) counts as *inside* (latch hold).

lcdoffset1 / ds / ds_lcdoffset1 siblings shift the same bracket by the LCD
offset and the 2-dot DS grid.

---

## 5. `lyc153_m1disable_ds_2` (cgb04c_outE0) [double speed]

### SETUP
CGB only (`data@143=c0`): `IE=0`, joypad `0x30`, `KEY1=1`, `stop` → **double
speed**. `LYC=0xFF`; wait `LY==0x96`; palette writes; `STAT=0x40`; `IE=0x02`;
`IF=0`; `ei`; `LYC=0x98`; slide. Vector: `jp lstatint`. lstatint:
**`STAT=0x50`** (bit6 LYC **kept** + bit4 mode1 **added**); **`LYC=0x99`**
(153); **`IF=0`**; slide; measure = **`STAT=0x40`** ("m1disable": drop bit4,
keep bit6), fixed IF read.

Design: from the 0x50 write on, the STAT line is held HIGH continuously by the
mode-1 condition (mfi=1 for all of lines 144-153). The LYC 152→153 rewrite
drops the LYC contribution without a dip (bit4 covers it), and the LYC=153
match rise at line-153 dot 6 is **blocked** (line already high). The only way
IF can ever be set again is if the bit4-disable lands **before** dot 6,
creating a dip (both contributions low) followed by a fresh 0→1 edge at dot 6.

### TIMELINE (DS: 1 M = 2 dots; anchor R = L152+4; dispatch 5 M = 10 dots)

| event (ds_1, 201 slide nops) | M after R | dots after L152 (+δ'_ds) |
|---|---|---|
| `jp lstatint` | +5..8 | |
| `STAT=0x50` write (M3) | +13 | [30,32) — line stays high (match152 ∨ mode1) |
| `LYC=153` write | +18 | [40,42) — LYC leg falls, bit4 holds line high, **no dip** |
| `IF=0` write | +21 | [46,48) |
| slide 0x100a..0x10d2 (ds_2: 202) | +22..222 | |
| `ld a,40` | +223..224 | |
| **`STAT=0x40` write (M3)** | +227 | **[458,460) = line-153 dots [2,4)** |
| 15 nops (ds_2: 14) | +228..242 | |
| **IF read (M2)** | +244 | [492,494) = L153 dots [36,38) — same both legs |

(ds_2: bit4-off at +228 → L153 dots [4,6); with δ'_ds ≈ 2-3 the commits land at
dots ≈[4,6) / ≈[6,8) respectively.)

DS FF41 conflict (`STAT_CGB_DOUBLE`): bit4 AND bit6 commit at **T0** (only
bit3 is held a T) — so the m1disable is effective at the M3 leading edge.

### MEASUREMENT
FF0F unmasked. `E2` = a dip-then-rise happened (bit4 dropped before the dot-6
match rise → fresh edge → IF.1). `E0` = the line never dipped: bit4 was still
high when lyfc=153 set the latch at dot 6, the LYC contribution seamlessly took
over (mode1→LYC handoff, no edge), and the eventual fall at dot 12
(lyfc=0 ≠ 153) is a 1→0 edge — no IF. bit0 clear (IF=0 at +21; read precedes
the next VBlank-set by a frame).

### LEG DIFF
Slide 201→202 nops, tail 15→14: **only the `STAT 0x50→0x40` write moves
+1 M (+2 dots); read fixed**. Discriminating event: bit4-off commit vs the
**lyfc=153 latch rise at line-153 dot 6 (DS: no -1 gap; window [6,12))**.

### CONSTRAINT
- ds_1 E2: bit4-off commit < L153+6 → line low during [commit, 6) → the dot-6
  LYC=153 rise is an edge → IF|=2.
- ds_2 E0 (the wanted value): bit4-off commit ≥ L153+6 → at dot 6 the PPU's
  own STAT_update sees the line already high (bit4 still set) → **rise
  blocked**; after the commit the line stays high via bit6·latch → no edge; at
  dot 12 the latch clears (lyfc=0) → falling edge only → IF stays 0.
- Pins, in one row: (a) the DS line-153 lyfc schedule (lyfc=153 at dot 6 with
  LY still reading 153 until dot 8, lyfc=0 at dot 12); (b) single-wired-line
  blocking across a mode1→LYC source handoff (no per-source edges); (c) the DS
  STAT-write conflict committing bit4 at T0 (a model holding all bits a T, or
  committing at cc+4, moves the commit past dot 6 in ds_1 / short of it in
  ds_2 and flips a leg).
- SS siblings `lyc153_late_m1disable_{1,2,3}` (measure 0x1060/61/62, 12/11/10
  tail nops, commits at L152+[452,456) / L153+[0,4) / L153+[4,8) + δ): leg1 E2
  both; leg2 DMG E2 (commit ≈ dots 2-3 < 6; DMG FF glitch is masked here —
  line already high via bit4 through T0, FF holds it high, so the *genuine*
  dip decides)、CGB E0 (commit ≈ dots 6-7 ≥ 6); leg3 both E0 (DMG commit ≈
  dots 6-7 ≥ 6). Same dot-6 boundary, δ_cgb = δ_dmg + 4.

---

## 6. Cross-check summary (the one consistent model)

With rise/fall laws {match rises dot 4 of its line (dot 6 on line 153), latch
held across every SS lyfc=-1 gap, lyc-153 window [6,12), LYC=0-wrap rise dot
12}, SameBoy's write conflicts {DMG FF-glitch @T0, CGB SS bit6 @T0+1, CGB DS
bit4/bit6 @T0}, and frame phases {δ_dmg≈2-3, δ_cgb≈6-7 = δ_dmg+1 M,
δ'_ds≈2-3}, **all 22 legs read across the five families reproduce** (rows
1-5 plus every `_1`/`_3`/`_ds` sibling checked above). No leg needs a second
mechanism beyond: single-line edge IF, latch-hold, the lyfc schedule, and the
per-model write-conflict table.

What each failing row most likely needs from slopgb's tier2 read/dispatch frame:

| row | law under test |
|---|---|
| 1 | disable-after-rise keeps IF; CGB leg lands 1 M later than DMG in the same leg (frame phase, not a law) |
| 2 | LYC match latch survives to next-line dot 4 (lyfc=-1 hold); enable-into-held-match fires; enable at ≥ dot 4 must not |
| 3 | DMG FF41-write FF-glitch (mode1 live in VBlank) sets IF on *any* STAT write with line low; CGB clean disable-vs-dot-12 race |
| 4 | line-153 lyc==153 window ends at dot 12 exactly; enable inside [8,12) -1-gap must fire (latch hold), at ≥12 must not |
| 5 | DS: mode1→LYC handoff with zero dip = no edge; bit4 commit at T0 vs the dot-6 latch rise |
