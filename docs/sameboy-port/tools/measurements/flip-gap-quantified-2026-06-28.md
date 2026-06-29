# Flip gap — quantified flag-on regression analysis (2026-06-28, full CGB probe)

Full 3524-row CGB gambatte probe, flag-ON vs flag-OFF (boot_with_reclock vs boot):
- flag-ON: 501 fails. flag-OFF: 487 fails. NET -14 on the OCR probe.
- **206 REGRESSIONS** (pass OFF, fail ON) — must be fixed or baselined (SameBoy-non-pass) to flip.
- 192 FIXES (fail OFF, pass ON) — the reclock's wins.

## Frame-alignment ruled OUT (build-measured)
All 206 regressions fail at OCR capture frame delta -1/0/+1/+2 (SLOPGB_FRAME_DELTA).
They are GENUINE render/engine bugs, NOT an OCR-capture-frame mis-alignment. No cheap
global frame lever exists; the flip needs per-cluster fixes.

## Regressions by mechanism (the per-cluster work to the flip)
```
     53 window
     19 lycEnable
     14 m1
     12 halt
      9 m0enable
      9 lcd_offset
      8 speedchange
      8 enable_display
      8 cgbpal_m3
      7 m2int_m3stat
      7 dma
      6 miscmstatirq
      6 m2enable
      5 vram_m3
      5 lyc153int_m2irq
      5 ly0
      4 tima
      4 serial
      4 oam_access
      3 m2int_m0irq
      3 display_startstate
      2 m2int_m2stat
      2 irq_precedence
      1 scx_during_m3
      1 m2int_m2irq
      1 m2int_m0stat
```

## want->got pattern (the failure shape)
```
     57 0->3
     34 E->E
     25 0->2
     18 3->0
     15 2->0
     15 1->3
     10 9->0
      5 0->7
      4 8->8
      4 3->1
      4 0->1
      3 1->0
      2 9->9
      2 6->7
      2 3->2
      2 1->7
      1 F->0
      1 5->A
      1 5->0
      1 0->0
```
- 57 0->3 = render OVER-extension (late_disable/reenable/window: mode-3 too long).
- 34 E->E = lycEnable ENGINE (STAT LYC/mode-bit delivery, stat_update_tick/lyc.rs).
- 25 0->2 / 18 3->0 / 15 2->0 / 15 1->3 = render length + engine mode delivery.
- ~32 are S6/S7 (lcd_offset/speedchange/dma/tima/serial) — out of render/engine scope.

## The honest attack order (highest-leverage first)
1. RENDER over-extension (57 0->3): the window-abort / late-write mode-3 shortening
   (late_disable CGB abort, late_reenable, late_scx SCX-latch). Render-pipeline,
   per-sub-mechanism. Biggest single bucket.
2. lycEnable/m1 ENGINE (34 E->E + 14 m1): the stat_update_tick LYC/mode delivery
   residual after #11j/k/l/r. Engine, the FF45-disable/late-enable timing.
3. halt (12) wake-clock + m2int/vram/oam/cgbpal render accessibility (~24).
4. S6/S7 DS batch (~32): lcd_offset/speedchange/dma/tima/serial — the double-speed
   read grid + DMA/timer reclock. Separate port stage.
Each is a tier2-gated A/B-swept slice; the flip lands when all 206 (+DMG) are GREEN
or baselined SameBoy-non-pass, then golden+ratchet rebaseline + C4 all-oracle-zero-drop.
