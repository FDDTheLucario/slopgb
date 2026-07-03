# gambatte hwtests: window + gdma_cycles — exact machine-cycle constraint tables

Static analysis of `/tmp/sbbuild/gambatte-src/test/hwtests/{window,window/arg,dma}/` asm
(2026-07-03). All tests are NOP-slide brackets: an IRQ anchors the timeline, code at fixed
addresses places one WRITE (moved 1 M per leg) and one READ (same machine dot in all legs).
The `_out<h>` filename suffix is the ground truth (real cgb04c / dmg08 hardware).

Conventions used below:
- M-cycle costs: IRQ dispatch = 5 M; `jp` 4; `nop` 1; `ld a,imm` 2; `ldff(nn),a` / `ldff a,(nn)` 3
  (IO access in final M); `ldff a,(c)` 2 (IO read in M2); `ld a,(nnnn)` 4 (read in M4).
- "commit window [a,b) M" = the access happens during the a-th M-cycle after T0 (dispatch M1
  start); dots = M×4 (SS) or M×2 (DS). Reads/writes sample at the leading edge of that window.
- Bare mode-3 exit ≈ `253 + SCX&7` (raw-dot crib frame); full-window exit ≈ `263 + SCX&7`.
- The LYC=153 DS anchor: dispatch T0 begins at line-153 dot `d0 ≈ 2-3` (pinned below by the
  gdma rows, same anchor structure). The m2int SS anchor: T0 ≈ line-1 dot 4-8 (mode-2 STAT
  rise dot ~3-4 + ≤1 M nop alignment).

SameBoy 1.0.2 reference (read for this analysis):
- `Core/display.c:508` `wy_check()`: latches frame-sticky `wy_triggered` iff LCD on ∧ LCDC.5 ∧
  `WY == comparison`, where comparison = `ly_for_comparison` (if ≠ −1) on DMG **and CGB-DS**,
  else `current_line`. Writing WY can never UN-latch; it only stops future latching.
- Check points: (a) every WY write (`memory.c:1452`) and every LCDC write (`memory.c:1561`)
  set `wy_check_scheduled`; the check then fires at the next 8-half-dot grid point:
  `cycles_to_check = 8 − ((wy_check_modulo + K) & 7)`, K = 6 (CGB-DS) / 0 (CGB-SS) / 2 (DMG)
  (`display.c:1552-1574`) — i.e. 0.5–4 dots after the write, PPU-phase aligned.
  (b) fixed per-line points: line-top (`:1775`, comparison still stale/−1), the **mode-2-entry
  point at internal dot ~4** (`:1810`, right after `ly_for_comparison = current_line` at `:1809`)
  — this is the check that decides all the late_wy rows — and line-end (`:1750`).
- `wy_triggered` reset per frame (`:602`, `:1681`, `:2257`).
- Window activation in the render loop (`:1894-1941`): per-pixel, `wx_triggered` set when
  `WX == position_in_line + 7`; processed BEFORE the object fetch at the same X (`:1957+`).
  WX==166 sets `wx_166_interrupt_glitch` (`:1935`) — see row 6. Window disable mid-line:
  `wx_triggered` is cleared at the fetcher's next GET_TILE_T1 step iff LCDC.5 off (`:937-938`)
  — see row 4.
- GDMA: `memory.c:1951` `GB_hdma_run`, called AFTER the FF55-writing instruction completes
  (`sm83_cpu.c:1718`): one setup advance of `cycles` = 4 T (DS) / 2 T (SS), then per BYTE
  {read src, advance `cycles`, write VRAM}. Per 0x10-byte block: 16×4 T = **16 DS M** (32 dots)
  / 16×2 T = **8 SS M** (32 dots — same wall clock). Total stall:
  **DS: G(n) = 16n + 1 DS M-cycles**; SS: G(n) = 8n M + 2 T (a half-M remainder!).

---

## Row 1 — `window/arg/late_wy_1toFF_ds_2` (cgb04c out3; failing leg. Sibling `_ds_1` out0)

SETUP: IE=0 → JOYP=30 → KEY1=1, `stop` → **double speed**. Wait LY=0x97 (151).
LCDC=0xB1 (LCD on, **WIN en**, data 8000, win map 9800, BG on, OBJ off); WX=0x07 (trigger x=0);
**WY=0x01**; LYC=0x99 (=153); STAT=0x40 (LYC source); IE=0x02; `ei`; nop-slide. SCX=0 (never
written). Anchor: **LYC=153 STAT IRQ**, T0 = dispatch M1 ≈ line-153 dot 2-3.

TIMELINE (M after T0; DS: 1 M = 2 dots, line = 228 M):
- dispatch 5 → `jp lstatint` 9 → nops (0x1000..): leg1 442 (`.text@11ba`), leg2 443 (`.text@11bb`).
- `ld a,ff; ldff(4a),a` → **WY=0xFF commit**: leg1 M [455,456) = dots [910,912) = **line-1 dot
  [d0−2, d0) ≈ [0,4)**; leg2 M [456,457) = **line-1 dot [d0, d0+2) ≈ [2,6)**.
  (456 M = 912 dots = exactly 2 lines: 153 → 0 → 1; the write straddles line-1's start.)
- READ (identical both legs): 127/126 nops + `ldff a,(c)` (c=41) → M [584,585) = dots
  [1168,1170) = **line-1 dot [d0+256, d0+258) ≈ [258,262)**.

MEASUREMENT: FF41 & 3. out3 = mode 3 at read (window ran on line 1 → exit ≈ 263 > read);
out0 = mode 0 (bare exit ≈ 253 < read). WY=1 matches only LY=1: the trigger line and the read
line are both line 1.

LEG DIFF: only the WY=FF write moves, exactly **1 DS M (2 dots)**; read fixed at the same
machine dot.

CONSTRAINT: the line-1 WY latch (the `WY==LY` compare that arms the window for line 1)
samples WY at **line-1 dot ∈ (leg1 commit, leg2 commit] ≈ (d0−2, d0+2] ≈ dot 2-5** (DS).
A WY→FF commit at/before ~dot 2 of line 1 kills the trigger (out0); 1 M later (≥ dot 4) the
latch has already sampled WY=1 → `wy_triggered` sticks, window draws from x=0 (WX=07),
mode 3 extends to ≈263, read=3. SameBoy equivalent: the `:1810` mode-2-entry check (internal
dot 4, using the freshly-committed `ly_for_comparison = 1` — CGB-DS takes the lyfc path).

---

## Row 2 — `window/late_wy_ds_2` (cgb04c out3; sibling `_ds_1` out0)

SETUP: identical skeleton to row 1 (single `stop` → DS; wait LY=151; LCDC=0xB1; WX=07;
LYC=0x99; STAT=0x40; IE=02) except **WY is never written → WY=0 (boot)** → the trigger line
is **line 0**. No IF clear (relies on IF1 clear). Anchor: LYC=153 IRQ, T0 ≈ line-153 dot 2-3.

TIMELINE: nops: leg1 216 (`@10d8`), leg2 217 (`@10d9`).
- WY=0xFF commit: leg1 M [229,230) = dots [458,460) = **line-0 dot [d0+2, d0+4) ≈ [4,7)**;
  leg2 M [230,231) = **line-0 dot [d0+4, d0+6) ≈ [6,9)**.
- READ both legs: 125/124 nops + `ldff a,(c)` → M [356,357) = dots [712,714) =
  **line-0 dot [d0+256, d0+258) ≈ [258,261)** — between bare exit 253 and window exit 263. ✓

MEASUREMENT: FF41 & 3; out3 = line-0 window (WX=07, from x=0) extended mode 3 alive at read.

LEG DIFF vs row 1: same shape, but the deadline sits **~4 dots later in the line** than row 1's
(line-0's latch is later than line-1's: line 0 re-enters from VBlank; `ly_for_comparison`
becomes 0 only at internal dot 3 (`display.c:1790`, `current_line ? -1 : 0`), so the deciding
check is the dot-4 `:1810` point in SameBoy's line-0 frame, which lands ~2 dots later in raw
dots than on mid-frame lines).

CONSTRAINT: the **line-0** WY latch samples at **line-0 dot ∈ (d0+2, d0+6] ≈ dot 5-8** (DS).
WY→FF committing at/before ~dot 5 of line 0 → never triggered this frame → bare → out0;
at/after ~dot 7 → too late, `wy_triggered` latched → out3. Note the write itself also schedules
a SameBoy `wy_check` 0.5-4 dots later — harmless, it re-reads WY=FF (no match).

---

## Row 3 — `window/late_wy_ds_lcdoffset1_2` (cgb04c out3; sibling `_1` out0)

SETUP: row-2 skeleton with the **triple-STOP dance**: KEY1=1,`stop` (→DS); KEY1=1,`stop`
(→SS); KEY1=1,`stop` (→DS). Ends DS with the **LCD phase shifted 1 dot** relative to the
CPU/DIV grid (each speed switch pauses the LCD a fixed sub-M amount — SameBoy
`double_speed_alignment`; net labeled "lcdoffset1"). Then wait LY=151; LCDC=0xB1; WX=07;
LYC=0x99; STAT=0x40 (via `ldff(41),a`); IE=02; **IF cleared**; `ei`. WY=0 (boot) → line-0
trigger, same as row 2.

TIMELINE: nops: leg1 215 (`@10d7`), leg2 216 (`@10d8`) — the whole write bracket sits
**exactly 1 DS M earlier in code position than row 2** (10d7/10d8 vs 10d8/10d9):
- WY=0xFF commit: leg1 M [228,229) = dots [456,458) = line-0 dot [d0, d0+2); leg2 M [229,230)
  = line-0 dot [d0+2, d0+4) — *CPU-frame* positions.
- READ: 125/124 nops + `ldff a,(41)` (3 M, direct form; mask `and a,07`) → M [356,357), the
  **same machine dot as row 2's read**.

MEASUREMENT: FF41 & 7 (LYC bit reads 0: LY=0 ≠ 0x99) → effectively mode; out3/out0 as row 2.

LEG DIFF: vs row 2, read unchanged, write bracket −1 M. Since the deadline is a PPU-line
event, this pins the STOP-dance LCD offset: the PPU line runs **1 dot ahead** of where the
aligned-DS run has it (deadline crosses the M-quantized write grid one bucket earlier).

CONSTRAINT: same line-0 WY-latch deadline as row 2 **in LCD coordinates** (line-0 dot ~5-8),
but the LCD frame is displaced 1 dot vs the CPU M grid, so in CPU code position the
un-match deadline = (leg1, leg2] commits = one DS M earlier than row 2's bracket. Any
emulator whose speed-switch does not shift the LCD/CPU phase by exactly this 1 dot passes
row 2 and fails row 3 (or vice versa) — the pair is a phase probe, not a new window law.

---

## Row 4 — `window/late_disable_spx10_wx0f_2` (dmg08+cgb04c out3; sibling `_1` out0)

SETUP: IE=0; wait LY=0x90 (144, VBlank). **OAM fill**: FE01..FE9F ← 0, then FE00 ← 0x10 (Y),
FE01 ← 0x10 (X) → **one sprite: Y=0x10, X=0x10, tile 0, attr 0** → left edge at screen x=8;
LCDC=0xB7 → bit2=1 = **8×16 sprites** → covers LY 0..15 (line 1 in range). LCDC=0xB7 also:
LCD on, WIN en, data 8000, **OBJ on**, BG on. WX=0x0F (window trigger at x=8 — same X as the
sprite!). WY=0 (boot) → window triggered from line 0. SCX=0. Poll FF41 until mode 3 (line 0),
then STAT=0x20 (mode-2 source), IF=0, IE=02, `ei`.
Anchor: **mode-2 STAT IRQ of line 1** (rise ≈ line-1 dot 3-4), T0 ≈ line-1 dot 4-8. SS.

TIMELINE (SS: 1 M = 4 dots):
- ISR: `jp` → nops (leg1 12, leg2 13) → `ld a,97; ldff(40),a` → **LCDC=0x97 = bit5 WINDOW
  DISABLE** (OBJ kept on) commit: leg1 M [25,26) = dots [100,104); leg2 M [26,27) = dots
  [104,108) (+T0 → raw ≈ [104,112)/[108,116)).
- READ both legs: 39/38 nops + `ldff a,(c)` → M [66,67) = dots [264,268) (+T0 → raw ~268-276).

MEASUREMENT: FF41 & 3. Line 1 with window(x=8)+sprite(x=8..15): full extension ≈ bare 253 +
window ~+10 + sprite ~+8 ⇒ exit ≈ 271+; aborted-window (bare+sprite) exit ≈ 261. The read at
~268-272 separates them: out3 = window extension survived the disable; out0 = aborted.

LEG DIFF vs the no-sprite siblings `late_disable_wx0f_{0,1,2}` (writes at M [25,26)/[26,27)/
[27,28), read at M [64,65)):
- **no sprite**: `_0` out0/out0, `_1` **dmg out3 / cgb out0**, `_2` out3/out3 →
  DMG point-of-no-return ∈ (M25-commit, M26-commit] ≈ dot (104, 108]+t0;
  CGB ∈ (M26, M27] ≈ dot (108, 112]+t0 — CGB 1 M LATER than DMG.
- **spx10**: `_1` (M25) out0 both, `_2` (M26) out3 both →
  **both models' deadline ∈ (dot ~104, ~108]+t0 — the sprite pulls the CGB deadline 1 M
  (4 dots) EARLIER, onto the DMG value**; DMG unchanged. Read shifted +2 M vs no-sprite
  (the sprite's ~8-dot mode-3 penalty moves both candidate exits).

CONSTRAINT: with `wy_triggered` set, WX=0x0F and an 8×16 OBJ at X=0x10 on the line, the
LCDC.5-clear commit deadline that decides whether line 1's window mode-3 extension survives
is **dot ≈ 106±2 (in T0+dots: between the M25 and M26 write commits) on BOTH dmg08 and
cgb04c**; without the sprite the cgb04c deadline is one M-cycle later. Mechanism (SameBoy):
window activation at x=8 precedes the object fetch at the same X (`display.c:1894`/`:1957`);
a disable takes effect when the fetcher next passes GET_TILE_T1 (`:937` clears `wx_triggered`);
the sprite fetch inserted directly after the window restart occupies the fetcher and removes
the late CGB abort slot.

---

## Row 5 — `window/m2int_wxA5_m0irq_2` (dmg08+cgb04c out2; sibling `_1` out0)

SETUP: wait LY=0x91 (145); LCDC=0xB1 (WIN en, OBJ off); **WX=0xA5 (165 → window trigger at
x=158: 2 window pixels at end of line)**; WY=0 (boot) → triggered from line 0; SCX=0
(explicitly written); poll mode 3 (line 0); STAT=0x20 (mode-2 src); IF=0; IE=02; `ei`.
Anchor: mode-2 IRQ of **line 1**, T0 ≈ line-1 dot 4-8. SS.

TIMELINE:
- ISR body: `ld a,08; ldff(41),a` → **STAT sources := mode-0 only**, commit M [13,14) =
  dots [52,56) (during mode 2 — clean). `xor a; ldff(0f),a` → IF=0, commit M [17,18) =
  dots [68,72) (still before mode 3 ends its first tile — clean).
- READ: nops (leg1 44, leg2 45) + `ldff a,(0f)` (3 M) → IF sample:
  leg1 M [64,65) = dots [256,260)+T0 ≈ raw 260-266; leg2 M [65,66) = [260,264)+T0 ≈ 264-270.

MEASUREMENT: IF & 3 (b=03). out0 = STAT IF (bit1) not yet set; out2 = set. Measures the
**mode-0 STAT IRQ rise dot** on a wxA5-extended line (VBlank bit can't interfere: line 1).

LEG DIFF: read moves 1 M; nothing else. Cross-pin with siblings `m2int_wxA5_m3stat_1/2`
(FF41&3 read at the *same* M buckets [64,65)/[65,66)): m3stat_1=3, m3stat_2=0 both models →
the **visible mode-0 edge** also lies in (sample@M64, sample@M65].

CONSTRAINT: with a window triggering at x=158, the mode-3 exit is pushed from bare ~253 to
**exit ∈ (T0+256, T0+260] ≈ dot 261-266 (both dmg08 and cgb04c)** — a **+8..+11 dot window
penalty** (window fetch restart ~6 dots + 2 remaining pixels + fine alignment) — and the
mode-0 STAT IF rise lands in the SAME 4-dot bucket (consistent with rise = visible edge +1
dot). An emulator must have BOTH: the wxA5 extension ≥ ~9 dots (else leg1 reads 2 → `_1`
breaks... n.b. `_1` wants 0) AND ≤ ~12 dots (else leg2 reads 0 and `_2` breaks). Bracket:
**m0 IF rise ∈ (T0+256, T0+260]**.

---

## Row 6 — `window/m2int_wxA6_vrambusyread_3` (dmg08+cgb04c out5; siblings `_1` out0/out0, `_2` dmg out5 / cgb out0)

SETUP: as row 5 but **WX=0xA6 (166 → trigger at x=159, the WX=166 quirk)** and a marker byte:
**`ld a,04; ld(8000),a` during VBlank** (tile 0, byte 0). ISR = pure nop slide (STAT source
stays mode-2; no re-fire on the measured line). Anchor: mode-2 IRQ of line 1, T0 ≈ dot 4-8. SS.

TIMELINE: READ = `ld a,(8000)` (4 M, VRAM read in M4):
- `_1` @0x1033: 51 nops → M [63,64) = dots [252,256)+T0;
- `_2` @0x1034: M [64,65) = [256,260)+T0;
- `_3` @0x1035: M [65,66) = [260,264)+T0. Then `inc a`, print.

MEASUREMENT: a mode-3-blocked VRAM read returns **0xFF → +1 = 0x00 → out0**; an open read
returns the marker **0x04 → +1 = 0x05 → out5**. So out5 ⇔ VRAM lock already released.

LEG DIFF (with the m3stat and m0irq wxA6 siblings, all M-bucket aligned):
| M bucket (dots+T0) | FF41&3 (m3stat) | VRAM read | IF&3 (m0irq) |
|---|---|---|---|
| [63,64) = [252,256) | 3 / 3 | FF / FF | 0 / 0 |
| [64,65) = [256,260) | dmg 0 / cgb 3 | dmg 04 / cgb FF | **2 / 2** |
| [65,66) = [260,264) | 0 / 0 | 04 / 04 | — |

CONSTRAINT: WX=0xA6 extends the mode-3/VRAM lock past bare 253: **dmg08 exit ∈ (T0+252,
T0+256] (≈ +3..+7 dots); cgb04c exit ∈ (T0+256, T0+260] (≈ +7..+11 dots) — CGB exactly 1 M
later**; the VRAM unblock co-moves with the visible FF41 exit in the same bucket (this row
pins the **CGB unblock ≤ T0+260**, i.e. by dot ~264-268 raw). Bonus quirk pinned by the
m0irq sibling: on cgb04c the mode-0 STAT IF rises in bucket [64,65) while FF41 still reads 3
and VRAM is still locked — the **WX=166 early-interrupt glitch** (SameBoy
`wx_166_interrupt_glitch`, display.c:1935: the window "activates during HBlank" at PPU
X=160): IF rise precedes the CGB visible exit by up to ~4 dots.

---

## Rows 7+8 — `dma/gdma_cycles_short_scx5_ds_1` and `dma/gdma_cycles_2xshort_scx5_ds_1` (cgb04c out3; siblings `_2` out0)

SETUP (identical both): IE=0; JOYP=30; KEY1, `stop` → **DS**. Wait LY=0x97 (151). LYC=0x99
(153); STAT=0x40 (LYC src). Marker 0x00→(8000); 0x01→(C000). HDMA1=0xC0, HDMA2=0x00 →
**src C000**; HDMA4=0x00, HDMA3=0x80 → **dst 0x8000** (high bits masked &0x1F). IF=0; IE=02;
`ei`; `ld hl,8000`; **SCX=0x05**; `halt`.
Anchor: LYC=153 STAT IRQ wakes the halt; T0 = dispatch M1 ≈ **line-153 dot 2-3** (pinned by
the bracket below; includes the CGB halt-wake latency).

TIMELINE (DS M; write/stall verified against SameBoy `GB_hdma_run`):
- dispatch 5 → `jp` 9 → `ld b,03` 11 → `ld a,00` 13 → `ldff(55),a` ends 16: **FF55=0x00 →
  GDMA, 1 block (16 B)**, commit dots [30,32) — deep in VBlank, no mode conflict.
- **GDMA stall G(1) = 17 DS M = 34 dots** (1 M setup + 16 M copy; 1 DS M per byte). CPU
  resumes at M 33 (short). 2xshort: second `ldff(55),a` ends 36, second stall → resumes M 53.
- READ `ldff a,(41)`, and a,03:
  - short: 320 nops → read M [355,356) = dots [710,712) → **line-0 dot [d0+254, d0+256) ≈
    [256,258+)** (leading-edge sample 256-257).
  - `_2` leg: 321 nops → [356,357) → sample dot 258-259.
  - 2xshort: 300 nops → read M **[355,356) — the identical machine dot as short** (the extra
    `ldff(55),a` 3 M + second 17 M stall are exactly compensated: 3+17 = 20 M = 317−297 nops).

MEASUREMENT: FF41 & 3 on **line 0, SCX=5** → bare mode-3 exit = 253+5 = **258**. out3 (`_1`)
= sample < 258 (mode 3); out0 (`_2`) = sample ≥ 258 (mode 0). The GDMA itself finishes at
line-153 dot ~66 (short) — the read measures the TOTAL elapsed CPU time (stall included)
against the fixed PPU exit, i.e. **the stall duration to ±1 DS M**.

DERIVED GDMA DURATION (three independent equations from sibling geometry — exact):
1. short_ds vs 2xshort_ds (same anchor LYC=153, same read dot): `335+G = 318+2G` → **G(1) =
   17 DS M**.
2. short_scx5_ds vs long_scx5_ds (len 0x7F = 128 blocks, LYC=0x90 anchor): totals must agree
   mod 228 M: `586+G(128) ≡ 338+G(1)` → `G(128) − G(1) = 2032 = 127×16` → **16 DS M per
   block, +1 DS M fixed setup: G(n) = 16n + 1**.
3. SS siblings (short_scx5 vs long_scx5): `Δ = 10 + 127×8 = 1026 = 9×114` → **8 SS M per
   block** (32 dots/block at both speeds).
   Matches SameBoy exactly: `GB_hdma_run` = advance(4 T DS / 2 T SS) setup + per-byte
   advance(4/2 T); runs after the FF55 write's instruction retires. (SS total = 32n+2 T
   carries a half-M remainder — SS-only quirk, not in these DS rows.)

CONSTRAINT (both rows): with G(n) = 16n+1 DS M and dispatch anchored at line-153 dot d0,
the `_1` FF41 sample = line-0 dot **d0+254 < 258** and `_2` = **d0+256 ≥ 258** ⇒ **d0 ∈ [2,4)**
— i.e. the bracket simultaneously pins (a) the GDMA stall to exactly 17 (short) / 2×17+3 M
(2xshort) — a ±1 M error in stall or in the halt-wake/dispatch anchor or a ±2 dot error in the
FF41 read frame flips exactly one leg — and (b) the line-0 scx5 bare exit at 258. For
2xshort specifically, the per-TRANSFER +1 M setup must apply to EACH FF55 write (a model with
0 or 2 M setup breaks `_2` or `_1` respectively while short shifts by only half as much).
