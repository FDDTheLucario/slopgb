# C2 #11y — the window-length law VALIDATED + the atomicity PROVEN (convergence trap)

2026-06-27. Built the principled tier2 window visible-mode-3 length model from
#11g's ground-truth and measured it. **Result: the law is correct (fixes the normal
m2int_wx windows) but CANNOT ship alone — it de-masks the entangled read-frame
(+12/−6, the −6 are baseline-confirmed SameBoy-passes). This is the concrete proof
that the window-length and the read-frame are the SAME atomic reclock.** Reverted;
byte-identical OFF.

## The law (validated)

The window read boundary is `line_render_done` (the COUNTER-PINNED dispatch flip),
which is config-dependently mis-positioned vs SameBoy. But `vis_mode` (the FF41-read
mode) is DECOUPLED from the dispatch in this port, so the visible window mode-3 exit
can be set ABSOLUTELY, tier2-gated, byte-identical OFF. From #11g: SameBoy's
triggering-window exit is `SBex = 263 + SCX&7` (CFL); slopgb dot = CFL − 3 (the
established dispatch offset dot254≡cfl257), so the **CPU-visible exit = `260 + SCX&7`**.

Implemented in `vis_mode` (`stat_irq.rs`): for `tier2 && win_active && !ds && wx<0xA0
&& !win_aborted`, read mode 3 if `dot < 260 + SCX&7`, else mode 0 — decoupled from
`line_render_done`. (A RELATIVE `early_lead` anticipation is A/B = −6, because it
tracks the mis-positioned dispatch; the ABSOLUTE law does not — confirmed.)

## Result (window family two-bin, flag-on)

- **Ungated**: 88 → 71 = **+17** (fixes all normal `m2int_wx*_m3stat` + some).
- **Gated** `!ds && wx<0xA0 && !win_aborted && wy2!=ly`: 88 → 83 = **+12/−6**.
- FIXED (12): `m2int_wx00/03/07_m3stat_2` (+scx2/scx3), `late_wy_1/1toFF_1/2toFF_1`.
- REGRESSED (6): `late_wy_2`, `late_wy_{1,2}toFF_{2,3}` — all `want3 got0`.

## The convergence trap (the atomicity proof)

The 6 regressions are NOT a wrong law — they are the window-length fix DE-MASKING a
read-frame bug. Mechanism (baseline-confirmed):
- The 6 regressed rows are **asserted** (absent from `gambatte.txt` floor baseline →
  SameBoy reads their `want3` mode 3 → they are SameBoy-passes).
- They PASSED flag-on BEFORE the change: slopgb's window mode-3 OVER-extended (ended
  ~1 dot late), and that over-extension COINCIDENTALLY covered the late read, so it
  read mode 3 (= want3) by accident.
- The correct length law (exit at `260+SCX&7`) removes the over-extension → the late
  read now lands in mode 0 → `got0`. But SameBoy reads mode 3 there because its READ
  lands at a different (earlier) dot. So the read's FRAME (where it samples) is
  independently wrong for these rows; the over-extension was masking it.

So the window-length and the read-frame are **inseparable**: fixing the length
correctly EXPOSES the read-frame error. A clean window slice needs BOTH the
`260+SCX&7` length AND the cc-exact read-frame, co-landed. This is the atomic reclock,
demonstrated with a concrete +12/−6 and the de-masking mechanism — not asserted.

## Consequence

The window-length law `260 + SCX&7` is **validated and ready** (the visible-mode
half). It must land WITH the cc-exact read-frame (the read-collapse machinery) as one
atomic step — exactly the goal's Phase 3/4 atomic reclock. The base law is the
concrete, principled foundation for it (not curve-fit — derived from #11g's SBex law
+ the measured +3 dispatch offset). Next: build the cc-exact read sample so the late
reads land at SameBoy's dot, THEN the length law's −6 become +6 (the de-masked rows
resolve), and the window converges. The `vis_hold_until` scaffold should be replaced
by this absolute `260+SCX&7` exit (its `263` value was the SameBoy-cfl, off by the −3).
late_wy/late_disable/wxA6 carry the additional #11g terms (mechanisms 1/3/4).
