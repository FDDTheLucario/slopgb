# SameBoy STAT-IRQ dispatch tracer (S5 ground-truth)

A ~10-line instrumentation of SameBoy 1.0.2 `Core/display.c` that logs the dot
(within-line position) at which `GB_STAT_update` raises the STAT interrupt
(`IF |= 2`, the rising edge at `display.c:558`). The principled ground truth for
the S5 engine port: where does SameBoy actually dispatch each mode's STAT IRQ?

## Build

In `GB_STAT_update` (`Core/display.c`), at the rising-edge `IF |= 2` site
(the `if (gb->stat_interrupt_line && !previous_interrupt_line)` block), add:

```c
gb->io_registers[GB_IO_IF] |= 2;
{
    static int trc = -1;
    if (trc < 0) trc = getenv("SB_TRACE") ? 1 : 0;
    if (trc) {
        fprintf(stderr, "SBTRACE STAT_IRQ ly=%d cfl=%d dc=%d mfi=%d stat=%02x lyc_line=%d\n",
                gb->current_line, gb->cycles_for_line, gb->display_cycles,
                (int8_t)gb->mode_for_interrupt, gb->io_registers[GB_IO_STAT],
                gb->lyc_interrupt_line);
    }
}
```

`make tester` (in `/tmp/sbbuild/SameBoy-1.0.2`). `cycles_for_line` is the
within-line position in 4 MHz T-dots (0..456, `LINE_LENGTH`), directly
comparable to slopgb's `Ppu::dot`; `display_cycles` is the sub-dot 8 MHz
fraction. Run with **`--length 2`** (the test must reach its measurement
window — `--length 1` fires nothing for some ROMs):

```sh
SB_TRACE=1 sameboy_tester --dmg --length 2 ROM.gbc 2>&1 >/dev/null \
  | grep "SBTRACE STAT_IRQ" | awk '{print $3,$4,$6}' | sort | uniq -c | sort -rn
```

The slopgb side: re-add the matching one-shot edge trace in
`ppu/stat_irq.rs::stat_update_tick` (after `pending_if |= IF_STAT`) logging
`line`/`dot`/`mfi`, gated on an env (see THESIS RESULT #11e), boot flag-on.

## Findings (2026-06-23 #11e, DMG)

| ROM | SameBoy dispatch | source | slopgb |
|---|---|---|---|
| `m2int_m3stat_1` (kernel) | **cfl=0, mfi=2** | mode-2 OAM line-start | dot 0 ✓ |
| `m0int_m3stat_2` (kernel) | **cfl=257, mfi=0** | mode-0 HBlank | tier2 retime ✓ (m0int passes) |
| `enable_display/frame0_m0irq_count` | cfl=257, mfi=0 | mode-0 | dispatch ✓ but count=0 (delivery) |
| `window/late_disable_1` (want=3) | cfl=0, mfi=2 | mode-2 | dispatch ✓ |
| `window/m2int_wxA6_scx3_m3stat_2` | cfl=0, mfi=2 | mode-2 | dispatch ✓ |
| `halt/m0int_m0stat_scx2_1` | cfl=257, mfi=0 | mode-0 | dispatch ✓ |
| `oam_access/10spritesprline_postread_1` | cfl=0, mfi=2 | mode-2 | dispatch ✓ |

**KEY:** SameBoy dispatches mode-2 STAT IRQs at **cfl=0** (line start) and
mode-0 at **cfl=257**, and slopgb's flag-on dispatch dots ALREADY MATCH these
(mode-2 at dot 0; mode-0 at 257 under the tier2 retime — the m0int kernel pin
passes). So the bulk of the flag-on gambatte regressions are **NOT** wrong IRQ
dispatch dots — they are the ISR's FF41-mode READ (the cc+0 leading-edge
`vis_mode` sample) landing at the wrong effective dot, or `vis_mode`'s mode-3
length being wrong under the deferred read-frame. This is the SAME class the
window `early_lead` fix (#11e) addressed, not a dispatch reclock. `frame_*_count`
is a separate DELIVERY/halt-mask issue (dispatch dot is correct).
