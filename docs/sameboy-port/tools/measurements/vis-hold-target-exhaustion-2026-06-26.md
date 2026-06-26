# vis-HOLD target exhaustion — both candidates refuted (2026-06-26, #11p)

Ground-truth verdict for the goal's "build the vis-HOLD primitive" task: **the
vis-HOLD primitive (a tier2 mechanism that EXTENDS CPU-visible mode-3 past the
dispatch flip to fix an FF41 mode-3 read) has NO viable clean tier2 target.**
Both candidate sub-families are refuted. mech-1 read-observer's clean
read-phase slices are EXHAUSTED at +58 net (harvested by #11n/#11o + prior).
The residual is S7 (sub-M-cycle read clock) + C2 (shared-render). No code
shipped this session (nothing clean to verify against; building speculatively
would violate the goal's never-red / ground-truth-first discipline).

## Tooling / method

- SameBoy 1.0.2 SB_TRACE tester (`/tmp/sbbuild/...`): `SBMODE` mode-3→0 EXIT
  cfl (measurement frame = the count-1 per-`ly` `vis=0` cfl; setup-bare repeats
  ~115×), `SBREAD ff41` FF41 read cfl+mode.
- slopgb `SLOPGB_S5DBG` (byte-identical OFF): `SLOPGB ff41` deferred read
  dot+mode, `SLOPGB visflip` CPU-visible mode-3→0 flip dot+kind.
- `run_gambatte` example OCRs the rendered framebuffer top tile row (the SAME
  protocol as the gbtr suite) — `SLOPGB_TIER2=1` for flag-on, bare for prod.
- probe `gbtr-<hash> --ignored flagon_probe` over a 233-row window+scx_during_m3
  rowlist (`SLOPGB_PROBE_OFF=1` for the prod baseline).

## Candidate 1 — scx_during_m3 / scx_m3_extend_1 — PIXEL-RENDER floor (NEW)

The goal's designated "simplest, no window state" START target. **It is not an
FF41-read test at all — it is a pixel-render test, so a CPU-visible-mode hold
cannot affect its output.**

DMG `scx_m3_extend_1_dmg08_cgb04c_out3` (baselined gambatte.txt:1107-1108, prod
fails too):

| source | mode-3 EXIT | FF41 measurement read | digit |
|---|---|---|---|
| SameBoy (measurement frame) | cfl262 (bare cfl257, **+5**) | NONE (only 1 ff41 read all run: ly1 cfl0 dc-372, spurious setup) | 3 (rendered) |
| slopgb flag-OFF | visflip dot254 (bare, no extension) | ly1 dot265 mode0 | 0 |
| slopgb flag-ON  | visflip dot254 (bare, no extension) | ly1 dot265 mode0 | 0 |

- **flag-OFF OCR == flag-ON OCR == `03232323232301010101`** → the tier2 read
  phase makes ZERO difference. The `0323…` per-X gradient is the rendered BG
  shifted by the real mode-3 length — a pure pixel readout, not a printed digit.
- SameBoy samples FF41 zero times for the output. slopgb's lone FF41 read
  (dot265 mode0) **matches** SameBoy (cfl↔dot offset +3 → SameBoy ext cfl262 =
  slopgb dot259 < read dot265 → both mode0). The FF41 read is correct; the
  **digit comes from pixels**.
- Root cause: slopgb's shared fine-scroll comparator (`render.rs` hunt) latches
  `hunt_done` at the line-start match (SCX=0 → match at mode3_dot5) and never
  re-opens on the mid-mode-3 SCX write, so mode-3 never extends. SameBoy's
  comparator is live. **This is a SHARED render under-extension** (prod == tier2),
  fixable only by changing the hunt → production-shared, A/B-swept (scx_during_m3
  is a baselined cluster) → **C2 rebaseline, NOT a tier2 vis-HOLD slice.**
- A visible-mode hold extends `vis_mode()` (what FF41 reads); it cannot move
  rendered pixels. **vis-HOLD structurally inapplicable here.**

## Candidate 2 — window mech #2 (normal-window boundary) — already refuted #11g

`window-groundtruth-2026-06-24.md` "ATTEMPT 1" already **implemented** a tier2
win-active vis-HOLD (`m0_flip_dot` + `win_vis_hold()` forcing `vis_mode==3` for
`scx&7` dots past the flip on `win_active` lines, byte-identical OFF) and
**measured 0/53 gain, then reverted**. The win-active family reads are
OPPOSITE-direction at the SAME ~dot261 boundary:

| ROM | want | got | slopgb read | flip | need |
|---|---|---|---|---|---|
| m2int_wx00_m3stat_2 | 0 | 3 | dot260 mode3 | dot261 win | mode-3 too LONG → EARLIER |
| m2int_wxA6_scx3_m3stat_2 | 0 | 3 | dot256 mode3 | dot260 win | too LONG → EARLIER |
| late_wy_FFto2_ly2_scx3_1 | 3 | 0 | dot264 mode0 | dot261 win | too SHORT → LATER |
| late_wy_10to0_ly1_1 | 3 | 0 | dot260 mode0 | dot254 bare | too SHORT → LATER |

A vis-HOLD extends mode-3 (helps the want=3 late_wy) but pushes the want=0
m2int_wx further wrong. The discriminator is the per-config read-vs-boundary
PHASE → the **S7 read-frame↔boundary atomic coupling** (the global reclock), NOT
a boundary lever. Re-confirmed this session: `late_wy_FFto2_ly2_scx5_2` slopgb
visflip win@dot261, SameBoy exit cfl268 (= 263+SCX&7, the doc's window-length
law), read at dot264 in the (261,268] gap → a vis-HOLD WOULD fix this one row,
but the same lever regresses the want=0 m2int_wx_2 siblings. Plus mechanisms 3
(late_wy `wy_ok=false`) + 4 (late_disable `en=false`) render BARE in slopgb (the
window never triggers) → no flip lever reaches them; those are render-level
WY-latch / abort bugs (production-shared) = C2.

## ATTEMPT 2 (this session) — own build of the vis-HOLD primitive, refined law, 0/233, reverted

To answer the goal's "build it" directly (not just cite #11g's ATTEMPT 1), the
primitive was **implemented this session** with the *refined* window-length law
(an ABSOLUTE `263 + SCX&7` exit, vs ATTEMPT 1's `scx&7`-past-flip relative
hold), measured, and reverted:

- `vis_hold_until: u16` field (mod.rs) — symmetric inverse of `vis_early`; reset
  per-line alongside `vis_early` (line_setup.rs ×2, regs.rs ×2, `m0_unflip`).
- `m0_flip_events` (mode0.rs): on the dispatch (`proj <= lead`), under
  `tier2_reclock && render.win_active`, set `vis_hold_until = 263 + (scx & 7)`.
- `vis_mode` (stat_irq.rs): after the `line_render_done || vis_early → 0` branch,
  `else if self.dot < self.vis_hold_until { 3 }` — keeps mode 3 past the
  dispatch without moving `line_render_done`. Always 0 in production
  (byte-identical OFF by construction).

**Result: 0 fixed / 0 broke over the 233-row window+scx_during_m3 probe** (same
net as ATTEMPT 1, now with the correct absolute law). The hold ACTIVATES
correctly (win-active rows get `vis_hold_until = 263+SCX&7`), but it is INERT
because of WHERE the failing rows actually sit:

- The want=3 rows that *would* use a later boundary (`late_wy_FFto2_ly2_scx5_*`
  etc.) render BARE on the measurement frame — `wy_ok=false`, `win_active=false`
  (the #11g table: `late_wy_FFto2_ly2_scx5_1` = `dot=259/bare`). The window
  never enters slopgb's render there (mechanism 3, WY-latch), so a `win_active`
  hold cannot reach them. The win@261 flips seen in an early trace were OTHER
  frames, not the ly2 measurement line.
- The win-active fails (`m2int_wx00_m3stat_2`, want=0) read BEFORE the dispatch
  (dot260 < flip261 → already mode3, already failing); extending mode-3 LATER
  leaves them mode3 (no change, no regression). They need EARLIER, not later.

So the win-active vis-HOLD is the wrong half of the fix: the rows need either
the WY-latch render trigger (mechanism 3, production-shared) or the read-collapse
read-frame (mechanism 2/S7) — neither is a CPU-visible-mode hold. **Reverted**
(scattered inert hot-path code earns no place; the C2 window-length model will
re-add the precise primitive it needs). This is the own-measured confirmation of
#11g, with the refined law and the exact root cause.

## Probe summary (233 baselined window+scx_during_m3 rows, flag-on vs flag-off)

- **58** rows fail OFF → pass ON (read-phase wins already harvested: #11n
  eighth-grid + #11o accessibility + prior).
- **0** tier2 regressions (fail ON but pass OFF).
- **34** rows fail BOTH (render floors — vis-HOLD on FF41-visible-mode cannot
  help). Only **7 in-scope DMG** (rest CGB ds/lcdoffset, out of scope):
  scx_m3_extend (hunt), late_enable_afterVblank ×2, late_reenable_scx5,
  late_wy_FFto2_scx5, late_scx_late_wy (want=0, over-extend), wxA6_spxA7_m0irq
  (want=2 sprite). All map to the goal's pre-classified render-level / S7
  mechanisms.

## VERDICT — do NOT build the vis-HOLD primitive

Both proposed targets are dead: scx_m3_extend is a pixel test (this session),
window is opposite-direction-entangled + render-bare (refuted #11g ATTEMPT 1).
A general vis-HOLD has nothing to verify against and a tier2-gated render-length
hack would risk the A/B-swept floor with no FF41-read justification. The C-stage
mech-1 clean read-phase work is **exhausted**. Remaining C-stage levers are
non-mech-1: mech-2 halt wake-clock (S7), mech-3 CGB lcd-offset, and the C2
render rebaseline (hunt re-open / WY-latch / window-length parallel model +
true vis-hold) — none a clean tier2 read-phase slice. Defaults NOT flipped.
