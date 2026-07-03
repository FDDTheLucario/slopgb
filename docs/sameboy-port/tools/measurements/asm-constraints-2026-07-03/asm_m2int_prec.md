# m2int / irq_precedence failing-row constraint tables (static asm analysis)

Sources: `/tmp/sbbuild/gambatte-src/test/hwtests/{m2int_m0irq,m2int_m2stat,m2int_m2irq,irq_precedence,lcdirq_precedence}/`
and `/tmp/sbbuild/SameBoy-1.0.2/Core/{display.c,sm83_cpu.c}`. STATIC ONLY — no emulator runs.

Note: rows 7/8 (`late_m0irq_retrigger*`) exist **only in `irq_precedence/`**; `lcdirq_precedence/` was
checked and contains no retrigger tests (its m0irq/m2irq files are unrelated single-shot mode-read tests).

---

## 0. Conventions and shared harness

### 0.1 Counting convention (used for every table below)

- **M0** = first M-cycle of an IRQ dispatch (the instruction boundary where IE&IF was sampled set).
- **Dispatch = 5 dead M-cycles M0–M4** (crib / gbctr model: aborted fetch, internal, push-hi, push-lo,
  vector-set). The handler's first instruction (the `jp lstatint` at 0x48) occupies **M5–M8**; `lstatint`
  code starts at **M9**. (SameBoy's code is 4 dead M + the vector fetch as the 5th — same total; its
  handler offsets are −1 M against this convention. The gambatte tests calibrate to the 5-dead-M frame:
  verified below against the passing `m2int_m2irq` base pairs.)
- **IF-acknowledge (ack)** = the instant the dispatch re-samples IF, picks the vector and **clears the IF
  bit**. SameBoy (`sm83_cpu.c:1680-1701`): re-sample **after the PC-high push**, clear executed at
  `pending_cycles−2` → **≈ dispatch M3.5** (M3 leading edge + 2 T). Under the 5-M frame I place it at
  **dispatch start + 14 T (SS) / +7 dots-worth (DS ≈ M3.5)**; the retrigger rows *measure exactly this
  instant*, so it is carried as `ack = disp2_start + A`, A ≈ 3.5 M.
- **IO reads commit at the read M-cycle's leading edge** (SameBoy deferred-commit, `cycle_read`:
  `advance(pending); read`). **IO writes likewise at the write M-cycle's leading edge**, EXCEPT
  FF0F/IF which is `GB_CONFLICT_WRITE_CPU` (`sm83_cpu.c:33/46/57`): `advance(pending+1); write` —
  the IF write lands **+1 T after** the normal slot and **the CPU value overwrites a same-instant PPU
  set** (this is what makes the `ifw _2` legs read 0 permanently).
- Dots: SS 1 M = 4 dots, DS 1 M = 2 dots. `D0` = dot-on-line of dispatch M0's leading edge
  (line start = dot 0). Quantization: the pre-IRQ code is a 1-M nop slide, so
  `D0 ∈ [rise_dot, rise_dot + 4)` SS / `[rise, rise+2)` DS.
- Timing crib per instruction confirmed against opcode byte counts in the asm (gaps between `.text@`
  segments are zero-filled = nop slides; `ldff a,(c)`=F2 1B/2M, `ldff(c),a`=E2 1B/2M,
  `ldff a,(nn)`=F0 2B/3M, `ldff(nn),a`=E0 2B/3M, `pop`=3M, `jpnz` nt=3M/t=4M).

### 0.2 Shared harness (all 8 rows)

```
setup:  [IE=0] [P1=30] [KEY1=1, stop → double-speed]        ; DS rows only
        c=41, b=03
waitm3: ldff a,(c); and b; cmp b; jrnz     ; poll FF41 until STAT&3==3 (mode 3, line L-1)
        STAT := 20 (mode-2 source only)     ; written during mode 3 → STAT line low, no edge
        [IF := 0]                           ; only in ifw / retrigger variants
        IE := 02, ei
        <huge nop slide 0x017x..0x0FFF>
        ; mode-2 STAT IRQ rises at the start of the NEXT line L → dispatch #1 → 0x48: jp lstatint
```

The test line L is boot-phase-determined (some line in 1..143); all such lines are equivalent for the
events measured. **D0 differs per family** (different pre-slide instruction counts shift the nop grid),
but is pinned per family by the passing sibling legs (done below).

### 0.3 SameBoy ground truth (display.c / sm83_cpu.c)

- **Mode-2 IRQ is a 1-dot PULSE at line start.** `display.c:1784-1814` (lines 1–143): at cfl 3, LY is
  already set (cfl 3), `mode_for_interrupt=2` + visible `STAT&=~3` → `GB_STAT_update` → **IF|=2 edge at
  cfl 3, one T-cycle before the visible mode-2 flip at cfl 4** (comment at 1792). Immediately after the
  cfl-4 update: `mode_for_interrupt = -1; GB_STAT_update()` (1813-1814) → **the mode-2 contribution to
  the STAT line drops again at cfl 4**. Consequence: with STAT=0x20 only, the STAT line is a ~1-dot pulse
  per line ⇒ **every line start is a fresh 0→1 edge — a previously consumed mode-2 IF bit is ALWAYS
  re-raised by the next line's pulse** (no level-blocking across lines). This is the crux of row 6.
- **Mode-0:** `display.c:2104-2122`. SS: visible `STAT&=~3` + `mode_for_interrupt=0` happen one cycle
  *before* the `GB_STAT_update` call ⇒ **SS mode-0 IF edge = visible flip + 1 dot**. DS: the early
  (`!cgb_double_speed`) block is skipped ⇒ **DS mode-0 IF edge is co-instant with the visible flip**.
  With STAT=0x08 the line then stays HIGH until the next line's cfl-3 (`mfi`→2) ⇒ exactly one edge per
  line at the mode-3 exit; **an IF-clear while the line is high is permanent for that line**
  (`GB_STAT_update` raises IF only on 0→1: display.c:567-573). This is the crux of rows 2/3/7/8.
- **Mode-3 visible entry:** `display.c:1838-1850`: after the 40×2-dot OAM loop starting at cfl 4,
  `STAT|=3` + `mfi=3` at **cfl 84** (= visible mode-2 start + 80). SCX-independent (rows 4/5).
- **Ack instant:** `sm83_cpu.c:1662-1701` as in §0.1. An IF bit rising **during dispatch M0–M3.5** is
  merged into the sample and consumed; a bit rising **after M3.5** survives (stays pending).
- **IF CPU-write conflict:** `GB_CONFLICT_WRITE_CPU` (`sm83_cpu.c:143-147`): machine advanced
  `pending+1` T, then the CPU value written ⇒ CPU write beats a co-instant PPU IF set.

### 0.4 Frame calibration from the PASSING sibling anchors

- `m2int_m2irq_ds_1/_2` (out1/out3, both pass): ISR = pure nop slide, read FF0F&3 at 0x10d9/0x10da
  → read M227/M228 → dots 454+D0 / 456+D0. Bracket: next-line mode-2 IF rise ∈ (454+D0, 456+D0].
  With the SameBoy pulse at line-relative dot ~456+3: **D0 ≈ 3–4 (DS)** — dispatch #1 starts ≈ 1 M
  after the cfl-3 pulse. bit0 = 1 in both outs (stale VBlank; IF never cleared, IE=02 only) ✓.
- `m2int_m2irq_1/_2` (SS, out0/out2): read M113/M114 → dots 452+D0 / 456+D0; pulse at 459 → **D0 ≈ 4–6
  (SS)**. (IF cleared in setup here, hence bit0=0 ✓.)

These anchors fix each family's D0 to within ~1 dot; residual ±1 M ambiguity between the gbctr and
SameBoy dispatch conventions is absorbed into D0 and cancels in every leg-relative bracket.

---

## ROW 1 — `m2int_m0irq/m2int_m0irq_ds_2` (cgb04c, want **out3**; sibling `_ds_1` out1) [DS]

**SETUP:** SCX=0 (never written), LYC=0 (unused; source off), STAT=0x20→0x08 (in ISR), IE=0x02,
IF **not** cleared (bit0 = stale VBlank = 1 in both legs), double speed.

**TIMELINE** (M0 = dispatch #1 start, ≈ line-L dot D0 ≈ 3–4):

| M (from disp#1) | dots (2/M) | event |
|---|---|---|
| M0–M4 | D0+0..9 | dispatch (ack ~M3.5; consumes the mode-2 pulse bit) |
| M5–M8 | | `jp lstatint` |
| M9–M10 | | `ld a,08` |
| M11–M13 | write M13 → **26+D0** | `ldff(41),a` → **STAT:=0x08** during OAM scan. Mode-2 source off, mode-0 source armed; STAT line low (mode≠0) — no edge |
| M14–M124 | | 111 nops |
| `_1`: M125–M126, read at **M126 → dot 252+D0** | | `ldff a,(c)` c=0F → **IF read** |
| `_2`: read at **M127 → dot 254+D0** | | (slide `.text@1073` vs `.text@1074`, +1 byte = +1 M) |

**MEASUREMENT:** A = IF & 0x03. bit0 ≡ 1 (stale). bit1 = "has the mode-0 STAT line (STAT=08) risen —
i.e. has mode 3 exited — by the read instant". out1 = not yet; out3 = yes.

**LEG DIFF:** `_2` reads exactly 1 M (2 dots) later. Nothing else differs.

**CONSTRAINT:** the DS mode-0 IF rise `E0(scx0)` on line L satisfies
**E0 ∈ (252+D0, 254+D0]** ≈ (255, 258] with D0=3–4.
Joint pin with the passing `scx5_ds` pair (SCX=5 written post-loop → same D0; reads +3 M at
`.text@1076/1077` → E0+5 ∈ (258+D0, 260+D0] → E0 ∈ (253+D0, 255+D0]):
**E0(scx0) = 254+D0 exactly (≈ 257-258), and E0(scx5) = 259+D0** — the pair pins the DS mode-0 IF edge
to single-dot precision *and* its parity on the CPU's 2-dot DS read grid.
SameBoy semantics: DS mode-0 IF edge is **co-instant with the visible mode-0 flip** (display.c:2116-2122).
slopgb tier2 failure (reads 1 on `_2`): its deferred FF0F read at the 254+D0 slot samples before its
mode-0 rise — edge/read relative frame ≥1 dot late (bonus: `_1` passing bounds the error to exactly the
straddling dot).

---

## ROWS 2+3 — `m2int_m0irq_scx{3,4}_ifw_ds_2` (cgb04c, want **out0**; `_1` siblings out2) [DS, read FF0F]

**SETUP:** SCX=3 (row 2) / 4 (row 3), written right after `stop` (before the wait loop). STAT=0x20→0x08
in ISR, IE=0x02, **IF cleared before `ei`** (bit0 = 0), DS.

**TIMELINE** (ISR: `ld a,08; ldff(41),a; xor a,a; <nops>; ldff(0f),a; ldff a,(0f); and a,07; jp`):

| event | scx3 `_1` | scx3 `_2` | scx4 `_1` | scx4 `_2` |
|---|---|---|---|---|
| STAT:=08 | M13 (dot 26+D0) | same | same | same |
| **IF := 0 write** (`ldff(0f),a`, WRITE_CPU: leading edge **+1 T**) | **M127 → 254+D0(+1T)** | **M128 → 256+D0(+1T)** | **M128 → 256+D0(+1T)** | **M129 → 258+D0(+1T)** |
| **IF read** (`ldff a,(0f)`) | M130 → 260+D0 | M131 → 262+D0 | M131 → 262+D0 | M132 → 264+D0 |

(slides: scx3 `.text@1073/1074`, scx4 `.text@1074/1075` — scx4 is +1 M vs scx3 for a +1-dot exit shift.)

**MEASUREMENT:** A = IF & 0x07, read 3 M after the ISR's own IF:=0 write.
out2 = the mode-0 STAT edge landed **after** the IF-write instant (write cleared nothing that mattered;
the edge then set bit1; read sees 2). out0 = the edge landed **at/before** the write instant → the write
clears the just-raised bit1, and because the STAT line is now held HIGH through end-of-line (STAT=08,
mode 0 persists; edge-triggered IF, display.c:567) **no second edge can re-raise it** → read sees 0.

**LEG DIFF:** `_2` shifts write+read by exactly 1 M (2 dots). scx4 vs scx3: slide +1 M (+2 dots) against
an exit shift of +1 dot — deliberately flipping the sub-M parity of edge-vs-write-grid.

**CONSTRAINT:** DS mode-0 IF edge `E(scx)` vs the CPU IF-write instant `W` (leading edge +1 T):
- row 2: **E(scx3) ∈ (254+D0+1T, 256+D0+1T]** ≈ (257.5, 260.5]
- row 3: **E(scx4) ∈ (256+D0+1T, 258+D0+1T]**, and E(scx4) = E(scx3)+1 → jointly
  **E(scx3) = 256+D0(+ε), E(scx4) = 257+D0(+ε)** — single-dot pins on opposite parities of the DS
  2-dot write grid. Consistent with row 1's family (E(scx0)=254+D0, fine-scroll +SCX&7).
- Additional hw law pinned: **co-instant CPU-IF-write vs PPU-IF-set resolves to the CPU write**
  (`GB_CONFLICT_WRITE_CPU`), and **no re-raise while the mode-0 line stays high**.
slopgb tier2 failure (reads 2 on `_2`): its edge fires after its write slot (edge too late / write too
early), or the co-instant write-vs-set conflict resolves PPU-first, or it re-raises IF level-wise after
the clear. The passing `_1` leg brackets the error to the 2-dot straddle.

---

## ROW 4 — `m2int_m2stat/m2int_m2stat_ds_2` (cgb04c, want **out3**; `_ds_1` out2) [DS]

**SETUP:** SCX=0, STAT=0x20 (never rewritten), IE=0x02, IF not cleared, DS. ISR = pure nop slide, read
**FF41** (c=41) & 0x03.

**TIMELINE:** dispatch M0–M4, `jp` M5–M8, nops M9..M38 (30), `ldff a,(c)` M39–M40:
- `_1` read at **M40 leading → dot 80+D0** (`.text@101e`)
- `_2` read at **M41 leading → dot 82+D0** (`.text@101f`)

**MEASUREMENT:** A = STAT & 3 = CPU-visible PPU mode. out2 = still mode 2; out3 = mode 3.

**LEG DIFF:** `_2` reads exactly 1 M (2 dots) later.

**CONSTRAINT:** the CPU-visible mode 2→3 flip `V` satisfies **V ∈ (80+D0, 82+D0]** ≈ **(83, 86]** with
D0=3–4. SameBoy: V = cfl 84 (`display.c:1838-1842`, visible mode-2 start cfl 4 + 80 dots of OAM scan),
i.e. **the flip sits 1 M (2 dots) after the "80-dot" grid point of the dispatch-locked read frame** —
with D0=3: `_1` reads dot 83 < 84 → 2 ✓, `_2` reads dot 85 ≥ 84 → 3 ✓. If D0=4 the `_2` verdict
requires PPU-update-before-CPU-read at the shared dot 84 (sub-dot order: PPU first).
slopgb tier2 failure (reads 2 on `_2`): its tier2/deferred FF41 read at the 82+D0 slot lands before its
visible mode-3 flip — the flip must commit ≤ the `_2` read instant while > the `_1` read instant.

---

## ROW 5 — `m2int_m2stat/m2int_scx4_m2stat_ds_2` (cgb04c, want **out3**; `_1` out2) [DS]

**SETUP:** identical to row 4 **plus `SCX:=4`** written immediately after `stop` (before the wait loop —
the loop polls mode-3 *entry*, which is SCX-independent, so the loop exit phase and D0 are unchanged).

**TIMELINE / MEASUREMENT / LEG DIFF:** byte-identical to row 4 (same `.text@101e/101f` reads, same
M40/M41 → dots 80+D0 / 82+D0). Only SCX differs.

**CONSTRAINT:** **V(scx4) = V(scx0) ∈ (80+D0, 82+D0]** — the visible mode-3 ENTRY does not move with
SCX (SCX shifts only the exit). This row exists precisely to pin SCX-independence of the entry against
the same dispatch-locked read grid. slopgb tier2 failing `_2` here (while presumably matching row 4 or
not) means its tier2 frame lets SCX (or the SCX-dependent render path) perturb the entry-side read
verdict — hardware forbids any SCX coupling at these two read dots.

---

## ROW 6 — `m2int_m2irq/m2int_m2irq_late_retrigger_1` (dmg08+cgb04c, want **out2**; `_2` out0) [SS]

**SETUP:** SS (no speed switch; cart flag 0x80 = DMG+CGB). STAT=0x20 the whole test (never rewritten!),
IE=0x02, IF=0 before `ei`. LYC unused.

**TIMELINE** (two dispatches; M0 = dispatch #1 at line-L start, D0 ≈ 5–6 per §0.4 calibration):

Pass 1 ISR: `pop af` M9–11 (A := return-PC, i.e. the interrupted slide address), `and a,10` M12–13
(Z if return addr < 0x1000 → first pass), `ldff a,(0f)` M14–16 (A:=IF, flags kept), `jpnz` nt M17–19,
`ld a,02` M20–21, nops M22..M103 (82), then:

| event | `_1` | `_2` |
|---|---|---|
| **IF := 0x02 CPU write** (manual STAT-IF re-raise; +1 T WRITE_CPU) | M104–106, write **M106 → dot 424+D0(+1T)** (`.text@105c`) | **M107 → 428+D0(+1T)** (`.text@105d`) |
| `ei` | M107 | M108 |
| EI-latency nop | M108 | M109 |
| **dispatch #2** (IE=02 & IF=02) | M109–M113 | M110–M114 |
| **ack (IF bit1 clear)** ≈ disp2+3.5 M | **≈ dot 450+D0** | **≈ dot 454+D0** |
| natural **mode-2 pulse, line L+1** | dot **456 + ~3** (SameBoy cfl 3; a fresh edge — see below) | same |
| pass-2: `jp` / `pop`(A&0x10≠0) / `ldff a,(0f)` | read ≈ **M125 → dot ~500+D0** = line L+1 dot ~44-50 (OAM region) | +4 dots |

**MEASUREMENT:** printed = IF & 0x07 at the pass-2 read. bit0 = 0 (IF cleared at setup; no VBlank
between). **out2 = the line-L+1 mode-2 pulse re-raised IF bit1 AFTER dispatch #2's ack consumed the
manual bit; out0 = the pulse landed at/before the ack and was consumed with it.**

**LEG DIFF:** `_2` moves the manual IF write (and hence `ei` + dispatch #2 + its ack) 1 M (4 dots) later.
The pulse dot is fixed; only the ack slides.

**CONSTRAINT:** let `P` = the line-(L+1) mode-2 IF rise and `ack1` = leg-1's ack instant:
**P ∈ (ack1, ack1+4]**, with ack1 ≈ 450+D0 (D0∈[5,6]) and P ≈ 459 — i.e. **the ack sits mid-dispatch at
≈ M3.5 and the pulse lands within the following 4-dot window.** Two hardware laws pinned:
1. **The mode-2 STAT line must be a ~1-dot pulse** (mfi 2 → −1 at cfl 3→4, display.c:1795/1813): the
   line dropped during line L's OAM scan, so line L+1's rise is a FRESH 0→1 edge that re-raises a
   consumed IF — a model that holds the mode-2 line high through OAM scan gets out0 on BOTH legs.
2. **The dispatch IF-acknowledge instant** (re-sample after push-hi, clear at pending−2 T,
   sm83_cpu.c:1680-1701): edges after it survive, edges before it are merged & consumed.
slopgb tier2 failure (reads 0 on `_1`, `_2` passes trivially): either its L+1 mode-2 edge lands before
its ack (edge early / ack late by ≥1 dot but <5, since `_2` passes), or its mode-2 line never drops so
no re-raise exists at all (then `_1`=0 fail, `_2`=0 "pass" — indistinguishable from the outputs; check
which by tracing whether ANY second IF rise occurs).

---

## ROWS 7+8 — `irq_precedence/late_m0irq_retrigger_ds_1` (cgb04c **outE2**) [DS] and `late_m0irq_retrigger_scx1_1` (dmg08+cgb04c **outE2**) [SS]

(`_2` siblings outE0; base SS pair `late_m0irq_retrigger_1/_2` outE2/outE0 passes and is used as the
joint pin. Files in `irq_precedence/` only.)

**SETUP:** SCX=0 (base, ds) / SCX=1 (scx1, written right after `lbegin` before the loop). STAT=0x20 →
**0x08 in pass 1** (source switched to mode-0). IE=0x02, IF=0 before `ei`. ds rows: double speed.

**TIMELINE** (M0 = dispatch #1 at line-L start):

Pass 1 ISR: `ldff a,(c)` M9–10 (A:=STAT), `and a,08` M11–12 (Z on first pass since STAT=0x20),
`ldff a,(0f)` M13–15, `jpnz` nt M16–18, `ld a,08` M19–20, `ldff(c),a` M21–22 → **STAT:=0x08 at M22**
(SS dot 88+D0 = mode 3; DS dot 44+D0 = OAM scan — either way STAT line low, no edge; the switch arms
the mode-0 source), `xor` M23, `ldff(0f),a` M24–26 → IF:=0 at M26, then nops:

| event | SS base `_1` | SS scx1 `_1` | DS `_1` |
|---|---|---|---|
| nops | M27..M50 (24) | +1 M (25) | M27..M114 (88) |
| `ld a,02` + **IF := 0x02 write** (+1 T) | write **M55 → dot 220+D0** (`.text@1026`) | **M56 → 224+D0** (`@1027`) | **M119 → dot 238+D0** (`@1066`) |
| `ei`; nop | M56; M57 | M57; M58 | M120; M121 |
| **dispatch #2** | M58–M62 | M59–M63 | M122–M126 |
| **ack ≈ disp2+3.5 M** | **≈ dot 246+D0** | **≈ dot 250+D0** | **≈ dot 251+D0** (2 dots/M; −2 T ≈ −1 dot sub-M) |
| `_2` sibling ack | +4 dots | +4 dots | **+2 dots** |
| natural **mode-0 IF edge** `E` (line L, STAT=08) | E(scx0) ≈ visible exit+1 (SS law) | E(scx1)=E(scx0)+1 | E_ds(scx0) = visible exit co-instant (DS law) |
| pass-2 read `ldff a,(0f)` (after `jp`,`ldff a,(c)`,`and`) | ≈ M73 → dot ~294+D0 (mode 0, line L) | +4 | ≈ M137 → dot ~274+D0 |

**MEASUREMENT:** printed = full IF as two hex digits (`swap`/`and 0f` in lprint). Bits 7–5 read 1 →
base 0xE0. bit0 = 0 (cleared pass 1, lines < 144). **E2 = the natural mode-0 edge re-raised bit1 AFTER
dispatch #2's ack; E0 = the edge landed at/before the ack and was consumed with the manual bit.**
(The manual IF write itself never races the edge: it lands ~35 dots (SS) / ~15 dots (DS) before the
mode-3 exit.)

**LEG DIFF:** `_2` = manual-IF-write/ei/dispatch#2/ack all +1 M (SS +4 dots, DS +2 dots). scx1 vs base:
ack grid +4 dots for an exit shift of +1 dot. ds vs base: ack grid step halves to 2 dots.

**CONSTRAINTS:**
- SS base (passing): **E(scx0) ∈ (ack1, ack1+4]**, ack1 ≈ 246+D0.
- **Row 8 scx1:** E(scx0)+1 ∈ (ack1+4, ack1+8] → E(scx0) ∈ (ack1+3, ack1+4]. Joint with the base pair:
  **E(scx0) = ack1+4 exactly, E(scx1) = ack1+5** — the base+scx1 pair pins the SS mode-0 IF edge to
  **single-dot precision relative to the dispatch-ack grid** (edge exactly 1 M after leg-1's ack, first
  dot of the `_2` ack's M-cycle). With SameBoy's SS law (edge = visible exit + 1 dot,
  display.c:2113-2122) this simultaneously pins the visible exit. slopgb failing scx1_1 (E0) while base
  `_1` passes ⇒ its edge-vs-ack distance is correct to <4 dots but **off by ≥1 dot in the early
  direction at the scx1 parity** — a sub-M (dot-level) misplacement of either the ack instant or the
  SCX fine-scroll exit shift.
- **Row 7 ds:** **E_ds(scx0) ∈ (ack1, ack1+2]**, ack1 ≈ 251+D0, D0∈[3,4] → E_ds ≈ (254, 257] — a 2-dot
  bracket on the DS grid, one dot of which is already excluded by row 1's E0 = 254+D0 pin if the frames
  are shared (they are, both DS mode-0 edges; nb. row 1's D0 is its own family's). slopgb failing ds_1
  (E0) with ds_2 passing ⇒ same class: DS edge at/before its ack by ≤2 dots, must land in (ack1, ack1+2].
  Note the DS ack's sub-M position: −2 T on the DS T-grid ≈ −1 dot from the M3-trailing edge — the DS
  pair straddles at HALF the SS granularity, so this row is 2× more sensitive to the ack's sub-M-cycle
  placement (the S6/S7 wake/ack T-position work).
- Same two hardware laws as row 6, applied to mode-0: **edge-triggered IF with the line held high**
  (only ONE edge per line, at the exit; no later re-raise for `_2`), and **the ack at ≈ dispatch M3.5**.

---

## Cross-row synthesis (what a fix must satisfy simultaneously)

1. **DS mode-0 IF edge** (rows 1–3, 7): E(scx) = (254+D0) + SCX&7 relative to the dispatch-locked
   read/write grid; pinned to single-dot precision by the scx0+scx5 and scx3+scx4 joint brackets, on
   BOTH parities of the 2-dot DS CPU grid. DS edge co-instant with the visible flip (no +1).
2. **SS mode-0 IF edge** (row 8 + passing base): exactly ack+4 on the base leg's frame; edge = visible
   exit + 1 dot. The scx1 leg is the parity probe.
3. **Visible mode-3 entry** (rows 4–5): 1 M after the 80-dot grid point (SameBoy cfl 84), with
   PPU-commit-before-CPU-read ordering at a shared dot, and strictly SCX-independent.
4. **Mode-2 STAT line = 1-dot pulse at line start** (row 6 + m2irq base): consumed IF is ALWAYS
   re-raised by the next line's pulse; the winner vs dispatch is decided at the ack instant.
5. **Ack instant = dispatch M3 + 2 T** (≈ M3.5 of 5): rows 6–8 all bracket exactly this point; the DS
   rows at 2-dot resolution.
6. **CPU IF write beats a co-instant PPU IF set, at write-slot +1 T** (rows 2–3): after the clear, a
   high STAT line cannot re-raise (edge-triggered).
