# ly0 / lyc153int_m2irq — exact machine-cycle constraint tables (static analysis)

Sources: `/tmp/sbbuild/gambatte-src/test/hwtests/ly0/*.asm`, `/tmp/sbbuild/gambatte-src/test/hwtests/lyc153int_m2irq/*.asm`,
`/tmp/sbbuild/SameBoy-1.0.2/Core/display.c`, `/tmp/sbbuild/SameBoy-1.0.2/Core/sm83_cpu.c`.
No emulator was run; every number below is counted from the assembly + the SameBoy display state machine.

## 0. Harness conventions (verified from the asm)

- Gaps between `.text@` sections are zero-filled ⇒ **NOP slides**. The timing knob of every `_1`/`_2`
  leg pair is the *address* of the measurement instruction — the pair differs by exactly **1 NOP = 1 M-cycle**.
- `lwaitly_b` polls FF44 (6 M per iteration) — coarse. It only selects the *line*; all fine timing is anchored
  on the **STAT IRQ dispatch** (deterministic: the CPU is in a 1-M NOP slide when IF rises).
- IRQ dispatch = 5 M, pushes PC (exploited by the `pop af` re-entry discriminator in `late_retrigger`),
  then vector `0x48: jp lstatint` = 4 M ⇒ handler code starts at **M10** counting dispatch start as M1.
- `lprint_a` renders A as two hex digits (OCR'd). For FF0F reads, unused bits read 1 ⇒ raw IF prints as
  `E0 | (IF & 0x1F)`. `E0` = IF empty, `E2` = bit1 (STAT) set. `and a,03` legs print `IF & 3`
  (bit0 = VBlank — provably 0 in every leg below, see §per-row).
- M-slot notation: **Mk = k-th M-cycle after dispatch start (dispatch = M1..M5)**. SS: Mk occupies dots
  `D + 4(k−1) .. +3` where D = dispatch-start dot; DS: 2 dots per M.
- Instruction byte/M sizes used (verified against the byte addresses): `ld a,imm`=2B/2M, `ldff(nn),a`=2B/3M
  (IO **write commits in the 3rd M**), `ldff a,(nn)`=2B/3M (**read in the 3rd M**), `ldff a,(c)`=1B/2M (read in 2nd M),
  `ldff(c),a`=1B/2M, `xor a,a`=1B/1M, `pop af`=1B/3M, `and a,imm`=2B/2M, `jp`(taken)=3B/4M, `jp cc`(not taken)=3M,
  `ei`=1M (IME set after the *following* instruction), `nop`=1B/1M.

## 1. SameBoy ground truth: line-152/153/0 schedule (display.c)

The display coroutine advances in dots (456/line). Verified offsets:

**VBlank lines 144–152** (display.c:2165–2183, no model split):
| dot | event |
|---|---|
| 0 | `ly_for_comparison = -1`, STAT_update |
| 2 | `LY := current_line` |
| 4 | `ly_for_comparison := current_line`, STAT_update ⇒ **LYC=N IRQ rise at dot 4** |

**Line 153** (display.c:2231–2254):
| dot | DMG / CGB≤C single-speed | CGB≤C double-speed | CGB>C |
|---|---|---|---|
| 0 | lyfc=-1, STAT_update | same | same |
| 2 | LY := 153 | LY := 153 | LY := 153 |
| 4 | — | — | lyfc:=153 ⇒ rise |
| 6 | **LY := 0** (early reset, :2238) and **lyfc := 153 ⇒ LYC=153 IRQ rise** (:2241) | **lyfc := 153 ⇒ rise at dot 6** (LY still 153) | — |
| 8 | lyfc := -1 (:2246; STAT bit2 drops, but `lyc_interrupt_line` latch **holds** — GB_STAT_update:542 only clears it when lyfc≠-1 ⇒ no re-arm) | LY := 0; lyfc **stays 153** | LY := 0; lyfc stays 153 |
| 12 | **lyfc := 0** (:2250) ⇒ LYC=153 line **falls**; LYC=0 match **rises** (if LYC=0) | same | same |
| 12–24 | (CGB LYC-write side-effect window, :2252) | | |

So: DMG **LY reads 153 only during dots 2–5** (one 4-dot M) then 0; CGB-C DS reads 153 during dots 2–7.
DMG STAT coincidence bit for LYC=153: dots **6–7 only**; the STAT-IRQ line (LYC source) is high dots **6–12**.
(Corroborated by the sibling suites: `lycint152_ly153_{1,2,3}` → 98/99/00 at 1-M spacing; `lycint152_ly153_ds_{1..5}` → 98/99/99/99/00;
`lycint152_lyc153flag_{1,2,3}` → C1/C5/C1; `_ds_{1..4}` → C1/C5/C5/C1.)

**Line 0 after the wrap** (display.c:1773–1815):
| dot | event |
|---|---|
| 2 | oam_write_blocked (CGB) |
| 3 | LY := 0; `lyfc := 0` (line-0 keeps 0, `current_line? -1 : 0` :1790); **no `mode_for_interrupt=2`** — the `current_line != 0` branch (:1794) is skipped ("The OAM STAT interrupt occurs 1 T-cycle before STAT actually changes, **except on line 0**"); DMG additionally clears visible STAT mode bits |
| 4 | visible STAT mode := 2; **`mode_for_interrupt := 2` + STAT_update ⇒ line-0 mode-2 IRQ rise at dot 4** (:1807–1812); then `mode_for_interrupt := -1` + STAT_update in the same dot ⇒ the mode-2 source is a **0-length pulse** (sets IF bit1 on the edge; contributes no lasting level) |

Lines 1–143 raise the mode-2 IRQ at **dot 3** (1 dot before the visible mode change); line 0 at **dot 4**.
`ly_for_comparison` stays 0 continuously from line-153 dot 12 through line 0 ⇒ **a LYC=0 match raises NO fresh edge on line 0**
(the line-153 dot-12 rise is the only one); and with VBlank source disabled nothing else holds the line across the wrap.

**Key separations (the whole point of these two families):**
- LYC=152 rise (line-152 dot 4) → LYC=153 rise (line-153 dot 6): **458 dots = 114.5 SS M = 229 DS M**.
- LYC=153 rise (line-153 dot 6) → line-0 mode-2 rise (line-0 dot 4): **454 dots = 113.5 SS M**.
- The read/write probes in every leg pair are spaced 1 M; SS lines are 114 M. The two families therefore sit on
  **opposite half-M phases**: they jointly pin the CPU IF-sample/dispatch/read frame to sub-M (2-dot) resolution.

**SameBoy dispatch internals** (sm83_cpu.c:1611–1712) used below:
- The dispatch decision (`interrupt_queue = IE & IF`, :1633) samples IF as of the **last flush** = the leading edge
  of the previous instruction's last M-cycle (for a NOP slide: 4 T stale).
- ISR/CPU IO reads and writes flush-then-access ⇒ they sample/commit at the **leading edge of the access M-cycle**,
  and PPU events scheduled at exactly that dot run **first** (flush is inclusive).
- The dispatch **IF-ack** (`IF &= ~bit`, :1699–1702) commits at dispatch-start **+18 T** (pending is flushed at 5M−2T,
  then the bit is cleared) ⇒ 2 T into the 5th dispatch M-cycle; IF is re-sampled with the low-byte push in M4 (:1688).

## 2. Joint sub-M model (derived; consistent with all 20 legs of both families)

Unknowns: λ(φ) = dots from an IF rise at line-dot-phase φ (mod 4) to dispatch start; ρ/ω = read-sample/write-commit
offset within the access M-cycle; α = ack offset within dispatch M5.

Constraints extracted below give the unique consistent solution family:
- **ρ = ω = 0 (leading edge), PPU-events-first at a shared dot** (a rise at exactly the read edge IS visible — pinned by
  `lyc153irq_ds_2`; a CPU IF-write at exactly the rise dot WINS — pinned by `lyc153irq_ifw_ds_2`).
- **λ = rise→dispatch ∈ [1,3] dots**: the IRQ is sampled in the *final M-cycle of the current instruction with ~0-dot lag*
  (SS grid sits at line-dot ≡ 3 (mod 4) in these tests: a dot-6 rise dispatches at dot 7, a dot-4 rise at dot 7;
  DS grid even: a dot-4 rise dispatches at dot 6).
- **α (ack) ∈ [1,4] dots into dispatch M5** (SameBoy's +18 T = α 2 sits inside).
- Numerically: ly0 family requires λ(dot4-rise)+ρ ∈ **[2,6)** dots; m2irq family requires λ(dot6-rise)+ρ ∈ **[0,2)** dots;
  the 2-dot phase difference between the anchors makes both hold at once — and *only* at half-M resolution.
  DS legs independently require λ_ds+ρ_ds ∈ **[2,4)** dots.

Note on SameBoy-as-written: its dispatch view is one fetch (4 T) stale (:1633 samples pre-flush state), giving
λ_sb = E(rise)+4−rise ∈ [4,8) dots. That still satisfies the ly0 family for boot phases with λ_sb+ρ < 6, but makes
`lyc153int_m2irq_1` **unsatisfiable at any phase** (needs λ+ρ < 2): statically, SameBoy 1.0.2 reads 2 on both m2irq
base legs — `_1` (want 0) is a gambatte-reference/hardware row, not a SameBoy-pass row.

---

## 3. ly0 family — `lycint152_lyc153irq*`

### Common SETUP (SS legs)
`.data@143`=0x80 (DMG+CGB). Wait LY=0x96=150 → `STAT(FF41)=0x40` (LYC source only) → `IE=0x02` (STAT only) →
`IF=0` → `ei` → **`LYC(FF45)=0x98=152`** → `ld c,0f` → NOP slide toward 0x1000.
First IRQ: LYC=152 match, **rise₀ = line-152 dot 4**. VBlank IF bit0 was cleared at LY≈150 and cannot re-set before
line 144 ⇒ bit0=0 at every read. IME stays 0 inside the handler (no reti/ei) ⇒ no nested dispatch.

DS legs (`.data@143`=0xC0, CGB only): P1=0x30, IE=0, KEY1=1, `stop` (→ double speed) → wait LY=150 → STAT=0x40 →
**LYC=152 written before** IF=0/`ei` (no race: LY still 150/151) → ei. Reads use `ldff a,(0f)` (3 M) instead of `ldff a,(c)`.

### Common handler (plain legs)
```
lstatint:            ; M10 (dispatch M1-5 + jp@48 M6-9)
  ld a, 99           ; M10-11
  ldff(45), a        ; M12-14  — LYC := 153 commits at M14 edge = line-152 dot ~59 (SS) / ~32 (DS):
                     ;   lyfc=152 ≠ 153 ⇒ the STAT line FALLS here ⇒ edge re-armed for line 153
  <NOP slide>        ; from 0x1004
  ldff a,(c|0f)      ; the probe
  jp lprint_a
```
Second rise: **rise₁ = LYC=153 match at line-153 dot 6** (dot 6 on DMG-SS *and* CGB-C-DS, §1). rise₁ − rise₀ = 458 dots.

### Row 1: `lycint152_lyc153irq_2` (dmg08+cgb04c, outE2) — sibling `_1` outE0

TIMELINE (anchor D0 = dispatch M1 of the LYC=152 IRQ; derived D0 = line-152 dot 7):

| M-slot | instr | event | dot (derived) |
|---|---|---|---|
| M1–5 | dispatch | IF bit1 (rise₀) acked | — |
| M6–9 | jp 0x48→0x1000 | | |
| M10–11 | ld a,99 | | |
| M12–14 | ldff(45),a | LYC:=153 commits (M14 edge) | line-152 dot 59 |
| M15–112 (`_1`) / M15–113 (`_2`) | 98 / 99 NOPs (0x1004–0x1065/66) | | |
| **M114** (`_1`) / **M115** (`_2`) | ldff a,(c) read FF0F | read samples at M-edge **D0+452** / **D0+456** | line-153 **dot 3** / **dot 7** |

MEASUREMENT: raw IF print. E0 ⇔ bit1 clear at the sample; E2 ⇔ set. Only possible setter between the ack and the
read: the LYC=153 rise (rise₁, line-153 dot 6).

LEG DIFF: `_2` = `_1` + 1 NOP ⇒ read exactly 1 M (4 dots) later. Nothing else differs.

CONSTRAINT: **rise₁ ∈ (sample₁, sample₂] with the samples 458−6=452+ρ and 456+ρ dots after D0 = rise₀+λ.**
In dots: `2 ≤ λ(rise₀)+ρ < 6`. With the SameBoy schedule (rise₀ = L152d4, rise₁ = L153d6) and leading-edge reads:
`_1` reads at line-153 dot 3 (must be < 6 ⇒ E0), `_2` at dot 7 (must be ≥ 6 ⇒ E2).
**The failing `_2` demands: an FF0F read whose access M-cycle begins ≥ rise₁ (as little as 1 dot after the rise) must
already see IF bit1** — the rise→CPU-visibility latency of a LYC STAT rise is < 1 dot at the read edge.

### Row 2: `lycint152_lyc153irq_ds_2` (cgb04c, outE2) — sibling `_ds_1` outE0 [double-speed]

TIMELINE (DS M = 2 dots; derived D0 = line-152 dot 6):

| M-slot | instr | dot (derived) |
|---|---|---|
| M1–14 | dispatch+jp+ld+ldff(45) | LYC:=153 at M14 edge = line-152 dot 32 |
| M15–225 (`_1`) / M15–226 (`_2`) | 211 / 212 NOPs (0x1004–0x10d6/d7) | |
| **M228** (`_1`) / **M229** (`_2`) | ldff a,(0f), read in 3rd M | edge **D0+454** / **D0+456** = line-153 **dot 4** / **dot 6** |

rise₁−rise₀ = 458 dots = 229 DS M exactly; `_2`'s read edge lands **exactly on the rise dot**.

LEG DIFF: +1 NOP ⇒ read 1 DS M = 2 dots later.

CONSTRAINT: `2 ≤ λ_ds+ρ_ds < 4` dots. With leading-edge reads: `_1` samples at line-153 dot 4 (< 6 ⇒ E0),
`_2` samples at **dot 6 = the rise dot itself and must see it set** ⇒ pins the ordering *PPU-rise-then-CPU-read within
the same dot* (flush-inclusive). This is the tightest read-side pin in the family: **a read co-instantaneous with the
LYC=153 rise returns E2.**

### Row 3: `lycint152_lyc153irq_ifw_2` (dmg08+cgb04c, outE0) — sibling `_ifw_1` outE2

Handler adds `xor a,a` (M15) after the LYC rewrite; the probe becomes a **write-then-read**:

| M-slot | instr | dot (derived) |
|---|---|---|
| M15 | xor a,a | |
| M16–111 (`_1`) / M16–112 (`_2`) | 96 / 97 NOPs (0x1005–0x1064/65) | |
| M112–114 (`_1`) / M113–115 (`_2`) | **ldff(0f),a — IF := 0 commits at M114 / M115 edge** | line-153 **dot 3** / **dot 7** |
| M115–117 / M116–118 | ldff a,(0f) — read at M117 / M118 edge | line-153 dot 15 / dot 19 |

MEASUREMENT: E2 ⇔ the rise re-set bit1 *after* the IF:=0 commit; E0 ⇔ the write landed at/after the rise and cleared
it, and **nothing re-raises before the read** (the LYC line stays high dots 6–12 — a level, not a new edge; it falls at
dot 12 via lyfc:=0; edge-triggered IF ⇒ no re-set. LYC=153 never matches lyfc=0/line-0).

LEG DIFF: +1 NOP ⇒ the IF write commits 1 M later. **The write slots (M114/M115) are the exact M-slots where the
plain legs read** — write-commit and read-sample sit at the same sub-M position (both leading-edge).

CONSTRAINT: **rise₁ ∈ (commit₁, commit₂]**, same bracket as Row 1: `2 ≤ λ+ω < 6` dots. The failing `_2` demands:
an IF:=0 whose commit edge is ≥ rise₁ (1 dot after, on the derived phase) **clears the just-risen bit and it stays
cleared** — i.e. (a) the write beats/overrides the rise when ordered at ≤ its commit instant, and (b) strict edge
semantics: the still-high LYC line (dots 6–12) must NOT re-assert IF bit1 after the write.

### Row 4: `lycint152_lyc153irq_ifw_ds_2` (cgb04c, outE0) — sibling `_ifw_ds_1` outE2 [double-speed]

| M-slot | instr | dot (derived) |
|---|---|---|
| M15 | xor a,a | |
| M16–225 / M16–226 | 210 / 211 NOPs (0x1005–0x10d6/d7) | |
| M226–228 / M227–229 | **ldff(0f),a — IF := 0 commits at M228 / M229 edge** | line-153 **dot 4** / **dot 6** |
| M229–231 / M230–232 | ldff a,(0f) — read at M231 / M232 edge | dot 10 / dot 12 |

LEG DIFF: +1 NOP ⇒ write commits 2 dots later, from dot 4 to **exactly the rise dot 6**.

CONSTRAINT: `2 ≤ λ_ds+ω_ds < 4` dots. The failing `_ds_2` is the **same-instant collision pin**: the IF:=0 commit
edge coincides with the LYC=153 rise dot and the **CPU write must WIN** (PPU-rise-first-then-write order: rise sets
bit1, write clears it in the same dot) ⇒ E0. Together with Row 2 (`read at the rise dot sees it SET`) this orders one
dot's events precisely: *PPU rise → CPU access (read observes set / write forces clear)*.
No re-raise afterwards (read at dot 12 — the LYC line falls at exactly dot 12; edge semantics as Row 3).

---

## 4. lyc153int_m2irq family

### Common SETUP
`.data@143`=0x80. Wait LY=0x97=151 → **STAT=0x60 (LYC + mode-2 sources)** → (base legs: IE=02, IF=0, `ld a,99`,
`ei`, `LYC:=153`; ifw/late legs: LYC:=153 then IE/IF/ei) → NOP slide.
First and only automatic IRQ: **rise₁ = LYC=153 match at line-153 dot 6** (mode-1 is active from line 144 but bit4
source is disabled; mfi=1 ⇒ mode-2 source contributes nothing during VBlank).
LYC line falls at line-153 dot 12 (lyfc:=0) ⇒ re-armed. Next rise: **rise₂ = line-0 mode-2 pulse at line-0 dot 4**
(§1; NOT dot 3 — line-0 exception; NO LYC=0 edge on line 0 since lyfc is continuously 0 from L153d12).
rise₂ − rise₁ = **454 dots = 113.5 SS M**. IF bit0: cleared at LY≈151, next VBlank set at line 144 ⇒ 0 at all reads.

### Row 5: `lyc153int_m2irq_1` (dmg08+cgb04c, out0) — sibling `_2` out2

TIMELINE (anchor D1 = dispatch M1 of the LYC=153 IRQ; derived D1 = line-153 dot 7):

| M-slot | instr | dot (derived) |
|---|---|---|
| M1–5 | dispatch (acks rise₁) | |
| M6–9 | jp 0x48→0x1000 | |
| M10–112 (`_1`) / M10–113 (`_2`) | 103 / 104 NOPs (0x1000–0x1066/67) | |
| **M114** (`_1`) / **M115** (`_2`) | ldff a,(c) read FF0F at edge **D1+452** / **D1+456** | line-0 **dot 3** / **dot 7** |
| +1 M | and a,03 | |

MEASUREMENT: prints IF&3. 0 ⇔ neither bit set (STAT acked, mode-2 not yet re-risen, no VBlank); 2 ⇔ the line-0
mode-2 pulse already set bit1.

LEG DIFF: `_2` = `_1` + 1 NOP ⇒ read 1 M later.

CONSTRAINT: **rise₂ (line-0 mode-2, dot 4) ∈ (sample₁, sample₂]** ⇒ in dots `λ(rise₁)+ρ ∈ [0,2)` — the dispatch of
the LYC=153 IRQ plus the ISR read offset must total < 2 dots, i.e. **dispatch begins ≈1 dot after the dot-6 rise and
the read samples at its leading edge**. With the derived phase: `_1` reads at line-0 **dot 3** and must see bit1
CLEAR — pinning both (a) the line-0 mode-2 rise is at **dot 4, not dot 3** (the "except on line 0" 1-dot-late quirk;
a normal-line dot-3 rise would flip this leg to 2), and (b) no other edge crosses the 153→0 wrap (no LYC=0 edge; no
spurious mode/OAM edge at line-0 dots 0–3). `_2` reads at dot 7 and must see it SET.
This row is unsatisfiable if the ISR read frame or the dispatch latency is ≥ 2 dots too late (cf. §2 note: SameBoy's
own 4-T-stale dispatch view fails `_1` at every phase — hardware/gambatte is strictly tighter).

### Row 6: `lyc153int_m2irq_late_retrigger_1` (dmg08+cgb04c, out2) — sibling `_2` out0

Handler (both entries):
```
lstatint:              ; entry: dispatch 5M + jp 4M
  pop af               ; M10-12  discard pushed PC; A := PC_high
  and a, 10            ; M13-14  entry #1: PC_high ≤ 0x0F ⇒ Z; entry #2: PC ≈ 0x105e ⇒ 0x10 ⇒ NZ
  ldff a, (0f)         ; M15-17  A := IF (read at M17 edge)
  jpnz lprint_a        ; M18-20  (not taken on entry #1; taken on entry #2 → prints IF&07)
  ld a, 02             ; M21-22
  <81 NOPs `_1` / 82 `_2`>      ; 0x100a–0x105a/5b
  ldff(0f), a          ; IF := 0x02 — the manual "late retrigger"
  ei
  <nop>                ; ei latency
  → second dispatch
```

TIMELINE (anchor D1 = first dispatch M1 = line-153 dot 7, derived):

| event | `_1` M-slot | `_1` dot | `_2` M-slot | `_2` dot |
|---|---|---|---|---|
| IF := 0x02 commits | M106 edge | L153 d427 | M107 edge | L153 d431 |
| ei / NOP | M107/M108 | | M108/M109 | |
| second dispatch D2 | M109–113 | d439–458 | M110–114 | d443–462 |
| **IF-ack of D2** (dispatch+16+α T, α∈[1,4], SameBoy +18) | in M113 | **line-0 dot 1** | in M114 | **line-0 dot 5** |
| jp / pop / and | M114–122 | | M115–123 | |
| **read FF0F** (`ldff a,(0f)` 3rd M) | M125 edge | line-0 **dot 47** | M126 edge | line-0 **dot 51** |

MEASUREMENT: prints IF&7 read ~11 M after D2. The second dispatch acks (clears) bit1 — which was manually set at
M106/M107. out2 ⇔ the line-0 mode-2 rise (dot 4) landed **after** the ack instant and re-set bit1; out0 ⇔ the rise
landed **at/before** the ack and was consumed together with the manually-set bit (they OR into the same bit; one ack
clears both).

LEG DIFF: +1 NOP before the IF write ⇒ the whole tail (write, D2, ack, read) shifts 1 M (4 dots) later.

CONSTRAINT: **rise₂ ∈ (ack₁, ack₂] = (line-0 dot 1, dot 5]** (consistent with rise₂ = dot 4). Precisely:
the IF-clear of a dispatch commits **mid-way through the final (5th) dispatch M-cycle** (α ∈ [1,4] dots into M5;
SameBoy: dispatch+18 T), and (a) a STAT rise occurring ≥ 2 dots after that instant survives the ack and is readable
11 M later (`_1` out2 — the failing row: an emulator whose ack/dispatch frame sits ~1 M late, or whose line-0 mode-2
rise fires early, consumes the rise and prints 0); (b) a rise occurring in the *same M-cycle before the ack instant*
is consumed (`_2` out0) — the ack clears IF *after* the PPU delivered the edge, and the consumed edge must NOT
re-dispatch or re-set (the mode-2 source is a 0-length pulse; nothing re-raises before the dot-47/51 read).

---

## 5. Cross-family summary table

| quantity | pinned value | pinned by |
|---|---|---|
| LYC=N rise, VBlank lines 144–152 | line-N **dot 4** | lyc153irq anchor (+ lycint152_m2irq/m0irq siblings) |
| LY=153 visible window | DMG dots **2–5**; CGB-C-DS dots **2–7** | ly153 / ly153_ds siblings |
| LYC=153 rise on line 153 | **dot 6** (DMG SS and CGB-C DS; CGB>C dot 4) | rows 1–4 + lyc153flag siblings |
| LYC=153 STAT-line high window | dots **6–12** (coincidence bit 6–8 DMG / 6–12 DS); falls at 12 via lyfc:=0 | ifw legs (no re-raise) + lyc153flag |
| LYC=0 match on line 153 | rises **dot 12**; lyfc stays 0 through line 0 ⇒ **no line-0 LYC=0 edge** | lyc0irq/lyc0flag siblings |
| line-0 mode-2 IRQ rise | **dot 4** (vs dot 3 on lines 1–143), a 0-length pulse | rows 5–6 |
| rise → dispatch-start latency | **1–3 dots** (next M boundary, ~0-lag sample in the final M of the current instr) | rows 1,2,5 jointly |
| ISR IF read sample | **leading edge** of the access M-cycle, PPU-events-first (same-dot rise visible) | row 2 (`_ds_2`) |
| CPU IF write vs same-dot rise | **write wins** (rise-then-write order within the dot) | row 4 (`_ifw_ds_2`) |
| dispatch IF-ack instant | **+17..+20 T** after dispatch start (2 T into M5 in SameBoy); rise-in-same-M-before-ack is consumed | row 6 |
| half-M discriminators | rise₁−rise₀ = **458 d = 114.5 SS M**; rise₂−rise₁ = **454 d = 113.5 SS M** | the two families jointly — no whole-M read/dispatch frame satisfies both |
