# CGB flip BUG/FLOOR classification (2026-06-28 #11ac, palette-agnostic SameBoy OCR)

Tool: `docs/sameboy-port/tools/classify_cgb_regr.py` — runs SameBoy (--cgb --length 4)
on each regression row, OCRs its bmp output (palette-agnostic: per-tile background =
the (0,0) pixel, 'on' iff differs; matched against the harness GLYPHS), compares the
first len(want) tiles to the gambatte reference digit. Replaces the lost /tmp/sb_ocr2.py.

## Result — the 200 CGB flip-regressions (current, post #11ac)
- **BUG (SameBoy == want, slopgb-tier2 wrong → must FIX, NEVER baseline) = 149**
- **FLOOR/DIFF (SameBoy != want → slopgb-tier2 correct or DIFF → baselines at the flip) = 51**
The real fix-count to the flip is **149** (not 200); the 51 floors self-resolve at the
C3 rebaseline. (cgb-groundtruth's old 248/39 included the now-fixed sprites.)

## The 149 BUG rows by family (the precise per-cluster fix work)
```
     39 window
     12 lycEnable
     12 halt
      8 speedchange
      8 enable_display
      7 m2int_m3stat
      7 lcd_offset
      7 cgbpal_m3
      6 dma
      5 vram_m3
      5 miscmstatirq
      4 oam_access
      4 m2enable
      4 ly0
      3 m2int_m0irq
      3 m1
      2 tima
      2 serial
      2 m2int_m2stat
      2 m0enable
      2 lyc153int_m2irq
      2 irq_precedence
      1 scx_during_m3
      1 m2int_m2irq
      1 m2int_m0stat
```
Single-speed BUGs = 84 (window/engine/render-accessibility — the C2/S5 work).
Double-speed BUGs = 64 (speedchange/lcd_offset/dma/tima/serial + _ds variants —
the S6/S7 DS read-grid + DMA/timer reclock, a separate port stage).

## Full lists
- BUG (fix): `buglist` below.
- FLOOR (baseline at flip): `floorlist` below (with sb/want).

### BUG (149):
```
gambatte/cgbpal_m3/cgbpal_m3end_ds_2_cgb04c_out0.gbc
gambatte/cgbpal_m3/cgbpal_m3end_scx2_2_cgb04c_out0.gbc
gambatte/cgbpal_m3/cgbpal_m3end_scx5_2_cgb04c_out0.gbc
gambatte/cgbpal_m3/cgbpal_m3end_scx5_ds_2_cgb04c_out0.gbc
gambatte/cgbpal_m3/cgbpal_m3start_2_cgb04c_out0.gbc
gambatte/cgbpal_m3/cgbpal_read_m3start_2_cgb04c_outFF.gbc
gambatte/cgbpal_m3/cgbpal_write_m3start_2_cgb04c_out00.gbc
gambatte/dma/gdma_cycles_2xshort_scx5_ds_1_cgb04c_out3.gbc
gambatte/dma/gdma_cycles_short_scx5_ds_1_cgb04c_out3.gbc
gambatte/dma/hdma_late_destl_1_cgb04c_out0.gbc
gambatte/dma/hdma_late_disable_ds_2_cgb04c_out1.gbc
gambatte/dma/hdma_late_length_1_cgb04c_out0.gbc
gambatte/dma/hdma_late_wrambank_1_cgb04c_out0.gbc
gambatte/enable_display/frame0_m0irq_count_scx2_1_dmg08_cgb04c_out90.gbc
gambatte/enable_display/frame0_m0irq_count_scx2_ds_1_cgb04c_out90.gbc
gambatte/enable_display/frame0_m0irq_count_scx3_ds_1_cgb04c_out90.gbc
gambatte/enable_display/ly0_late_scx7_m3stat_scx1_1_dmg08_cgb04c_out87.gbc
gambatte/enable_display/ly0_m0irq_scx0_ds_1_cgb04c_outE0.gbc
gambatte/enable_display/ly0_m0irq_scx1_1_dmg08_cgb04c_outE0.gbc
gambatte/enable_display/ly0_m0irq_scx1_ds_1_cgb04c_outE0.gbc
gambatte/enable_display/ly1_late_cgbpw_2_cgb04c_out55.gbc
gambatte/halt/late_m0int_halt_m0stat_scx2_1a_dmg08_cgb04c_out0.gbc
gambatte/halt/late_m0int_halt_m0stat_scx2_2a_dmg08_cgb04c_out0.gbc
gambatte/halt/late_m0int_halt_m0stat_scx2_3a_dmg08_cgb04c_out0.gbc
gambatte/halt/late_m0int_halt_m0stat_scx2_4a_dmg08_cgb04c_out0.gbc
gambatte/halt/late_m0int_halt_m0stat_scx3_3a_dmg08_cgb04c_out0.gbc
gambatte/halt/late_m0irq_halt_dec_scx2_2_dmg08_cgb04c_out6.gbc
gambatte/halt/late_m0irq_halt_dec_scx3_2_dmg08_cgb04c_out6.gbc
gambatte/halt/late_m0irq_halt_m0stat_scx2_1a_dmg08_cgb04c_out0.gbc
gambatte/halt/late_m0irq_halt_m0stat_scx2_2a_dmg08_cgb04c_out0.gbc
gambatte/halt/late_m0irq_halt_m0stat_scx3_3b_dmg08_cgb04c_out2.gbc
gambatte/halt/m0int_m0stat_scx2_1_dmg08_cgb04c_out0.gbc
gambatte/halt/m0irq_m0stat_scx2_1_dmg08_cgb04c_out0.gbc
gambatte/irq_precedence/late_m0irq_retrigger_ds_1_cgb04c_outE2.gbc
gambatte/irq_precedence/late_m0irq_retrigger_scx1_1_dmg08_cgb04c_outE2.gbc
gambatte/lcd_offset/offset1_lyc99int_m0irq_count_scx1_ds_1_cgb04c_out90.gbc
gambatte/lcd_offset/offset1_lyc99int_m0irq_count_scx2_ds_1_cgb04c_out90.gbc
gambatte/lcd_offset/offset1_lyc99int_m0stat_count_scx1_ds_1_cgb04c_out90.gbc
gambatte/lcd_offset/offset1_lyc99int_m0stat_count_scx2_ds_1_cgb04c_out90.gbc
gambatte/lcd_offset/offset2_lyc99int_m0stat_count_scx1_1_cgb04c_out90.gbc
gambatte/lcd_offset/offset3_lyc99int_m0stat_count_scx1_1_cgb04c_out90.gbc
gambatte/lcd_offset/offset3_lyc99int_m2irq_count_1_cgb04c_out98.gbc
gambatte/ly0/lycint152_lyc153irq_2_dmg08_cgb04c_outE2.gbc
gambatte/ly0/lycint152_lyc153irq_ds_2_cgb04c_outE2.gbc
gambatte/ly0/lycint152_lyc153irq_ifw_2_dmg08_cgb04c_outE0.gbc
gambatte/ly0/lycint152_lyc153irq_ifw_ds_2_cgb04c_outE0.gbc
gambatte/lyc153int_m2irq/lyc153int_m2irq_1_dmg08_cgb04c_out0.gbc
gambatte/lyc153int_m2irq/lyc153int_m2irq_late_retrigger_1_dmg08_cgb04c_out2.gbc
gambatte/lycEnable/ff41_disable_2_dmg08_out0_cgb04c_out2.gbc
gambatte/lycEnable/ff45_disable_2_dmg08_out1_cgb04c_out3.gbc
gambatte/lycEnable/late_ff41_enable_2_dmg08_out2_cgb04c_out0.gbc
gambatte/lycEnable/late_ff45_enable_2_dmg08_out3_cgb04c_out1.gbc
gambatte/lycEnable/late_ff45_enable_3_dmg08_cgb04c_out1.gbc
gambatte/lycEnable/late_ff45_enable_ds_2_cgb04c_out1.gbc
gambatte/lycEnable/lyc0_ff41_disable_2_dmg08_cgb04c_outE2.gbc
gambatte/lycEnable/lyc0_ff45_disable_2_dmg08_outE0_cgb04c_outE2.gbc
gambatte/lycEnable/lyc0_late_ff45_enable_2_dmg08_outE2_cgb04c_outE0.gbc
gambatte/lycEnable/lyc153_late_ff41_enable_2_dmg08_outE2_cgb04c_outE0.gbc
gambatte/lycEnable/lyc153_m1disable_ds_2_cgb04c_outE0.gbc
gambatte/lycEnable/lyc_ff45_disable2_2_dmg08_out1_cgb04c_out3.gbc
gambatte/m0enable/late_enable_2_dmg08_out2_cgb04c_out0.gbc
gambatte/m0enable/lycdisable_ff41_ds_1_cgb04c_out2.gbc
gambatte/m1/lyc143_late_m0enable_lycdisable_2_dmg08_cgb04c_out1.gbc
gambatte/m1/lycint143_m1irq_late_retrigger_ds_1_cgb04c_out3.gbc
gambatte/m1/lycint_vblankirq_late_retrigger_ds_1_cgb04c_out1.gbc
gambatte/m2enable/late_enable_ly0_ds_lcdoffset1_1_cgb04c_out2.gbc
gambatte/m2enable/late_enable_ly0_lcdoffset2_1_cgb04c_out2.gbc
gambatte/m2enable/lyc0_late_m2enable_lycdisable_2_dmg08_out2_cgb04c_out0.gbc
gambatte/m2enable/lyc1_m2irq_late_lyc255_2_dmg08_out2_cgb04c_out0.gbc
gambatte/m2int_m0irq/m2int_m0irq_ds_2_cgb04c_out3.gbc
gambatte/m2int_m0irq/m2int_m0irq_scx3_ifw_ds_2_cgb04c_out0.gbc
gambatte/m2int_m0irq/m2int_m0irq_scx4_ifw_ds_2_cgb04c_out0.gbc
gambatte/m2int_m0stat/m2int_m0stat_ds_2_cgb04c_out2.gbc
gambatte/m2int_m2irq/m2int_m2irq_late_retrigger_1_dmg08_cgb04c_out2.gbc
gambatte/m2int_m2stat/m2int_m2stat_ds_2_cgb04c_out3.gbc
gambatte/m2int_m2stat/m2int_scx4_m2stat_ds_2_cgb04c_out3.gbc
gambatte/m2int_m3stat/m2int_m3stat_ds_2_cgb04c_out0.gbc
gambatte/m2int_m3stat/scx/late_scx4_2_dmg08_cgb04c_out0.gbc
gambatte/m2int_m3stat/scx/late_scx4_ds_2_cgb04c_out0.gbc
gambatte/m2int_m3stat/scx/m2int_scx2_m3stat_ds_2_cgb04c_out0.gbc
gambatte/m2int_m3stat/scx/m2int_scx4_m3stat_ds_2_cgb04c_out0.gbc
gambatte/m2int_m3stat/scx/m2int_scx6_m3stat_ds_2_cgb04c_out0.gbc
gambatte/m2int_m3stat/scx/m2int_scx8_m3stat_ds_2_cgb04c_out0.gbc
gambatte/miscmstatirq/lycstatwirq_trigger_ly00_10_50_ds_1_cgb04c_outE0.gbc
gambatte/miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_4_dmg08_cgb04c_outE0.gbc
gambatte/miscmstatirq/lycstatwirq_trigger_m0_late_ly44_lyc44_08_40_ds_2_cgb04c_outE0.gbc
gambatte/miscmstatirq/lycwirq_trigger_m0_late_ly44_4_dmg08_cgb04c_outE0.gbc
gambatte/miscmstatirq/lycwirq_trigger_m0_late_ly44_ds_2_cgb04c_outE0.gbc
gambatte/oam_access/postread_ds_2_cgb04c_out0.gbc
gambatte/oam_access/postread_scx5_ds_2_cgb04c_out0.gbc
gambatte/oam_access/postwrite_ds_2_cgb04c_out1.gbc
gambatte/oam_access/postwrite_scx1_ds_2_cgb04c_out1.gbc
gambatte/scx_during_m3/scx_m3_extend_ds_1_cgb04c_out3.gbc
gambatte/serial/start_wait_trigger_int8_read_if_1_dmg08_cgb04c_outE8.gbc
gambatte/serial/start_wait_trigger_int8_read_if_ds_1_cgb04c_outE8.gbc
gambatte/speedchange/m2int_m3stat_lcdoffds_2_cgb04c_out0.gbc
gambatte/speedchange/speedchange2_frame1_m2int_m3stat_scx2_2_cgb04c_out0.gbc
gambatte/speedchange/speedchange2_lcdoff_m2int_m3stat_scx2_2_cgb04c_out0.gbc
gambatte/speedchange/speedchange2_lcdoff_nopx2_m2int_m3stat_scx2_2_cgb04c_out0.gbc
gambatte/speedchange/speedchange2_m2int_m3stat_scx2_2_cgb04c_out0.gbc
gambatte/speedchange/speedchange2_nop_lcdoff_m2int_m3stat_scx2_2_cgb04c_out0.gbc
gambatte/speedchange/speedchange2_nop_lcdoff_nopx2_m2int_m3stat_scx2_2_cgb04c_out0.gbc
gambatte/speedchange/speedchange2_nop_m2int_m3stat_scx4_2_cgb04c_out0.gbc
gambatte/tima/tc00_irq_late_retrigger_1_dmg08_cgb04c_outE4.gbc
gambatte/tima/tc00_irq_late_retrigger_ds_1_cgb04c_outE4.gbc
gambatte/vram_m3/postread_ds_2_cgb04c_out0.gbc
gambatte/vram_m3/postread_scx5_ds_2_cgb04c_out0.gbc
gambatte/vram_m3/preread_ds_2_cgb04c_out3.gbc
gambatte/vram_m3/prewrite_ds_1_cgb04c_out1.gbc
gambatte/vram_m3/prewrite_ds_2_cgb04c_out0.gbc
gambatte/window/arg/late_wy_10to0_ly1_1_dmg08_cgb04c_out3.gbc
gambatte/window/arg/late_wy_10to1_ly1_1_dmg08_cgb04c_out3.gbc
gambatte/window/arg/late_wy_1toFF_1_dmg08_cgb04c_out0.gbc
gambatte/window/arg/late_wy_1toFF_ds_2_cgb04c_out3.gbc
gambatte/window/arg/late_wy_2toFF_1_dmg08_cgb04c_out0.gbc
gambatte/window/arg/late_wy_FFto0_ly0_1_dmg08_cgb04c_out3.gbc
gambatte/window/arg/late_wy_FFto0_ly2_1_dmg08_cgb04c_out3.gbc
gambatte/window/arg/late_wy_FFto1_ly2_1_dmg08_cgb04c_out3.gbc
gambatte/window/arg/late_wy_FFto2_ly2_1_dmg08_cgb04c_out3.gbc
gambatte/window/arg/late_wy_FFto2_ly2_scx2_1_dmg08_cgb04c_out3.gbc
gambatte/window/arg/late_wy_FFto2_ly2_scx3_1_dmg08_cgb04c_out3.gbc
gambatte/window/arg/late_wy_FFto2_ly2_wx0f_1_dmg08_cgb04c_out3.gbc
gambatte/window/late_disable_early_scx00_wx0f_ds_1_cgb04c_out0.gbc
gambatte/window/late_disable_early_scx00_wx10_ds_1_cgb04c_out0.gbc
gambatte/window/late_disable_early_scx00_wx11_ds_1_cgb04c_out0.gbc
gambatte/window/late_disable_early_scx00_wx12_ds_1_cgb04c_out0.gbc
gambatte/window/late_disable_early_scx03_wx0f_1_dmg08_cgb04c_out0.gbc
gambatte/window/late_disable_early_scx03_wx10_1_dmg08_cgb04c_out0.gbc
gambatte/window/late_disable_early_scx03_wx11_1_dmg08_cgb04c_out0.gbc
gambatte/window/late_disable_early_scx03_wx12_1_dmg08_cgb04c_out0.gbc
gambatte/window/late_disable_spx10_wx0f_2_dmg08_cgb04c_out3.gbc
gambatte/window/late_enable_ly0_ds_2_cgb04c_out0.gbc
gambatte/window/late_reenable_2_dmg08_cgb04c_out0.gbc
gambatte/window/late_reenable_scx2_2_dmg08_out3_cgb04c_out0.gbc
gambatte/window/late_reenable_wx0f_2_dmg08_cgb04c_out0.gbc
gambatte/window/late_scx_late_disable_0_dmg08_cgb04c_out0.gbc
gambatte/window/late_wx_scx5_1_dmg08_cgb04c_out0.gbc
gambatte/window/late_wy_ds_2_cgb04c_out3.gbc
gambatte/window/late_wy_ds_lcdoffset1_2_cgb04c_out3.gbc
gambatte/window/m2int_wx03_m3stat_ds_2_cgb04c_out0.gbc
gambatte/window/m2int_wx07_m3stat_ds_2_cgb04c_out0.gbc
gambatte/window/m2int_wx0C_m3stat_ds_2_cgb04c_out0.gbc
gambatte/window/m2int_wx57_m3stat_ds_2_cgb04c_out0.gbc
gambatte/window/m2int_wxA5_m0irq_2_dmg08_cgb04c_out2.gbc
gambatte/window/m2int_wxA6_firstline_m3stat_3_dmg08_cgb04c_out0.gbc
gambatte/window/m2int_wxA6_m3stat_ds_2_cgb04c_out0.gbc
gambatte/window/m2int_wxA6_scx5_m3stat_ds_2_cgb04c_out0.gbc
gambatte/window/m2int_wxA6_vrambusyread_3_dmg08_cgb04c_out5.gbc
gambatte/window/m2int_wxDefault_m3stat_ds_2_cgb04c_out0.gbc```
### FLOOR/DIFF (51):
```
gambatte/cgbpal_m3/cgbpal_m3end_scx3_2_cgb04c_out0.gbc	sb=7	want=0
gambatte/display_startstate/stat_2_cgb04c_out84.gbc	sb=80	want=84
gambatte/display_startstate/stat_scx2_2_cgb04c_out84.gbc	sb=80	want=84
gambatte/display_startstate/stat_scx5_2_cgb04c_out84.gbc	sb=80	want=84
gambatte/dma/hdma_late_disable_scx5_ds_2_cgb04c_out1.gbc	sb=0	want=1
gambatte/lcd_offset/offset1_lyc99int_m2irq_count_ds_2_cgb04c_out91.gbc	sb=01	want=91
gambatte/lcd_offset/offset3_lyc99int_m2irq_count_2_cgb04c_out91.gbc	sb=01	want=91
gambatte/ly0/lycint152_lyc0irq_late_retrigger_ds_1_cgb04c_outE2.gbc	sb=E0	want=E2
gambatte/lyc153int_m2irq/lyc153int_m2irq_ifw_1_dmg08_cgb04c_out2.gbc	sb=0	want=2
gambatte/lyc153int_m2irq/lyc153int_m2irq_ifw_ds_1_cgb04c_out2.gbc	sb=0	want=2
gambatte/lyc153int_m2irq/lyc153int_m2irq_late_retrigger_ds_1_cgb04c_out2.gbc	sb=0	want=2
gambatte/lycEnable/late_ff41_enable_ds_lcdoffset1_2_cgb04c_out0.gbc	sb=2	want=0
gambatte/lycEnable/late_ff45_enable_ds_lcdoffset1_2_cgb04c_out0.gbc	sb=2	want=0
gambatte/lycEnable/lyc0_m1disable_2_dmg08_outE2_cgb04c_outE0.gbc	sb=E2	want=E0
gambatte/lycEnable/lyc153_late_enable_m1disable_2_dmg08_outE2_cgb04c_outE0.gbc	sb=E2	want=E0
gambatte/lycEnable/lyc153_late_ff41_enable_ds_lcdoffset1_2_cgb04c_outE0.gbc	sb=E2	want=E0
gambatte/lycEnable/lyc153_late_ff45_enable_ds_lcdoffset1_2_cgb04c_outE0.gbc	sb=E2	want=E0
gambatte/lycEnable/lyc153_late_m1disable_2_dmg08_outE2_cgb04c_outE0.gbc	sb=E2	want=E0
gambatte/m0enable/disable_2_dmg08_out0_cgb04c_out2.gbc	sb=0	want=2
gambatte/m0enable/disable_scx4_2_dmg08_out0_cgb04c_out2.gbc	sb=0	want=2
gambatte/m0enable/enable_wxA6_2x_spxA7_1_dmg08_cgb04c_out2.gbc	sb=0	want=2
gambatte/m0enable/lycdisable_ff45_2_dmg08_out2_cgb04c_out0.gbc	sb=2	want=0
gambatte/m0enable/lycdisable_ff45_scx1_2_dmg08_out2_cgb04c_out0.gbc	sb=2	want=0
gambatte/m0enable/lycdisable_ff45_scx2_2_dmg08_out2_cgb04c_out0.gbc	sb=2	want=0
gambatte/m0enable/lycdisable_ff45_scx3_2_dmg08_out2_cgb04c_out0.gbc	sb=2	want=0
gambatte/m1/ly143_late_m0enable_2_dmg08_out3_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/ly143_late_m0enable_ds_lcdoffset1_2_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/m1irq_late_enable_2_dmg08_out2_cgb04c_out0.gbc	sb=2	want=0
gambatte/m1/m1irq_m0disable_2_dmg08_out3_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/m1irq_m2disable_lycdisable_2_dmg08_out3_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/m1irq_m2disable_lycdisable_3_dmg08_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/m1irq_m2disable_lycdisable_ds_2_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/m1irq_m2enable_lyc_1_dmg08_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/m1irq_m2enable_lyc_ds_1_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/m2m1irq_ifw_2_dmg08_cgb04c_out1.gbc	sb=3	want=1
gambatte/m1/m2m1irq_ifw_ds_2_cgb04c_out1.gbc	sb=3	want=1
gambatte/m2enable/late_enable_m1disable_ly0_2_dmg08_out2_cgb04c_out0.gbc	sb=2	want=0
gambatte/m2enable/late_m1disable_ly0_2_dmg08_out2_cgb04c_out0.gbc	sb=2	want=0
gambatte/miscmstatirq/lycstatwirq_trigger_ly00_10_50_1_dmg08_cgb04c_outE0.gbc	sb=E2	want=E0
gambatte/serial/start_wait_trigger_int8_read_if_2_dmg08_outE8_cgb04c_outE0.gbc	sb=E8	want=E0
gambatte/serial/start_wait_trigger_int8_read_if_ds_2_cgb04c_outE0.gbc	sb=E8	want=E0
gambatte/tima/tc00_irq_late_retrigger_2_dmg08_outE4_cgb04c_outE0.gbc	sb=E4	want=E0
gambatte/tima/tc00_irq_late_retrigger_ds_2_cgb04c_outE0.gbc	sb=E4	want=E0
gambatte/window/arg/late_wy_1_dmg08_cgb04c_out0.gbc	sb=3	want=0
gambatte/window/arg/late_wy_1toFF_ds_lcdoffset1_2_cgb04c_out3.gbc	sb=0	want=3
gambatte/window/late_disable_late_scx03_wx0f_2_dmg08_out3_cgb04c_out0.gbc	sb=3	want=0
gambatte/window/late_disable_scx2_1_dmg08_out3_cgb04c_out0.gbc	sb=3	want=0
gambatte/window/late_disable_scx3_1_dmg08_out3_cgb04c_out0.gbc	sb=3	want=0
gambatte/window/late_disable_scx5_1_dmg08_out3_cgb04c_out0.gbc	sb=3	want=0
gambatte/window/late_reenable_scx3_2_dmg08_out3_cgb04c_out0.gbc	sb=3	want=0
gambatte/window/late_wy_1_dmg08_cgb04c_out0.gbc	sb=3	want=0```
