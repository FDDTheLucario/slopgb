# DMG window-exit READ-DEBT calibration — REFUTED at the read granularity: the 7 rows are NOT a uniform read-debt subclass. Their decisive FF41 reads split into THREE incompatible fix-directions (rphd-DOWN ×2, rphd-UP ×2, exit=None/render ×2), and every debt-reachable row is WELDED to a want-opposite `_1`/`_2` sibling sharing byte-identical PPU-observable read state (line·dot·render-recorder·win-flags·read_carried·rphd·exit). No PPU-side discriminator separates the pairs; the split is the ROM's sub-read-cycle poll phase, unreachable on the whole-dot (and current half-dot `dhalf`) read frame (2026-07-11, #11ea)

Base: `finish-port-halfdot @ 24c20aa` (= #11dz). **NO CODE SHIPPED — tree
byte-identical @ 24c20aa** (`git diff HEAD crates/` empty). All experiments were
env-gated scaffolding (`SLOPGB_WSCOPE`/`WDEBT`/`WDUMP`/`WDUMP2` on a `winexit_debt_exp`
peek + a `read_laws.rs` line-272 subtract) and REVERTED. Measure-only REFUTE (the
#11br / #11bu / #11bw / #11dz pattern).

## TL;DR — REFUTE, with the decisive-read trace

#11dz's read-frame prescription was: *"this DMG window-exit subclass wants ~0hd
read-debt, NOT the #11by/#11cb +8hd; gate an `eager_value` reduced-debt arm on the
DMG window-exit FF41 read and land all 7 on their true exit side."* **The A/B
measurement refutes it three ways:**

1. **The 7 do not share a fix DIRECTION.** Their decisive FF41 reads (the read whose
   mode value becomes the OCR digit — isolated per-ROM via a `vis_mode_read`-tail dump
   correlating the baseline `got` digit) split:
   - **rphd-DOWN** (want a *smaller* read position → reduce debt): `late_disable_early_scx03_wx11_2`
     (decisive read `rphd 512 == exit 512`, at the boundary — needs debt ≥ 1),
     `late_disable_late_scx03_wx11_2` (`rphd 520`, exit 512 — needs debt ≥ 9).
   - **rphd-UP** (want a *larger* read position → NEGATIVE debt / lower exit):
     `late_reenable_2`, `late_reenable_wx0f_2` (decisive read `rphd 512 < exit 518` →
     mode 3; want 0, so the read must reach the exit — reducing debt makes it WORSE).
   - **exit = None** (the debt lever cannot touch them): `late_wy_FFto2_ly2_scx2_1`
     (decisive read dot 260, native mode 0, NO exit arm fires),
     `late_scx_late_disable_0` (decisive read dot 252, native mode 3, exit None). The
     read-debt only applies inside the `read_pos_hd() < exit_adj` compare, which is
     never reached when `vis_exit_hd` returns None → these are RENDER/exit-arm misses,
     not read-frame misses.

   A single debt sign cannot satisfy both rphd-DOWN and rphd-UP; two rows are
   unreachable by the debt lever entirely. #11dz's "+8hd is the wrong offset, they want
   ~0" holds ONLY for the two `late_disable_early/late` rows, and even there it is a
   sibling-welded boundary flip, not a clean re-host.

2. **Every debt-reachable row is WELDED to a want-opposite sibling at identical read
   state.** The `_1`/`_2` (and SCX) siblings poll the SAME PPU dot with the SAME render
   (identical `wx_match_dot`/`win_predraw_abort`/`win_reenable_dot`/`win_active`/
   `read_carried`/`rphd`/`exit`), but latch the opposite want:

   | debt lever | recovered | dropped (want-opposite siblings) | net |
   |---|---|---|---|
   | `bpre` (predraw-abort ∧ on-screen ∧ `rphd==exit` ∧ carried), debt +2 | `late_disable_early_scx03_wx11_2` | `late_disable_early_scx03_wx0f/10/11/12_1`, `late_disable_early_scx03_wx12_2` | **+1 / −5** |
   | `reen` (win_active ∧ reenable ∧ on-screen ∧ carried), debt −8 | `late_reenable_2`, `late_reenable_wx0f_2` | `late_reenable_1`, `late_reenable_scx2/3/5_1`, `late_reenable_scx2/3_2`, `late_reenable_wx0f_1` | **+2 / −7** |
   | `on` (any window ∧ on-screen), debt +8 | `late_disable_early_scx03_wx11_2` | +19 (the `m2int_wx*_m3stat_2` extend family + `_1` siblings) | **+1 / −19** |
   | `act` (win_active ∧ on-screen ∧ !stalled), debt +8 | — | — | +0 / −0 (fires on none of the 7 — the targets' decisive reads have `win_active==false` or are the carried boundary read) |

   The decisive reads of `late_disable_early_scx03_wx11_1` (want 0) and `_2` (want 3)
   are byte-identical in every PPU-observable field. No discriminator in `read_pos_hd`
   or the exit compare can separate them — the split is the ROM's internal M-cycle poll
   phase (the gambatte `_1`/`_2` NOP count), which the whole-dot read frame does not
   carry. This is the #11bu S6-completion / #11bw "no read-time discriminator" weld,
   reproduced for the DMG window-exit family.

3. **The exit is peek-agnostic where an arm fires; where none fires it is a render
   miss.** For `late_wy_FFto2_ly2_scx2_1` the decisive read sits at dot 260 with
   `win_active==true` yet `render.active==false` (render already done) and native mode
   0, so neither arm D1 (needs `m==3`) nor the #11cr HALFDOT Part-A fallback (needs
   `render.active`) fires → `exit == None` → native 0 returned regardless of debt. The
   window's extend is simply not reconstructed at that read dot. `late_scx_late_disable_0`
   is the same class (wx 0x27, native mode 3 at the decisive read, no arm). These want a
   RENDER-length / exit-arm fix (an off-arm window reconstruction), NOT a read-debt.

## Baselines reproduced (exact, at 24c20aa)

| metric | value | gate |
|---|---:|---|
| `flagon_probe[ON]` EV DMG | pass 1551 / **fail 52** / skip 1819 | steady-state floor ✓ |
| golden_fingerprint | byte-identical (tree == HEAD, no crates change) | THE gate ✓ |

## The 7 target rows — decisive-read profile (own dump, reverted)

`vis_mode_read`-tail dump (env `SLOPGB_WDUMP2`), `eager_value`, per-ROM, the read
whose native `m` matches the baseline `got` digit:

| row | want | got | decisive read | native `m` | exit | direction |
|---|:--:|:--:|---|:--:|:--:|---|
| `late_wy_FFto2_ly2_scx2_1` | 3 | 0 | ly2 dot260 rphd528 | 0 | **None** | render (no arm) |
| `late_wy_FFto2_ly2_scx3_1` | 3 | 0 | (mirror of scx2) | 0 | **None** | render (no arm) |
| `late_disable_early_scx03_wx11_2` | 3 | 0 | ly1 dot252 carr rphd512 | 3 | 512 (==rphd) | rphd-DOWN, welded to `_1` |
| `late_disable_late_scx03_wx11_2` | 3 | 0 | ly1 dot256 carr rphd520 | 0 | 512 | rphd-DOWN ≥9, welded |
| `late_reenable_2` | 0 | 3 | ly1 dot252 carr rphd512 | 3 | 518 | rphd-UP, welded to `_1` |
| `late_reenable_wx0f_2` | 0 | 3 | ly1 dot252 carr rphd512 | 3 | 518 | rphd-UP, welded |
| `late_scx_late_disable_0` | 0 | 3 | ly1 dot252 carr rphd512 | 3 | **None** | render (no arm) |

## Why this is the read frame's floor, not a missed calibration

The +8hd read-debt (#11by/#11cb) is a WHOLE-CONFIG shift: it moves both siblings of a
gambatte `_1`/`_2` pair by the same 8hd, so it can only re-side a pair whose boundary
sits *between* the two reads. Where the pair's decisive reads coincide at the same PPU
dot with the same render (the DMG window-exit `late_disable`/`late_reenable` families),
no debt re-sides one without the other — the discriminator the split needs is the ROM's
sub-M-cycle poll phase (`dhalf`-finer), which the eager whole-dot read frame does not
resolve (the current `dhalf` stays 0 at SS). This is the same barrier #11bw named for
the CGB window `vis_exit_hd` arms and #11bu for the S6 completion reads. The two
`exit==None` rows are a DIFFERENT residual again (a render-side off-arm window
reconstruction), not the read frame at all.

## Gates (all hold — NO code shipped, tree byte-identical @ 24c20aa)

1. `golden_fingerprint` byte-identical — tree == HEAD (no crates change).
2. EV DMG **52** unchanged; EV CGB **295**, tier2 **291/116** unchanged (nothing shipped).
3. Zero regression (nothing shipped).
4. All probe edits (`winexit_debt_exp` peek on `engine.rs`, a line-272 subtract + a
   `WDUMP2` dump on `read_laws.rs`) REVERTED; `git diff HEAD crates/` empty.
5. No file grew (untouched at HEAD: `read_laws.rs` 999, `engine.rs` 589).

## Do-not-re-chase ledger (add)

- The 7 DMG window-bar rows are NOT a uniform read-debt subclass. Their decisive FF41
  reads split rphd-DOWN ×2 / rphd-UP ×2 / exit=None ×2 — no single debt sign fixes
  them, and the debt-reachable four are WELDED to want-opposite `_1`/`_2`/SCX siblings
  at byte-identical PPU-observable read state (`bpre` +1/−5, `reen` +2/−7, `on`
  +1/−19). Do NOT re-attempt a reduced-debt arm on the DMG window-exit read — it is a
  sub-M-cycle poll-phase weld, not a frame miscalibration.
- CORRECTS #11dz's read-frame prescription ("they want ~0hd read-debt"): true only for
  the two `late_disable` rows (and there a sibling-welded boundary flip), FALSE for the
  reenable pair (opposite direction) and the `late_wy`/`late_scx_late_disable` pair
  (exit=None, a render-side miss). #11dz correctly refuted the RENDER-recorder arm; the
  read-frame it pointed at is itself a weld/render split, not a tractable calibration.
- The `late_wy_FFto2_ly2_scx*_1` + `late_scx_late_disable_0` rows are a RENDER-side
  off-arm window reconstruction (the read lands where `win_active` holds but the render
  is done and no `vis_exit_hd` arm reconstructs the extend → native mode returned).
  That is the #11cr HALFDOT Part-A render residual, not a read-debt lever.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd7
BIN=$(ls -t target/hd7/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# EV DMG baseline (52 fail)
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/dmg_rowlist.txt \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\[ON\]'
# The A/B weld (re-add the reverted winexit_debt_exp scopes + line-272 subtract):
#   SLOPGB_WSCOPE=bpre SLOPGB_WDEBT=2  -> +1/-5   (late_disable predraw family)
#   SLOPGB_WSCOPE=reen SLOPGB_WDEBT=-8 -> +2/-7   (late_reenable family)
#   SLOPGB_WSCOPE=on   SLOPGB_WDEBT=8  -> +1/-19  (breaks the m2int extend family)
# decisive read (re-add the reverted WDUMP2 dump at the vis_mode_read tail):
#   SLOPGB_WDUMP2=1 ... per-ROM -> the exit=None / rphd/exit split above.
golden: SLOPGB_REQUIRE_ROMS=1 cargo test -p slopgb-core --test gbtr --release golden_fingerprint
```
