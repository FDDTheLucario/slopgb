# WINDOW family ground truth — SameBoy SBMODE vs slopgb (2026-06-24, #11g)

Exhaustive per-config measurement of all **53 DMG window flip-regr rows** (the
107 `window` regr = 53 DMG + CGB/DS). Tooling: the rebuilt SameBoy 1.0.2
SB_TRACE tester (SBMODE mode-exit + SBREAD FF41) and the committed slopgb
`SLOPGB_S5DBG` tracer (now incl. the `visflip` mode-3→0 flip dot in
`render/mode0.rs` and the `winmatch` trigger state in `render/window.rs`).

## Method — isolate the MEASUREMENT frame (corrects the prior diagnosis)

gambatte window ROMs loop ~15 frames: SETUP frames render the line bare (no
window), and ONE late frame writes WY/LCDC so the window triggers on the target
line (`ly` is in the filename). The mode-3→0 EXIT to compare is the
**measurement frame's** exit, not the bare setup exit. Heuristic: the per-`ly`
`vis=0` cfl that occurs **once** (count 1) is the measurement frame; the
115×-repeated cfl is setup-bare.

```sh
# SameBoy measurement-frame exit for ROM at line ML:
SB_TRACE=1 sameboy_tester --dmg --length 2 ROM 2>log >/dev/null
grep "SBMODE ly=$ML .*vis=0" log | grep -oE 'cfl=[0-9]+' | grep -v cfl=0 \
  | sort | uniq -c | sort -n | head -1     # the rare (count-1) exit
# slopgb measurement-frame flip + trigger state:
SLOPGB_ROWLIST=oner.txt SLOPGB_S5DBG=1 gbtr-<hash> --ignored flagon_probe --nocapture
#   grep "visflip ly=$ML" / "winmatch ly=$ML"  (rare = measurement frame)
```

## REFUTATION of the prior window diagnosis (handoff #11f)

The #11f claim — *"m2int_wx00 OVER-extends; SameBoy SBMODE exits cfl257 = bare;
low-WX (<7) wrongly adds the window penalty"* — was a **measurement artifact**:
it read the SETUP-frame bare exit (cfl257), not the measurement-frame exit.
Fresh measurement: **SameBoy extends ALL window measurement lines to ≈ 263 + SCX&7**
(wx00, wxA5, late_wy, late_disable alike). There is NO low-WX no-extend case and
NO over-extend; the window penalty (+6 over bare) is present everywhere a window
triggers. slopgb's bug is the opposite — it UNDER-extends most window lines.

## Full table (53 DMG window regr rows)

`LE`/`T2` = leading-edge-only / tier2 verdict (P pass, F fail; all OFF-pass).
`SBex` = SameBoy measurement-frame mode-3→0 exit cfl. `winmatch` = slopgb
measurement-frame trigger state (`wy_ok,en,active`). `visflip` = slopgb
measurement-frame CPU-visible mode-3→0 dot + line kind.

```
ROM                                     ml LE T2 SBex  winmatch(meas)      visflip(meas)
late_wy_10to0_ly1_1                      1  P  F  263  false,entrue,afalse dot=254/bare
late_wy_10to0_ly1_2                      1  F  F  263  false,entrue,afalse dot=254/bare
late_wy_10to1_ly1_2                      1  F  F  263  false,entrue,afalse dot=254/bare
late_wy_1                                1  F  F  263  true,entrue,afalse  dot=261/win
late_wy_1toFF_1                          1  P  F  257  true,entrue,afalse  dot=261/win
late_wy_1toFF_2                          1  F  F  257  true,entrue,afalse  dot=261/win
late_wy_2toFF_1                          1  P  F  257  false,entrue,afalse dot=254/bare
late_wy_2toFF_2                          1  F  F  257  false,entrue,afalse dot=254/bare
late_wy_FFto0_ly0_2                      0  F  F  263  true,enfalse,afalse dot=250/glitch
late_wy_FFto0_ly2_1                      2  P  F  263  false,entrue,afalse dot=254/bare
late_wy_FFto0_ly2_2                      2  F  F  263  false,entrue,afalse dot=254/bare
late_wy_FFto1_ly2_1                      2  P  F  263  false,entrue,afalse dot=254/bare
late_wy_FFto1_ly2_2                      2  F  F  263  false,entrue,afalse dot=254/bare
late_wy_FFto2_ly2_2                      2  F  F  263  false,entrue,afalse dot=254/bare
late_wy_FFto2_ly2_scx2_1                 2  F  F  265  true,entrue,afalse  dot=261/win
late_wy_FFto2_ly2_scx2_2                 2  F  F  265  false,entrue,afalse dot=256/bare
late_wy_FFto2_ly2_scx3_1                 2  F  F  266  true,entrue,afalse  dot=261/win
late_wy_FFto2_ly2_scx3_2                 2  F  F  266  false,entrue,afalse dot=257/bare
late_wy_FFto2_ly2_scx5_1                 2  F  F  268  false,entrue,afalse dot=259/bare
late_wy_FFto2_ly2_wx0f_2                 2  F  F  263  false,entrue,afalse dot=254/bare
late_disable_1                          1  F  F  263  true,enfalse,afalse dot=254/bare
late_disable_early_scx03_wx0f_1         1  F  F  260  true,enfalse,afalse dot=257/bare
late_disable_early_scx03_wx10_1         1  F  F  260  true,enfalse,afalse dot=257/bare
late_disable_early_scx03_wx11_1         1  F  F  260  true,enfalse,afalse dot=257/bare
late_disable_early_scx03_wx12_1         1  F  F  260  true,enfalse,afalse dot=257/bare
late_disable_early_scx03_wx12_2         1  F  F  260  true,enfalse,afalse dot=257/bare
late_disable_late_scx03_wx0f_2          1  F  F  266  true,enfalse,afalse dot=257/bare
late_disable_late_scx03_wx10_2          1  F  F  266  true,enfalse,afalse dot=257/bare
late_disable_late_scx03_wx11_2          1  F  F  266  true,enfalse,afalse dot=257/bare
late_disable_spx10_wx0f_2               1  F  F  274  true,enfalse,afalse dot=264/spr
late_disable_wx0f_1                     1  F  F  263  true,enfalse,afalse dot=254/bare
late_reenable_2                         1  F  F  257  true,entrue,afalse  dot=261/win
late_reenable_wx0f_2                    1  F  F  257  true,entrue,afalse  dot=261/win
late_scx_late_disable_0                 1  F  F  257  true,enfalse,afalse dot=258/bare
late_wx_scx3_2                          1  P  F  260  true,entrue,afalse  dot=261/win
late_wx_scx5_1                          1  P  F  262  true,entrue,afalse  dot=261/win
m2int_wx00_m3stat_2                      1  F  F  263  true,entrue,afalse  dot=261/win
m2int_wx03_m3stat_2                      1  F  F  263  true,entrue,afalse  dot=261/win
m2int_wx07_m3stat_2                      1  F  F  263  true,entrue,afalse  dot=261/win
m2int_wx17_wxA5_m3stat_2                 1  F  F  263  true,entrue,afalse  dot=261/win
m2int_wxA5_m0irq_2                       1  F  F  263  true,entrue,afalse  dot=261/win
m2int_wxA5_m3stat_2                      1  F  F  263  true,entrue,afalse  dot=261/win
m2int_wxA6_firstline_m3stat_2           1  F  F  257  true,entrue,afalse  dot=257/win
m2int_wxA6_m0irq2_2                      1  F  F  257  true,entrue,atrue   dot=257/win
m2int_wxA6_m0irq_2                       1  F  F  257  true,entrue,atrue   dot=257/win
m2int_wxA6_m3stat_2                      1  P  F  257  true,entrue,atrue   dot=257/win
m2int_wxA6_oambusyread_2                 1  F  F  257  true,entrue,atrue   dot=257/win
m2int_wxA6_scx2_m3stat_2                 1  F  F  259  true,entrue,atrue   dot=259/win
m2int_wxA6_scx3_m3stat_2                 1  F  F  260  true,entrue,atrue   dot=260/win
m2int_wxA6_scx5_m3stat_2                 1  F  F  262  true,entrue,atrue   dot=262/win
m2int_wxA6_spxA7_m3stat_2               1  F  F  263  true,entrue,atrue   dot=254/bare
m2int_wxA6_vrambusyread_2               1  F  F  257  true,entrue,atrue   dot=257/win
```

## FOUR distinct mechanisms (no single tier2 lever)

The offset `SBex − slopgb_visflip` is **bidirectional and non-uniform even
within one sub-family** → confirms the handoff's "per-config, no uniform offset."

1. **wxA6 read-frame / sub-dot wall (~13 rows, S7).** `visflip == SBex EXACTLY`
   (257=257, 259=259, 260=260, 262=262) and LE passes for the `_1` phase, yet
   tier2 fails. The CPU-visible boundary is PERFECT; the tier2 deferred read
   over-advances past the (correct) boundary. This is the eighth-grid sub-dot
   read-observer wall (recipe S7), NOT a window-length bug. Untouchable without
   the sub-M-cycle read clock.
2. **Normal-window boundary −2 (~7 rows: m2int_wx00/wx03/wx07/wxA5).**
   `visflip 261 vs SBex 263` (offset −2) for the WX 0–165 windows that trigger
   correctly (`wy_ok,en,active`=t,t). slopgb flips 2 dots early. The window
   `lead` is calibrated against the cc+4 dispatch; the deferred frame needs +2.
   But the flip can only ANTICIPATE (vis_early), it cannot DELAY — extending the
   visible mode-3 PAST the dispatch needs a new "vis-hold" mechanism, and the
   `lead` is production-shared + kernel-pinned.
3. **late_wy `wy_ok=false` (~16 rows).** slopgb's WY-latch is FALSE on the
   measurement frame where SameBoy's window TRIGGERS → slopgb renders the line
   **bare** (`dot=254/bare`, no +6) while SameBoy extends to 263. The window
   never enters slopgb's render, so no tier2 flip lever can reach it — fixing it
   is a WY-latch render change (production-shared, breaks byte-identical OFF).
4. **late_disable `en=false` aborted window (~10 rows).** `wy_ok=true` but the
   window is DISABLED at the match dot → slopgb renders bare; SameBoy keeps the
   aborted window's +6 mode-3 cost (263/266). slopgb's `win_aborted` SUBTRACTS
   from `lead` (flips earlier) — opposite of SameBoy. Render-level.

Plus mixed: `late_wy_1toFF`/`late_reenable` are kind=win but `visflip 261 vs
SBex 257` (offset +4, slopgb flips LATE — opposite of mechanism 2);
`scx` rows need the window flip to grow with SCX&7 (it is flat at 261 while
SameBoy grows 263→268).

## ATTEMPT 1 (implemented + measured + reverted) — tier2 win-active vis-HOLD

To satisfy "refute ≤5×" empirically (not just by the measurement above), a
tier2-gated active-window vis-HOLD was implemented: a new `m0_flip_dot` field +
a `win_vis_hold()` predicate forcing `vis_mode == 3` for `scx&7` dots past the
flip on `render.win_active` lines (byte-identical OFF; the kernel + 7 pins are
never `win_active`). **Result: 0/53 gain.** The dot-level read/flip/want data
shows WHY — within the win-active family the direction is OPPOSITE:

```
ROM                       ly want got  slopgb read   slopgb flip
m2int_wx00_m3stat_2        1   0   3   dot260 mode3  dot261 win   mode-3 too LONG → need EARLIER
m2int_wxA6_m3stat_2        1   0   3   dot256 mode3  dot257 win   too LONG → EARLIER
m2int_wxA6_scx3_m3stat_2   1   0   3   dot256 mode3  dot260 win   too LONG → EARLIER
late_wy_FFto2_ly2_scx3_1   2   3   0   dot264 mode0  dot261 win   mode-3 too SHORT → need LATER
late_wy_10to0_ly1_1        1   3   0   dot260 mode0  dot254 bare  too SHORT → LATER
```

The flip dot is ~261 for both the want=0 (need earlier) and want=3 (need later)
configs — the tests read FF41 at DIFFERENT cycles (read 256/260/264), so the
SAME boundary gives OPPOSITE results. **No boundary lever (vis-hold either
direction) can fix both.** The discriminator is the per-config read-vs-boundary
PHASE — the read-frame↔boundary atomic coupling (recipe's S7 / the global
reclock), confirmed here at dot granularity. A vis-hold extends mode-3 (helps
the want=3 late_wy) but pushes the want=0 m2int_wx* further wrong. Attempts 2-5
of the same boundary class would be thrashing — reverted.

## VERDICT

There is **no clean tier2-gated window slice.** The 53 rows span (1) the S7
sub-dot read wall (boundary already perfect), (2) bidirectional ±2/±4
boundary offsets among triggering windows, and (3)/(4) render-level
window-trigger discrepancies (`wy_ok`, aborted-extend) that never enter
slopgb's render so no flip lever reaches them. A real fix is the multi-config
port the goal scopes: a tier2-gated PARALLEL window mode-3-length model that
(a) replicates SameBoy's window trigger (incl. late-WY + disabled-extend) and
(b) adds a "vis-hold" so the CPU-visible mode-3 can lag the dispatch — neither
exists today. Banked as the refined, measured map; no row shipped (no
SameBoy-passing row touched; tracer additions byte-identical OFF).
