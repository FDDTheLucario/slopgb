# The window-Y trigger, and the FF41 read-law engine it feeds

The FF41 read-law engine (`ppu/stat_irq/read_laws.rs` + `read_laws_exit.rs`) is a
compensating layer: it computes what a STAT read *should* observe because the
render does not produce it. This file records what each part of it is actually
compensating for, and the state of the work to delete it. The window-Y third is
done; the other two are not.

## The two defects behind the arm table, separated

The arms do not all have the same cause. They split cleanly:

| arms | compensating for | fixable at whole-dot? |
|---|---|---|
| ~~2, 6, 7, D6, D-wx0 + `win_extends_sb` / `win_extend_deadline`~~ **DELETED** | the window-Y trigger was sampled at fixed per-line dots instead of SameBoy's scheduled compare | **done** — it was a render fix, not a read fix |
| 3, 4, 5, D3, D5, D-wx, 3b | the mid-line window enable / disable / WX-rewrite fetcher slot is not modelled | yes, in principle |
| 8, 8-spr + `read_pos_hd` | the CPU-visible mode-3 exit sits at a half-dot the whole-dot render cannot represent | no — needs the half-dot render FSM |

Only the third row is what the "sub-dot re-clock" fixes. The first two are
ordinary render bugs wearing a read-law costume — the first is now fixed at the
render, where it belonged.

## What the window-Y trigger really is (SameBoy `wy_check`)

`display.c:508`. `wy_triggered` is a frame-sticky latch set when the window is
enabled and WY equals the PPU's comparison line — `current_line` on CGB single
speed, `ly_for_comparison` on DMG and in double speed. Every window activation
gates on it (`check_window`, `display.c:1315`), so the mode-3 length it produces
is the render's own.

The compare is **not** continuous, and it is **not** at fixed line dots. It runs:

1. at the line's start, before mode 2 (`display.c:1755`);
2. again at the mode-2 rise, once `ly_for_comparison` holds the line
   (`display.c:1815`);
3. **once more after any WY or LCDC write** — `memory.c:1459`/`:1570` set
   `wy_check_scheduled`, and `display.c:1557-1578` runs the compare
   `8 - ((wy_check_modulo + K) & 7)` half-dots later, i.e. 1-8 half-dots out,
   landing on a fixed 4-dot phase (`K` = 0 CGB single speed, 2 DMG, 6 double
   speed).

Point 3 is the whole story of the `late_wy` family. A mid-line WY write gets its
own compare a beat later, so it can trigger — or un-trigger — the window on the
very line it lands in. slopgb instead samples `win_en && WY == LY` at gambatte's
three weMaster dots (line 0 dot 2, dots 450/454) and separately runs a lagged
`wy2` live compare at the WX match, which catches neither the mid-line write nor
the line-boundary one. Arms 2/6/7/D6/D-wx0 and roughly 150 lines of FF4A
special-cases in `regs.rs` exist only to paper over that.

## Port status: LANDED

The three shadow latches (`wy_trig_sb` / `wy_trig_sb_raw` / `wy_xline_trig`,
plus `wy_trig_sb_line`/`_dot`), the gambatte weMaster sampler `wy_latch`, the
lagged copy `wy2`/`wy2_delay`, the ~150 lines of FF4A boundary/un-latch
special-cases in `regs.rs`, and read-law arms 2, 6, 7, D6, D-wx0 with
`win_extends_sb`/`win_extend_deadline` are all GONE. `read_laws_exit.rs`
808 → 598 lines. The FF4A write handler is SameBoy's two lines,
`wy = value; schedule_wy_check()`.

What replaces them: `wy_triggered` + `wy_check_in` (a half-dot countdown), and
`wy_check`, `wy_comparison`, `schedule_wy_check`, `schedule_wy_check_at`,
`wy_check_scheduled_tick`, `win_activation_lead`, `wy_triggered_for_activation`,
plus `Render::win_pending_until`.

### What the dual trace settled

Built with the SameBoy tracers extended by `SBWYCHK` (every `wy_check`),
`SBWINACT` (every window activation, with the window row counter) and `SBWWY`
(every FF4A write) — all three now in
`docs/sameboy-port/tools/build_sameboy_tracers.sh`.

* **The write frame agrees.** On `late_wy_FFto2_ly2_scx5_1` both emulators land
  the WY write at dot 96 and its scheduled compare at dot 100.
* **The compare runs before the line wrap.** SameBoy's scheduled-check block
  sits at the top of `GB_display_run`, ahead of the line-length rollover, so a
  tail write compares against the OLD line (`late_wy_FFto0_ly2_1`: hit at
  `ly0 cmp=0`, then line 1's own compares miss but the latch is sticky). That is
  the whole of the retired `wy_xline_trig`.
* **slopgb's WX comparator leads SameBoy's activation by the fine scroll.** For
  WX <= 7 slopgb matches at a fixed `pos_dot == WX + 6` while SameBoy's
  `position_in_line` waits the SCX discard out: activation at dot 97, 99, 100,
  102 for SCX&7 = 0, 2, 3, 5, against slopgb's 97 throughout. WX >= 8 matches on
  `lx`, which has already absorbed the discard, and needs no lead (`..._wx0f`:
  both at 105). A compare landing exactly ON the activation instant does not
  make it (`..._scx3_2`) — the bound is strict.
* **The activation test is live, not a single sample.** SameBoy re-tests
  `wy_triggered && WX == position_in_line + 7` every dot until it fires, so a WY
  write landing BETWEEN slopgb's match and SameBoy's instant still catches the
  line (`late_wy_FFto2_ly2_scx5_ds_1`: slopgb's match at dot 97, write at 98,
  compare at 99, SameBoy's activation at cfl 102). `Render::win_pending_until`
  keeps the match live across that span.
* **An LCDC write's compare lands one dot later than a WY write's.** Dual-traced
  on `late_enable_afterVblank_2`: SameBoy takes the bit-5 enable at the line
  boundary (`ly0 cfl0`) and its compare falls on the NEXT line — a miss, so the
  window never draws — while the same write reaches slopgb at `ly0 dot452`.
* **The scheduled-compare phase `K`.** SameBoy's own values are 0 (CGB single
  speed) / 2 (DMG) / 6 (double speed); measured against slopgb's write frame as
  0 / 0 / 2. DMG `K = 0` also fixed `eager_dmg_lyc153_cluster_passes`.

### Rows recovered

Three rows left the floor: `late_scx_late_wy_FFto4_ly4_wx20_3 [Dmg]`,
`late_enable_afterVblank_ds_1` and `late_enable_afterVblank_ds_lcdoffset1_1`
(both [Cgb]) — all SameBoy-PASS, i.e. genuine must-fix floor rows before this.

### Rows floored, and why

Four joined the floor (class W in `baselines/gambatte.txt`, each with its lift
condition). Three are **SameBoy-FAIL**: SameBoy's own FF41 read disagrees with
the ROM, and slopgb passed them before only by over-fitting past SameBoy in the
`vis_exit_hd` arms — the exact rule-bend this port exists to remove.
`late_wy_ds_1` is dual-traced end to end: both emulators latch at `ly0` and
activate at `ly0 cfl97`, and SameBoy reads mode 3 at `cfl262` where the ROM
wants 0.

The fourth, `late_scx_late_wy_FFto4_ly4_wx00_2 [Cgb]`, is SameBoy-PASS but its
cause sits outside the window machinery: slopgb's kernel runs 4 dots early on
the mid-line-SCX-rewrite path (traced — SCX write at line dot 76 against
SameBoy's 80, WY write 88 against 92), so the scheduled compare beats an
activation instant SameBoy's misses. The read-law arms were masking that
pre-existing phase error; its `_wx20_2` sibling was already floored for it.

### The `win*_b` golden drift: reviewed, benign

The re-captured `golden_fingerprint` moves 25 rows: 7 gambatte window rows whose
verdict changed, and 18 `gbmicrotest/win*_b [Cgb]`. No commercial-game row
moved. The `win*_b` set was checked three ways before re-capturing:

* slopgb HEAD, slopgb+port and SameBoy all activate the window on every visible
  line at dot 95 with the same window row counter (2304 activations per 16
  frames in both slopgb builds) — HEAD and the port are byte-identical there, so
  the drift cannot come from the window trigger;
* no `win*` row is baselined in `baselines/gbmicrotest.txt` and every one still
  passes — their verdict is a HRAM value, not the screen;
* the changed pixels are the on-screen echo those ROMs draw, downstream of the
  FF41 read verdict they poll.

Also ruled out: the LCD-enable glitch line (suppressing `wy_check` there took
the drift from 27 to 47) and any `win_line` off-by-one.

### Probe

`gambatte/window/**/*wy*` runs in ~10 s off the debug runner:

```sh
cargo build -p slopgb-core --release --example run_gambatte
# then, per ROM: target/release/examples/run_gambatte <rom> [dmg|cgb]
# expectation is in the filename: _dmg08_out<H> / _cgb04c_out<H> / _dmg08_cgb04c_out<H>
```

Five of the 120 are already in `baselines/gambatte.txt`
(`late_scx_late_wy_FFto4_ly4_wx20_2 [Cgb]`, `..._wx20_3 [Dmg]`,
`late_wy_lcdoffset1_1 [Cgb]`, `arg/late_wy_1 [Cgb]`, `window/late_wy_1 [Cgb]`).

## Rule note

Nothing here special-cases a test ROM: the mechanism is SameBoy's, with its own
citations, and it lives in the render where the hardware puts it. This was the
first of the three deletions that retire the arm table. The window
enable/disable/WX-rewrite arms (3, 4, 5, D3, D5, D-wx, 3b) turn out NOT to be
whole-dot-fixable at the render — they compensate an ISR dispatch-frame offset
(see below) — and arms 8/8-spr wait on the half-dot render FSM.

## Next group: the window abort / re-enable / WX-rewrite arms (3, 4, 5, 3b, D3, D5, D-wx)

Measured by deleting them and running the matrix: they hold **31 rows** in three
families — `late_disable_*` (20), `late_reenable_*` (6), `late_wx_*` (3).

Dual-traced `late_disable_{early,late}_scx03_wx0f` (CGB, WX=15, SCX&7=3):

| | SameBoy | slopgb |
|---|---|---|
| window activation, ly0 | cfl 105 | never activates |
| window activation, ly1 | cfl 108 | never activates |
| LCDC.5 clear, `_1` / `_2` | cfl 108 / 112 | same |
| mode-3 exit, `_1` / `_2` | cfl 260 / 266 | 254 / 254 (bare) |

So SameBoy's exit moves +6 dots as the clear moves +4 (the clear lands in
different fetcher slots: at the activation dot the window ships nothing, four
dots later its first tile is committed), while slopgb never draws the window at
all on these lines and both legs collapse onto the bare exit. The arms patch the
read verdict on top of that.

Why slopgb misses the window here: `wy_triggered` latches only ONCE across 16
frames. These ROMs clear LCDC.5 mid-line and leave it clear across the frame
boundary, so line 0's dot-4 compare sees the window disabled and returns early;
no later compare can latch it, because `WY == comparison` only holds at line 0.
SameBoy latches anyway, so one of its compares sees LCDC.5 still set where
slopgb's does not.

Probed and rejected:

* **Adding back the line-start (dot 0) compare** — SameBoy runs `wy_check` at
  both the line start and the mode-2 rise, but enabling both takes the arms-cut
  failure count from 31 to **37**. Dot 4 alone stays the measured optimum.
* **Moving the frame reset ahead of the wrap's scheduled compare** — correct in
  itself (landed, see below) but changes none of the 31.

Landed from this probe list: the activation gate now reads the ARCHITECTURAL
LCDC, like SameBoy's `check_window` (`display.c:1315`) and like `wy_check`,
instead of the pipeline view `eff.lcdc` (which sees a write ~2 dots early —
right for the fetch/addressing side, wrong for the enable test). Zero suite
regressions, and the arms' reach drops **31 → 29**. Golden moved two rows:
`mealybug ppu/m3_lcdc_win_en_change_multiple_wx [Dmg]`, which is already
baselined (it fails against the photo before and after, so this is one wrong
output for another, not a photo regression), and `gbmicrotest/ppu_win_vs_wx
[Dmg]`, whose matrix still passes — its verdict is a memory value, the screen is
an echo.

### The remaining 29 are a CPU-frame offset, NOT a render gap

**This corrects the earlier reading of this group.** Disassembling the ROMs
(`rom-disasm-gaps`) shows the arms are compensating for the ISR write frame, not
for a missing fetcher-slot model, so no window/render change can reach them.

The `late_*` family is STAT-IRQ driven. Vector `0048` is `jp 1000`; `1000` is a
NOP sled ending in the write under test:

```
0048  C3 00 10  jp 1000
1000..100D      nop            ; the sled — one NOP longer in `_2` than `_1`
100E  3E 91     ld a,91        ; LCDC.5 clear
1010  E0 40     ld (ff00+40),a
```

`cmp -l` on the `_1`/`_2` pair is a single inserted `00` at `0x100D` shifting the
run down one byte, i.e. the rungs step the write by exactly one M-cycle. **Both
emulators reproduce that +4-dot step correctly** — the ladder is not the problem.

The problem is the absolute frame, and it decomposes exactly
(`late_disable_early_scx03_wx10`, ly1, dual-traced `ACK` against `SBACK`):

| | SameBoy | slopgb | delta |
|---|---|---|---|
| STAT dispatch (ack) | dot 18 | dot 16 | **−2** |
| ack → LCDC.5 clear, `_1` | 90 | 88 | **−2** |
| ack → LCDC.5 clear, `_2` | 94 | 92 | **−2** |
| resulting clear dot | 108 / 112 | 104 / 108 | −4 |

Two dots of STAT dispatch plus two of ISR path. The WX match dot agrees
perfectly (both 109), so the render is aligned; only the CPU's arrival is not.
SameBoy's rule is simply "did the clear beat the match" — at 108 it did (no
window, exit 260), at 112 it did not (window activates, exit 266) — and slopgb's
104/108 both land on the same side, collapsing the pair onto one exit (257/257).
The same −2 dispatch was confirmed on `late_reenable_2`, so it is systematic
across the family.

slopgb's dispatch dot is not free to move: it is counter-pinned (the CLAUDE.md
rule — a PPU advance at dispatch hangs mooneye `intr_2`/`int_hblank`/
`di_timing`), and mooneye is green at 439/439 with dot 16. So slopgb and SameBoy
genuinely disagree here and mooneye backs slopgb; the gambatte `late_*`
expectations need SameBoy's relationship. That is a real trade, not a bug with a
one-sided fix — and it is the same root cause as the
`late_scx_late_wy_FFto4_ly4_wx00_2` row floored above.

Probes run against this, all consistent with the diagnosis:

| change | arms-cut rows |
|---|---|
| baseline | 29 |
| arm 8 extended to aborted-window lines (emergent `2*flip` exit) | 25 |
| + enable test on the deferred `eff.render_lcdc` | 20 |
| deferring the WHOLE LCDC.5 window pathway 4 dots (view + abort flags together) | 29 — no change |

The first two cannot be combined with the arms present (they double-correct: 8
rows regress with arms intact), and the pair alone still leaves 20. The third —
the one the frame measurement seemed to call for — moves nothing, which is what
the disassembly predicts: the offset is upstream of the window machine
entirely. A dedicated per-dot LCDC.5 lag was likewise rejected (2 dots ties the
render view at 20, 3 gives 26, 4 gives 32).

**Conclusion: this group is not the next whole-dot-fixable one.** Retiring it
needs the STAT dispatch frame reconciled with mooneye first, which is a separate
piece of work on the counter-pinned dispatch — not a window law.

### The `win*_b` golden drift: reviewed, benign

The re-captured `golden_fingerprint` moves 25 rows: 7 gambatte window rows whose
verdict changed, and 18 `gbmicrotest/win*_b [Cgb]`. No commercial-game row
moved. The `win*_b` set was checked three ways before re-capturing:

* slopgb HEAD, slopgb+port and SameBoy all activate the window on every visible
  line at dot 95 with the same window row counter (2304 activations per 16
  frames in both slopgb builds) — HEAD and the port are byte-identical there, so
  the drift cannot come from the window trigger;
* no `win*` row is baselined in `baselines/gbmicrotest.txt` and every one still
  passes — their verdict is a HRAM value, not the screen;
* the changed pixels are the on-screen echo those ROMs draw, downstream of the
  FF41 read verdict they poll.

Also ruled out: the LCD-enable glitch line (suppressing `wy_check` there took
the drift from 27 to 47) and any `win_line` off-by-one.

### Probe

`gambatte/window/**/*wy*` runs in ~10 s off the debug runner:

```sh
cargo build -p slopgb-core --release --example run_gambatte
# then, per ROM: target/release/examples/run_gambatte <rom> [dmg|cgb]
# expectation is in the filename: _dmg08_out<H> / _cgb04c_out<H> / _dmg08_cgb04c_out<H>
```

Five of the 120 are already in `baselines/gambatte.txt`
(`late_scx_late_wy_FFto4_ly4_wx20_2 [Cgb]`, `..._wx20_3 [Dmg]`,
`late_wy_lcdoffset1_1 [Cgb]`, `arg/late_wy_1 [Cgb]`, `window/late_wy_1 [Cgb]`).

## Rule note

Nothing here special-cases a test ROM: the mechanism is SameBoy's, with its own
citations, and it lives in the render where the hardware puts it. This was the
first of the three deletions that retire the arm table. The window
enable/disable/WX-rewrite arms (3, 4, 5, D3, D5, D-wx, 3b) turn out NOT to be
whole-dot-fixable at the render — they compensate an ISR dispatch-frame offset
(see below) — and arms 8/8-spr wait on the half-dot render FSM.

## Next group: the window abort / re-enable / WX-rewrite arms (3, 4, 5, 3b, D3, D5, D-wx)

Measured by deleting them and running the matrix: they hold **31 rows** in three
families — `late_disable_*` (20), `late_reenable_*` (6), `late_wx_*` (3).

Dual-traced `late_disable_{early,late}_scx03_wx0f` (CGB, WX=15, SCX&7=3):

| | SameBoy | slopgb |
|---|---|---|
| window activation, ly0 | cfl 105 | never activates |
| window activation, ly1 | cfl 108 | never activates |
| LCDC.5 clear, `_1` / `_2` | cfl 108 / 112 | same |
| mode-3 exit, `_1` / `_2` | cfl 260 / 266 | 254 / 254 (bare) |

So SameBoy's exit moves +6 dots as the clear moves +4 (the clear lands in
different fetcher slots: at the activation dot the window ships nothing, four
dots later its first tile is committed), while slopgb never draws the window at
all on these lines and both legs collapse onto the bare exit. The arms patch the
read verdict on top of that.

Why slopgb misses the window here: `wy_triggered` latches only ONCE across 16
frames. These ROMs clear LCDC.5 mid-line and leave it clear across the frame
boundary, so line 0's dot-4 compare sees the window disabled and returns early;
no later compare can latch it, because `WY == comparison` only holds at line 0.
SameBoy latches anyway, so one of its compares sees LCDC.5 still set where
slopgb's does not.

Probed and rejected:

* **Adding back the line-start (dot 0) compare** — SameBoy runs `wy_check` at
  both the line start and the mode-2 rise, but enabling both takes the arms-cut
  failure count from 31 to **37**. Dot 4 alone stays the measured optimum.
* **Moving the frame reset ahead of the wrap's scheduled compare** — correct in
  itself (landed, see below) but changes none of the 31.

Landed from this probe list: the activation gate now reads the ARCHITECTURAL
LCDC, like SameBoy's `check_window` (`display.c:1315`) and like `wy_check`,
instead of the pipeline view `eff.lcdc` (which sees a write ~2 dots early —
right for the fetch/addressing side, wrong for the enable test). Zero suite
regressions, and the arms' reach drops **31 → 29**. Golden moved two rows:
`mealybug ppu/m3_lcdc_win_en_change_multiple_wx [Dmg]`, which is already
baselined (it fails against the photo before and after, so this is one wrong
output for another, not a photo regression), and `gbmicrotest/ppu_win_vs_wx
[Dmg]`, whose matrix still passes — its verdict is a memory value, the screen is
an echo.

### Probed for the remaining 29 (measured, not landed)

Two ingredients each move the arms-cut failure count, and both are
directionally right, but they cannot be combined with the arms still present —
the arms were calibrated against the old views and double-correct (8 rows
regress with arms intact). It is either/or, and the pair alone still leaves 20,
so the group is not ready:

| change | arms-cut rows |
|---|---|
| baseline | 29 |
| arm 8 extended to aborted-window lines (`win_gone`: `!win_active && wx_match_dot != 0 && wy_triggered`, taking the emergent `2*flip` exit) | 25 |
| enable test on the deferred `eff.render_lcdc` instead of the architectural bit | 20 (with `win_gone`) |

The enable-view finding is exact. Dual-traced `late_disable_early_scx03_wx10`
(CGB, WX=16, SCX&7=3), where the WX match dots agree perfectly (both 109):

| | SameBoy | slopgb (arch view) |
|---|---|---|
| `_1` LCDC.5 clear | cfl 108 (< match -> no window) | dot 104 |
| `_2` LCDC.5 clear | cfl 112 (> match -> window) | dot 108 |
| `_1` / `_2` exit | 260 / 266 | 257 / 257 |

slopgb's clear lands a uniform 4 dots early on the architectural view, so both
legs fall on the same side of the match and the pair collapses. The deferred
render view moves it back.

Rejected by measurement: a dedicated per-dot LCDC.5 lag (`win_en_lag`, the
`obj_en_lag` shape) — 2 dots ties the render view at 20, 3 gives 26, 4 gives 32,
i.e. it gets WORSE exactly where the trace says it should get better. The reason
is that the enable test and the abort are separate paths: `window_abort_flags`
fires at cc+0 regardless, so delaying only the activation gate makes the window
activate and then be aborted by an already-committed clear. **Reconciling those
two frames is the next step for this group** — not another view swap.

The remaining rows also need the fetcher-slot model itself: SameBoy's exit moves +6
dots as the LCDC.5 clear moves +4, because the clear lands in different fetcher
slots. slopgb's `window_abort_render` re-anchors the BG fetch and the line ends
at the bare-line dot, discarding the window restart cost already paid. That is a
render change with pixel consequences (the mealybug `m3_lcdc_win_en_change*`
photos pin the drawn columns), so it wants the same measure-first treatment the
window-Y group got.

## Group 3 scoped: the "sub-dot" verdict does not survive

The remaining arms (1, D1, 8, 8-spr) hold **112 rows**, of which **96 of the 98
CGB rows are SameBoy-PASS** (classifier) — genuine, not floor. Prior maps called
this the atomic half-dot render FSM rewrite. Scoped with `rom-diff-weld` +
`rom-disasm-gaps`, it splits into three separable pieces and most of it was never
sub-dot:

| piece | rows | shape |
|---|---|---|
| arms 1 + D1 (closed-form window length `259`/`263 + SCX&7`) | 33 | table-shaped; candidates for the emergent flip, like arm 8 already uses |
| the post-switch STOP table (the two `stop_anchor_midframe` branches) | 32 | render re-pacing after a speed switch, NOT a read law |
| arm 8's emergent core + 8-spr | ~47 | already emergent from the render's own flip — legitimate modelling |

### Not a weld: the pairs separate at whole-dot

`cmp -l` on the speedchange siblings is a **single inserted `00`** shifting a run
(`m2int_m3stat_lcdoffds` at 0x1074, `speedchange2_m2int_m3stat_scx2` at 0x1032),
and disassembly shows what it shifts is the observable read itself:

```
1074  F2        ld a,(ff00+c)      ; the FF41 read — one M-cycle later in `_2`
1075  A0        and b
1076  C3 00 70  jp 7000
```

One inserted byte = one whole M-cycle, the `rom-diff-weld` signature for
REPRESENTABLE. Traced end to end on `speedchange2_m2int_m3stat_scx2_1/_2`, the
pair separates cleanly on the whole-dot grid:

| | read dot | rphd | exit | verdict | want |
|---|---|---|---|---|---|
| `_1` | 251 | 510 | 514 | mode 3 | 3 |
| `_2` | 255 | 518 | 514 | mode 0 | 0 |

The exit sits exactly between the two reads. Arm 8's emergent `2*flip + 2`,
anchored to the render's own projection, is what does that — no table, no
half-dot.

### The post-switch 32 are a render projection error

On `speedchange_ly44_m3_m3stat_2` (DS, want mode 0) with the table disabled the
emergent exit is 508 against a true 504, i.e. the render's post-switch flip
projects 255 where it should be 253 — **2 dots, whole-dot representable**. The
reads also sit 4-6 dots off SameBoy's on that line, the same ISR-frame class as
the group-2 finding. So this block wants the speed-switch render re-pacing
corrected, not a sub-dot read.

### What this means for the rewrite

The blanket "coupled multi-session half-dot render FSM" verdict does not hold for
the bulk of this group. Two of the three pieces are ordinary whole-dot work
(closed forms that should be emergent; a post-switch re-pace), and the third is
already emergent and behaving. What is NOT established here is that the ~47
emergent rows need nothing at all — only that arm 8's core is modelling rather
than compensation, so it is not what the reviewer's critique is aimed at.

Method note: the earlier verdict came from sweeping uniform read-frame levers,
which shift every affected row equally and so can never separate want-opposite
siblings — the exact trap `rom-diff-weld` documents. Five prior verdicts in this
tree fell the same way.

## Landed from the group-3 scoping: the window line pays its fine scroll

Deriving arm 1's constant from the ROMs (rather than sweeping it) showed why the
arm is table-shaped. Across `late_wy_FFto2_ly2_scx{2,3,5}` the render's flip was
**identical — `2*flip = 522` at SCX 2, 3 and 5** — and in DS, `524` at both SCX 0
and SCX 5. The render's window-line flip did not carry the SCX fine scroll at
all; the closed form's `+ SCX&7` was standing in for it.

Cause: a WX <= 7 window activates inside the prefill, while the BG fine-scroll
discard is still being paid out, and `window_trigger_step` **overwrote** the
outstanding discard (`r.discard = 7 - wx`). Only the WX=0 case put it back
(`+ fine + 1`). Carrying it for every WX makes the flip track SCX
(`2*flip` now 526 / 528 / 532 for SCX 2 / 3 / 5), which is SameBoy's
`263 + SCX&7`.

Effect with the arms still in place: **+2 / −2**, a straight swap inside two
`scx5_ds` pairs, and on the fidelity axis a net **+1 SameBoy-PASS** —
`late_disable_scx5_ds_2` and `late_wx_scx5_ds_2` recovered (both SameBoy-PASS),
`late_disable_scx5_ds_1` lost but **SameBoy-FAIL** (`sb=3, want=0`, i.e. slopgb
had been passing it only by over-fitting past SameBoy), `late_wx_scx5_ds_1` lost
and SameBoy-PASS. That last one welds to its own `_2` sibling: both trace
identically — activation ly1 dot 97, reads rphd 528/532, flip 267, no arm fires —
so per `rom-diff-weld` the discriminator is outside the render FSM.

golden re-captured for exactly those four rows; no mealybug, gbmicrotest or
commercial-game row moved.

**Arms 1/D1 still cannot go emergent**, and the derivation says why precisely: even
with the flip tracking SCX the feasible-constant intervals do not intersect
(`k > 10` from `late_wy_FFto2_ly2_scx2_1` against `k <= 0` from
`m2int_wx03_m3stat_ds_2`). SS and DS want different constants and the SS rows
disagree among themselves, so the remaining gap is a second render term, not a
read-frame offset. Note the derivation samples each ROM's LAST FF41 read, which
is not always the observable one — pin the observable read by PC before trusting
those intervals for a specific row.
