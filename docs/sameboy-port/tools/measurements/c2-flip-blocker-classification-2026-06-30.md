# The DEFINITIVE C2/C3 flip-blocker classification (2026-06-30 #11am)

The terminal-lift question answered with a fresh, full-battery, SameBoy-ground-truth
two-bin: **of the 183 CGB flip-BUGs, exactly which would the C3 default flip DROP
(SameBoy-pass = forbidden regression) vs REBASELINE (gambatte-ref = allowed)?** This
is the sharpest quantification yet of what the flip actually costs — it converts the
prior "~106 of 114 DMG are slopgb-LE bugs" estimate (#11ah) into a measured
per-row classification of the full CGB set.

## Method (fresh binaries, this session)

- Two-bin: `flagon_probe` ON (`boot_with_reclock`) vs OFF (production) vs LE
  (`set_leading_edge_reads`), all from the fresh HEAD bin (`target/gbtr_head`,
  `gbtr-2a71f7405c56c400`), over `scratchpad/cgb_rowlist.txt` (3422 rows).
- SameBoy classification: `docs/sameboy-port/tools/classify_cgb_regr.py` — runs
  `sameboy_tester --cgb --length 4` on each flip-BUG ROM, OCRs the framebuffer
  (gambatte-glyph), compares to the `_out<hex>` expectation. **sb==want ⇒ SameBoy
  PASSES the row ⇒ the flip would DROP a SameBoy-pass (forbidden). sb≠want ⇒
  gambatte-reference ⇒ the flip REBASELINES it (allowed).**

## The census

```
ON  (boot_with_reclock):  pass=2546  fail=476     (was 479 @ #11ah; the +3 #11al shipped)
OFF (production):         pass=2536  fail=486
LE  (leading_edge only):              fail=597
flip-BUGs (OFF-pass ∧ ON-fail):       183          (185 @ #11ah; −2 net from #11al + variance)
  LE-split: 139 ENGINE-DISPATCH (LE-fail) / 44 RENDER-FRAME (LE-pass)
  speed:     84 double-speed / 99 single-speed
```

## THE RESULT — 132 of 183 flip-BUGs are SameBoy-PASSES (the true flip blockers)

```
132  SameBoy-pass (sb==want)  → TRUE REGRESSIONS the flip would drop — MUST be FIXED, can't rebaseline
 51  gambatte-ref (sb≠want)   → REBASELINE-OK at the flip (SameBoy renders ≠ gambatte too)
```

**The flip is blocked on FIXING 132 SameBoy-pass rows.** A SameBoy-pass that flag-OFF
passes and the flip drops is a forbidden regression (the goal's NEVER-drop rule), so
these cannot join `gambatte.txt` — they must be made to pass flag-on first. The 51
gambatte-ref rows rebaseline cleanly (the #11ah serial/tima `_2` pattern, generalized).

### The 132 blockers by atomic class (the C2-lever each needs)

```
RENDER-LENGTH  56   the production render mode-3 LENGTH port (visible 3→0 boundary at
                    SameBoy 167+SCX&7+penalties, EXTEND+SHORTEN), per-config-exact
ENGINE-IF      30   the stat_update_tick dispatch/match-dot read-frame (#11al boundary)
S6-DS          21   the double-speed read grid + cycle_write conflict table (PORT-PLAN S6)
READ-FRAME     13   the −4 ISR deferred read position (serial/tima S6-completion + SS)
WAKE-CLOCK     12   the mode-0-halt-wake sub-M-cycle T-phase (halt *_m0stat_*)
```

LE-split of the 132: **98 ENGINE-DISPATCH / 34 RENDER-FRAME.** Speed: **68 DS / 63 SS.**

### The 51 rebaseline-OK rows by family

```
m1 11 · window 8 · m0enable 7 · lycEnable 7 · lyc153int_m2irq 3 · display_startstate 3 ·
tima 2 · serial 2 · m2enable 2 · lcd_offset 2 · miscmstatirq 1 · ly0 1 · dma 1 · cgbpal_m3 1
```

**Note the families in BOTH sets** (m1, m0enable, lycEnable, window, lyc153, serial,
tima…): these are the **A/B `_1`/`_2` pairs** — the `_1` leg is a SameBoy-pass (blocker,
must fix), the `_2` leg is gambatte-ref (rebaseline). They COLLAPSE to the same slopgb
deferred read dot (the read straddle), so any whole-dot lever that fixes `_1` inverts
`_2` — the textbook read-frame A/B the dispatch reclock must decouple, NOT a slice.

## The decisive proof — even "RENDER-LENGTH" blockers are the −4 ISR READ FRAME

`window/late_disable_early_scx03_wx0f_1` [Cgb] (want 0, got 3), traced both emulators:

```
slopgb (tier2 ON):  FF41 read  ly1 dot256  → mode 3   (got=3)   visible 3→0 exit ≈ dot 254/257
SameBoy:            SBREAD ff41 ly1 cfl260  → mode 0   (want=0)  SBMODE vis 3→0 exit cfl 257
```

The window is disabled early, so BOTH emulators render the line BARE with the visible
mode-3 exit at the SAME dot (≈257). **The render LENGTH is correct.** The ONLY divergence
is the read POSITION: slopgb's deferred ISR read lands at dot 256, SameBoy's at cfl 260
— a uniform **−4** (slopgb_dot = SameBoy_cfl − 4, the deferred cc+0 vs SameBoy's
effective cc+4 within the M-cycle). 256 < the 257 exit → mode 3; 260 > 257 → mode 0.

So "RENDER-LENGTH 56" is partly a misnomer: many are the −4 ISR read straddling a
CORRECTLY-placed boundary. The kernel passes only because its read (252) and SameBoy's
(256) are BOTH before the boundary (254/258); the −4 flips the verdict precisely when
the read sits within 4 dots of the exit on the opposite side.

## Why no global +4 read-frame lever closes it (the algebraic wall, re-confirmed)

- **A uniform +4 of read ∧ visible-boundary is a NO-OP for the FF41 read** (both shift
  +4; the read-vs-boundary relationship is frame-invariant). The dispatch-decoupling
  (`vis_early`/`vis_hold_until` already separate the visible boundary from
  `line_render_done`) does NOT change this — confirmed by the late_disable algebra:
  read 256→260, boundary 257→261, still 260<261 → mode 3 ✗. The recipe's "read+4 ∧
  boundary+4 cancel" holds even WITH the decoupling.
- **+4 read-only (boundary fixed) = cc+4 = production** → fails the kernel (m2int
  252→256 > boundary 254). Refuted (recipe).
- **+4 PPU machine-advance** (#11ai option A) → moves the counter-pinned IRQ dispatch →
  intr_2/int_hblank/di_timing HANG (mooneye 91→89). Refuted.
- **sub-M-cycle read clock** (#11ab) → the want-pair reads are CO-TEMPORAL (`cfl*2+dc`
  identical); a read clock at any resolution cannot separate them. Refuted.

**Conclusion:** the FF41-read verdict depends ONLY on (read dot) vs (visible mode-3
exit dot), both in slopgb's own frame. The blockers split into exactly two needs:
1. **Per-config render-length precision** (the 56 RENDER-LENGTH + the engine match-dots):
   place the visible exit at SameBoy's exact `167+SCX&7+penalties` for EVERY config
   (late_disable/late_reenable abort-shorten, sprite-extend, cgbpal/vram/oam access,
   enable_display glitch) — the window law (`vis_mode_read`, +13 shipped) is the proven
   template; the rest is the per-config slog #11af parked. Buildable, row-by-row, with
   SameBoy SBMODE ground truth.
2. **The architectural ISR-read-position model** (READ-FRAME 13 + WAKE-CLOCK 12 + the
   ENGINE-IF/RENDER read-straddle ≈ the A/B `_2` pairs): SameBoy's read lands a
   config-dependent number of T-cycles after the dispatch (the handler's M-cycle
   offset); slopgb's deferred clock collapses the per-handler offset to a uniform −4.
   This is the genuine S6/S7 interrupt-service read-position reconciliation — the
   "atomic multi-session rewrite" the port has always named. NOT a read clock, NOT a
   machine advance — the read POSITION as a function of the interrupt-service T-count.

**Even a perfect render-length port (drains the 56) leaves ≈76 blockers needing the
architectural read-position model, so the flip cannot land until BOTH are built.** This
is why C2 is the atomic co-land, not a slice — re-confirmed by fresh full-battery
SameBoy ground truth.

## The single sharpest remaining lever

**The production render mode-3 LENGTH port (goal lever 3), generalized from the shipped
`vis_mode_read` window law to every config** — it is the largest single dent (56 of 132
blockers, the proven-pattern direction) and the ONLY blocker class that is buildable
incrementally with SameBoy SBMODE ground truth (no architectural rewrite). Build order
by tractability: late_disable/late_reenable abort-shorten (the late_disable proof above
gives the exact +4-too-long visible exit) → sprite-extend → cgbpal/vram/oam access-edge
→ enable_display glitch. Each drains its family; the residual ≈76 read-position rows
co-land with the S6/S7 interrupt-service model in the terminal flip.

## Data files (this session)

- `c2-blockers-sameboy-pass-2026-06-30.txt` — the 132 SameBoy-pass blockers (rel paths)
- `c2-rebaseline-ok-gambatte-ref-2026-06-30.txt` — the 51 gambatte-ref rows (rel\tsb\twant)
- `c2-blockers-by-class-2026-06-30.tsv` — class\trel for every blocker
- `scratchpad/s7/{base_on,base_off,base_le}.txt`, `flipbugs.keys`, `blockers.keys`

## Gate (no code shipped — pure measurement; byte-identical OFF held)

mooneye flag-on 91/91, gbtr OFF byte-identical, 27 tier2 pins. Defaults NOT flipped;
`pixel-pipe-reclock` green, `phase-b-s7` holds the cumulative flag-gated reclock at
the same SHA.
