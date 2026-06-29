# Glitch-line mode-0 IRQ dispatch — the `frame0_m0irq_count` root (2026-06-29 #11ad)

## TL;DR (build-measured, the goal's "engine delivery" premise CORRECTED)

The canary `enable_display/frame0_m0irq_count_scx2_1` (`want90 got00`, flag-on)
is **NOT** "the engine fires 2299 mode-0 dispatches but the CPU delivers 0 IRQs"
(the prior-session hypothesis). The ROM uses **NO interrupts** (IE stays 0); it
**polls FF0F**. The real root: on the **CGB LCD-enable glitch line** the tier2
mode-0 STAT IRQ rises **2 dots early** (dot 252 instead of SameBoy `cfl=257` =
our dot 254), so the ROM's dot-252 IF poll observes the mode-0 bit a poll early
and mis-measures.

Fixed CGB (+2/−0, gate green). DMG is a **genuine multi-mechanism atomic**
(measured negative, documented below) — left as a baselined floor.

## The ROM (disassembled, `frame0_m0irq_count_scx2_1`)

```
015D: LCDC=0x91          ; LCD ON  → next line (ly0) is the LCD-enable GLITCH line
0165: IF=0               ; clear pending
0168: STAT=0x08          ; mode-0 (HBlank) STAT source enable
016C..0199: NOP ×46      ; calibrated delay → the poll lands at ly0 dot ~252
019A: LDH A,(FF0F)       ; read IF        ← THE POLL
019C: CP E0              ; expect 0xE0 = NO IF bit set (FF0F reads 0xE0|intf)
019E: JR NZ,021C         ; any IF bit set → branch away
01A0: XOR A; LDH(FF0F),A ; else clear IF, loop to next line
...
021C: LDH A,(FF44)       ; (on a set bit) read LY
021E: JP 7000            ; display LY → the rendered number
```

The loop clears IF every line, so each line's mode-0 STAT pulse is wiped before
it is ever polled; the loop runs until the **VBlank** bit (bit 0) appears at
LY=144 → reads `FF44 = 144 = 0x90`. The pass value 0x90 is the VBlank line, NOT
a mode-0 count. The mode-0 STAT bit must stay UNSEEN by the dot-252 poll.

## The measurement (slopgb tier2 flag-on, `SLOPGB_S5DBG`)

| | poll read (ly0) | mode-0 raise (ly0) | result |
|---|---|---|---|
| OFF (production) | dot 252, `intf=00` | after the poll | `90` ✓ |
| ON (no fix) | dot 252, `intf=02` | **dot 252** (glitch) | `00` ✗ (bail) |
| ON (fix) | dot 252, `intf=00` | **dot 254** | `90` ✓ |

The deferred FF0F read samples at the leading edge of its M-cycle (dot 252); the
tier2 glitch-line mode-0 rise at dot 252 falls in the *paid* M-cycle → folded
into `intf` before the sample → seen. Bare lines (ly1+) raise at dot 254 (>252)
→ unseen → fine. **Only the glitch line raised early.**

## Root: the glitch-line IRQ followed `vis_early`, not `line_render_done`

Bare lines dispatch the mode-0 STAT IRQ at `line_render_done` (dot 254 = SameBoy
`cfl=257`; `m0_flip_events`: "the IRQ side keys on `line_render_done`, not
`vis_early`"). The glitch branch of `update_mode_for_interrupt` used
`vis_mode()`, which goes mode-0 at `vis_early` (the `lcdon_timing-GS` FF41-read
back-date, 2 dots early). SameBoy raises the glitch-line mode-0 STAT at the same
`cfl=257` as every bare line (`SBTRACE STAT_IRQ ly=0 cfl=257 mfi=0`, verified).

**Fix** (`reclock.rs`, CGB + tier2 + SS): key the glitch-line mode-0 IRQ on
`line_render_done` (dot 254), not `vis_early`. The FF41 read side
(`vis_mode`/`vis_early`) is untouched. +2/−0 CGB (`frame0_m0irq_count_scx2_1`,
`ly0_m0irq_scx1_1`); pinned `tier2_glitch_m0irq_dispatch_passes`.

## The DMG measured NEGATIVE (a genuine atomic — left a floor)

SameBoy renders **90 on DMG too** (OCR-verified) — so the DMG row is a real bug.
But on DMG the glitch `line_render_done` already lands at dot **252**, and the
SAME glitch-line mode-0 rise drives BOTH:

- the **poll** path (`frame0_m0irq_count`): wants the rise PAST the dot-252 poll
  (→ dot 254), and
- the **halt-wake** path (`gbmicrotest int_hblank_halt_scx0-7`, the
  62,62,62,63,63,63,63,64 grid, pinned `tier2_int_hblank_halt_passes_dmg`):
  calibrated at the dot-252 frame.

Moving the DMG glitch dispatch +2 lands the poll but breaks `int_hblank` +1
(scx1: 0x63 vs 0x62) — and vice-versa. SameBoy resolves both at its sub-T-cycle
IF-raise phase (`cfl=257` is a specific T within the M-cycle: the mid-cycle halt
sampler sees it, the leading-edge poll read does not); slopgb's whole-dot raise
cannot place a single dot that satisfies both. So the DMG `frame0_m0irq_count`
stays a baselined floor (production renders 00) and the fix is **CGB-gated** —
DMG byte-identical, `int_hblank` green. This is the documented irreducible-at-
whole-dot residual, to be revisited only with a sub-M-cycle IF-raise phase.

## Scope (build-measured, two-bin both bins rebuilt)

- CGB glitch-family (371 rows): **+2 / −0** (297→299).
- DMG glitch-family: **0 / 0** (byte-identical — CGB-gated).
- 686 baselined CGB rows: **0 / 0** (no floor disturbed).
- The "~40-row shared engine-delivery root" the goal hypothesized is **WRONG**:
  only the enable_display/glitch family shares this root (the canary lands 3 of
  the 149 CGB flip-BUG rows, 1→3). halt/lycEnable/m2enable/etc. are separate
  mechanisms (the poll-vs-halt split is per-ROM, not one engine bug).

## Gate (all green this session)

gbtr OFF 208/208 (incl golden) · mooneye OFF 91/91 · mooneye flag-on 91/91 ·
24 tier2 pins · `int_hblank_halt` DMG green · clippy −D clean · reclock.rs
`--edition 2024` fmt-clean. Defaults NOT flipped (flag-on convergence slice).
