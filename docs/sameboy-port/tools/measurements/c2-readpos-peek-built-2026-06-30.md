# C2 #11ar — the per-ISR read-POSITION PEEK BUILT: first CLEAN read-position-decoupled slice (+6/−0) + the DEFINITIVE per-class read-offset table (ESCAPE on the global 132-convergence)

2026-06-30. Executed the goal's single sharpest lever — **the full per-ISR
deferred-read POSITION reclock, decoupled from the IF dispatch** — and drove it
to a decision. Result = **the first CLEAN read-position slice in the entire
C-stage (`+6/−0`, byte-identical OFF, mooneye flag-on 91/91), pinned**, plus the
**definitive per-class read-frame offset table** (all 5 blocker classes measured,
the new work #11aq's DS mode-2/mode-0 pair could not generalise). The global
132-convergence does NOT land (ESCAPE): the offset table proves the read-frame
error is **per-read-CLASS, not global** — it spans `{−20 … +18}` dots, is
**opposite-signed** between FF41 mode reads (+) and FF0F IF-delivery reads (−),
and varies WITHIN a class by read line-position. The "carry EVERY read to
SameBoy's cfl + ONE SBex exit" thesis is refuted at the code level; the
read-position peek is the correct mechanism but is cleanly applicable to exactly
one sub-family. Defaults NOT flipped; `pixel-pipe-reclock` core byte-identical;
the slice + tracers on `phase-b-s7`.

## The mechanism SHIPPED — the peek override (distinct from #11aq's machine carry)

#11aq carried the ISR read by adding **real** `pending` debt (`carry_read`) at
`dispatch_retime`, which advances the WHOLE machine — so it mis-positions every
non-m3stat STAT-ISR read (m0stat/m2stat/enable read native mode 2/0 at a
position the +4/+2 advance breaks). Build-measured: the machine carry + the
scoped SBex override is **`+29/−58`** — WORSE than #11aq's blanket M2HOLD
(+22/−50), the regressions dominated by m0stat/m2stat/m2irq/enable/oam/vram
(`M2CARRY` alone breaks all 5 spot-checked). The carry is over-broad: a constant
per-source offset applied to reads that need position-dependent offsets.

The fix (`stat_irq.rs::vis_mode_read`, armed by `interconnect.rs::dispatch_retime`
via `Ppu::read_carried`): a **transient PEEK** — shift only the FF41 read's mode
VERDICT, never the machine clock:

```
if read_carried && tier2 && cgb && ds && line∈[1,144) && m == 3
   && !win_active && !win_aborted && !wy_trig_sb && !glitch && n_sprites == 0 {
     let off  = if stat_rise_oam { 4 } else { 2 };        // per-source: OAM +4 / HBlank +2 dots
     let sbex = 257 + (scx&7) + ds + (scx&1);             // SameBoy bare exit (+ #11ap parity)
     return if dot + off < sbex { 3 } else { 0 };         // FULL 3↔0 override at the carried frame
}
```

Three guards are load-bearing (each removes a −N drop):
1. **`read_carried`** (one-shot, set at DS OAM/HBlank STAT dispatch, cleared after
   the FF41 read): scopes the override to the carried ISR read — the #11aq −50
   fix (the blanket M2HOLD fired for non-carried polled reads whose native frame
   was already right).
2. **`m == 3`**: fires ONLY for reads that natively see mode 3 (the m3stat family,
   reading near the mode-3→0 exit). Excludes m0stat/m2stat/enable (native mode
   2/0 — they probe a DIFFERENT boundary that one SBex cannot serve; without this
   guard they drop, `+13/−?`).
3. **`!wy_trig_sb`** (sticky per-frame `WY == LY` latch): excludes the
   `late_disable_*` family — its `_ds_1`/`_ds_2` pair reads the IDENTICAL slopgb
   dot (`ly1 dot254`, MEASURED) with OPPOSITE wants (a co-temporal RENDER-LENGTH
   A/B: SameBoy extends mode 3 on the window-then-disabled line; slopgb renders
   bare, indistinguishable at the read). Without this guard `+13/−7` (all 7 the
   `_ds_2` SameBoy-pass siblings of the 7 `_ds_1` fixes — a pure swap).

No machine advance ⇒ mooneye flag-on 91/91 (the counter-pinned dispatch dot + IF
delivery untouched); production (flag-off) byte-identical (`read_carried` is set
only on the tier2 dispatch path, false in production).

## The +6/−0 slice (full-CGB two-bin vs base flag-on `pass=2544`)

```
FIXED (6):
  m2int_m3stat/m2int_m3stat_ds_2                    (out0)
  m2int_m3stat/scx/m2int_scx2_m3stat_ds_2           (out0)
  m2int_m3stat/scx/m2int_scx4_m3stat_ds_2           (out0)
  m2int_m3stat/scx/m2int_scx6_m3stat_ds_2           (out0)
  m2int_m3stat/scx/m2int_scx8_m3stat_ds_2           (out0)
  speedchange/m2int_m3stat_lcdoffds_2               (out0)
REGRESSED: 0     (flag-on pass 2544 → 2550)
```

This is the COMPLETE m2int_m3stat DS blocker set (the m0int + all SS m3stat
variants already pass in base). Pinned `tier2_m2int_m3stat_ds_readpos_passes`.

## The DEFINITIVE per-class read-offset table (the ESCAPE deliverable)

`offset = slopgb read dot − SameBoy read cfl` (SameBoy 8 MHz `cfl*2+dc`, ÷2 to
dots), the LAST FF41/FF0F read of each blocker, both emulators, 132 blockers.
Artifacts (`|off|>30` = the measurement read landed on a different absolute line)
excluded.

| class | reg | dominant offsets (count) | mechanism |
|---|---|---|---|
| RENDER-LENGTH | ff41 | **+4** (24) · +0 (8) · +18 (7) · +8 (1) | +4 = read-position (m2int/late_disable); +0 = exit-position (late_wy window-extend); +18 = accessibility (vram_m3/cgbpal, reads near mode-3 START); +8 = enable_display glitch |
| ENGINE-IF | ff0f | **−8** (7) · −4 (4) · −16 (3) · −12 (3) · −6/−10/−11/−20 | IF-DELIVERY: slopgb reads FF0F LATER than SameBoy (the STAT-IRQ dispatch↔read straddle) |
| S6-DS | ff41 | +7 (4) · +5 (3) · +2 (2) · +4 (1) | per-config DS read-grid; no clean cluster |
| READ-FRAME | ff41/ff0f | +4 (2) · −2 · +4 | serial/tima S6-completion + SS deferred read (mostly line-artifact, small n) |
| WAKE-CLOCK | ff41 | **−4** (7) · +4 (2) · 0 | halt m0stat: slopgb reads mode-0 LATER than SameBoy (the sub-M-cycle wake clock, #11i) |

### What the table proves (the model, sharpened past #11aq)

1. **The offset is per-read-CLASS, NOT global.** Values: `{−20,−16,−12,−11,−10,−8,
   −6,−4,−2,0,+2,+4,+5,+7,+8,+18}`. No single carry — or single machine advance,
   or single SBex — serves them. #11aq's "carry EVERY read to SameBoy's cfl + one
   SBex" is refuted at the code level: a uniform carry breaks the classes at other
   offsets.
2. **The offset is OPPOSITE-SIGNED by register.** FF41 mode reads are mostly
   POSITIVE (slopgb reads EARLY, the leading-edge cc+0 frame) — the read-position
   family. FF0F IF-delivery reads are NEGATIVE (slopgb reads LATE — the IF bit
   lands a cycle after SameBoy's). These are two different subsystems (mode
   boundary vs interrupt delivery); no lever spans both.
3. **The offset varies WITHIN RENDER-LENGTH by line-position** (`+0` at the mode-3
   exit for a polled window read · `+4` at the exit for an ISR read · `+18` near
   mode-3 START for an accessibility read). This is why "RENDER-LENGTH" is not one
   lever: the deferred-clock↔PPU-dot phase error is a function of the read's
   absolute line position, so a constant per-source carry is insufficient.
4. **Only the m2int_m3stat DS +4 subset is cleanly fixable.** It is the ONLY
   sub-family where (a) the offset is uniform (+4), (b) the `_1`/`_2` reads are
   genuinely 2 dots apart (dot252/254 — NOT co-temporal), and (c) the exit is the
   bare exit (not window/sprite/accessibility-extended). Every other sub-family
   fails ≥1: late_disable is co-temporal (b); late_wy needs the extended exit (c);
   accessibility reads a different register at a different position (a); ENGINE-IF
   is IF-delivery not mode; WAKE-CLOCK is mode-0 at the wrong clock.

## Why the 132 do NOT converge (the residual, by class)

- **RENDER-LENGTH 56 → 50:** −6 (the peek). The rest: late_disable (co-temporal
  render-length A/B, needs the mode-3 EXTEND port), late_wy (window shadow, +0
  offset, render-length), vram_m3/cgbpal (+18 accessibility register model),
  enable_display (glitch). Each a distinct render-length or accessibility lever.
- **ENGINE-IF 30:** FF0F IF-delivery, offset −4…−20. The STAT-IRQ dispatch↔read
  straddle; needs the IF-lifecycle reclock (NOT a mode-read peek).
- **S6-DS 21:** per-config DS read-grid / cycle-write conflict (PORT-PLAN S6).
- **READ-FRAME 13:** serial/tima S6-completion + SS deferred read position.
- **WAKE-CLOCK 12:** the −4 halt m0stat sub-M-cycle wake clock (#11i) — a clean
  −4 cluster, but reads mode 0 (the peek's `m==3` guard excludes it); the next
  candidate slice is a mode-0 wake analogue of this peek.

Even a perfect render-length port + this peek leaves the ENGINE-IF + WAKE +
S6-DS + READ-FRAME (≈76) needing the IF-delivery / wake-clock / S6-completion
reclocks — the atomic C-stage, re-confirmed. The peek is a genuine dent (the
first read-position win), not the whole lever.

## The single sharpest remaining lever (refined)

**The WAKE-CLOCK mode-0 peek** — the same transient-verdict mechanism as this
slice, applied to the halt m0stat family (the uniform `−4` cluster, 7 rows):
peek the FF41 mode-0 verdict at the sub-M-cycle wake clock instead of slopgb's
M-cycle-quantized wake. It is the next-most-uniform offset cluster and the only
other one with a single dominant value. The FF0F ENGINE-IF class (30, IF-delivery)
is the largest but needs the interrupt-lifecycle reclock (not a mode peek), and
is entangled with the counter-pinned dispatch — the true atomic core.

## Gate (END CLEAN — production unchanged)

mooneye flag-on 91/91 (no lever env — the peek is tier2-unconditional) + OFF
91/91; gbtr OFF full 212/0 byte-identical (the peek is inert flag-off);
`tier2_m2int_m3stat_ds_readpos_passes` green; clippy clean; `pixel-pipe-reclock`
core byte-identical. Tooling: `/tmp/s7/measure.sh` (per-row both-emulator offset),
`offsets.tsv` (the 132-row table). Data: `c2-readpos-offset-table-2026-06-30.tsv`.
