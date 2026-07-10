# The half-dot (8 MHz) pixel-pipe reclock — constructive build plan

Status (2026-07-02 #11be): **§6 steps 2-4 EXECUTED (stages 2-4 of the goal.md
run). Part B is COMPLETE: `read_deferred` is the `GB_display_sync` analogue
(the grain resolves the PPU to the read's exact half-dot; `Ppu::read_pos_hd`
+ `isr_read_carry_hd` are THE read-position API), fp-dual-trace-gated — the
frame mapping is a UNIFORM +8 hd in-line BOTH speeds (the "+3 DS" was a
cfl-only artifact; §3B's read side needs no further motion). Part C is
COLLAPSED: the seven `vis_mode_read` shadow laws are ONE comparison
`read_pos_hd < vis_exit_hd(config)` (arms fold min/max per verdict class) and
the `early_lead` tower is two named residues (glitch +2, bare-cc2 parity);
the §2 law table is retired. Part A-render landed as the EMERGENT exit model
(SS `2*flip+2`, DS `2*flip−2+2*(SCX&1)` — anchored to the render's own
flip/projection, tracking mid-line SCX extends) rather than a mechanical
half-dot FSM re-step (no consumer needs sub-dot FSM transitions yet; the
grain + read API + exit model capture the physics). Part D re-derivations:
palette entry 84/release pipe_end+1 SS +0 DS; the straddle STAMPS
(PalAccess + M0Access reads + OAM write) bypassed tier2 — they are cc+4-frame
devices; DS line-END release extended to OAM writes; the CGB carryover hold
generalized (unshifted); the LYC-disable delayed view; the FF45 fire
edge-only under a non-LYC-held line. Result: two-bin 411→386 (−25/+0 ZERO
drops), blockers 78→58; 36 pins, mooneye 91/91 ON+OFF every commit, gbtr OFF
221/0. Stage 4's re-measure verdict: ENGINE-IF was NOT one class — three
clean write-frame slices came out (lycEnable −9, miscmstatirq −2); the
residue (58) is the S6/S7 core: WAKE-instant (halt 13 DMG + 5 CGB,
#11ay-confirmed 8-hd split), the window render-trigger race (late_wy_ds
co-temporal pairs), S6-DS (dma/speedchange/lcd_offset), and the ly0/lyc153
LYC-153 read-straddles. Detail: `ppu-subdot-ladder.md` "#11be" + the three
`measurements/*-2026-07-02.md` maps.**

Prior status (2026-07-02 #11bd): **the goal.md ONE-SESSION convergence run LANDED
five slices across attack items 1-5 (`phase-b-s7` `931e629`..`5bf6e69`):
the LCD-offset PHASE REPRESENTATION (item 1 keystone — `sb_dsa8` shadow of
SameBoy's `double_speed_alignment`, alignment-dependent leave advance
k = dsa7==4 ? 6 : 2, `lcd_shift_dots` + `law_pos()` un-shifted-frame law
classification, PORT-1 halves re-enabled), the CGB halt wake (item 2 — the
cc==4 hold is DMG/DS-only), the boundary-WY cross-line window extend
(item 4, +4/−0, pin 36) and the tier2 VRAM read-frame laws (item 5,
5 blockers → 1). Item 3 (ENGINE-IF FF0F) build-measured ATOMIC: the sub-M
FF0F sample + read-view visibility delay both A/B-swap (per-source offsets
{−4..−16} re-confirmed at mechanism level; scaffold deleted). Plus item 5b
(`521fe8d`): the first-frame palette lock (84) + the DS glitch-line m0-IRQ
arm (+5/−1). Flag-on two-bin 437→411; blockers **104→78** (+45 rebaseline);
36 pins; mooneye 91/91 ON+OFF; gbtr OFF 221/0. Two documented bonus-losses
(`speedchange2_nop_m2int_m3stat_scx1_1` + `ly0_m0irq_scx0_ds_2`, both
read-frame atomic). The remaining 78 sit in the measured-atomic classes —
the next constructive lever is Part A-render+B proper. Detail:
`ppu-subdot-ladder.md` "#11bd".**

Prior status (2026-07-02 #11bc): **the THREE PORTS are BUILT on the grain+write-strobe
(`phase-b-s7`): PORT 1 the unified half-dot bare-exit comparison (the #11ar DS
law rewritten exactly on the `2*dot + dhalf + carry < exit_hd` grid + the NEW
SS arm with the EMERGENT exit `2*flip + 2` from the render's own
`flip_dot`/`flip_projection`), PORT 2 the sub-M-cycle WAKE clock (the DMG halt
loop samples per 2 T like `GB_cpu_run`; a mid wake forgives the idle tail and
the skew is consumed by the handler's first FF41 read), PORT 3 the S6
completion frame (per-T serial completion + the exact SameBoy ack-squash
T-threshold). Flag-on two-bin 445→437 (+8/−0 ZERO drops), DMG halt 21→13
(+11/−3, CGB-two-bin-blind), 35 pins, mooneye 91/91 ON+OFF, gbtr OFF 220/0.
Defaults NOT flipped.** The measured w-couplings (the SS HOLD direction + the
m2-ISR carry + STOPADV leave-only w=4's +14/−11 vs the lcd-offset-frame
constants) pin the NEXT lever precisely: the Part-D lcd-offset constant
re-derivation on the +2-dot post-leave frame, co-landing w=4 + the E-hold +
the m2 carry (all three built, disabled, and documented at their sites).
Remaining blocker classes after the ports: window/wy_check race,
ENGINE-IF (~30 FF0F read-frame), lcd_offset (9), cgbpal/vram accessibility,
S6-DS conflict — see `ppu-subdot-ladder.md` "#11bc".

Prior status (2026-07-02 #11ba): **Part A-infra (the half-dot grain) LANDED — the first
structural reclock code since C1.3, byte-identical, `phase-b-s7` `5622329`.** The
8 MHz half-dot advance is now wired on the tier2 deferred path (`Ppu::tick_half`
+ `dhalf`; `advance_machine_t` runs the PPU per half-dot via `fold_ppu_events` on
the dot-completing half — reproducing the old `dot_ticks_on_cc` grid exactly);
production (`tick_machine`, whole-dot `tick`) untouched. Gate all-green: lib
660/660; 32 tier2 pins; mooneye flag-on 91/91; gbtr OFF 217/0 (production
byte-identical + 146 golden); flag-on two-bin ON 455 / OFF 486 (= base `6f375fe`,
unchanged). `sub_dot()` exposes the mid-dot read position (the Part B seam,
`#[allow(dead_code)]` until consumed). **The grain is the load-bearing foundation
the coupled render+read+write half-dot rewrite (Parts A-render/B/C/D, §3) stands
on — no prior session had it (they worked whole-dot).**

**The DS m3stat convergence mechanism, pinned empirically this session (dual-trace
`late_scx4_ds` + `m2int_scx4_ds`, `fp`):** slopgb COLLAPSES both legs of every
`_1`/`_2` pair to ONE read (both → slopgb `ly135 dot256 clk1264 mode3`,
identical), while SameBoy SEPARATES them across a HALF-DOT-resolved flip. Two
distinct DS sub-mechanisms, both needing the half-dot grain:
- **`late_scx4_ds`** = the mode-3 **LENGTH** differs (the late SCX write's half-dot
  commit lands on opposite sides of the fine-scroll comparator sample): `_1` read
  `cfl260 mode3 fp…848`, `_2` read `cfl261 dc-2 mode0 fp…850` — reads **2 fp
  apart**, the exit MOVES between them. Needs the half-dot **write-strobe**
  (`regs.rs::strobe_tick`/`commit_eff`) + half-dot fine-scroll sample (Part A-render + D).
- **`m2int_scx4_ds`** = the **READ POSITION** differs (same exit `cfl257 dc6`,
  both legs): `_1` read `cfl260 mode3 fp…848`, `_2` read `cfl263 dc-2 mode0
  fp…854` — reads **6 fp apart**, straddling the fixed flip. Needs the half-dot
  **read** landing each leg at its `fp` (Part B).
The SameBoy visible mode-3→0 flip lands at a genuine HALF-DOT (`SBMODE … cfl257
dc=2` for `late_scx4`, `dc=6` for `m2int_scx4` — the `dc` = `display_cycles`
half-dot remainder), which slopgb's whole-dot `line_render_done`/`visexit dot254`
cannot represent. **This is the direct empirical confirmation of §5's atomicity:
neither the read (`m2int_scx4`) nor the length (`late_scx4`) lever converges its
pair alone, and both live below the whole-dot grid — exactly what the half-dot
render+read+write rewrite (Part A-render + B + C + D) lands together.**

**#11bb (2026-07-02, same session): the WRITE-STROBE half of Part A/D LANDED +
Part C's DELETE executed. Five commits on `phase-b-s7` (`8c7e0bb` `4737011`
`d2c29a4` `724b9b4` after the grain `5622329`): flag-on 455→445, ZERO
SameBoy-pass drops, 33 pins, mooneye 91/91, gbtr OFF 217/0.** What landed:
- **Write-strobe render-frame deferral (Part A/D partial):** the #11as
  "deferred WRITE collapse" root-caused (`Ppu::write` cancelled the deferred
  stage → eff committed at the leading edge vs the +4-late-calibrated render
  geometry). FF43 stages 3 dots tier2, stage survives the arch write →
  **`late_scx4` SS+DS CONVERGED** (§5's flagship). Per-register: LCDC+4
  measured NET-NEGATIVE, WY inert — the deferral is SCX-only until the
  per-config exits land. Laws sample ARCH `self.scx`; glitch lines immediate.
- **DS pre-draw abort law:** boundary `(89+WX)&!1` (wx-DEPENDENT, the
  first-fetch M-cycle start on the DS 2-dot grid).
- **`SLOPGB_STOPADV` scaffold + the SameBoy STOP spec** (freeze=5T PPU
  withhold; single→double never reads `double_speed_alignment`; forced-even):
  K=2 half-dots = +21/−8 on speedchange — the residual is the per-scx
  HALF-DOT mode-3 exit (the #11ao parity wall) at the post-switch frame;
  odd-K (true `dhalf` skew) net-worse. **The clean lift = the per-scx
  half-dot exit port co-landing with the alignment.**
- **Part C DELETE (first half):** the five refuted env experiments removed
  (~250 lines; stat_irq.rs 1109→1030). The seven SHIPPED laws remain — they
  collapse only when the per-config render exits land (Part A-render).
**What blocks the flip (measured this session, not assumed):** the remaining
~112 SameBoy-pass blockers sit behind (a) the per-scx half-dot mode-3 exit
port (speedchange/window/m3stat parity pairs — every whole-dot lever built
this session A/B-swapped on scx parity), (b) the WY-write↔`wy_check` race
inside the render trigger (late_wy — the fallback vis-HOLD was +4/−4), (c)
the FF0F IF-delivery read-frame (ENGINE-IF ~30, offsets −4..−16 non-uniform),
(d) the sub-M-cycle halt wake (12), (e) S6-DS conflict/completion (~20).
(a) is now BUILDABLE on the shipped grain + write-strobe: the render exits
must move to per-config half-dot positions WITH the read at its true
half-dot — Part A-render + B proper, the next session's single target.

Prior status (2026-07-01 #11az): confirmed the base (`6f375fe`): flag-on 455 /
off 486 → 165 flip-BUGs = **115 SameBoy-pass + 50 rebaseline**; extracted the
definitive SameBoy porting spec (§1) and produced this plan.

`fp = absolute_debugger_ticks − display_cycles` is the SameBoy time axis for
every measurement (NEVER `cfl*2+dc`, which is non-monotonic — #11ay).

---

## 1. The SameBoy per-tick order (the spec being ported)

Citations `file:line` against `~/.cache/sbbuild/SameBoy-1.0.2/Core/`.

**Two clocks, one divisor.** `GB_advance_cycles` (timing.c:432) normalises CPU
T-cycles to an 8 MHz budget: single speed `cycles <<= 1` (timing.c:478–480) so
each CPU-T is **2 half-dots**; double speed is fed un-doubled (**1 half-dot per
CPU-T**). `GB_display_run` runs the PPU as a divisor-2 coroutine
(`GB_BATCHABLE_STATE_MACHINE(gb, display, cycles, 2, …)`, display.c:1615): logic
is authored in **4 MHz dots** (`GB_SLEEP(…,N)` costs `N*2` half-dots), the
budget/accounting is **8 MHz half-dots**. **The PPU dot-rate is fixed across
speed** — DS changes only the input pre-doubling, never the divisor.

**The two-latch decoupling (`GB_STAT_update`, display.c:523–574).** `STAT&3`
(what FF41 returns) and `mode_for_interrupt` (what sources the STAT IRQ) are
**independent latches**. The STAT-IRQ line = `(mode_for_interrupt`-selected STAT
enable bit`)` OR `(STAT&0x40 && lyc_interrupt_line)`; `IF|=2` fires only on its
**rising edge** (`line && !previous_line`, :567–572). **`mode_for_interrupt == 3`
(and the `-1` sentinel) select `default → false`: mode 3 never sources a STAT
IRQ.** LYC compares against `ly_for_comparison`, never `LY` (:537).

**The IRQ swing.** Mode-2 IRQ fires **1 dot before** the visible mode→2 edge:
state 6 sets `mode_for_interrupt=2; STAT&=~3; GB_STAT_update` (display.c:1794–
1801) — IRQ up while FF41 still reads mode 0 — then `GB_SLEEP(7,1)` and state 7
raises the visible `STAT|=2` (:1805). Mode-0 IRQ fires **1 dot after** the
visible edge: single speed sets visible `STAT&=~3; mode_for_interrupt=0` with
**no** `GB_STAT_update` (:2104–2111), then `GB_SLEEP(22,1)`, then the raising
`GB_STAT_update` (:2116–2122). In DS the pre-block is skipped → visible edge and
IRQ coincide.

**Mode-3 length is emergent.** The pixel-transfer FSM ends at
`position_in_line == 160` (display.c:2048); each iteration burns one dot
(`GB_SLEEP(21,1)`, :2050) plus sprite penalties (`GB_SLEEP` 27/41/20/39/40) and
the SCX fine-scroll drops in `render_pixel_if_possible` (:700–718). A closed-form
fast path `mode3_batching_length` returns `167 + (SCX&7)` for trivial
(objectless, windowless) lines (display.c:1507), else 0 (fall to the FSM).

**`wy_check` (display.c:508–521)** latches `wy_triggered` sticky-true for the
frame on `WY == comparison` (`current_line` on CGB-single-speed else
`ly_for_comparison`), gating every window activation → mode-3 length.

**The CPU read (`read_high_memory`, memory.c:540).** FF00–FF7F reads call
`sync_ppu_if_needed → GB_display_sync = GB_display_run(gb, 0, true)`
(display.h:51) — a **zero-cycle force run** that drains the prologue
(wy/overflow/delayed-hblank) so mode/LY/STAT are current **at the read's exact
T**, then returns `STAT&3 | 0x80` (memory.c:632). This is the whole game: **the
read observes the visible mode at the exact half-dot the CPU samples.**

---

## 2. The slopgb model today (what is wrong, precisely)

| SameBoy | slopgb today | seam |
|---|---|---|
| PPU advances per 8 MHz half-dot | `Ppu::tick()` advances **one whole dot** (`ppu/mod.rs:940`); `advance_machine_t` runs T-by-T but each dot-ticking cc runs a **whole** `tick_machine_dot` (`interconnect/tick.rs:241`) | `ppu/mod.rs::tick`, `interconnect/tick.rs::advance_machine_t` |
| FF41 read = 0-cycle sync to exact half-dot, return `STAT&3` | `read_deferred` advances to `clock.now()` in **whole dots**, reads a whole-dot mode (`interconnect/cycle.rs:147`) | `interconnect/cycle.rs::read_deferred` |
| Emergent mode-3 length, half-dot precise | Emergent **whole-dot** length (`render_step`, `ppu/render.rs:367`); `m0_flip_events` projects the pipe end (`ppu/render/mode0.rs:82`) | `ppu/render.rs`, `ppu/render/mode0.rs` |
| Visible flip vs dispatch = two latches 1 dot apart | The `early_lead` **case-tower** (`mode0.rs:212–279`) + **seven** `vis_mode_read` **shadow laws** (`stat_irq.rs:29`) hand-fit the whole-dot flip vs the counter-pinned dispatch | `mode0.rs`, `stat_irq.rs::vis_mode_read` |
| `mode_for_interrupt` a first-class latch | present + decoupled (`update_mode_for_interrupt`, `reclock.rs`) but the *visible* side is the tower | `ppu/stat_irq/reclock.rs` |
| sub-cc positions native | the `event_phase`/`lead_eighths`/`ACCESS_PHASE` **eighths scaffold** (`interconnect.rs:45–200`) — the correct-but-insufficient stamp approximation, retired at S7 | `interconnect.rs` |

**The seven `vis_mode_read` shadow laws that the correct half-dot render length
SUBSUMES** (`ppu/stat_irq.rs`, all tier2+CGB, byte-identical OFF) — each exists
*only because the whole-dot render length is wrong* for one window/read config:

1. Triggering-window length law `SBex = 263+SCX&7`, read `259+SCX&7` SS /
   `260+SCX&7` DS (#11z/#11ag).
2. Shadow late-WY extend for polled reads (#11af), + its DS exit `264+SCX&7`
   (#11ag).
3. CGB pre-draw window-abort bare-exit (`win_predraw_abort`, #11at).
4. CGB window-REENABLE length (`win_reenable_dot`, #11au).
5. CGB late-WY UN-trigger bare (`wy_trig_sb_raw`, #11aw).
6. The scoped carried-read exit + full per-read SBex carry for bare mode-3 FF41
   (#11ar).
7. The `m0stat` line-start read-frame slice (#11ar).

Plus five **refuted, env-gated, dead** experiments to DELETE at the rewrite:
`BARELAW` (+23/−27), `HDEXIT`, `CARRYOVR`/`M2CARRY`, `WAKEPEEK` (+3/−13),
`M2HOLD` (−50), `DSM2DELAY` (+29/−26). The `early_lead` tower in `mode0.rs` is
the same approximation on the write/render side.

---

## 3. The build (four coupled parts; all tier2-gated, production byte-identical)

### Part A — half-dot PPU + render-FSM advance
Make the PPU advance in 8 MHz half-dots. `dot_phase` (interconnect.rs:367, the
inert scaffold) carries the half-dot offset; wire it.
- `interconnect/tick.rs::advance_machine_t`: today `tick_machine_dot(cc)` runs a
  whole dot per dot-ticking cc. Split into **two half-dot substeps** per dot (SS:
  2 half-dots per CPU-T; DS: 1). The mode flip / STAT update / IF raise fire at
  their exact half-dot **inside** the advance, in SameBoy's order (mutate → 
  `GB_STAT_update` → advance), not batched to the dot/M-cycle end.
- `ppu/mod.rs::tick` + `step_dot`: convert the per-dot FSM to a half-dot FSM.
  The fetcher already steps at 2 dots/read (`FetchPhase` wait states,
  `render.rs:88`) — at half-dot that is 4 half-dots/read; the SCX fine-scroll
  comparator hunt (`render.rs:385`, `mode3_dot`/`prefill_pos`) and the pixel pop
  (`render.rs:502`) advance per half-dot. This is what gives the mode-3 length
  **half-dot precision** — the `late_scx4` legs' 8-half-dot flip split (§5).
- The dispatch dot **stays put** (counter-pinned; a PPU machine-advance at
  dispatch HANGS mooneye intr_2/int_hblank/di_timing, B=42). Move only the sub-T
  phase of read↔flip↔IF↔wake, via the decoupled `mode_for_interrupt`.

### Part B — reads sample at the deferred clock's exact half-dot
`interconnect/cycle.rs::read_deferred` already advances to `clock.now()` (the
M-cycle leading edge). Resolve the PPU to the **half-dot** at that T (not the
rounded whole dot) and return the register/accessibility verdict as of that true
half-dot, BEFORE the post-access advance — the slopgb analogue of
`GB_display_sync`. Use `fp` to pin each read to SameBoy's half-dot. This is the
read half of the `late_scx4` separation (reads 2 half-dots apart).

### Part C — collapse the flip approximation into the two-latch model
Replace the `early_lead` case-tower (`mode0.rs:212–279`) and all seven
`vis_mode_read` shadow laws (§2) with the single principled boundary: the
CPU-visible mode 3→0 flip is where the emergent **half-dot** render length ends
(SameBoy's `STAT&=~3`, 1 dot before the mode-0 IRQ in SS / coincident in DS);
`mode_for_interrupt` flips at the counter-pinned dispatch. `vis_mode_read`
becomes: return `STAT&3` resolved at the read's half-dot (Part B) against the
half-dot render boundary (Part A). The shadow laws stop being needed *as the
half-dot render becomes correct* — do NOT keep both.

### Part D — re-derive every whole-dot boundary constant to the half-dot frame
| constant | file | today (dots) | half-dot re-derivation |
|---|---|---|---|
| `vis_early` / `early_lead` tower | `mode0.rs` | case tower 0–4 | deleted → emergent (Part C) |
| `line_render_done` / `m0_src` proj lead | `mode0.rs:82` (`lead` 2/1/0) | whole-dot proj−lead | half-dot pipe-end; dispatch pinned |
| `mode3_entry_dot` / render start (84, glitch 82) | `mod.rs:1114`, `render.rs` | dot 84 | ×2 = half-dot 168 (+ glitch offsets) |
| `LINE_DOTS` 456 / `GLITCH_LINE_DOTS` | `mod.rs` | 456 | 912 half-dots |
| `SCAN_OFF` 3 / `scan_latch_dot` | `render.rs:296` | 2·i+3 dots | half-dot re-anchor (the `_ds` siblings resolve here) |
| wy_latch dots 450/454 + `late` | `mod.rs:1093` | dots | half-dot; `wy_check` continuous compare |
| `halt_ly_phase` `HALT_LY_PHASE_BY_CC` | `tick.rs:131` | per-cc dots | sub-M-cycle wake half-dot (WAKE class) |
| `m0_halt_hold` | `tick.rs:144` | M-cycles | half-dot re-derive |
| C0 DIV `div += 4` | `interconnect/boot.rs` | +4 T | re-validate for the half-dot read frame |
| OAM/VRAM/palette locks (SS+DS) | `ppu/blocking.rs` | dot bounds | half-dot bounds; retire the `event_phase` stamps |
| `ACCESS_PHASE`/`event_phase`/`lead_eighths` | `interconnect.rs` | eighths scaffold | delete once the native half-dot subsumes it |

---

## 4. Per-class landing (all 5 classes, `fp` gives the target half-dot)

- **RENDER-LENGTH (41)** — window 17 / cgbpal 7 / vram 5 / oam 3 / enable_display
  6 / scx_during 1 / m2int_m3stat 2. Part A (half-dot render length) + Part B
  (half-dot read) + Part C (collapse the shadow laws). `late_scx4`: the SCX write
  is observable (`_1`@fp…818 / `_2`@…810, straddling the fine-scroll drop) → the
  half-dot pipe ends 8 half-dots apart, the two reads (2 apart) land on opposite
  sides.
- **WAKE-CLOCK (12)** — halt `*_m0stat`. Part A's half-dot wake: the want-0 legs
  read at the line-2 mode-2 rise, the want-2 legs 8 half-dots later; slopgb
  quantises the wake onto the wrong dots (`1a/2a`→ly2 dot4, `3b`→ly2 dot0 —
  opposite sides). The `halt_ly_phase` analogue for FF41-mode at half-dot.
- **READ-FRAME (12)** — cgbpal/serial/tima/m2int/irq_precedence. Part B lands
  each read at its `fp`; serial/tima additionally need the S6 deferred-
  **completion** frame (the leading-edge FF0F read samples IF as of the previous
  M-cycle; the completion lands 1 M-cycle late).
- **ENGINE-IF (30)** — lycEnable 11 / miscmstatirq 5 / ly0 4 / m2enable 3 / m1 3
  / m0enable 2 / lyc153int 2. The STAT edges already fire at the right lines
  (±dots); the ISR read straddles — resolved by Part B + Part C once the read
  half-dot + the two-latch flip are correct.
- **S6-DS (20)** — speedchange 7 / lcd_offset 7 / dma 6 (+ DS serial/tima). The
  half-dot advance in DS (divisor unchanged, §1) + the S6 conflict-write /
  completion reconciliation (`sm83_cpu.c:131–318`, the `cycle_clock::Conflict`
  table already banked but discarded by `Bus::write`).

---

## 5. Why it is one atomic landing (worked proof, not assertion)

**`late_scx4` (RENDER-LENGTH), the sharpest case.** The two legs read 2 half-dots
apart (same program point); SameBoy's discriminator is the mode-3 FLIP, 8
half-dots apart, set by which side of the fine-scroll drop the SCX write lands.
- Half-dot render length **alone** (Part A, whole-dot reads): both legs' reads
  land at the same whole dot D. If the two half-dot flips are at D.5 and D.5+4,
  a whole-dot read at D sees both as mode 3 (or both mode 0) → **A/B swap**.
- Half-dot read **alone** (Part B, whole-dot render): both legs render to the
  same whole-dot flip; two half-dot reads 2 apart land the same side → **A/B
  swap**.
- **Both** → `_2` reads mode 0, `_1` reads mode 3, together. This generalises:
  every RENDER-LENGTH pair needs render∧read at half-dot; WAKE needs the half-dot
  wake ∧ read; READ-FRAME needs the read position ∧ (serial/tima) completion. The
  115 span `{−20..+18}`-dot per-class offsets, opposite-signed by register (FF41
  mode `+`, FF0F IF-delivery `−`) — **no single lever, and no flag-gated subset,
  moves them without dropping a SameBoy-pass** (measured 14× across #11ai–#11au:
  BARELAW +23/−27, M2HOLD −50, DSM2DELAY +29/−26, HDEXIT, WAKEPEEK +3/−13,
  halt_mode_phase +5/−13, raw-WY +1/−27, PALLOCK84 +4/−2, the read-position carry
  +9/−0 then exhausted). The reclock is not a slice — it moves ALL reads to
  SameBoy's frame at once. Intermediate states are RED; converge the whole thing,
  then measure clean.

Corollary: **the dispatch stays counter-pinned** (mooneye holds 91/91 only if the
IRQ dispatch dot does not move), so Parts A–C move the *sub-T phase* of
read↔flip↔IF↔wake via `mode_for_interrupt`, never the dispatch itself.

---

## 6. Recommended staging for the multi-session build

The convergence is atomic, but the build is testable in an order that keeps the
flag-off path byte-identical throughout and validates each part against `fp`
before the joint landing:

1. **A-infra (byte-identical): ✅ LANDED #11ba (`5622329`).** `advance_machine_t`
   now advances the PPU in half-dots (`Ppu::tick_half` + `dhalf`,
   `fold_ppu_events` on the completing half); all transitions still whole-dot →
   flag-on == today flag-on. Net-zero gate met (mooneye 91/91, two-bin 455/486
   unchanged, 32 pins green, gbtr OFF 217/0 byte-identical). `sub_dot()` = the
   Part B seam. **Do NOT use `dot_phase` for the initial offset — it stays 0 (the
   aligned even-cc grid); the grain is carried by `dhalf` on the PPU, persisted
   across the fractional `advance_machine_t` spans so a DS mid-dot read lands at
   `dhalf==1`.**
2. **B (read sync):** resolve `read_deferred` to the half-dot; validate the FF41
   read dot lands at SameBoy's `fp` on the kernel + m2int_m3stat via single-ROM
   dual-trace. Expect an A/B two-bin (that is the point — it is half the pair).
3. **A-render (half-dot length):** move the render boundary to half-dot; delete
   the `early_lead` tower + the seven shadow laws in the SAME step as C. With B
   already in, the RENDER-LENGTH class converges (`late_scx4` first). This is the
   convergence point — expect RED before it, GREEN (for RENDER-LENGTH) after.
4. **WAKE + READ-FRAME + ENGINE-IF:** the half-dot wake (`halt_ly_phase`
   analogue), the serial/tima completion frame, then re-measure ENGINE-IF (it
   should fall out of B+C).
5. **S6-DS:** the conflict-write table (`cycle_clock::Conflict` → consumed by
   `Bus::write`) + DS half-dot.
6. **The flip (C3), one commit:** `lib.rs:66` `new_inner(…, false)` → `true`;
   rebaseline `tests/gbtr/baselines/gambatte.txt` (the 50 rebaseline rows join
   the floor, the 115 PASS); split `ppu/mod.rs` (>1000). **Only when 115 converge
   ∧ 146 golden clean ∧ every oracle zero-drop.**

Method every iteration (never assert — surveys/levers overturned ≥14×): rebuild
SameBoy (`tools/build_sameboy_tracers.sh`), flag-on full-CGB two-bin
(`flagon_probe` ON vs OFF → `classify_cgb_regr.py`), mooneye flag-on gate
(`SLOPGB_MOONEYE_RECLOCK`), single-ROM dual-trace on `fp`. NEVER drop a
SameBoy-pass; NEVER move the dispatch dot; production byte-identical OFF.

---

## 7. Tooling

- SameBoy tester + `fp`-emitting tracers:
  `tools/build_sameboy_tracers.sh` (idempotent, survives `/tmp` wipe) →
  `~/.cache/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester` (`--cgb --length
  4`, `SB_TRACE=1`; SBMODE/SBREAD/SBPALR/SBWSCX emit `fp=`).
- Two-bin: `flagon_probe` (`crates/slopgb-core/tests/gbtr/gambatte_flagon_probe.rs`,
  `#[ignore]`) with `SLOPGB_ROWLIST` (3422 CGB rows), `SLOPGB_PROBE_OFF=1` for the
  OFF bin, `SLOPGB_S5DBG=1` for the slopgb ff41/ff0f/pal/oam/vram tracers.
- Classify: `tools/classify_cgb_regr.py <flipbugs.txt>` → SameBoy-pass (fix) vs
  rebaseline. Blocker list regenerated this session:
  `measurements/c2-halfdot-build-plan-2026-07-01.md`.
