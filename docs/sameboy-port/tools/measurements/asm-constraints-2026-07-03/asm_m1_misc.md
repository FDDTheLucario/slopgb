# m1 + miscmstatirq failing rows — exact machine-cycle constraint tables

Static analysis of gambatte hwtests asm (`/tmp/sbbuild/gambatte-src/test/hwtests/{m1,miscmstatirq}/`)
+ SameBoy 1.0.2 Core (`/tmp/sbbuild/SameBoy-1.0.2/Core/{display.c,memory.c,sm83_cpu.c}`).
No emulator was run.

## Conventions

- `t0` = machine instant the CPU **accepts** the first (setup) LYC STAT IRQ = start of dispatch #1.
  All tests enter via `.text@48: jp lstatint`; the CPU idles on a 1-M NOP sled, so accept latency ≤ 1 M.
  Empirically (from the leg verdicts, see §Anchor) t0 ≈ dot 2–4 of the LYC line.
- M-cycle indices are **exact** (counted from the asm byte layout); dot positions are t0-relative
  exact, absolute-in-line ±2 dots (the only free parameter is t0's dot).
- SS: 1 M = 4 dots, line = 114 M. DS: 1 M = 2 dots, line = 228 M. A `ldff(nn),a` write and a
  `ldff a,(nn)` read commit in the instruction's 3rd M; `ldff(c),a` / `ldff a,(c)` in the 2nd.
- IRQ dispatch = 5 M. The **IF acknowledge** (bit clear + vector selection) happens in dispatch M4
  (SameBoy `sm83_cpu.c:1680-1701`: IE&IF sampled at the PC-low push, `IF &= ~bit` flushed 2 T before
  M4's end). "ack" below ≈ dispatch_start + 3.5 M.
- IF upper 3 bits read 1 → miscmstatirq prints the full IF byte as 2 hex digits: `E0` = STAT IF (bit1)
  clear, `E2` = STAT IF set. m1 prints `IF & 7` (or `&3`): bit0 = VBlank IF, bit1 = STAT IF.
- "engine line" = the internal STAT interrupt line (OR of enabled sources); IF bit1 sets only on its
  0→1 edge. "mfi" = SameBoy `mode_for_interrupt` (the interrupt-view mode, decoupled from visible mode).

## SameBoy 1.0.2 reference semantics (verified in source)

### GB_STAT_update (`display.c:523-573`)
Single evaluator, edge on the latched bool:
```
line := (mfi==0 ? STAT&08 : mfi==1 ? STAT&10 : mfi==2 ? STAT&20 : false)
        || (STAT&40 && lyc_interrupt_line)
if line && !previous  →  IF |= 2
```
**LYC latch hold** (`:537-547`): the compare block runs only `if (lyfc != -1 || (model<=CGB_C && !ds))`;
inside, `lyc_interrupt_line` is cleared **only when lyfc != -1**. Net effect, all relevant models:
while `ly_for_comparison` is in the boundary-invalid gap, `lyc_interrupt_line` **holds the previous
line's match** (on CGB-C DS the whole block is skipped; on DMG/CGB-SS only the visible bit2 is
cleared). This is the load-bearing latch for rows 1, 4, 5, 6.

### Line-start ordering, mid-frame lines 1–143 (`display.c:2150-2153`, `:1786-1815`)
Relative to new-line dot 0:
- **dot 0** (end of previous line's iteration, `:2152`): `mfi := 2` — with **NO** `GB_STAT_update`
  call. The engine line keeps its latched value (e.g. HIGH from mode-0) until something re-evaluates.
- **dot 3** (`:1789-1801`): `LY := new`, `lyfc := -1` (lines ≥1), `mfi := 2` again, visible
  `STAT &= ~3`, then `GB_STAT_update` → with a mode-0-only mask (e.g. STAT=08) the engine line
  **naturally drops here**; `lyc_interrupt_line` still holds (lyfc==-1).
- **dot 4** (`:1804-1815`): visible mode := 2, `lyfc := new line` → LYC latch finally cleared
  (if LYC==old line); `mfi=2` update, then `mfi := -1` update (the OAM source is a 1-dot pulse).
- The mode-2 IRQ therefore rises 1 dot before the visible mode-2 edge; line 0 skips the dot-0/dot-3
  `mfi=2` (OAM pulse only at dot 4; `lyfc := 0` at dot 3, no invalid gap on line 0).

### Line-144 entry (`display.c:2165-2196`; line 143 is `LINES-1` so the dot-0 `mfi:=2` is skipped — mfi stays 0 from mode-0)
- dot 0: `lyfc := -1`, STAT_update (LYC latch holds → line still HIGH if it was).
- dot 2: `LY := 144`; **line-144 OAM quirk** (`:2174-2176`): `if (!line && STAT&20) IF |= 2` (direct
  IF set, no edge latch).
- dot 4: `lyfc := 144`, STAT_update → **LYC(143) latch drops here** (the #11j dip).
- dot 5 (`:2186-2196`): visible STAT := mode 1; **`IF |= 1` (VBlank)**; OAM quirk re-check;
  `mfi := 1`; STAT_update → **mode-1 STAT re-rise → `IF |= 2`**. VBlank IF raise and mode-1 STAT IF
  re-raise are the SAME event instant (one call sequence, dot 5).

### Line 153 → 0 (`display.c:2233-2256`, model ≤ CGB_C values)
dot 0: lyfc := -1 · dot 2: LY := 153 · dot 6: LY := 0 (SS only), lyfc := 153 · dot 8: LY := 0,
lyfc := -1 (SS) / stays 153 (DS) · **dot 12: lyfc := 0** → with LYC=0 the match rises here and the
latch stays true through all of line 0 (line 0 sets lyfc:=0 at dot 3, never -1). mfi stays **1**
from line-144 dot 5 all the way to **line-0 dot 4** (the state-7 OAM step; line 0 has no dot-0/dot-3
mfi=2). At line-0 dot 4: mfi 2-pulse → STAT_update (mode-1 source deasserts HERE) → mfi := -1.

### FF41 write conflict models (`sm83_cpu.c:31-70, 149-187`)
- **DMG** (`GB_CONFLICT_STAT_DMG`, `:149-166`): STAT reads as **0xFF for 1 T** → write 0xFF (runs
  STAT_update with ALL sources enabled — the classic DMG STAT-write bug: fires if the line was low
  and ANY source active), advance 1 T, write the real value. Hack: at display state 7 (HBlank→OAM
  edge) with `(STAT & 0x28)==0x08` the pulse is `~0x20` (OAM source excluded).
- **CGB single-speed** (`GB_CONFLICT_STAT_CGB`, `:168-177`): two-step: 1 T of
  `(old & 0x40) | (value & ~0x40)` (old LYC-enable kept), then the full value.
  NOTE: for source-swaps like 08→40 this makes a 1-T all-off intermediate (0x00) → SameBoy dips and
  re-rises → predicts E2 where gambatte hardware says E0 (see row 5 caveat).
- **CGB double-speed** (`GB_CONFLICT_STAT_CGB_DOUBLE`, `:179-187`): two-step: 1 T of
  `(value & ~8) | (old & 8)` (old mode-0-enable kept), then the full value. For 08→40 this is a 1-T
  **union** 0x48 (no dip) — matches hardware's no-edge swap.
- FF45 (LYC) write: CGB-SS map = `GB_CONFLICT_WRITE_CPU`, DMG + CGB-DS = `GB_CONFLICT_READ_OLD`;
  the write path (`memory.c:1455-1489`) calls GB_STAT_update immediately except around the LY-change
  display states.

### Hardware asymmetry pinned by the sibling family (not in SameBoy's literal model)
`ly143_late_m0enable_{1,2}` (ISR sets STAT:=00 early, then 00→08 at commit M#113/114 = line-144
dots ≈ −2/+2): leg1 fires **both** models (mode-0 source active late in line 143); leg2 fires
**DMG only** (out3) — cgb04c out1. ⇒ **CGB deasserts the mode-0 interrupt source at the line-144
boundary (dot ~0); DMG holds it ≈4 dots into line 144** (until the dot-4/5 events; the DMG FF-pulse
also sees mode-1 from dot 5). SameBoy's literal mfi (stays 0 until dot 5 for all models) would fire
leg2 on CGB too — a known-fragile point of the 1.0.2 model. slopgb must encode the CGB release-at-
boundary to pass these rows.

### Anchor calibration
All families place their trigger commit at M# ≈ 113-115 SS / 227-229 DS after t0 — i.e. exactly one
line after the setup-IRQ accept. Solving all leg verdicts simultaneously puts t0 at dot ~2-4 of the
LYC line and the trigger commits at the dots given per row below (leg spacing is exact: 4 dots SS,
2 dots DS; absolute placement ±2 dots).

---

## Row 1 — `m1/lyc143_late_m0enable_lycdisable_2` (dmg08+cgb04c **out1**; slopgb wrong)

**SETUP.** Wait LY=0x8D(141); STAT := **0x40** (LYC source only); LYC := **143**; IE=0x02 (STAT);
IF:=0; `ei`. LYC=143 match fires the STAT IRQ at line-143 start; the engine line stays HIGH (LYC
match) for the whole of line 143.

**TIMELINE** (t0 = dispatch #1 start ≈ line-143 dot 2-4):
| M# from t0 | event |
|---|---|
| 1-5 | dispatch #1 (consumes the LYC IF) |
| 6-9 | `jp lstatint` |
| 10-11 | `ld a,08` |
| 12-111 (leg2: ×100 NOPs) | sled |
| 112-114 | `ldff(41),a` → **STAT := 0x08** (mode-0 en, LYC DISABLED); commit in M#114 ≈ t0+456 dots ≈ **line-144 dot ~2** |
| 115-117 | `ldff a,(0f)` → read IF (commit ≈ line-144 dot ~14) |
| 118-119, 120-123 | `and a,07`; `jp lprint_a` |

Legs: _1 commit M#113 ≈ line-143 dot ~454 · _2 M#114 ≈ line-144 dot ~2 · _3 M#115 ≈ dot ~6.
DS legs: _ds_1 M#228 ≈ line-144 dot ~1, _ds_2 M#229 ≈ dot ~3 (both out1).

**MEASUREMENT.** `IF & 7`. Candidates: **1** = VBlank IF only (raised at line-144 entry; IE=02 so it
never dispatches) — the STAT write raised nothing. **3** = VBlank + a fresh STAT IF from the write
window. (Bit1's only possible source post-ISR is the write instant: with LYC disabled and STAT=0x08,
no natural mode-0 source recurs before the read.)

**LEG DIFF.** Pure trigger-address slide: 0x1065/0x1066/0x1067 (+1 M each); ds 0x10d8/0x10d9.
_1 out1/out1 · **_2 out1/out1** · _3 **dmg out3** / cgb out1.

**CONSTRAINT.** A STAT write 0x40→0x08 committing at line-144 dot D:
- D ∈ [0, ~4) (leg _2, ds legs): **must NOT set IF bit1 on either model.**
  DMG: the LYC(143) comparison latch is STILL asserted during the lyfc-invalid gap (dots 0-3) →
  the pre-write line is HIGH → neither the 1-T FF pulse nor the value can make a 0→1 edge.
  CGB: same LYC hold, and additionally the mode-0 interrupt source has ALREADY deasserted at the
  boundary (dot ~0) → the new mask 0x08 evaluates LOW → high→low, no edge.
- D ∈ [~4, …] (leg _3): DMG **must** fire (out3): the LYC latch drops at dot 4 → line LOW; the
  DMG write pulse (STAT≡FF for 1 T) sees an active source (DMG mode-0 hold until ~dot 4-5, then
  mode-1 from dot 5) → 0→1 → IF|=2. CGB must still NOT fire (mask 0x08 sees no active source: CGB
  released mode-0 at dot 0, mfi=1 from dot 5 — never an old-low/new-high overlap).
- Boundary bracket: DMG fire threshold = the LYC-latch drop, pinned ∈ (commit _2, commit _3] =
  (line-144 dot ~2, dot ~6] — SameBoy places it at exactly dot 4.
- slopgb failure mode (reads 3): either its LYC match is dropped at dot 0 (no lyfc-gap hold) while
  its interrupt-mode still reads 0 at dots 0-3 (DMG-style carry on CGB) → the freshly-enabled mode-0
  source makes old-low/new-high; or its FF41-write pulse fires on the wrong side of the drop.

---

## Row 2 — `m1/lycint143_m1irq_late_retrigger_ds_1` (cgb04c **out3**, double-speed; slopgb wrong)

**SETUP.** DS switch (`stop`); wait LY=141; STAT := **0x50** (mode-1 + LYC); LYC := **143**;
IE=0x02; IF:=0; `ei`. LYC IRQ at line-143 start. ISR: `pop af` reads the pushed return PC —
`and a,0x10` distinguishes 1st entry (return in the low sled, bit4 clear) from 2nd (return ≈0x10d1,
bit4 set → `jpnz lprint_a` prints the IF read).

**TIMELINE** (DS; t0 ≈ line-143 dot 2-4):
| M# from t0 | event |
|---|---|
| 1-5, 6-9 | dispatch #1, `jp lstatint` |
| 10-22 | `pop af`(3) `and a,10`(2) `ldff a,(0f)`(3) `jpnz`(3,nt) `ld a,02`(2) |
| 23-218 | 196 NOPs (ds_1) |
| 219-221 | `ldff(0f),a` → **IF := 0x02** (manual STAT IF; wipes any pending; VBlank IF not yet raised) — commit ≈ t0+441 dots (line-143 dot ~443) |
| 222 | `ei` |
| 223 | NOP (ei delay) |
| 224-228 | **dispatch #2** (STAT vector — IF bit1 & IE bit1); **ack (IF bit1 clear) ≈ M#227.5 ≈ t0+455 dots ≈ line-144 dot ~0-1** (ds_1); ds_2: +2 dots |
| 229-232 | `jp lstatint` (0x48) |
| 233-240 | `pop af`, `and`, `ldff a,(0f)` → **read IF ≈ line-144 dot ~20** |

**MEASUREMENT.** `IF & 7` at 2nd entry. Bit0 = VBlank IF (raised line-144 entry; ack only clears
bit1 → bit0 survives in both legs). Bit1 = STAT IF: the manual 02 was consumed by dispatch #2's ack;
bit1 at the read ⇔ the **natural line-144 mode-1 STAT re-rise landed AFTER the ack**.
**3** = re-rise after ack (survived) · **1** = re-rise at/before ack (consumed with the manual bit).

**LEG DIFF.** IF-write address only: ss 0x105b/0x105c (+1 M = 4 dots), ds 0x10ce/0x10cf (+1 M = 2
dots). _1 out3 · _2 out1 (both speeds).

**CONSTRAINT.** With STAT=0x50, LYC=143: the engine line is held HIGH through line 143 by LYC, dips
when the LYC latch drops (line-144 dot 4), and re-rises on the mode-1 source (`mfi := 1`) at
**line-144 dot 5, the same instant the VBlank IF (bit0) is set** (one event: `IF|=1`, `mfi=1`,
STAT_update → `IF|=2`; SameBoy `display.c:2186-2196`). The re-rise instant R is pinned:
**ack(ds_1) < R ≤ ack(ds_2)** — a 2-dot bracket at line-144 dot ~0-5.
ds_1 (want 3): the ack of a dispatch whose M4 ends ≈ line-144 dot 0-1 must complete BEFORE the PPU's
mode-1 IF re-raise; ds_2: one M later the ack must consume it.
slopgb failure (reads 1 presumably): its mode-1 IF re-raise lands at/before its ds_1 ack — i.e. the
raise is ≥1 DS-M too early relative to the CPU ack instant in the deferred frame (or the ack too
late). Note the dip+re-rise itself is REQUIRED (base `lycint143_m1irq_1/2`: out0→out3 in one M —
both IF bits appear within a single 4-dot window; a model where LYC-high hands over to mode-1 with
no new edge reads 1 forever and fails `_2`/`late_retrigger_1`).

---

## Row 3 — `m1/lycint_vblankirq_late_retrigger_ds_1` (cgb04c **out1**, double-speed; slopgb wrong)

**SETUP.** DS switch; wait LY=141; STAT := **0x40** (LYC only); LYC := 143; **IE=0x03**
(VBlank+STAT); IF:=0; `ei`. Same pop-af return-address discriminator. ISR writes **IF := 0x01**
(manual VBlank IF). Vector 0x40 is a NOP slide into 0x48's `jp lstatint` (+8 M on the 2nd entry,
after the ack — doesn't move the ack).

**TIMELINE.** Machine-identical to row 2 up to dispatch #2 (same trigger addresses 0x105b/0x105c,
0x10ce/0x10cf; same NOP counts): IF:=01 commit M#221 (ds), `ei`, NOP, **dispatch #2 = VBlank vector**
(IF bit0 & IE bit0; bit1 is 0 — the LYC IF was consumed at dispatch #1 and STAT=0x40 raises nothing
new until next frame). **Ack clears bit0 at ≈ M#227.5 ≈ line-144 dot ~0-1** (ds_1) / +2 dots (ds_2).
Read IF at 2nd entry ≈ +12 M later (8 NOPs + jp + pop + and: read commit ≈ line-144 dot ~35).

**MEASUREMENT.** `IF & 7`. Bit1 stays 0 (no enabled source re-rises: LYC≠144, mode-1 source not
enabled). Bit0 at the read ⇔ the **hardware VBlank IF raise V (line-144 entry) landed AFTER the
ack** (the manual bit was consumed; only a post-ack raise survives).
**1** = V after ack · **0** = V at/before ack (consumed).

**LEG DIFF.** _1 out1 / _2 out0 (both speeds); +1 M IF-write slide, identical to row 2.

**CONSTRAINT.** **V ∈ (ack(_1), ack(_2)]** — the same 2-dot (DS) bracket as row 2's R, with the SAME
leg addresses ⇒ **V and R are co-timed within 1 M at both speeds** (SameBoy: both at line-144 dot 5).
ds_1 (want 1): the VBlank IF set must land strictly after the ds_1 ack instant.
slopgb failure (reads 0 presumably): its VBlank IF raise is early (or ack late) by ≥1 DS-M relative
to hardware — same root as row 2, one shared fix: the line-144-entry IF raises (bit0 and the mode-1
bit1 re-rise) sit strictly between the two legs' dispatch-ack instants.
Same-instant rule: a CPU ack and a PPU raise in the same cycle → the ack wins (leg 2 reads 0).

---

## Row 4 — `miscmstatirq/lycstatwirq_trigger_ly00_10_50_ds_1` (cgb04c **outE0**, DS; slopgb wrong)

**SETUP.** DS switch; LYC := 0xFF; wait LY=0x97(151); STAT := **0x40**; IE=0x02; IF:=0; `ei`;
LYC := **0x99 (153)**. LYC=153 match → STAT IRQ at line-153 start (lyfc:=153 at dot ~6).
ISR fixed part: **LYC := 0** (commit ≈ entry+5 M; on line 153 lyfc becomes 0 at dot 12 → the LYC=0
match RISES mid-line-153 → with STAT still 0x40 this fires a second STAT IF — deliberately wiped
later); **STAT := 0x10** (mode-1 source only, LYC source off; commit ≈ entry+10 M). The engine line
is then HIGH via mode-1 (mfi=1 during VBlank) continuously.

**TIMELINE** (DS; t0 ≈ line-153 dot 2-4):
| M# from t0 | event |
|---|---|
| 1-9 | dispatch #1 + `jp` |
| 10-19 | `ld a,00` `ldff(45),a` (LYC:=0) `ld a,10` `ldff(41),a` (STAT:=0x10) |
| 20-219 | 200 NOPs (ds_1) |
| 220-223 | `xor a,a`; `ldff(0f),a` → **IF := 0** (wipes the LYC=0-rise STAT IF) commit ≈ line-153 dot ~450 |
| 224-225 | `ld a,50` |
| 226-228 | `ldff(41),a` → **STAT := 0x50** commit M#228 ≈ t0+455 ≈ **line-0 dot ~2-3** (ds_1); ds_2 M#229 ≈ dot ~4-5 |
| 229-230, 231-232 | `nop nop`; `ldff a,(c)` → read IF ≈ line-0 dot ~12 |

**MEASUREMENT.** Full IF byte printed as hex. **E0** = the 10→50 write raised nothing.
**E2** = the write produced a fresh STAT IF edge.

**LEG DIFF.** Trigger slide only: ss 0x105e/0x105f (E0/E2), ds 0x10d0/0x10d1 (E0/E2); +1 M each.
(lcdoffset variants shift the LCD phase — out-of-scope here.)

**CONSTRAINT.** State at the write: LY=0 (the 153-alias), LYC=0 → **lyc latch TRUE from line-153
dot 12, held through all of line 0** (line 0 has no lyfc-invalid gap). mfi = 1 (VBlank) from
line-144 until the **line-0 OAM step at dot ~4** (mfi 2-pulse → -1; SameBoy `display.c:1804-1815`,
line 0 skips the dot-0/dot-3 mfi=2). STAT=0x10 → engine line HIGH exactly while mfi==1.
- Write commits at dot < ~4 (ds_1, want **E0**): mode-1 source still asserted → line HIGH;
  0x10→0x50 only ADDS the LYC source → high→high, **no edge**. (CGB-DS write conflict step1
  `(0x50&~8)|(old&8)` = 0x50 — clean single-step.)
- Write commits at dot ≥ ~4 (ds_2, want **E2**): the machine's dot-4 update with STAT=0x10 dropped
  the line (mfi left 1); the write then enables LYC with the match held TRUE → 0→1 edge → IF|=2.
- Boundary bracket: **the mode-1 interrupt-source deassert instant ∈ (commit ds_1, commit ds_2]**,
  a 2-dot window at line-0 dot ~3-5 (SameBoy: exactly the dot-4 OAM step).
- slopgb failure (reads E2): its interrupt-mode leaves 1 (or its engine line drops) ≥2 dots before
  the hardware instant in the DS deferred frame — e.g. dropping mode-1 at line-0 dot 0 instead of
  holding it to the OAM step (the #11k line-0 VBlank-carry law, extended: the carry must persist to
  dot ~4, not just dots 0-3).

---

## Rows 5+6 — `miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_4` (dmg08+cgb04c **outE0**) and `..._ds_2` (cgb04c **outE0**, DS)

**SETUP.** (DS leg: speed switch first.) LYC := 0xFF; wait LY=0x42(66); STAT := **0x40**; IE=0x02;
IF:=0; `ei`; LYC := **0x44 (68)**. LYC IRQ at line-0x44 start. ISR fixed: **STAT := 0x08** (mode-0
source, LYC off; commit ≈ entry+5 M, during line-0x44 mode 2 — no edge: line was high via LYC, goes
low, mode-0 not yet active; the mode-0 entry (~dot 250) then raises the line HIGH via 0x08 — that
edge sets an IF bit which the later `IF := 0` wipes).

**TIMELINE** (t0 ≈ line-0x44 dot 2-4):
| M# from t0 | event |
|---|---|
| 1-9 | dispatch #1 + `jp` |
| 10-15 | `ld a,08` `ldff(41),a` (STAT:=0x08) `xor a,a` |
| 16-… | NOP sled (88-91 SS / 203-206 DS) |
| … | `ldff(c),a` → **IF := 0** (c=0F; commit 7 M before the trigger) |
| … | `ld a,40`; `ldff(41),a` → **STAT := 0x40** (mode-0 off, LYC on) — the trigger |
| +2 M | `nop nop`; `ldff a,(c)` → read IF (≈13 dots after the trigger commit) |

Trigger commit M# (≈ dot, new-line = line-0x45 frame):
| leg | M# | ≈ dot | want |
|---|---|---|---|
| _1 | 110 | 0x44:~442 (mode 0) | E0 |
| _2 | 111 | 0x44:~446 (mode 0) | E0 |
| _3 | 112 | 0x44:~450 (mode 0) | E0 |
| **_4** | 113 | **0x45:~0-2 (boundary hold window)** | **E0** |
| _ds_1 | 225 | 0x44:~452-454 (mode 0) | E0 |
| **_ds_2** | 226 | **0x45:~0-1 (hold window)** | **E0** |
| _ds_3 | 227 | **0x45:~2-3 (drop window)** | **E2** |
| _ds_4 | 228 | 0x45:~4-5 (post-LYC-clear) | E0 |

**MEASUREMENT.** IF hex: **E0** = no fresh STAT IF from the source-swap write; **E2** = the swap
produced a 0→1 edge.

**LEG DIFF.** Pure +1-M trigger slides. The `early` sister family (commit M#58-66, line-0x44 dots
~230-262, straddling the mode-3→0 flip) is E2 ×8 then E0 (_9): enabling LYC (match high all line)
while mode-0 is NOT yet active fires; once mode 0 is active (line already high via 0x08) the swap
stops firing — pinning the swap-no-edge law inside the line. `lycwirq_trigger_m0_late_ly44_*`
(LYC-restore instead of STAT-swap, same slots): all E0; `..._lyc45_*`: all E2 (the new LYC=0x45
match on line 0x45 rises after the mode-0 source drop → always an edge) — controls proving the
boundary gap exists and is source-ordering-sensitive.

**CONSTRAINT.** Around the line-0x44→0x45 boundary (LYC=0x44: match TRUE all line 0x44), three
ordered events:
1. **dot ~0**: interrupt-mode becomes 2 latently — the engine line keeps its latched HIGH (from
   mode-0) until the first re-evaluation;
2. **dot ~3**: the machine re-evaluates with the old mask 0x08 → the engine line NATURALLY DROPS
   (mode-0 source no longer active); `lyfc := -1` — **the LYC latch HOLDS its TRUE value**
   (CGB-DS: compare frozen; DMG/CGB-SS: cleared only when lyfc valid);
3. **dot ~4**: `lyfc := 0x45` → LYC(0x44) latch finally FALSE.
A STAT write 0x08→0x40 committing at dot D of line 0x45:
- D ∈ [0, ~3) (**_4**, **_ds_2** — the failing rows): pre-write engine line still latched HIGH,
  post-write line = LYC-en & held-match = HIGH → **no edge, E0 on both models**. The write must be
  atomic (no intermediate all-off value): a 1-T dip (SameBoy's CGB-SS step1 = 0x00) or a continuous
  re-evaluation that drops the line at dot 0 turns this into a spurious E2. DMG: the FF-pulse sees
  the line already high → no edge either.
- D ∈ [~3, ~4) (**_ds_3**, want E2): the natural drop has happened, the LYC latch still holds →
  the write's LYC-enable makes old-low/new-high → **edge, IF|=2**. Only the 2-dot DS grid can hit
  this ~1-dot window; the 4-dot SS legs miss it (all SS legs E0).
- D ≥ ~4 (_ds_4): both old and new masks evaluate LOW (match gone, mode-2 source not enabled) →
  no edge.
- slopgb failure (reads E2 at _4/_ds_2): its engine drops the old line before the write commits
  (mode-source deassert evaluated at dot 0, or a write-induced dip) while its LYC latch (correctly
  or incorrectly) still holds → spurious rise. The pin: **the engine-line drop for a stale mode-0
  mask happens at the dot-3 re-evaluation, not at the line boundary, and the LYC latch survives
  through the lyfc-invalid dot** — both latches must be modeled to get E0/E0/E2/E0 across the DS
  legs.
- SameBoy caveat: its CGB-SS FF41 two-step (`(old&0x40)|(value&~0x40)` = 0x00 for 08→40) dips and
  would fire E2 for the SS mode-0-active legs (_1-_3, and the base `m0_ly44_lyc44_08_40`/`bf_40`
  rows, gambatte-verified E0). The CGB-DS step (`(value&~8)|(old&8)` = 0x48 union) matches hardware.
  Do NOT port the CGB-SS intersection literally.

---

## Unified diagnosis across the 6 rows

All six pin the same small set of boundary latches, at 1-M leg resolution:

1. **LYC-latch persistence** (rows 1, 4, 5, 6): `lyc_interrupt_line` holds the previous line's match
   through the boundary lyfc-invalid gap (mid-frame: dots ~0-4; line-144: dots 0-4; line-153→0:
   match held continuously once lyfc:=0 at dot 12). It is cleared only by a VALID mismatching
   compare (mid-frame dot ~4 / line-144 dot ~4).
2. **Event-driven engine re-evaluation** (rows 4, 5, 6): the STAT engine line is a latched level
   re-computed at discrete events (boundary writes, the dot-~3 line-start update, the dot-~4
   lyfc/mfi updates) — a stale mode-source keeps the line HIGH from the line boundary until the
   dot-~3 update. Continuous per-dot recomputation fires spurious edges in [0,~3).
3. **Line-144-entry event order** (rows 1, 2, 3): LYC dip at dot ~4 → VBlank IF (bit0) AND mode-1
   STAT re-rise (bit1) at the SAME instant, dot ~5 — both pinned between two dispatch-ack instants
   1 M apart (rows 2/3), and both strictly after a write committing at dot ~2 (row 1).
4. **CGB releases the mode-0 interrupt source at the line-144 boundary; DMG holds ~4 dots**
   (row-1 sibling `ly143_late_m0enable_2`: dmg out3 / cgb out1).
5. **DMG FF41-write pulse** (row 1 leg 3): STAT≡0xFF for 1 T — fires only if the line is LOW and
   some source is active at the commit instant; never fires while the LYC hold keeps the line HIGH.
6. **Ack-vs-raise ordering** (rows 2, 3): the dispatch IF-acknowledge (M4 of the 5-M dispatch,
   ~2 T before M4's end) consumes a raise landing at/before it; a raise 1 M later survives.

Likely slopgb roots (per the S5 ladder vocabulary): rows 5/6 + 4 + 1 = the ENGINE-side line-start
hold/latch semantics (the level must be event-latched, not dot-recomputed, and the LYC hold must
span the lyfc gap — the #11k/#11l carry laws extended to the last-visible-line and VBlank-exit
boundaries); rows 2/3 = the line-144 IF-raise instant vs the deferred-frame dispatch ack (a
read/ack-frame phase, 1 DS-M).

## Source references

- asm: `/tmp/sbbuild/gambatte-src/test/hwtests/m1/lyc143_late_m0enable_lycdisable_{1,2,3,ds_1,ds_2}*.asm`,
  `ly143_late_m0enable_{1,2,ds_1,ds_2}*.asm`, `lycint143_m1irq_late_retrigger_{1,2,ds_1,ds_2}*.asm`,
  `lycint143_m1irq{_1,_2}*.asm` + `lycint143_m1irq.txt`, `lycint_vblankirq_late_retrigger_*.asm`,
  `lycint_vblankirq.txt`; `/tmp/sbbuild/gambatte-src/test/hwtests/miscmstatirq/lycstatwirq_trigger_ly00_10_50_{1,2,ds_1,ds_2}*.asm`,
  `lycstatwirq_trigger_m0_{early,late}_ly44_lyc44_08_40_*.asm`, `lycstatwirq_trigger_m0_ly44_lyc44_*.asm`,
  `lycwirq_trigger_m0_late_ly44*.asm`.
- SameBoy 1.0.2: `display.c:523-573` (GB_STAT_update + LYC hold), `:1786-1815` (line-start dots 3/4),
  `:2150-2153` (line-end mfi:=2, no update), `:2165-2196` (line-144: OAM quirk dot 2, LYC drop dot 4,
  VBlank IF + mfi=1 re-rise dot 5), `:2233-2256` (line 153→0), `memory.c:1564-1586` (FF41 write),
  `:1455-1489` (FF45 write), `sm83_cpu.c:31-70` (conflict maps), `:149-187` (STAT_DMG/CGB/CGB_DOUBLE),
  `:1662-1706` (dispatch + ack).
