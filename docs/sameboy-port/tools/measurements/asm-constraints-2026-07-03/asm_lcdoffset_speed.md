# lcd_offset + speedchange2 STOP-dance rows — exact machine-cycle constraint tables

Static analysis of gambatte hwtests asm (`/tmp/sbbuild/gambatte-src/test/hwtests/{lcd_offset,speedchange}/`)
and SameBoy 1.0.2 STOP/speed-switch code. All rows CGB (`cgb04c_out..`). No emulators run.

Conventions used throughout:
- 1 M = 4 dots single-speed (SS) / 2 dots double-speed (DS). Line = 456 dots = 114 M SS / 228 M DS.
- `d0` / `G` = the dot at which the IRQ dispatch's first M-cycle begins (the first CPU M-grid
  boundary at which the freshly-raised IF bit is taken). Dispatch = 5 M.
- IO reads (`ldff a,(nn)` = 3 M) sample in the 3rd M-cycle; IO writes commit in their last M-cycle.
- `L` = the dot at which the 3rd (IO) M-cycle of the *first* poll read begins (leading-edge frame,
  i.e. slopgb tier2's cc+0 convention). If the true hw sample sits σ ∈ [0,M) into that M-cycle, every
  numeric equality below shifts by the constant σ; the *bracket widths and leg differences do not*.
- Nop slides: gaps between `.text` sections are zero-filled → executed as nops, 1 M each. The
  measurement phase is set purely by section addresses; `_1`→`_2` legs differ ONLY by the poll/read
  section address (+1 byte = +1 nop = +1 M) and, for count rows, the compare constant.

---

## 1. The measurement machinery (shared by all 5 lcd_offset rows)

### 1.1 Common setup (offsetN, LCD ON the whole time — LCDC is never touched)

```
ld a,30 / ldff(00),a      ; JOYP=0x30 (no select → STOP won't exit by joypad)
xor a  / ldff(ff),a       ; IE = 0  → at every STOP: interrupt_pending=false → full switch-halt path
inc a  / ldff(4d),a       ; a=1, KEY1.0 armed
<< STOP dance, see per-row >>
ld a,SCX / ldff(43),a     ; SCX
ld b,97 / call lwaitly_b  ; coarse sync: spin until LY==0x97 (151)
ld a,99 / ldff(45),a      ; LYC = 0x99 (153)
ld a,40 / ldff(41),a      ; STAT enable = LYC only (bit6)
ld a,02 / ldff(ff),a      ; IE = STAT
xor a  / ldff(0f),a       ; IF = 0
ei                        ; → nop slide 0x16E..0x1000 (thousands of 1-M nops)
```

The LYC=153 STAT rise (line 153, dot r ≈ 0–6; the LY=153 "line-153 quirk" window — the match fires
at the start of line 153) is the **anchor**: it lands mid-nop-slide, so the dispatch begins at the
next CPU M-grid boundary `d0 = roundup_grid(r)`. Everything after is a fixed M-count. The STOP dance
has shifted the CPU M-grid phase relative to the PPU dot grid — **that phase is the tested quantity**;
`lwaitly_b` resyncs only coarsely (whole lines), the sub-M phase survives it.

### 1.2 ISR + slide — both variants land the poll at the identical machine cycle

```
.text@48: jp lstatint                    ; 4 M   (dispatch itself = 5 M before this)
m0irq ISR : ld a,08 / ldff(41),a        ; 2+3 M, ends 0x1004 → slide 0x151=337 nops → 0x1155
m0stat ISR: xor a  / ldff(41),a         ; 1+3 M, ends 0x1003 → slide 0x152=338 nops → 0x1155
```
Total dispatch-start → arrival at the poll section: **5+4+2+3+337 = 5+4+1+3+338 = 351 M** — the byte
difference is exactly compensated by the slide, so the m0irq and m0stat families sample at the SAME
machine cycle. m0irq leaves STAT=0x08 (mode-0 IRQ source armed; IME stays OFF — no reti/ei in the
ISR — so mode-0 rises only latch IF, never dispatch). m0stat leaves STAT=0x00 (all sources off) and
polls FF41 directly.

### 1.3 The poll loops — period is EXACTLY one line, so the poll phase is line-locked

DS m0irq loop (`_1` at 0x1155):
```
ltest_if: ldff a,(0f)     ; 3 M  ← IF sampled in 3rd M      = the poll
          cmp a,e0        ; 2 M    (_1: continue while IF==E0 i.e. NO flags)
          jrnz lprint_ly  ; 2 M nt (exit → print LY)
          xor a / ldff(0f),a ; 1+3 M ← IF cleared 8 M after the poll sample
          ld c,35         ; 2 M
lwait_nly: dec c / jrnz   ; 52*4+3 = 211 M
          nop / jr        ; 1+3 M
                          ; TOTAL = 228 M = exactly 1 DS line
```
DS m0stat loop (`_1` at 0x1155): `ldff a,(41)(3) cmp a,83(2) jrnz(2) ld c,36(2) inner 215 nop 1 jr 3`
= **228 M**. SS m0stat loop (offset2/3, `_1` at 0x10a4): `ld c,19`, inner 99, `nop nop nop` → **114 M**
= exactly 1 SS line. So every subsequent poll re-samples the SAME line phase; the FIRST poll already
decides the row, and each later line re-checks the same inequality.

First-poll placement: poll-IO M-cycle = dispatch-start + 353 M (DS) / +176 M (SS)
→ **L = d0 + 706 dots (DS)** / **L = d0 + 704 dots (SS)**. With d0 ≈ line-153 dot 4–8, the first
poll lands on **line 0, dot ≈ 250–260** — right on the mode-3 exit of line 0. That is by design:
the first poll IS the bracket sample.

### 1.4 What out90 means (the count mechanism)

The loop must survive **145 polls (lines 0..144)**. On lines 0..143 the continue-condition must hold
every time. On the line-144 poll the loop exits via VBlank, and `lprint_ly` prints FF44 = **0x90 = 144**:
- m0irq rows: VBlank sets IF bit0 at line 144 dot 0 regardless of IE → poll reads 0xE1 (≠E0 and ≠E2).
- m0stat rows: FF41 reads 0x81 (mode 1) ≠ 0x83 and ≠ 0x80.

Continue-conditions per leg (this is the exact "first poll must read" answer):

| leg | polls | continue while | first poll MUST read | meaning |
|---|---|---|---|---|
| m0irq `_1` (0x1155) | FF0F | == 0xE0 | **0xE0** | line-0 mode-0 IF rise NOT yet latched at the sample |
| m0irq `_2` (0x1156) | FF0F | == 0xE2 | **0xE2** | rise already latched 1 M later |
| m0stat `_1` (0x1155/0x10a4) | FF41 | == 0x83 | **0x83** (mode 3, LYC flag clear — LYC=153 never matches lines 0-143) | visible mode still 3 at the sample |
| m0stat `_2` (0x1156/0x10a5) | FF41 | == 0x80 | **0x80** | visible mode already 0, 1 M later |

m0irq `_1` additionally requires the rise to land **inside the 8-M dead arc** (poll-sample, IF-clear]:
the clear commits poll+8 M (= +16 dots DS). Rise after the clear → the NEXT poll reads E2 → early
exit (prints 01/02). Rise at/before the sample → FIRST poll reads E2 → exit printing **00** — the
observed slopgb failure. m0irq `_2` requires the opposite: the rise at/before ITS sample (poll+1 M),
where it is then wiped by the clear each line so line 144's poll sees a clean E1.

### 1.5 The four-row conjunction pins the event to 1 dot

Let F1 = visible m3→0 flip dot of a line at SCX&7=1, R1 = the mode-0 IF-set dot (same line), both in
the same absolute frame as L. From the poll addresses (scx1: `_1`@0x1155 `_2`@0x1156; scx2: `_1`@0x1156
`_2`@0x1157; +1 SCX ⇒ flip +1 dot, poll +1 M = +2 dots DS):

```
m0stat: scx1_1: F1 > L        scx1_2: F1 ≤ L+2
        scx2_1: F1+1 > L+2    scx2_2: F1+1 ≤ L+4
        ⇒  F1 ∈ (L+1, L+2]   ⇒  F1 = L+2   (integer-dot solution)
m0irq : identical algebra on R1 ⇒ R1 = L+2
```
So on cgb04c, after the offset1-ds dance, **both the visible FF41 flip and the mode-0 IF rise land
exactly 2 dots (1 DS M) after the first-poll sample instant**, and they coincide with each other at
this resolution. Note the integer solution is edge-degenerate (scx1_2 has 0 margin at the leading
edge); a half-dot-valued phase F1 = L+1.5 satisfies all four legs with uniform ½-dot margin — exactly
what a per-switch half-dot (2.5-dot) PPU stall × 3 switches would produce (see §3).

---

## 2. Per-row tables — lcd_offset

### Rows 1–4: `offset1_lyc99int_m0{irq,stat}_count_scx{1,2}_ds_1` (all out90)

SETUP (dance) — 3 STOPs, ends in DS. M-counts between events (speed of each segment noted):
```
ldff(4d),a  (3 M, SS)  → STOP #1  SS→DS   [KEY1 armed, IE=0 → freeze+0x20008-T halt path]
ldff(4d),a  (3 M, DS)  → STOP #2  DS→SS   [re-arm is the ONLY instruction between stops]
ldff(4d),a  (3 M, SS)  → STOP #3  SS→DS   → measurement runs in DS
```
Then SCX write, LY-151 sync, LYC=153, STAT=0x40, IE=02, IF=0, ei (all per §1.1).

TIMELINE (anchored to the LYC-153 dispatch):
```
line 153 dot r (≈0–6): LYC STAT rise → d0 = next CPU-grid dot
d0+0..9      : dispatch (5 M)
d0+10..17    : jp lstatint (4 M)
d0+18..27    : ISR writes STAT (m0irq: 08 @ ~d0+24; m0stat: 00 @ ~d0+22, commit in write M)
d0+28..705   : nop slide (337/338 nops)
d0+706       : FIRST POLL IO M-cycle begins  = line 0, dot d0+250  (456 consumed by line 153)
             every subsequent poll: +228 M = same line phase, lines 1,2,…,144
```

CONSTRAINT (hardware, per §1.5):
- rows 1–2 (m0irq scx1/scx2): the line-N mode-0 IF-set dot must satisfy
  **R(scx) = L + 1 + (SCX&7)** with L = d0+706+2·(row's extra nop: scx2 polls at L+2). Equivalently:
  R(scx1) − first_poll_sample = **exactly +2 dots**, and it must fall in (sample, sample+16] every
  visible line (the 8-M clear arc) — automatic once the +2 relation holds, since the loop is
  line-periodic.
- rows 3–4 (m0stat scx1/scx2): identical with the *visible FF41 flip* F in place of R.
- Exit: line-144 poll reads E1 (m0irq) / 0x81 (m0stat) → prints 0x90.

LEG DIFF: `_2` = `_1` + 1 nop before the loop (poll +1 M = +2 dots) + inverted continue constant
(E0→E2 / 83→80). `_1` and `_2` both expect out90; together they are a 1-M bracket, per-scx:
`_1` says event strictly after its sample, `_2` says event at/before sample+2 dots.

FAILURE MODE (slopgb, `_1` legs): first poll reads E2 / 0x80 → exit at line 0 → prints 00.
⇒ slopgb's rise/flip is **≥ 2 dots (1 DS M) EARLY relative to its post-3-STOP CPU grid** (or the
grid is 1 M late). Its `_2` siblings stay green for any earliness up to ~the full line, which is why
only the `_1` legs fail — a one-sided phase error, not a broken counting loop.

### Row 5: `offset3_lyc99int_m0stat_count_scx1_1` (out90, SS)

SETUP (dance) — 2 STOPs + one DS nop, ends in SS:
```
ldff(4d),a (3 M, SS) → STOP #1 SS→DS
ldff(4d),a (3 M, DS) → nop (1 M, DS = 2 dots) → STOP #2 DS→SS   ← the nop displaces STOP #2 by 2 dots
```
ISR = m0stat (STAT=0x00), slide 161 nops → poll at 0x10a4, loop period 114 M = 1 SS line.
First poll IO M at dispatch-start + 176 M → **L' = d0 + 704 dots = line 0, dot d0+248**.

CONSTRAINT: offset3's own scx0 pair (`_1`@0x10a3, `_2`@0x10a4) + scx1 pair (`_1`@0x10a4, `_2`@0x10a5):
```
scx0: F0 > L'−4 ,  F0 ≤ L'        scx1: F0+1 > L' ,  F0+1 ≤ L'+4
⇒ F0 ∈ (L'−1, L'] ⇒ F0 = L'  ⇒  F1 = L'+1
```
Row 5 itself (scx1 `_1`): must read **0x83** at L' — true value has exactly **1 dot of margin**
(F1 = L'+1 > L'). The scx0 `_2` sibling reads 0x80 with 0 margin. Survive 144 lines → 0x81 at
line 144 → 0x90.

LEG/SIBLING DIFF: vs `offset1` non-ds (dance without the nop; rows scx2@0x10a4/scx3@0x10a5 →
F(scx2) = L'+4) and `offset2` (4 STOPs; scx1@0x10a4/scx2@0x10a5 → F(scx1) = L'+4 ⇒ F(scx0) = L'+3):
**δ(offset2) = δ(offset1) + 1 dot; δ(offset3) = δ(offset1) + 2 dots (mod 4)**, where δ = the PPU-vs-
CPU-grid phase produced by the dance. I.e.:
- one extra DS→SS/SS→DS round trip (offset1→offset2) shifts the phase by **+1 dot (mod 4)**;
- displacing the DS→SS STOP by 1 DS M (offset1→offset3, the single nop) shifts it by **exactly the
  2 dots of the displacement** — the offset carries through the switch un-requantized. This is the
  direct assertion of the "odd mode": a 2-dot (half-SS-M) CPU-vs-PPU offset survives the return to
  single speed and is observable. (SameBoy's `dsa & 7` check exists precisely because it cannot
  represent this; see §3.)

FAILURE MODE: first poll reads 0x80 → prints 00 ⇒ slopgb's flip is ≥ 1 dot early against its
post-(2-STOP+nop) grid — its margin here is the thinnest in the family (1 dot), so this is the first
SS row to fall to any phase error ≥ 1 dot in this direction.

---

## 3. SameBoy's STOP/speed-switch model (code-exact, for reference)

`sm83_cpu.c stop()` (KEY1 armed, JOYP idle, IE&IF==0 in all these tests → `interrupt_pending=false`):
1. `flush_pending_cycles`; `enter_stop_mode` (DIV write-reset; `div_cycles=-4` if !IME);
   `cycle_read(pc++)` — the 2nd STOP byte costs 1 M.
2. Odd-mode check (BEFORE the flip): `if (LCDC on && cgb_double_speed && (double_speed_alignment & 7))
   speed_switch_freeze = 2;` — plus a log: *"ROM triggered a PPU odd mode, which is currently not
   supported. Reverting to even-mode."* SameBoy explicitly cannot represent the phase the offset3 /
   lcdoff s-even rows pin.
3. Direction: DS→SS flips `cgb_double_speed` **immediately**; SS→DS sets `speed_switch_countdown=6`
   (the flag flips 6 pre-doubled T into the following advances) and `freeze=1`.
4. No pending interrupt (all these rows): `speed_switch_halt_countdown = 0x20008` and
   **`speed_switch_freeze = 5` (overwriting the 1 or the odd-mode 2!)**; KEY1 cleared;
   `leave_stop_mode`; `halted=true`. The halted loop advances 4 pre-T per iteration; 0x20008 ≡ 0
   (mod 4) → the CPU wakes exactly 131080 pre-T later, on-grid.
5. `GB_advance_cycles` order (timing.c): countdown-flip → timers → halt-countdown → **freeze eats the
   cycles before the PPU sees them** → `cycles <<= 1` if SS → `double_speed_alignment += cycles`
   (post-doubled, only while LCDC on) → `GB_display_run`.

Net per-switch PPU-vs-CPU offset in SameBoy: the PPU (and dsa) is stalled **5 pre-doubled T per
armed STOP** — 5 dots when consumed at SS rate, **2.5 dots (a half-dot-odd amount)** at DS rate —
i.e. every PPU event thereafter occurs that much LATER in CPU-instruction time. `double_speed_alignment`
= post-doubled (half-dot) units since LCD enable, reset to 0 on BOTH LCD enable and disable
(memory.c GB_IO_LCDC); `dsa&7 == 4` ⟺ dots-since-enable ≡ 2 (mod 4) ⟺ returning to SS would leave
the PPU line phase half-an-SS-M off the CPU grid — the real hardware state the tests demand, which
SameBoy "reverts". Two standing TODOs in the code admit the approximation: *"Speed switch timing
needs far more tests. Double to single is wrong to avoid odd mode."* and *"speed switching takes 2
extra T-cycles (so 2 PPU ticks in single->double and 1 PPU tick in double->single)"*.
(This matches the #11bd `sb_dsa8` shadow: +2 per dot while LCD on, reset at enable, −4/−5-class
stall per STOP pause.)

---

## 4. The measurement machinery — speedchange2 rows 6–9

### 4.1 Common skeleton (`speedchange2[_nop]_lcdoff[_nopx2]_m2int_m3stat_scx2_2`)

```
ld b,91 / call lwaitly_b   ; sync LY=0x91=145 (VBlank)
xor a / ldff(40),a         ; LCD OFF                      (so STOP #1 is a clean, PPU-invisible switch)
ldff(ff),a                 ; IE=0
ld a,30 / ldff(00),a       ; JOYP idle
ld a,01 / ldff(4d),a       ; arm
STOP #1  SS→DS  (LCD OFF; freeze/halt invisible to the PPU — isolates the second switch)
[nop ×e]                   ; e=1 for the nop_lcdoff_* variants (1 DS M = 2 dots)
ld a,91 / ldff(40),a       ; LCD ON, in DS — PPU restarts here; SameBoy: dsa=0, display_cycles=0
ld a,01 / ldff(4d),a       ; re-arm (2+3 M DS)
[nop ×s]                   ; s=2 for *_nopx2 variants (2 DS M = 4 dots)
STOP #2  DS→SS  (LCD ON)   ; ← the measured switch: PPU runs on through the freeze+halt
lbegin_waitm3: ldff a,(c)/and/cmp/jrnz  ; 7 M/iter — sync into mode 3 of some line N (coarse)
ld a,20 / ldff(c),a        ; STAT = 0x20 (mode-2 source)   [ldff(c),a = 2 M]
ld a,02 / ldff(ff),a ; xor a / ldff(0f),a ; ei   ; IE=STAT, IF=0 → nop slide to 0x1000
```
Dispatch = the **mode-2 STAT rise of line N+1** (dot ≈ 0). Let **G** = the dispatch-start dot.

M-counts around STOP #2 (all DS): LCD-ON commit = wake(STOP#1) + (2+3+e) M, i.e. the write IO M is
the (5+e)-th M after wake; STOP #2's execution point = enable + (2+3+s+2) M = **enable + (7+s) M
= enable + (14+2s) dots**. So dots-since-enable at the switch ≡ 2 (mod 4) for s ∈ {0,2}
(SameBoy `dsa&7 == 4`, the "odd mode") and ≡ 0 (mod 4) for s = 1. **All four failing rows are the
s-even (odd-mode) class.**

### 4.2 ISR + read

```
dispatch 5 M → jp 4 M → ld a,02 (2 M) → ldff(43),a (3 M)   ; SCX=2 commits ~G+52..55, before mode 3 (dot 80)
→ slide: _1: 46 nops (0x1004→0x1032)  _2: 47 nops (→0x1033)
→ ldff a,(41) ; and a,07 ; jp lprint_a                      ; prints FF41 & 7 = the mode
```
Read-IO M-cycle offset from dispatch start: `_1`: M62 → **sample at G+248**; `_2`: M63 → **G+252**.
(The same 248/252 pair as the plain kernel `m2int_m3stat` family — the speedchange rows are the
kernel pair re-run behind the switch dance.)

### 4.3 Constraint algebra and the variant map

Let F = the measured line's visible m3→0 flip dot (SCX&7=2 ⇒ base+2). Per leg:
`_1` (out3): **F > G+248**. `_2` (out0): **F ≤ G+252**. Single pair ⇒ F ∈ (G+248, G+252] — a 1-M
bracket anchored to the mode-2 dispatch.

Poll-address table (read `_1` address; `_2` always +1):

| variant | e (enable shift, DS M) | s (STOP#2 shift, DS M) | scx→addr | conjunction pins |
|---|---|---|---|---|
| base (no lcdoff) | LCD on both switches | — | 2→0x1032, 3→0x1033 | F(scx2) = G+252 |
| lcdoff | 0 | 0 | 2→0x1032, 3→0x1033 | F(scx2) = G+252 |
| lcdoff_nop | 0 | 1 | 1→0x1033, 4→0x1033 | F(scx1) = G+253 ⇒ F(scx2) = G+254 |
| lcdoff_nopx2 | 0 | 2 | 2→0x1032, 3→0x1033 | F(scx2) = G+252 |
| nop_lcdoff | 1 | 0 | 2→0x1032, 3→0x1033 | F(scx2) = G+252 |
| nop_lcdoff_nop | 1 | 1 | 1→0x1033, 4→0x1033 | F(scx2) = G+254 |
| nop_lcdoff_nopx2 | 1 | 2 | 2→0x1032, 3→0x1033 | F(scx2) = G+252 |
| lcdoff2 (both switches LCD-off, enable after STOP#2) | — | — | 3→0x1033, 4→0x1034 | F(scx3) = G+253 |

(The scx1/scx4-same-address trick in the s=1 variants pins to 1 dot with a single address:
F(scx1) ∈ (G+252, G+253] ⇒ = G+253.)

Derived hardware laws (the family's whole point):
1. **s-law**: displacing STOP #2 by +2 dots (s 0→1) shifts the post-switch phase by exactly +2 dots;
   by +4 dots (s 0→2) shifts it by 0 (mod 4, absorbed by the dispatch quantizer). The DS→SS switch
   conserves the CPU-vs-PPU offset at 2-dot granularity — no snap to the SS 4-dot grid. Same
   statement as lcd_offset's offset3 law (§2 row 5), proven here on the enable-anchored frame.
2. **e-law**: displacing the LCD-ENABLE by +2 dots (e 0→1) changes NOTHING (identical addresses and
   scx sets). So the DS LCD-enable anchors the PPU frame on a 4-dot-quantized boundary — the
   half-SS-M part of the enable instant does not propagate. The phase that matters is
   (STOP#2 instant − enable) mod 4 dots, = 2 for the failing rows.
3. **base ≡ lcdoff(s=0)**: the LCD-on-through-both-switches dance nets the same mod-4 phase as
   off-switch + DS-enable + on-switch. (Anchors the two sub-families to one constant.)

### 4.4 Rows 6–9 (all `_2`, want 0 = mode 0 at G+252)

| row | e | s | constraint (hw) | slopgb |
|---|---|---|---|---|
| 6 `lcdoff_m2int_m3stat_scx2_2` | 0 | 0 | F(scx2) ≤ G+252 (and = G+252 by the scx3 sibling) | reads 3 → F ≥ G+253: ≥1 dot LATE |
| 7 `lcdoff_nopx2_m2int_m3stat_scx2_2` | 0 | 2 | same (s=2 ≡ s=0 mod 4) | same |
| 8 `nop_lcdoff_m2int_m3stat_scx2_2` | 1 | 0 | same (e irrelevant) | same |
| 9 `nop_lcdoff_nopx2_m2int_m3stat_scx2_2` | 1 | 2 | same | same |

The `_1` siblings (out3, F > G+248) pass trivially when F is late — again a one-sided phase error.
BRACKET: the `_1`(out3)/`_2`(out0) pair brackets the mode-3 exit of the measured line to
**(G+248, G+252]** = (dispatch+62 M, dispatch+63 M] with the exit exactly at the top edge
(or G+251.5 in a half-dot frame). slopgb's exit sits ≥ 1 dot above the bracket after this dance.

---

## 5. Synthesis — what the 9 rows jointly pin

1. All 9 are pure **CPU-grid-vs-PPU phase** measurements behind STOP dances; the measurement code is
   the already-green kernel machinery (poll loops exactly 1 line long; m2int_m3stat read offsets
   248/252 — identical to the kernel pair). Nothing else (sprites, window, palettes) is involved.
2. Hardware demands (leading-edge frame): lcd_offset offset1-ds → event = first_sample+2 dots (DS);
   offset3-SS → event = first_sample+1 dot (scx1); speedchange lcdoff s-even → flip = G+252 exactly.
3. slopgb's error is **direction-split by dance**: after the LCD-on 3-STOP / 2-STOP+nop dances
   (lcd_offset) its event is ≥1 M **early** vs its grid; after the lcdoff-anchored DS-enable + DS→SS
   dance (speedchange) its flip is ≥1 dot **late**. One scalar "k dots per STOP" cannot fix both;
   the model must reproduce (a) the per-switch PPU stall (SameBoy: 5 pre-T ⇒ 2.5 dots in DS — a
   half-dot quantum), (b) the **2-dot carry-through** of the DS→SS switch (no grid snap; the odd
   mode), and (c) the **4-dot quantization of the DS LCD-enable** anchor (e-law) — consistent with
   the #11bd keystone (`sb_dsa8` + leave-only advance k = dsa7==4 ? 6 : 2) being the right shape:
   the correction is conditional on the half-SS-M parity at the leave switch, exactly the s-even
   class these four speedchange rows occupy.
4. Both families' conjunctions land on integer-edge-degenerate solutions; a uniform half-dot offset
   (freeze=5 pre-T = odd) makes all margins ½ dot — further static evidence the post-dance phase is
   half-dot-valued, i.e. this is S6/half-dot-grid territory, not a whole-dot constant.

## Appendix: verification cribs

- Slide counts: m0irq 0x1004→0x1155 = 337; m0stat 0x1003→0x1155 = 338 (DS) / →0x10a4 = 161 (SS);
  speedchange 0x1004→0x1032 = 46 (`_1`) / 47 (`_2`).
- Loop sums: DS m0irq 3+2+2+1+3+2+(52·4+3)+1+3 = 228; DS m0stat 3+2+2+2+(53·4+3)+1+3 = 228;
  SS m0stat 3+2+2+2+(24·4+3)+3+3 = 114.
- offset1_ds m3stat_count siblings (not failing, context): poll@0x10fe/0x10ff cmp 82/83 → sample at
  dispatch+266 M → line-0 dot d0+76: brackets the visible mode-2→3 flip (dot 80) with the same
  machinery — the family measures BOTH mode-boundary edges of line 0 against the same dance.
- SameBoy refs: sm83_cpu.c:384–457 (`stop`), timing.c:432–524 (`GB_advance_cycles`; freeze at 477,
  dsa at 496–499), memory.c GB_IO_LCDC (~1505–1560; dsa reset on enable AND disable),
  sm83_cpu.c:1611–1633 (stopped/halted advance loop; 4 pre-T per iteration).
