# C3-flip census + go/no-go — the fresh full two-bin classification (2026-07-04, #11bt)

The C3-FLIP-CHECKLIST §1/§3 executed end to end on the shipped tree: fresh
CGB **and DMG** gambatte-OCR two-bins + the gbmicro/wilbertpol DMG probes,
every flip-BUG classified against SameBoy ground truth (framebuffer OCR for
gambatte, patched `sameboy_tester` FF82/register dump for gbmicro/mooneye).

## VERDICT: **NO-GO** — the flip bar is NOT met.

**Census of SameBoy-PASS flip blockers (forbidden drops) = 98**, target 0:

| universe | flip-BUGs (OFF-pass ∩ ON-fail) | SameBoy-PASS (BLOCKER) | SameBoy-FAIL (rebaseline) |
|---|---:|---:|---:|
| **CGB gambatte-OCR** (3422 rows) | 37 | **0** | 37 |
| **DMG gambatte-OCR** (3422 rows) | 91 | **79** | 12 |
| **gbmicro** (513 DMG rows) | 31 | **9** | 22 |
| **wilbertpol/age** (mooneye fork) | 10 (+age pre-floored) | **10** | 0 |
| **TOTAL** | 169 | **98** | 71 |

The CGB-OCR bar HOLDS (0 blockers, the census the CLAUDE.md State tracked is
real). **The flip is blocked entirely on the DMG side** — 98 SameBoy-PASS rows
the flip DROPS, all in the DMG interrupt-service / timer-completion /
dispatch-count / read-frame families. These are exactly the rows prior sessions
(#11bj/#11bm) characterized as "land WITH the flip's global dispatch/render
reclock (the C3 event itself)". **That reclock does not exist:** the
2026-07-04 #11bs eager-PPU/deferred-CPU split was BUILT and REFUTED
(`dispatch-retime-plan-2026-07-04.md` §8) — moving the DMG dispatch to
SameBoy's frame drops 53 coherent-count rows and hangs mooneye `intr_2_0_timing`
(88/91). The dispatch dot is welded to the read frame by the count tests; no
achievable lever fixes these 98 without regressing the counts. So they cannot
be fixed AND cannot be rebaselined (SameBoy passes them → forbidden drop). The
flip must abort until the DMG interrupt-service reclock is solved (an open
architectural problem, thrice-refuted: #11ai C2ADV, #11br fold, #11bs eager).

## 0. Base + preconditions (all fresh, same tree)

- Worktree `phase-b-s7` @ **`d3d7d40`**, `git diff --quiet` clean (verified
  before and after; no production/baseline edits).
- SameBoy tester rebuilt from `/tmp/sbbuild/SameBoy-1.0.2` (`make tester`);
  register-dump patch added at `Tester/main.c:525` for the mooneye fib verdict
  (`REGS ... bc/de/hl`), md5 `d48ea309…` (was `2e363c8f…` FF82-only).
- **Pixel two-bin (render half): ON 100/100, OFF 100/100** — the §3b render
  reclock is fully landed (byte-identical OFF). `scratchpad/pixel100.txt`.
- **mooneye flag-on (`SLOPGB_MOONEYE_RECLOCK=1`): 91/91.** Convergence gate met.
- `boot_with_reclock` (= `GameBoy::new_with_reclock`, both flags set at
  construction before `apply_post_boot_state`) is byte-identical to the
  post-flip default (C3-CHECKLIST §2; `harness.rs:27` doc + `lib.rs:82`
  `new_inner`). Every ON bin = the true post-flip behavior. Every asserted
  matrix (`gambatte::run_case`, `gbmicro::run_case`, `wilbertpol::run_mooneye`)
  boots via plain `harness::boot` → `GameBoy::new` → flag-on post-flip, so a
  flip-BUG is a real matrix regression.

## 1. The four two-bins (fresh, `d3d7d40`)

Method: `flagon_probe` ON (`boot_with_reclock`) vs OFF (`SLOPGB_PROBE_OFF`,
production) on the archived rowlists; flip-BUG = `comm -23 on_fail off_fail`
(OFF-pass ∩ ON-fail = the rows the flip BREAKS).

- **CGB gambatte-OCR** (`scratchpad/cgb_rowlist.txt`): ON fail **291** / OFF
  fail **486** → **37 flip-BUGs**, **232 flip-FIXes** (net +195). Archives:
  `scratchpad/c3_cgb_{on,off}_fail.txt`, `c3_cgb_flipbugs.txt`,
  `c3_cgb_floorlist.txt`.
- **DMG gambatte-OCR** (`scratchpad/dmg_rowlist.txt`): ON fail **116** / OFF
  fail **103** → **91 flip-BUGs**, 78 flip-FIXes (net **−13**, the flip makes
  DMG-OCR WORSE). Archives: `c3_dmg_flipbugs.txt`, `c3_dmg_sbpass_blockers.txt`,
  `c3_dmg_rebaseline.txt`.
- **gbmicro** (`scratchpad/gbm_all_rows.txt`, 513 DMG): ON fail **68** / OFF
  fail **62** → **31 flip-BUGs** (`c3_gbm_flipbugs.txt`).
- **wilbertpol/age** (`scratchpad/wp_flipblockers.txt`, full model expansion):
  ON fail 8→16 across models / OFF 1 (age only). age `halt-m0-interrupt` fails
  BOTH → pre-existing floor, not a flip-BUG. **10 wilbertpol flip-BUGs.**

## 2. Classification → the flip-bar census

### CGB gambatte-OCR: 37/37 SameBoy-FAIL — bar HOLDS (0 blockers)

`classify_cgb_regr.py` (SameBoy `--cgb --length 4` OCR vs `_out<hex>`):
`BUG(sb==want)=0  FLOOR(sb!=want)=37  UNK=0`. Every flip-BUG is a gambatte-ref
`_2`/glitch leg SameBoy also fails → rebaseline-OK. This confirms the census
the State tracked; the CGB two-bin is genuinely clean.

### DMG gambatte-OCR: 79 SameBoy-PASS BLOCKERS + 12 rebaseline

`classify_dmg.py` (SameBoy `--dmg --length 4` OCR, +1px glyph trial):
`BUG(sb==want)=79  FLOOR=12  UNK=0`. Spot-verified 4/4 manually
(`enable_display/frame0_m0irq_count_scx2_1`→SB 90=want, `tima/tc00_1stopstart_
ff_tma_2`→SB 00=want). The 79 by family (== the #11bm "measured parks"):

| family | n | mechanism (the welded read↔dispatch frame) |
|---|---:|---|
| `tima/*` | 45 | S6 timer-completion — leading-edge FF0F reads IF one M-cycle early |
| `enable_display/frame*_m0irq_count*` | 8 | dispatch-COUNT — deferred read loses a mode-0 dispatch |
| `m2int_m0irq/*` | 8 | m2-chained mode-0 read-frame |
| `window/*` | 5 | render-length / window-trigger residual |
| `m0int_m0irq` 2 · `lyc0int_m0irq` 2 · `lycEnable` 2 | 6 | line-start STAT read-frame |
| `sprites` 2 · `serial` 1 · `oamdma` 1 · `miscmstatirq` 1 · `m2enable` 1 · `m0enable` 1 | 7 | IF-lifecycle / completion / STAT-service |

All 79 fail flag-on with the SAME signature as the gbmicro/wilbertpol dispatch
core — the DMG interrupt service samples one M-cycle off SameBoy's frame.

### gbmicro: 9 SameBoy-PASS BLOCKERS + 22 rebaseline

Patched `sameboy_tester --dmg` FF82 dump (ff82=01 ⇒ SameBoy PASS ⇒ blocker),
re-confirmed at length 2 AND 4:

- **22 SameBoy-FAIL → rebaseline** (ff82=ff): `hblank_int_scx{0-7}_if_b` (8),
  `hblank_int_scx{1-7}_nops_a` (7), `_nops_b` (7). SameBoy fails them exactly as
  slopgb-flip does (the #11bs finding — the dispatch cannot move to SameBoy's
  frame; these baseline). This is the "43-wall is a mirage" set the task cited.
- **9 SameBoy-PASS → BLOCKER** (ff82=01): `hblank_int_scx7` (2E→2F,
  dispatch-count), `hblank_scx3_if_a/if_b/if_c/int_a` (4, read-frame gap the
  #11bk DELIVER/SERVICE-CLEAR doesn't cover), `int_timer_halt`,
  `int_timer_halt_div_b` (2, S6 timer-completion), `stat_write_glitch_l1_a`,
  `stat_write_glitch_l143_a` (2, slopgb-flip fires a spurious mid-mode STAT
  glitch IF SameBoy lacks — E2 vs E0). Direct per-row re-run: all 9 pass slopgb
  OFF, fail slopgb ON; none in the gbmicro baseline or `TESTBENCHES` exempt list
  → live matrix regressions. The #11bs §8 explicitly flagged these as
  "genuinely-open" and did NOT resolve them; this census resolves them:
  SameBoy-PASS → blockers.

### wilbertpol: 10 SameBoy-PASS BLOCKERS

Patched tester register dump (PASS ⇔ bc=0305 de=080d hl=1522; positive control
`ly_lyc_153-GS/-C` both PASS). All slopgb-flip fails are B=48 (dispatch one
M-cycle late):

- `ly_lyc_153_write-GS` on [Dmg,Mgb,Sgb,Sgb2] = **4** (SameBoy `--dmg` PASS)
- `ly_lyc_153_write-C` on [Cgb,Agb] = **2** (SameBoy `--cgb` PASS)
- `timer_if` on the 4 grayscale models = **4** (SameBoy `--dmg` PASS; Cgb/Agb
  pass flag-on, not flip-BUGs)

Not in `baselines/wilbertpol.txt`, not in main mooneye → live regressions.

## 3. GO / NO-GO reasoning

The C3-CHECKLIST §3 rule: *SameBoy-PASS flip-BUG ⇒ STOP, forbidden drop — fix
it or abort.* There are **98** such rows. None is fixable:

1. **The DMG dispatch cannot be reclocked** (the root cause of ~90 of the 98).
   Three independent refutations, all this project: #11ai (C2ADV +4 PPU
   advance, mooneye 89/91 `B=42`), #11br (imminent-rise fold, drops 9 counts +
   `intr_2` hang), **#11bs (the genuine eager-PPU/deferred-CPU split — the §4
   build plan — BUILT and REFUTED: +24 presence rows / −53 coherent-count rows,
   mooneye 88/91)**. Cc+4 dispatch ∧ cc+0 reads is INCOHERENT for the
   dispatch-COUNT tests, and there is no bus-observable discriminator between a
   presence row (wants the move) and a count row (wants no move) using the same
   mode-0 rise. Production passes both only because its reads are ALSO cc+4 —
   which is the CGB leading-edge read-frame the whole reclock exists to remove.
2. **S6 timer/serial completion** (tima 45 + serial 1 + gbmicro `int_timer_halt`
   ×2 + wilbertpol `timer_if` ×4): a timer-domain deferred-completion advance,
   not the PPU dispatch; the C0-DIV sweep `{−4..12}` has zero effect (#11ai
   DO-NOT-RETRY). Orthogonal, still unbuilt.
3. **Engine glitch-IF** (gbmicro `stat_write_glitch` ×2): slopgb-flip fires a
   spurious STAT edge SameBoy suppresses — a separate glitch-suppression lever.

Because the fix is unavailable AND SameBoy passes every one, the 98 can neither
be fixed nor baselined. **Flipping now ships 98 SameBoy-pass regressions** (a
LESS-accurate DMG than production) and fails the C4 "every-oracle-zero-drop /
never drop a test SameBoy passes" gate. → **NO-GO.**

The prior "the real blockers are the CGB two-bin" assertion (#11bs §8 verdict,
unmeasured) is **CORRECTED**: the CGB two-bin is clean (0 blockers); the
blockers are 100% DMG-side, in the exact dispatch/timer families #11bs proved
un-reclockable. The C3 flip is gated on that open architectural problem, not on
any remaining CGB/render slice.

## 4. Rebaseline manifest (the 71 SameBoy-FAIL flip-on fails)

Ready to paste at the flip **once the 98 blockers clear** (cannot apply before —
the flip itself is blocked). Floor-class letters per the `gambatte.txt` header;
the 37+12 gambatte rows form one dated **C3 leading-edge read-frame** swap block
(the flip installs SameBoy's cc+0 read frame → the `_2`/glitch legs where
gambatte-ref disagrees with SameBoy become floor).

### 4a. CGB gambatte.txt — 37 rows (10 class A · 27 class H)

Class **A** (ds / lcd-offset sub-cycle phase — 10):
`lcd_offset/offset3_lyc98int_ly_count_1`,
`lycEnable/{late_ff41,late_ff45,lyc153_late_ff41,lyc153_late_ff45}_enable_ds_lcdoffset1_2`,
`lycEnable/lycwirq_trigger_ly00_stat50_ds_lcdoffset1_2`,
`m1/{m1irq_late_enable_ds_lcdoffset1_2, m1irq_m2disable_lycdisable_ds_2,
m1irq_m2enable_lyc_ds_1, m2m1irq_ifw_ds_2}`.

Class **H** (single-speed leading-edge read-frame `_2`/`_1` A/B trade — 27):
`display_startstate/stat_{2,scx2_2,scx5_2}`,
`lyc153int_m2irq/lyc153int_m2irq_ifw_1`,
`lycEnable/{lyc0_m1disable_2, lyc153_late_enable_m1disable_2,
lyc153_late_m1disable_2}`,
`m0enable/lycdisable_ff45_{,scx1_,scx2_,scx3_}2` (4),
`m1/{ly143_late_m0enable_2, m1irq_late_enable_2, m1irq_m0disable_2,
m1irq_m2disable_lycdisable_2, m1irq_m2disable_lycdisable_3, m1irq_m2enable_lyc_1,
m2m1irq_ifw_2}` (7),
`m2enable/{late_enable_m1disable_ly0_2, late_m1disable_ly0_2}`,
`miscmstatirq/lycstatwirq_trigger_ly00_10_50_1`,
`window/{arg/late_wy_1, late_disable_late_scx03_wx0f_2, late_disable_scx2_1,
late_disable_scx3_1, late_disable_scx5_1, late_wy_1}` (6).
Full list w/ sb/want: `scratchpad/c3_cgb_floorlist.txt`.

### 4b. DMG gambatte.txt — 12 rows (class H, DMG read-frame `_2`)

`lyc153int_m2irq/lyc153int_m2irq_ifw_1`,
`m0enable/{disable_scx3_2, disable_scx7_2, lycdisable_ff41_2,
lycdisable_ff41_scx3_2}`,
`m1/{m1irq_m2disable_lycdisable_3, m1irq_m2enable_lyc_1, m1irq_m2enable_lyc_2,
m2m1irq_ifw_2}`,
`miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_4`,
`window/{arg/late_wy_1, late_wy_1}`. Full: `scratchpad/c3_dmg_rebaseline.txt`.

### 4c. gbmicrotest.txt — 22 rows (class H, DMG dispatch, SameBoy also fails)

`hblank_int_scx{0-7}_if_b` (8) · `hblank_int_scx{1-7}_nops_a` (7) ·
`hblank_int_scx{1-7}_nops_b` (7).

**Manifest total: 71** (37 CGB + 12 DMG gambatte + 22 gbmicro). Plus the
pre-seeded §3 joiners (13 pixel DMG-rebaseline, already classified) — but the
manifest is INERT until the 98 blockers are resolved.

## 5. Banked flip-FIXes (the flip's benefit, for context)

CGB gambatte **+232**, gbmicro **+25**, DMG gambatte-OCR **+78** now-pass legs
(would un-baseline at the flip). Real, but outweighed by the 98 forbidden drops
+ the −13 net DMG-OCR regression — the DMG reclock is a net fidelity LOSS as
shipped.

## 6. Reproduction

- Two-bins: `SLOPGB_ROWLIST=scratchpad/<cgb|dmg>_rowlist.txt
  target/pixel/release/deps/gbtr-* --ignored gambatte::flagon_probe::flagon_probe
  --nocapture` (+ `SLOPGB_PROBE_OFF=1`). ALWAYS invoke the built binary directly
  or pass an ABSOLUTE `SLOPGB_ROWLIST` — `cargo test -p` runs the binary with
  CWD = the crate dir, so a relative rowlist path is NotFound.
- CGB classify: `classify_cgb_regr.py flipbugs.txt`. DMG classify:
  `SLOPGB_GBTR_ROOT=<worktree>/test-roms/game-boy-test-roms-v7.0
  classify_dmg.py flipbugs.txt outprefix`.
- gbmicro/mooneye SameBoy verdict: patched `sameboy_tester --dmg --length 4`,
  `grep FF82RESULT` (ff82=01 ⇒ pass) / `grep REGS` (bc=0305 de=080d hl=1522 ⇒
  fib pass). Patch = the `fprintf` pair at `Tester/main.c:524-525`.
