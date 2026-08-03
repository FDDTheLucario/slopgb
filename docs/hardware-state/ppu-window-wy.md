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
first of the three deletions that retire the arm table; the window
enable/disable/WX-rewrite arms (3, 4, 5, D3, D5, D-wx, 3b) are the next
whole-dot-fixable group, and arms 8/8-spr wait on the half-dot render FSM.

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

The remaining 29 need the fetcher-slot model itself: SameBoy's exit moves +6
dots as the LCDC.5 clear moves +4, because the clear lands in different fetcher
slots. slopgb's `window_abort_render` re-anchors the BG fetch and the line ends
at the bare-line dot, discarding the window restart cost already paid. That is a
render change with pixel consequences (the mealybug `m3_lcdc_win_en_change*`
photos pin the drawn columns), so it wants the same measure-first treatment the
window-Y group got.
