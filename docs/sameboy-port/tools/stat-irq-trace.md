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

The slopgb side: the matching tracer is now **committed** (not session-local),
gated on `SLOPGB_S5DBG` (byte-identical when unset; `ppu::s5dbg_on()` caches the
env once). Two sites: `ppu/stat_irq.rs::stat_update_tick` (after
`pending_if |= IF_STAT`) logs `SLOPGB dispatch ly/dot/mfi`, and
`interconnect/cycle.rs::read_deferred` logs `SLOPGB ff41 ly/dot/mode` for the
deferred FF41 read. Drive it flag-on via the example runner:
`SLOPGB_TIER2=1 SLOPGB_S5DBG=1 cargo run -p slopgb-core --example run_gambatte
--release -- <rom> dmg 2>trace.log` (OCRs to stdout, traces to stderr).

**2026-06-24:** the SameBoy tester + this tracer were both rebuilt from a cold
`/tmp` and verified to reproduce every dot in the tables below exactly (kernel
m2int read ly1 cfl256/dot252 mode3, m0int cfl261/dot256 mode0; dispatch
m2int@0, m0int slopgb dot254 / SameBoy cfl257). See the recipe's 2026-06-24 box
(`../atomic-reclock-recipe.md`) for the refutation of the literal "read +4".

**SBMODE trace (the mode-3 EXIT ground truth, the window-length lever):** add to
SameBoy `GB_STAT_update` (after the DMA-mode-2 clear) a `SB_TRACE`-gated
`fprintf(stderr, "SBMODE ly=%d cfl=%d dc=%d vis=%d\n", current_line,
cycles_for_line, display_cycles, io_registers[GB_IO_STAT] & 3)` guarded by a
static `prevmode`/`prevline` so it logs only on a visible-mode change (`ly<144`).
This pins the per-config mode-3→0 EXIT dot.

> **2026-06-24 #11g — the "cfl257 = bare / low-WX over-extends" reading was a
> MEASUREMENT ARTIFACT and is REFUTED.** gambatte window ROMs loop ~15 frames;
> the SETUP frames render the line bare (exit cfl257), and ONE late frame writes
> WY/LCDC so the window triggers on the target line. The exit to compare is the
> **measurement-frame** exit, isolated as the per-`ly` `vis=0` cfl that occurs
> **once** (count 1; the 115×-repeated cfl is setup-bare). Fresh measurement:
> **SameBoy extends ALL window measurement lines to ≈263+SCX&7** (wx00/wxA5/
> late_wy/late_disable alike) — NO low-WX no-extend; slopgb UNDER-extends most.
> Full 53-row table + 4-mechanism map: `measurements/window-groundtruth-2026-06-24.md`.

The slopgb counterpart is now **committed** (`SLOPGB_S5DBG`, byte-identical OFF):
`render/mode0.rs` logs `SLOPGB visflip ly/dot/kind` at the `vis_early` rise (the
CPU-visible mode-3→0 flip — window mode-2-only lines have no mode-0 dispatch, so
the dispatch tracer can't see their flip), and `render/window.rs` logs
`SLOPGB winmatch ly/dot/wx/wy_ok/en/active` at the WX comparator.

**SBLEVEL trace (the rising-edge LEVEL engine ground truth — the S5
engine-dispatch / spurious-IRQ lever):** the IF|=2 tracer logs only the rising
EDGE; to characterize WHY SameBoy fires (or doesn't) — the `stat_interrupt_line`
level transitions vs slopgb's `stat_update` engine — add to `GB_STAT_update`,
just before the `if (stat_interrupt_line && !previous_interrupt_line)` block, a
`SB_TRACE`-gated `fprintf(stderr, "SBLEVEL ly=%d cfl=%d %d->%d mfi=%d
lyc_line=%d stat=%02x\n", current_line, cycles_for_line, previous_interrupt_line,
stat_interrupt_line, (int8_t)mode_for_interrupt, lyc_interrupt_line,
io_registers[GB_IO_STAT])` guarded by `stat_interrupt_line != previous_interrupt_line`
(`current_line < 154`). #11g use: the m1/lycEnable `want=E0 got=E2` spurious-IRQ
rows (e.g. `lycwirq_trigger_ly00_stat50`) — SameBoy dispatches NO STAT IRQ at ly0
(the LYC source level stays HIGH across ly153→ly0); slopgb's engine re-arms and
fires a spurious `ly0 dot1 mfi0` edge. Buried in steady-state volume → isolate
the measurement frame as for SBMODE. 12/13 m1+lyc DMG rows fail LE-only (the
engine core, not render-frame).

Detail: `measurements/flip-regr-2026-06-24-summary.txt`,
`measurements/window-groundtruth-2026-06-24.md`.

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

## FF41-READ ground truth (decisive, 2026-06-23 #11e)

Add the same `SB_TRACE` gate at `read_high_memory`'s `case GB_IO_STAT` (the FFxx
read, `Core/memory.c:629`): `fprintf(stderr, "SBREAD ff41 ly=%d cfl=%d dc=%d
mode=%d\n", gb->current_line, gb->cycles_for_line, gb->display_cycles,
gb->io_registers[GB_IO_STAT] & 3);`. slopgb side: the tier2 read goes through
`Interconnect::read_deferred` (`cycle.rs`, NOT `leading_edge_sample` — that's the
LE-only path) — trace its `read_no_tick(0xFF41)` + `ppu.line_dot()`. Filter to
visible lines (`ly < 144`) to skip the vblank polling reads; the m3stat
measurement read is on ly 1.

| ROM (want) | SameBoy READ | slopgb READ | slopgb boundary |
|---|---|---|---|
| kernel m2int scx0 (3) | ly1 **cfl=256** mode 3 | ly1 dot **252** mode 3 ✓ | ~256 |
| m0int scx0 (0) | ly1 **cfl=261** mode 0 | ly1 dot **256** mode 0 ✓ | ~256 |
| m2int **scx3** (0) | ly1 **cfl=260** mode 0 | ly1 dot **256** mode **3** ✗ | ~259 (256+SCX&7) |

**ROOT CAUSE (hard-measured):** slopgb's deferred FF41 read lands **~4–5 dots
EARLIER** than SameBoy's (kernel Δ+4, m0int Δ+5, scx3 Δ+4), and slopgb's read
dot does NOT shift with SCX (scx0 and scx3 both read at dot 256; SameBoy reads
261 vs 260). slopgb's mode-3 boundary DOES extend by SCX&7 (scx3 ≈ 259), so its
early read at 256 falls just BEFORE the boundary → mode 3, while SameBoy's later
read at 260 falls AFTER → mode 0. The read frame and the boundary are each
self-consistent within slopgb's (cc+4-derived) frame and within SameBoy's
(cc+0) frame, offset ~4 dots — **shifting either one alone breaks the scx0
kernel pin** (slopgb reads kernel@252 vs boundary@256; +4 read → 256 ≈ boundary
→ flips to mode 0, kernel fails). This is the **atomic read-frame↔boundary
reclock** — confirmed here with exact dot numbers, the documented multi-session
core (recalibrating it touches the whole cc-phase cluster + the ~7000-row
rebaseline). The window `early_lead` slice was the one corner where a
tier2-gated `vis_early`-only nudge sufficed without crossing the kernel frame;
the SCX-extended bare-line m3stat reads do NOT have that slack.

## halt `*_m0stat_*` — ATTEMPTED + proven the sub-M-cycle wall (2026-06-23 #11e)

The post-mode-0-halt-wake FF41-mode read (`want=0 got=2`, 20 DMG rows). Measured
(tracer + slopgb `read_deferred` FF41 trace): SameBoy reads at **ly2 cfl0
mode0**, slopgb at **line2 dot4 mode2** — a uniform +4-dot DMG over-advance of
the deferred wake (CGB samples cc+0, no over-advance — gating CGB out2 rows
broke; matches the int_hblank DMG-only law). Built the full C1.3-analogue
(`halt_vis_back` carry set on the mode-0 wake, DMG-only, first post-wake FF41
read in `[4,8)` → mode 0): **+11 DMG fixed, but −3** (`scx5_2`×2 + `scx3_2b`,
all `want=2`, SameBoy-passing) → net A/B, NOT shippable.

Tried to gate by the rise's `cc` (the C1.3 discriminator): **REFUTED — cc does
not separate them.** Measured `cc` at the wake: scx2→cc4, scx3_2→cc1, scx4→cc2
(all `want=0`, over-advance), scx5→cc3 (`want=2`). BUT `scx3_2b` (`want=2`) is
ALSO **cc=1** — the same `cc` as `scx3_2` (`want=0`). Two configs with OPPOSITE
expected modes collapse to identical slopgb `(line2, dot4, cc1)` state. `cc` is
the finest phase slopgb's M-cycle-quantized deferred clock has; the
distinguisher is the sub-cc (T-within-M) wake phase it quantizes away. So the
halt m0stat family is **genuinely unresolvable at this resolution** (any gate
fixing scx3_2/cc1/want0 also breaks scx3_2b/cc1/want2) — the deep S7
deferred-halt-wake residual, the same wall C1.3 only partly climbed for the LY
read. Needs the sub-M-cycle wake clock (record the IRQ rise at its T-phase, not
the M-cycle boundary). REVERTED (can't drop SameBoy-passing rows).
