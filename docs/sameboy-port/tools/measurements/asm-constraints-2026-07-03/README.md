# gambatte hwtests asm constraint tables (2026-07-03, #11bg fan-out)

Seven per-family machine-constraint analyses derived from the gambatte
hwtests assembly sources (gambatte-core `test/hwtests`) by the #11bg
7-agent fan-out — the asm-first method input for the ENGINE-IF run. The
lycEnable/ly0-DS/m2enable/m2stat constraints are LANDED (#11bg); the
remaining tables (m2int_m0irq/irq_precedence FF0F, m1, lcd_offset,
speedchange, window wxA5-A6/spx10, gdma) are the ready-made inputs for the
next slices. Wants are exact machine constraints; absolute dots carry a
±1 M anchor ambiguity that cancels in leg-relative brackets — re-anchor
against SBDISP/SBMODE when landing.
