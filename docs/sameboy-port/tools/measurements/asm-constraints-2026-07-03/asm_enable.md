# m2enable / m0enable / enable_display — exact machine-cycle constraint tables

Static analysis of gambatte hwtests asm (`/tmp/sbbuild/gambatte-src/test/hwtests/`) for the 5
failing tier2 rows, cross-checked against SameBoy 1.0.2 `Core/display.c`/`Core/memory.c` and
gambatte's own `libgambatte/src/video.cpp` + `video/lyc_irq.cpp` + `video/mstat_irq.h`
(these ROMs are gambatte's, hw-calibrated; gambatte's constants define the CPU-write-cc frame
the tests were tuned in).

Conventions used below:

- 1 M-cycle = 4 dots single-speed (SS), 2 dots double-speed (DS). Line = 456 dots.
- `t+N` = N dots after the anchor, measured at the IO-access M-cycle of the instruction
  (`ldff(nn),a` writes on M3 of 3; `ldff a,(nn)` reads on M3 of 3; `ldff(c),a` writes on M2 of 2;
  `ldff a,(c)` reads on M2 of 2). All accesses share the same intra-M sub-phase, so **relative**
  M counts are exact; absolute sub-M phase carries a ±2-dot convention slack, and IRQ-anchored
  chains carry an extra `d` = NOP-grid alignment slack, `d ∈ (0, 4]` SS / `(0, 2]` DS
  (dispatch starts at the first instruction boundary at/after the IF rise).
- IRQ dispatch = 5 M. ISR entry `0x48: jp lstatint` = 4 M. So first ISR instruction begins
  at rise + d + 9 M.
- `cfl` = cycle-in-line (dots), line-local.

## Gambatte's timing model (the frame the tests are calibrated in)

Extracted constants (`video.cpp` unnamed namespace + `lyc_irq.cpp` `schedule()`):

| event | time |
|---|---|
| mode-2 STAT IRQ, lines 1–143 | previous line cfl **452** (`mode2_irq_line_cycle = 456-4`) = line start − 4 |
| mode-2 STAT IRQ, line 0 | line 153 cfl **454** (`mode2_irq_line_cycle_ly0 = 456-2`) = line-0 start − 2 |
| LYC=N (N>0) STAT IRQ | line N start − 2 (`lycReg*456 − 2`) |
| LYC=0 STAT IRQ | line 153 cfl **+6** (`(154-1)*456 + 6`) |
| mode-1 IRQ | `144*456 − 2` |
| mode-0 STAT IRQ | `predictedNextXposTime(lcd_hres+6)` = xpos 166 ≈ cfl **251–252** for SCX=0, no sprites (visible mode-0 flip / HDMA use xpos 167, 1 dot later) |

FF41-write immediate-trigger predicates (`LCD::lcdstatChange` → `statChangeTriggersStatIrq`):

- **DMG** (`statChangeTriggersStatIrqDmg`) — the DMG STAT-write bug, **value-independent**
  (ignores the written data; only `old` matters):
  - ly < 144, current line's m0 already passed or in hblank (`m0IrqTime ≥ next-LY` false path):
    fires iff `!(old & m0irqen) && !(cmpLY == LYC && (old & lycirqen))` → any FF41 write during
    hblank fires, **all the way to the LY increment**.
  - ly ≥ 144: fires iff `!(old & m1irqen) && !(cmpLY == LYC && (old & lycirqen))`.
- **CGB** (`statChangeTriggersStatIrqCgb`) — value-dependent, requires a **newly enabled** source
  (`data & ~old & sources`), with hw-measured cutoffs:
  - m0/LYC path: a fresh bit3 enable during hblank fires iff `timeToNextLy > 4 + 4*ds`
    (i.e. the m0 catch window closes **4 dots (SS) / 8 dots (DS) before line end**);
    inside that tail only a LYC-period enable fires.
  - m2 path (`statChangeTriggersM2IrqCgb`): requires `old` bit5 clear AND
    `(data & (bit5|bit3)) == bit5`; fires iff the write lands in
    `timeToNextLy ∈ (2, 4·(1+ds)]` of lines <143, or `(2, 2·(1+ds)]` of **line 153**
    (SS: empty; **DS: a 2-dot slot at cfl [452,454)** — the retroactive line-0-m2 catch).
- Scheduled-event blocking (`MStatIrqEvent`, `mstat_irq.h`): the event evaluates against
  **latched** copies `statReg_`/`lycReg_`. `statRegChange` latches the new STAT only if
  `cc + 2*cgb < nextEventTime` → **on CGB a STAT write within 2 dots of a pending m0/m1/m2
  event is invisible to that event's blocking logic; on DMG the latch is unconditional.**
  `doM2Event` blocked iff `(statReg_ & lycirqen) && (ly==0 ? 0 : ly-1) == lycReg_` (or ly==0
  && m1irqen). `doM0Event` blocked iff `(statReg_ & lycirqen) && ly == lycReg_`.

## SameBoy 1.0.2 semantics (display.c / memory.c)

- `GB_STAT_update` (display.c:523): `stat_interrupt_line = f(mode_for_interrupt: 0→bit3,
  1→bit4, 2→bit5, else false) OR (bit6 && lyc_interrupt_line)`; a 0→1 edge sets `IF |= 2`.
  LYC block: on `model ≤ CGB_C && !ds`, when `ly_for_comparison == -1` the visible bit2 clears
  but `lyc_interrupt_line` is **held** (the SS hold); on DS the whole compare is skipped while
  lyfc == −1.
- **Line start, lines 1–143** (display.c:1773–1815): cfl 3: `LY := line`, `lyfc := −1`,
  `mfi = 2` + STAT_update (**m2 IRQ 1 dot before the visible mode**); cfl 4: visible STAT=2,
  `lyfc := line` (LYC IRQ), STAT_update, then `mfi = −1` + STAT_update. **mode-2 is a 1-dot
  pulse, not a level.**
- **Line 0** (same loop, `current_line == 0`): NO mfi=2 at cfl 3 (DMG additionally forces
  visible mode 0 at cfl 3); the m2 trigger is a **zero-width mfi=2 pulse at cfl 4** inside the
  cfl-4 update pair.
- **Mode-0 entry** (display.c:2106–2122): SS: visible STAT=0 + `mfi=0` at the pos==160 break
  dot, but `GB_STAT_update` (the IRQ edge) only after `GB_SLEEP(22,1)` — **m0 IRQ 1 dot after
  the visible flip**; DS: visible+IRQ together after the sleep. mfi then **stays 0 (a level)**
  until the next line's cfl-3 `mfi=2`.
- **Line 153** (display.c:2233–2251): cfl 0 `lyfc=−1`; cfl 2 `LY=153`;
  cfl 6 (`model ≤ CGB_C`): SS also `LY=0`; `lyfc=153` + STAT_update (LYC=153 IRQ);
  cfl 8: `LY=0`, `lyfc = ds ? 153 : −1`; cfl 12: `lyfc=0` + STAT_update (**LYC=0 IRQ**);
  then sleeps to line end. (Gambatte's frame puts the same LYC=0 event at cfl 6 — a constant
  inter-model frame offset; each model is internally consistent.)
- **FF41 write** (memory.c:1560): plain `GB_STAT_update` with the new value, EXCEPT the
  "annoying edge timing case": `cgb_double_speed && display_state == 8 &&
  oam_search_index == 0 && display_cycles == 0 && (value & 0x20)` → pulse `mfi=2`,
  STAT_update, `mfi=−1`. I.e. **in DS, a bit5-enabling write landing in the very first
  machine slot of the OAM state still fires the m2 IRQ retroactively** — SameBoy's analogue of
  gambatte's line-153 `(2,4]` window. **SameBoy 1.0.2 does NOT model the DMG STAT-write bug**
  (no value-independent DMG path exists), so it cannot pass the DMG legs that depend on it
  (row 3 leg 2).
- **LCD-enable glitch line** (display.c:1676–1720): DMG-only `GB_SLEEP(display, 23, 1)`
  (**PPU starts 1 dot later on DMG**); then line 0: `lyfc=0`, STAT&3=0 (visible **fake
  mode 0**), `mfi=−1` (NO mode-2 pulse at all), OAM/VRAM unblocked, sleep `MODE2_LENGTH−4`
  = 76; +2 (oam_write_blocked); then visible mode 3 + `mfi=3` at machine dot ~78 with
  `cycles_for_line += 8` ("mode 0 is shorter on the first line 0") → the glitch line is
  8 dots short in its own accounting.

---

## ROW 1 — `m2enable/late_enable_ly0_ds_lcdoffset1_1` (cgb04c out2; sibling `_2` out0)

### SETUP
- `.data@143 c0` (CGB-only). `ldff(00)←0x30` (joypad), `IE←0`.
- **lcdoffset1 STOP dance**: `KEY1←1; stop` ×3 → SS→DS→SS→DS (ends in DS). Each speed-switch
  STOP halts the CPU for `0x20000 + 4` dots (gambatte memory.cpp:446, `intevent_unhalt`)
  while the LCD keeps running (131076 mod 456 = 204 dots of line-phase slip per STOP), and
  each switch changes the M-cycle length 4↔2 dots. Net effect of the 3-STOP dance vs the
  plain 1-STOP `late_enable_ly0_ds` pair: the CPU M-grid is offset an **odd number of dots
  ("offset1") against the LCD line phase**, so this pair probes between the plain pair's
  2-dot DS slots — sub-M (half-DS-M-cycle) resolution.
- `lwaitly_b` until LY=0x97 (151); `STAT←0x40` (LYC source only); `LYC←0x99` (=153);
  `IE←2`; `IF←0`; `ei`.

### TIMELINE (anchor = LYC=153 IF rise; gambatte: line-153 start − 2; DS, 2 dots/M)
| M (from dispatch start) | event | ≈ dots after rise |
|---|---|---|
| M0–4 | dispatch | d..d+10 |
| M5–8 | `jp lstatint` | |
| M9 | `xor a` | |
| M10–12 | `ldff(41)←0x00` (drop bit6; line falls) | ≈ rise+d+26 → line-153 cfl ~24 |
| M13–14 | `ld a,0x20` | |
| M15–224 | 210 × `nop` | |
| M225–227 | **`ldff(41)←0x20` (enable bit5)** | ≈ rise + d + 454..456 → **line-153 cfl ≈ 452–456** (leg `_2`: +2) |
| M228–230 | `ldff a,(0f)` read IF | write + 6 |

### MEASUREMENT
`A = IF & 0x07` printed. bit1 = STAT. **2** = the bit5 enable produced a mode-2 STAT IRQ for
line 0; **0** = it did not. (bit0/vblank is clean: IF was cleared at LY≈151, next vblank is
after the read.)

### LEG DIFF
`_1` write block at `.text@10d7`, `_2` at `.text@10d8` — one extra NOP ⇒ the enable write
(and the IF read) shift **+1 DS M-cycle = +2 dots**. Nothing else differs.

### CONSTRAINT
The line-0 mode-2 STAT event sits at **line-153 cfl 454 (= line-0 start − 2)** in gambatte's
frame (SameBoy: the zero-width `mfi=2` pulse at line-0 cfl 4 in its frame). With the STAT
line low (ISR cleared all sources), an FF41 write setting bit5 (bit3 clear):
- **fires IF ⇔ write-cc < 454** (the write schedules/unblocks the event), **PLUS the same-slot
  grace**: a DS write landing in the machine slot `cfl ∈ [452, 454)` — i.e. `timeToNextLy ∈
  (2,4]` — **still fires immediately** (gambatte `statChangeTriggersM2IrqCgb` ly==153 DS arm;
  SameBoy memory.c `ds && display_state==8 && oam_search_index==0 && display_cycles==0`
  re-pulse). In SS this grace window is empty.
- `_1` (want 2) = the write lands in the **last catching 2-dot slot**; `_2` (+2 dots, want 0) =
  first missing slot (event passed, no re-pulse, next m2 event is a frame away; the read comes
  6 dots later).
- The odd-phase offset from the STOP dance is load-bearing: the deadline is resolved to
  **2 dots, on the odd phase** — a whole-M (4-dot) or even-2-dot-grid model that quantizes the
  write instant or the pulse slot to the wrong side fails exactly this leg pair.

---

## ROW 2 — `m2enable/lyc0_late_m2enable_lycdisable_2` (dmg08 out2, cgb04c out0; `_1` both 2, `_3` both 0)

### SETUP
SS, `.data@143 80`. Wait LY=0x97 (151); `STAT←0x40`; `LYC←0x00`; `IE←2`; `IF←0`; `ei`.
The LYC=0 IRQ fires on **line 153** (gambatte cfl 6; SameBoy lyfc=0 at cfl 12), and the LYC=0
match then **holds through line 153's tail and all of line 0** (LY reads 0 for ~1.5 lines;
lyfc stays 0) — the STAT line stays HIGH via bit6 the whole time.

### TIMELINE (anchor = LYC=0 IF rise at 153:6; SS, 4 dots/M)
| M | event | ≈ dots after rise |
|---|---|---|
| M0–4 | dispatch | d..d+20 |
| M5–8 | `jp lstatint` | |
| M9–10 | `ld a,0x20` (ISR does NOT clear STAT — old = 0x40 stays) | |
| M11–107 | 97 × `nop` | |
| M108–110 | **`ldff(41)←0x20`** (bit6 OFF + bit5 ON in one write) | rise + d + ~440 → **line-153 cfl ≈ 446–458 across legs** (straddling 454) |
| M111–113 | `ldff a,(0f)`; then `and 07`, print | write + 12 |

### MEASUREMENT
`A = IF & 7`; **2** = a STAT IRQ was raised between the ISR entry (dispatch acked the LYC IRQ)
and the read; **0** = none.

### LEG DIFF
Write block at `.text@1063` / `@1064` / `@1065` — legs step **+1 M = +4 dots** each; read
address moves with it (fixed distance write→read).

### CONSTRAINT
One write simultaneously drops the line-holding source (bit6, LYC=0 match active) and enables
mode-2 (bit5). For IF bit1 to rise, the **line-0 m2 event at 153:454** must fire un-blocked:
- The event's blocking check uses the **latched** STAT (`MStatIrqEvent::doM2Event`:
  blocked iff latched bit6 set && latched LYC == 0). The latch takes the new 0x20 only if
  `write-cc + 2·cgb < 454`.
- **DMG: catch ⇔ write < 454** (latch unconditional). The DMG STAT-write bug does NOT help
  here (cmpLY==0==LYC && old bit6 set blocks the value-independent path) — leg `_3` (write ≥
  454) is 0 on DMG too.
- **CGB: catch ⇔ write ≤ 451** (`write + 2 < 454`). A write in `[452, 454)` lets the event
  fire but **blocked by the stale 0x40 latch** → no IF. (The ly==153 SS immediate window is
  empty, and old bit6 forbids the lycperiod path.)
- Leg map: `_1` ∈ ~(446,450] → both catch. **`_2` ∈ ~(450,454] → DMG catches (< 454), CGB
  blocked (≥ 452): the dmg2/cgb0 split IS the CGB-only 2-dot (half-M) STAT-latch margin
  before a pending m2 event** — no PPU-geometry difference needed. `_3` ≥ 454 → both miss.
- SameBoy frame equivalent: write must land before the line-0 cfl-4 pulse with the line low;
  writing in the pulse's own slot orders after it. SameBoy has no CGB −2 latch margin
  (it would get `_2` cgb wrong the other way only if its write/pulse ordering differs).

---

## ROW 3 — `m0enable/late_enable_2` (dmg08 out2, cgb04c out0; `_1` both 2, `_3` both 0)

### SETUP
SS, `.data@143 80`. Poll `STAT&3 == 2` (first mode 2 after handoff → line L, mid-frame);
`STAT←0x08` (mode-0 source ON) → the next hblank (line L) raises the m0 STAT IRQ;
`IF←0`; `IE←2`; `ei`; `c=0x0f`.

### TIMELINE (anchor = m0 IF rise of line L at T0 ≈ L cfl 251±1, SCX=0 no sprites — gambatte xpos-166 event)
| M | event | ≈ dots after rise |
|---|---|---|
| M0–4 | dispatch | |
| M5–8 | `jp lstatint` | |
| M9 | `xor a` | |
| M10–12 | `ldff(41)←0x00` (disable all; line falls) | |
| M13–14 | `ldff(c)←0` → IF=0 | rise + d + ~56 |
| M15–158 | 144 × `nop` | |
| M159–160 | `ld a,0x08` | |
| M161–163 | **`ldff(41)←0x08`** (re-enable m0) | rise + d + ~652 → **line L+1 cfl ≈ 448–460 across legs** ("late next mode0": 0–8 dots before line L+2) |
| M164–166 | `ldff a,(c)` read IF | write + 12 |

The next **scheduled** m0 event (line L+2's T0) is ~460 dots past the read — the result rides
entirely on the **immediate** FF41-write trigger.

### MEASUREMENT
`A = IF & 3`; **2** = the late bit3 enable during the ongoing hblank raised IF immediately;
**0** = it did not.

### LEG DIFF
Write block at `.text@1094` / `@1095` / `@1096` — **+1 M = +4 dots** per leg (read fixed
relative to write).

### CONSTRAINT
Enabling bit3 while the mode-0 STAT condition is active (hblank, STAT line low) raises a fresh
edge → IF — but the catch window closes **earlier on CGB than on DMG**:
- **CGB** (`statChangeTriggersM0LycOrM1StatIrqCgb`): fires iff `timeToNextLy > 4` (SS) —
  the mode-0 IRQ condition de-asserts **4 dots before the LY increment**. (SameBoy analogue:
  `mfi` leaves 0 at the *next* line's cfl 3 in its machine frame.)
- **DMG** (`statChangeTriggersStatIrqDmg`, the value-independent STAT-write bug): ANY FF41
  write during hblank fires (old bit3 clear, no LYC match: LYC=0 ≠ L+1), **right up to the LY
  increment** (while `ly == L+1`).
- Leg map (with d pinned by the wants): `_1` write ∈ (448, 451] of line L+1 → both fire.
  **`_2` ∈ (452, 455] — inside the CGB dead-tail (ttnl ≤ 4) but still ly==L+1 → DMG fires,
  CGB does not.** `_3` ∈ (456, 459] = line L+2 cfl 0–3, mode-2 zone → neither fires
  (CGB: not hblank; DMG: `time-to-next-LY > 216` path requires the LYC match, absent).
- Boundary brackets: CGB m0-enable-catch deadline = line-end − 4 (exclusive, 4-dot-resolved
  by `_1`/`_2`); DMG deadline = the line boundary itself (4-dot-resolved by `_2`/`_3`).
  slopgb needs BOTH: the CGB 4-dot early cutoff AND the DMG STAT-write bug (SameBoy 1.0.2
  models neither the bug nor the cutoff).

---

## ROW 4 — `m0enable/lycdisable_ff41_ds_1` (cgb04c out2; sibling `_2` out0)

### SETUP
DS, `.data@143 c0`. `IE←0`; joypad; `KEY1←1; stop` (SS→DS, single switch). Poll
`STAT&3 == 2` (line L); `STAT←0x08`; `IE←2`; `ei` (IF not re-cleared here — cleared later in
the slide). The m0 IRQ fires at line L's hblank (T0_L ≈ cfl 251, SCX=0).

### TIMELINE (anchor = m0 IF rise T0_L; DS, 2 dots/M)
| M | event | ≈ position |
|---|---|---|
| M0–8 | dispatch + `jp` | |
| M10–12 | `ldff a,(44)` → A=L | |
| M13 | `inc a` | |
| M14–16 | `ldff(45)←L+1` (LYC = next line) | |
| M17–21 | `ld a,0x48; ldff(41)←0x48` (LYC+m0 sources ON) | line L hblank |
| M22–26 | `ld a,0; ldff(43)←0` (SCX=0, deterministic m3 length) | |
| M27–209 | 183 × `nop` — during the slide: LYC=L+1 event at line-L cfl 454 (line L+1 start − 2) sets IF bit1; the **line stays HIGH all of line L+1** (LYC match) | |
| M210 | `xor a` | |
| M211–212 | `ldff(c)←0` → **IF=0** | ≈ T0_L + 424 + d → line L+1 cfl ~218 (safely after the LYC rise, before T0_{L+1}) |
| M213–223 | 11 × `nop` (leg `_2`: 12) | |
| M224–225 | `ld a,0x08` | |
| M226–228 | **`ldff(41)←0x08`** (bit6 OFF, bit3 stays ON) | ≈ T0_L + d + 456 = **T0_{L+1} ± 2** (leg `_2`: +2) |
| M229–247 | 19 × `nop` (leg `_2`: 18) | |
| M248–250 | `ldff a,(c)` read IF (fixed `.text@10e8` both legs) | ≈ write + 44 → well after T0_{L+1} |

### MEASUREMENT
`A = IF & 3`; **2** = line L+1's m0 event raised IF after the IF-clear; **0** = it stayed
blocked/seamless.

### LEG DIFF
`_2` has one extra NOP **before** `ld a,08` and one fewer after ⇒ only the disable-write
shifts **+1 DS M = +2 dots**; the read M-cycle is identical.

### CONSTRAINT
The write adds no new source (0x48→0x08) — no immediate trigger; everything hangs on the
**scheduled m0 event at T0_{L+1}** and its blocking latch:
- Before the write, the STAT line is HIGH via LYC (LY=L+1=LYC all line). At T0, `doM0Event`
  is blocked iff the **latched** `statReg_` still has bit6 && latched `lycReg_ == L+1`
  (wired-OR: the m0 rise joins an already-high line → no edge).
- The write un-blocks it only if it commits at least one DS M-cycle early:
  `write-cc + 2 < T0` (the CGB `statRegChange` margin).
- **`_1` (want 2): write ≤ T0 − 3 ⇒ latch = 0x08 at the event ⇒ LYC contribution gone, m0
  rise is a fresh 0→1 edge ⇒ IF|=2.**
- **`_2` (want 0): write ∈ [T0−2, T0) ⇒ the event still sees 0x48 ⇒ blocked; after the write
  the line is held by m0 instead of LYC — a seamless level swap, never an edge ⇒ IF stays 0.**
- This is the DS analogue of row 2's CGB latch margin, applied to the m0 event: the
  disable-vs-event race is resolved at **2-dot (1 DS-M) granularity**, and the "blocked"
  outcome requires modelling that a source swap arriving inside the event's own M-slot keeps
  the OLD source for the edge decision.

---

## ROW 5 — `enable_display/ly0_late_scx7_m3stat_scx1_1` (dmg08+cgb04c out87; `_2` out84)

### SETUP
SS, `.data@143 80`. `SCX←1` (initial fine scroll); wait LY=0x91 (145, vblank);
`LCDC←0x11` (LCD **off**); **`LCDC←0x91` (LCD ON — anchor t0 = the enable write access)**;
then a timed `SCX←7` write; then a timed FF41 read. No IRQs anywhere — the whole chain is a
rigid instruction stream from t0 (no `d` slack; only the constant intra-M sub-phase).

### TIMELINE (anchor t0 = LCDC-enable write access; SS)
| M after t0 | scx0 legs | scx1 legs | scx3 legs |
|---|---|---|---|
| SCX←7 write | M20/M21/M22 → t0+80 / **+84** / +88 | M21/M22 → **+84** / +88 | M21/M22 → +84 / +88 |
| FF41 read | M63 → **t0+252** | M63 → **t0+252** | M64 → **t0+256** |

Calibration rows (same family): `stat` reads FF41 at t0+8 → 0x84; `nextstat_1` at t0+68 →
0x84; `nextstat_2` at t0+72 → 0x87 ⇒ **the glitch line's visible mode-3 flip ∈ (t0+68, t0+72]**
in this (gambatte end-of-M) frame. SameBoy's internal picture: fake mode 0 (no mode 2, OAM/VRAM
open, `mfi=−1`) for 76 machine dots (+1 DMG start delay), visible mode 3 + `mfi=3` at ~78, and
`cycles_for_line += 8` (the glitch line is 8 dots short); the ~6–10 dot frame offset vs the
bracket is the usual read-convention delta.

### MEASUREMENT
`A = STAT` full 8-bit, printed as two hex digits (`swap`/`and 0f` split).
- **0x87** = `0x80 (bit7 reads 1) | 0x04 (LYC coincidence: LYC=0 and LY=0/lyfc=0 for the whole
  glitch line) | 3 (mode 3)` → the read at t0+252 landed **inside** mode 3.
- **0x84** = same but mode **0** → mode 3 had already ended.

### LEG DIFF (whole family)
- Within a pair (`_1`→`_2`): only the SCX←7 write shifts +1 M (+4 dots); the read is fixed.
- Across scx variants: initial SCX ∈ {0,1,3} and the read at +252 (scx0/scx1) or +256 (scx3).
- Wants: scx0: +80→87/87, **+84→87(DMG)/84(CGB)**, +88→84/84. scx1: +84→**87/87**, +88→84/84.
  scx3: +84→87/87, +88→84/84 (read +256).

### CONSTRAINT
Two independent laws pinned by the family:

1. **Glitch-line mode-3 exit** `E(p)` (first read-dot returning mode 0; p = effective
   fine-scroll penalty = SCX&7 at sample time):
   from `E(7) > 252` (scx1_1), `E(1) ≤ 252` (scx1_2), `E(7) > 256` (scx3_1), `E(3) ≤ 256`
   (scx3_2) ⇒ **E(0) ∈ (249, 251], i.e. E(p) ≈ t0 + 250±1 + p** — the glitch line's mode 3
   runs ~250+SCX&7 dots from enable (≈ 8 dots earlier than a normal line's exit, matching
   SameBoy's `+8` short-line accounting).
2. **SCX sample deadline** `D(scx_init)` — the last write instant whose SCX value still feeds
   the fine-scroll discard of the first tile:
   - CGB: `D(0) ∈ [80, 84)` (scx0_2 misses), `D(1) ≥ 84` (scx1_1 catches), `D(1), D(3) < 88`.
   - DMG: `D(0) ∈ [84, 88)` (scx0_2 catches, scx0_3 misses).
   - So the deadline moves **later with a larger initial fine scroll** (~+1 dot per SCX&7 unit
     — the fetcher is still discarding scx_init pixels, keeping the sample open), and **DMG's
     deadline is 1–4 dots later than CGB's** (SameBoy mechanism: the DMG-only
     `GB_SLEEP(display, 23, 1)` — the PPU starts 1 dot later on DMG, so every PPU-internal
     deadline lands 1 dot later relative to the CPU's write grid).
- **The failing row (scx1_1, want 0x87) pins the conjunction**: a SCX=7 write at t0+84 with
  initial SCX=1 must still be honored (D(1) > 84 — the +1 initial fine scroll keeps the sample
  open past the scx0-CGB deadline), the penalty must extend the exit to E(7) ≈ 257–258, and
  the t0+252 read must land inside (returns 0x87 with the coincidence bit from the enable-time
  LYC=0 match). Getting any one wrong (exit −8 accounting, deadline as a fixed dot independent
  of scx_init, or coincidence bit not set on the glitch line) flips the read to 0x84 / 0x83.

---

## Cross-row synthesis (what slopgb tier2 must represent)

1. **The m2 STAT event is a point event, not a level** (SameBoy: 1-dot mfi pulse at cfl 3/4;
   gambatte: scheduled dot 452/454-of-prev-line). Late bit5 enables catch it only *before* the
   trigger dot — plus a **2-dot DS retroactive grace slot** (rows 1, 2).
2. **The m0 STAT condition IS a level** spanning hblank, but the *enable-catch* window closes
   4 dots (SS CGB) before line end while DMG's runs to the boundary via the value-independent
   DMG STAT-write bug (row 3).
3. **Event-vs-write races on CGB resolve against a 2-dot-early latched register file**
   (`cc + 2 < event`): a STAT write inside the event's own DS M-slot is invisible to that
   event's edge/blocking decision (rows 2, 4) — the wired-OR "seamless source swap = no edge"
   outcome depends on the OLD value at the event.
4. **The LCD-enable glitch line**: no mode 2, fake mode 0 ~76 dots, mode 3 ≈ (t0+69..72,
   exit t0+250±1+SCX&7], LY=LYC=0 coincidence held, SCX fine-scroll sampled ~t0+80..88 with
   the deadline scx_init-dependent and 1+ dot later on DMG (row 5).
