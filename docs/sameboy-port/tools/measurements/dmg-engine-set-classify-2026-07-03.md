# DMG engine set (§3b) — measured atomic classification (2026-07-03, #11bj)

The §3b DMG engine rows — **gbmicrotest 68 + wilbertpol 10 + age 1 = 79** —
measured flag-on and dual-traced against SameBoy `--dmg`. **VERDICT: every
family is the counter-pinned dispatch / boot-frame / sub-M-cycle-read-clock
atomic core — no clean flag-gated slice exists.** They land with the flip's
global dispatch reclock (the same conclusion the CGB inventory reached,
`c2-flip-blocker-classification-2026-06-30.md` / `engine-if-class-drained-
2026-06-30.md`). Honorable measured park per the goal's park rule.

Measured with new flag-on probes (`#[ignore]`'d, byte-identical OFF):
`gbmicro_flagon_probe` (FF82 verdict) + `wilbertpol_flagon_probe` (fib regs);
trace harness `scratchpad/gbm_measure.sh` (slopgb `ff0f`/`m0rise`/`dispatch`
↔ SameBoy `SBIF`/`SBREAD ff0f`/`SBREAD ff41`). Flag-on: gbmicrotest 1/68 pass,
wilbertpol 0/10 pass.

## gbmicrotest 68 — three atomic sub-classes

### hblank_int / hblank (37) — the mode-0 IF-delivery READ-FRAME collapse

`hblank_int_scx*_if_b/c/d`, `_nops_a/b`, `hblank_scx3_*`, `hblank_int_scx7`.
The test arms a mode-0 (HBlank) STAT interrupt and reads FF0F at a family of
dots straddling the mode-0 rise. **The `_a/_b/_c/_d` variants read at
consecutive dots across slopgb's mode-0 rise (dot 254) with OPPOSITE wants —
the textbook read-frame A/B:**

```
hblank_int_scx0_if_a  read dot244  IF=00
hblank_int_scx0_if_b  read dot248  IF=00  want=FF   (SameBoy delivered; slopgb not yet)
hblank_int_scx0_if_c  read dot252  IF=00  want=E2
hblank_int_scx0_if_d  read dot256  IF=02  want=00   (SameBoy already SERVICED+cleared)
```

slopgb's mode-0 rise fires at **dot 254** (`m0rise ly=1 dot=254`); SameBoy
delivers ~8 dots earlier AND completes the ISR (IF cleared) by if_d. A read
peek (the #11bh `ff0f_stat_peek` DMG face) folding the imminent rise would
deliver if_b/c early but CANNOT also clear if_d (want 00) — the read-frame
collapse. **The mode-0 rise dot is COUNTER-PINNED** (moving it hangs mooneye
`intr_2_*` at B=42, the hard constraint) → the only fix is the global
dispatch reclock landing the rise + the sub-M-cycle read clock together.

### poweron_* (20) — the C0 boot-DIV read-frame CHAIN

`poweron_stat_*` (9), `poweron_oam_*` (5), `poweron_vram_*` (4),
`poweron_ly_*` (2). Each reads a register (STAT/OAM/VRAM-access/LY) at a
precise cycle after power-on. The reclock's C0 boot-DIV +4 frame shifts every
boot read, so `want[N] == got[N+1]` throughout the STAT chain:

```
poweron_stat_006  got=85 want=84     poweron_stat_120  got=84 want=80
poweron_stat_007  got=84 want=86     poweron_stat_121  got=80 want=82
poweron_stat_027  got=86 want=87     poweron_stat_141  got=82 want=83
poweron_stat_070  got=87 want=84     poweron_stat_184  got=83 want=80
                                     poweron_stat_235  got=80 want=82
```

Each slopgb read lands one boot-sequence step late (reads its successor's
value). A uniform boot-frame correction is a CURVE-FIT — the +4 DIV is PINNED
by `boot_div` (9) + `boot_sclk_align` (2), which pass flag-on, so these
different-cycle reads need a per-read-position frame = the sub-M-cycle read
clock (S7). Atomic.

### misc (11) — int_timer_halt (2), stat_write_glitch (2), win10_scx3 (1)

`int_timer_halt`/`_div_b` got=0E/02 want=0F/03 (TIMA off by 1 — the deferred
timer-completion frame). `stat_write_glitch_l1/l143_a` got=E2 want=E0 (the
mid-mode STAT-write glitch read-frame). All dispatch/read-frame.

## wilbertpol 10 — the line-153 / timer-IF dispatch class

`ly_lyc_153_write-C/-GS` (6 models) + `timer_if` (4 models): ALL fail flag-on
with the IDENTICAL failing register `B=48` (want the fib 03; D=01 GS / 02 C =
the round number only). The line-153 LYC-write timing and the timer-IF
delivery both shift by the same counter-pinned dispatch amount under the flip.
The #11bg DS line-153 lyfc table is CGB; the SS/DMG line-153 write behavior is
the same dispatch-frame shift, not a separable law. Atomic.

## age 1 — halt-m0-interrupt (dispatch/wake)

`halt-m0-interrupt-dmgC-cgbBCE` — the DMG mode-0 halt-wake IF delivery, the
wake-clock face of the hblank_int class. Atomic.

## Conclusion

The §3b engine set is 100% the dispatch / boot-frame / read-clock atomic core.
There is no flag-gated law slice to ship (unlike the window family, where the
`vis_mode_read` READ-side verdict decouples cleanly from the counter-pinned
dispatch). These 79 rows are FIXED by the flip's own global dispatch reclock —
which is the C3 flip event itself, not an incremental §3b lever. Parked with
this measured classification; the probes (`gbmicro_flagon_probe`,
`wilbertpol_flagon_probe`) are banked for the flip-time re-measurement.
