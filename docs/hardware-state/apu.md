# APU

## Post-boot warmup

- The APU is warmed ~1 emulated second post-boot so the boot beep's envelope is decayed at hand-off: PCM12/FF76 reads `$00`, and NR52 keeps the ch1 status bit.
- Parked: "simplifying" the warmup away — it is load-bearing for the post-boot PCM12/NR52 state. Do keep the ~1 s warmup.

### The beep is still running, at `$7C1`, mid-duty-cycle (2026-08-02)

The decayed beep is not silent state: channel 1 is enabled with its frequency
unit free-running, and a game that retriggers it inherits both the frequency and
the duty position. Two constants carry that, and `gambatte/sound/ch1_init_pos_1..8`
pins them.

**NR13 = `$C1`, not `$83`.** Every boot ROM plays *two* chime notes through one
helper, and the post-boot table must hold the second. Original Nintendo
`dmg_boot.bin`: `$0070` sets `c = $13`, then `ld e,$83` / `cp h,$62` / `ld
e,$C1` / `cp h,$64` before `ld a,e; ld (ff00+c),a; inc c; ld a,$87; ld
(ff00+c),a` — `h` counts up, so the `$C1` pass is the last one. SameBoy's DMG
and CGB replacements have the same shape (`ld a,$83; call` then `ld a,$C1; call`
into `ldh ($13),a; ld a,$87; ldh ($14),a`). Hand-off therefore leaves `freq =
$7C1`: **252 T (63 M-cycles) per duty step**, 2016 T per full cycle. NR13 is
write-only and NR14 reads back bit 6 only, so `boot_hwio` cannot see this — the
ladder is the only oracle.

**`PostBootState::beep_duty_advance`** (2 MHz cycles, applied to channel 1 alone
after the warmup; the DMG-cart value on CGB, `cgb_cart_cut` subtracted like
`div_counter`/`lcd_phase_dots`). DMG 860, CGB/AGB 1298.

The ladder's kernel runs from the entry point: `NR51 = 0`, `NR10 = 0`, `NR11 =
NR12 = $80` (duty 2, volume 8, envelope period 0), a `dec b` delay, then `NR50 =
$77`, `NR51 = $11`, and a loop that alternates `NR12` between `$80` and `$C0`
and retriggers `NR14 = $80` every ~134 M-cycles. That retrigger period is far
shorter than the 63-M-cycle duty step's reload, so the duty position **freezes**
at whatever the delay left it on, for the whole run. The volume then alternates
8/12 per trigger, which the harness's raw-sample comparator reads as sound — but
only if the frozen duty bit is 1. `_outaudio1` ⇔ frozen on a high position.

The eight rungs are four pairs one machine cycle wide, at delays 88/89, 119/120,
340/341 and 371/372 M-cycles, so each model gets two boundaries pinned to a
single machine cycle:

| model | sound rungs | rising | falling |
|---|---|---|---|
| DMG | 2-5 | 88→89 | 340→341 |
| CGB | 8-3 | 371→372 | 119→120 |

Both spans are 252 M-cycles — four duty steps, which is what fixes the step at
63 M-cycles and so the frequency at `$7C1` independently of the phase.

Do **not** carry the advance by lengthening the warmup instead: that also moves
the frame sequencer's hand-off phase, which the `ch2_init_env_counter_timing`
rows pin separately (below). Measured — a warmup long enough to place the duty
phase scores +11/−3, winning three `ch2_init_env_counter_timing` rows on DMG and
losing the same three on CGB; advancing channel 1 alone scores **+8/−0**.

### The frame sequencer is mid-round at hand-off (2026-08-02)

`PostBootState::apu_div_step` — DMG-family **1**, CGB/AGB **0**. The DIV-APU
divider counts from the boot ROM's own NR52 power-on write, so the hand-off step
is not 0, and the warmup cannot supply it: its synthetic DIV runs 512 falling
edges, a whole number of eight-step rounds, leaving the divider exactly where the
hwio replay's NR52 write put it.

`gambatte/sound/ch2_init_env_counter_timing_1..4` pins it two-sided on each
model. The kernel triggers channel 2 with `NR22 = $09` (volume 0, *increasing*,
period 1), runs a nested `dec b`/`dec c` delay, then writes `NR22 = $08` —
period 0, which locks the envelope where it stands — and spins forever. Volume 0
gives a constant stream, volume 1 a varying one, so `_outaudio1` ⇔ the envelope's
step beat the locking write. The four rungs are one machine cycle apart, and the
period-1 envelope's step lands between two frame-sequencer events (`divider & 7
== 7` decrements the countdown, the next event applies the volume), so a
step-value error of one moves the whole race a full 8192 T:

| model | step | sound rungs | neighbouring step values |
|---|---|---|---|
| DMG | 1 | 2-4 | 0 → no rung sounds · 2 → rung 1 sounds too |
| CGB | 0 | 4 | 1 → rungs 1-3 sound, 4 does not · 7 → none sound |

Only the two values above satisfy their model, and each is bracketed on both
sides. Scores **+3/−0** (the DMG rungs; CGB was already right at 0).
### The `_reset_` variant is a zombie-write race, not a phase (2026-08-02)

`ch2_init_reset_env_counter_timing_1..16` (4 rows left: `5`/`11` on DMG,
`7`/`15` on CGB) is the same kernel with an `NR52 = $00; NR52 = $80` power
cycle in front, so the frame sequencer restarts there and `apu_div_step` above
cannot reach it. The sixteen rungs are **eight pairs**, each pair one machine
cycle apart, over two knobs: the delay before the power cycle (`ld b,c` at
`$0151`) and the delay before the locking `NR22 = $08` after the trigger.

| pair | pre-cycle | post-trigger | lock | DMG want | DMG ours | CGB want | CGB ours |
|---|---|---|---|---|---|---|---|
| 1,2 | `3E,01` | `EB,0F` | `$186` | 0,1 | 0,1 | 0,0 | 0,0 |
| 3,4 | `10,02` | `EB,0F` | `$186` | 0,0 | 0,0 | 0,1 | 0,1 |
| 5,6 | `3E,01` | `E9,11` | `$188` | 0,1 | **1,1** | 1,1 | 1,1 |
| 7,8 | `10,02` | `E9,11` | `$188` | 1,1 | 1,1 | 0,1 | **1,1** |
| 9,10 | `E0,3E` | `FC,02` | `$186` | 0,1 | 0,1 | 0,0 | 0,0 |
| 11,12 | `E0,3E` | `F0,12` | `$186` | 0,1 | **1,2** | 1,1 | 1,1 |
| 13,14 | `E0,3E` | `FC,02` | `$189` | 0,0 | 0,0 | 0,1 | 0,1 |
| 15,16 | `E0,3E` | `F0,12` | `$189` | 1,1 | 1,1 | 0,1 | **1,2** |

0 = `_outaudio0`; the "ours" columns are channel 2's volume at the lock write,
probed, where volume 0 is the silent stream.

Each model fails exactly two pairs, in two shapes: the low rung one too high
(DMG 5,6 · CGB 7,8) and *both* rungs one too high (DMG 11,12 · CGB 15,16).
Every failure is our volume one step further along than the reference's, so a
single "step later" lever would move all four — and the twelve correct pairs
are what it has to survive.

The mechanism is **not** the divider phase. Probed on DMG 9-12, which share a
pre-cycle delay and therefore a power-on (`prev_div = $ABFC`, DIV bit 12 clear,
no skip glitch) and differ only in the post-trigger delay: the envelope's step
lands between events 7 and 8 for both, while the two pairs' locks land at event
7 and event 15. A step time fixed by the power-on cannot give pair 9,10 a
straddle at event 7 and pair 11,12 one at event 15, so what the reference reads
at the later lock is produced by the locking `NR22` write itself — NRx2 zombie
mode — not by the frame-sequencer step. Start there, not on the divider.


## SameBoy countdown model

The APU follows SameBoy's countdown model (`src/apu/`):

- Pulse/noise step on a machine 2 MHz grid (`Apu::phase`, bit 1 = `lf_div`); triggers anchor to that grid.
- The duty bit / LFSR sample is LATCHED at expiries.
- NR52 power-on resets the divider chain (with the DIV-event skip glitch when the DIV-APU bit is high).
- Envelopes use the countdown + rising-edge-arming + lock scheme.
- Noise runs a free-running 14-bit counter that triggers do NOT reset.

### Test status (same-suite apu)

Same-suite apu is green except for these known-exempt rows — read the baseline comments before touching:

| Row(s) | Reason exempt |
|---|---|
| `channel_1_freq_change_timing-cgb0BC` | revision-dependent (the model-specific APU 2 MHz write phase) |
| ch4 `align` / `freq_change` (NR43 corruption tables) | upstream-documented non-deterministic |

## Ch1 sweep (`pulse.rs`)

Ch1 sweep is SameBoy's calculation-countdown machinery:

- The 128 Hz fire writes the frequency at once, but the shadow/addend refresh + overflow check complete only `reload_timer + shift` 1 MHz cycles later (this kills trail fires/triggers by several M-cycles).
- NR10 writes hit the live machinery: zombie step, cleared-shift pause, and the completed-addend negate-clear kill.
- Triggers hold shadow refreshes for `channel_1_restart_hold` 2 MHz cycles.

### old-negate bit: CGB-revision policy

The completed-addend negate-clear kill uses the **E form** of the old-negate bit (per the §CGB-revision-policy companion rule), NOT SameBoy's ≤C behavior:

| Revision | old-negate bit behavior |
|---|---|
| E form (slopgb) | negate-clear kill |
| SameBoy ≤C | forced-true |

### Test status / residual

- Same-suite `channel_1_sweep` + `restart` + `restart_2` (the README's "even SameBoy-E fails it" ROM) all pass.
- Residual: gambatte `ch1_init_reset_sweep_counter_timing` rows need the 128 Hz grid phase pinned <4 dots against the instruction stream per model — see the baseline comment.
- Parked: whole-M-cycle ordering tweaks — they break same-suite.

## Speed switch

- The DIV reset of a STOP that **leaves** double speed restarts the counter but
  emits **no** frame-sequencer edge (`Apu::div_write_switching`, called from
  `Interconnect::stop_impl`): the DIV-APU tap is moving from bit 13 back to bit
  12 in that instant and neither tap explains the corpus alone. The gambatte
  `speedchange*_ch2_nr52` `a` rungs stop on the two adjacent DIV values `1FFC`
  (one below the bit-13 boundary) and `2000` (exactly on it) and both must leave
  the length counter unclocked — bit 12 fires on the first, bit 13 on the second.
  Entering double speed, and a non-switching (deep) STOP, keep the ordinary
  `Apu::div_write` falling-edge test. Unit test:
  `leaving_double_speed_restarts_div_without_a_frame_event`.
- Residual in that family, measured and two-sided — do not retry a uniform
  shift. The `a`/`b` pair brackets the expiry to one M-cycle, so the demanded
  off-cycle is exactly the `b` read. Probing the ch2-off cycle against the NR52
  read cycle: every single-speed rung lands on it, and so do
  `speedchange3_ch2_nr52_{1,2}` in double speed, but the six double-speed rungs
  of `speedchange{,5}` and `speedchange2_ds` land one M-cycle early (their `a`
  read shows delta 0 where the correct rungs show +2). Shifting the
  double-speed length clock one M-cycle later fixes those six and breaks
  `speedchange3_ch2_nr52_{1,2}b`, which already sit on the boundary — +6/−2. The
  split is not the switch count either (1 and 5 are early, 3 is correct), so it
  is the sub-M-cycle phase the pause end lands on, which the whole-M-cycle pause
  quantises per configuration. All six are bucketed EXCEED; SameBoy misses them
  too.

### Entering double speed re-paces the APU one machine cycle late (2026-08-02)

`Apu::set_double_speed_lag`, raised by `Interconnect::stop_impl` on a switching
STOP that **enters** double speed: the frequency units keep dividing for single
speed across the first machine cycle of the pause, so that pause hands the 2 MHz
grid one extra cycle. The CPU and PPU switch on the cycle before, unchanged —
the pause length, the leave direction and the deep (non-switching) STOP are all
untouched, so this moves audio only. Scores **+6/−0**; unit test
`entering_double_speed_lags_the_frequency_units_one_machine_cycle`.

`gambatte/{sound,speedchange}/*ch1_duty0_pos6_to_pos7_timing*` pins it. The
kernel powers the APU off and on (which resets the duty position to 0), sets
duty 0 and `NR13 = $C0`/`$E0`, triggers `NR14 = $87` (frequency `$7C0`/`$7E0`),
runs a `dec b` delay across zero or more `ldh (4D),a; stop` speed switches, then
retriggers `NR14 = $80` in a loop. `$80` drops the frequency high bits, so the
reloaded period is far longer than the ~272-machine-cycle loop and the duty
position **freezes** at whatever the delay left it on. Duty 0 is high on
position 7 only, so `_outaudio1` ⇔ frozen on 7 and `_outaudio0` ⇔ frozen on 6.

The rungs come in `_1`/`_2` pairs one machine cycle apart, and the whole family
reduces to one invariant: **the duty step's expiry must fall in the machine
cycle immediately after the `_1` rung's retrigger write.** (In single speed that
is 1-2 2 MHz cycles after the write, in double speed exactly 1 — the same
statement either way.) Measured over all 44 ROMs before the fix, the expiry sat
one 2 MHz cycle late per *entering* switch and correct otherwise.

Residual: 12 rows, measured, do **not** re-sweep per-switch pause corrections.
Writing each ROM's error as a sum of per-switch corrections keyed on direction
plus the APU's 2 MHz phase (and the leave's `k`) is **refuted two-sided**:
`speedchange2_..._nop` and `speedchange_..._nop_ds` demand opposite-sign
corrections from leaves that are identical in both keys, while
`speedchange3_nop_ch1` and `speedchange2_nop_ch1_ds` demand 2 and 0 from the
same enter/leave pair. A full sweep of pause length (whole machine cycles and
odd dots, both directions, phase-keyed) tops out at 14 of the 22 pairs, and
every configuration past that regresses an `_1` rung. The lever left is finer
than one dot.

Refuted separately: re-reading the inactive pulse trigger's delay as `base + 4 +
lf_div` (or the flat `base + 5`) instead of `base + 6 - lf_div`. Both are forced
by the *zero-switch* rungs `ch1_duty0_pos6_to_pos7_timing_ds_{5,6}` and
`speedchange_ch1_nr4init_...`, which have no pause between trigger and
retrigger, and both score `+6/−6` and `+10/−5` on the corpus: they take
same-suite `channel_1/2_align{,_cpu}` with them. The `- lf_div` term stays.

## The granule grid (2026-08-05)

The APU does not advance a machine cycle at a time. It advances in whole 2 MHz
**granules** — 2 CPU cycles at single speed, 4 at double, 4 CPU cycles being one
machine cycle at either speed — off a grid that can sit one cycle away from the
CPU's own counter. `Apu::lag` holds that remainder (0 or 1), and whatever falls
inside the unfinished granule is not observable yet.

This is gambatte's model, `sound.cpp` `PSG::generateSamples`:

```cpp
unsigned long const cycles = (cpuCc - lastUpdate_) >> (1 + doubleSpeed);
lastUpdate_ += cycles << (1 + doubleSpeed);
```

gambatte calls it lazily — from the `0xFF10..=0xFF3F` access sites in
`memory.cpp` — but the call sites are not the mechanism. The advance is monotone
in `cc` and idempotent, so running it once per machine cycle (as
`Interconnect::tick_machine` already does) hands every observer at a
machine-cycle boundary the same state a per-access advance would, and under
tick-then-access every slopgb access is at one. The truncation is the mechanism.

**A DIV-APU edge is deferred to a granule boundary.** `Apu::pending_edge` holds
the edge with how far ahead of the APU's clock it was raised; `Apu::run_granule`
fires it at the first boundary at or past that point. In double speed a granule
*is* a machine cycle, so an edge raised on a trailing grid lands in the next
machine cycle — which is how two FF26 reads a machine cycle apart straddle the
length clock that disables channel 2 (`gambatte/speedchange/*ch2_nr52_1a` reads
the channel still on, `_1b` reads it off). All six of those rows pass, and
age `spsw-ch2-lc-delay-cgbBCE` comes with them.

The deferral is scoped to **double speed** (`Apu::raise_edge`). At single speed
both granules of a machine cycle sit inside the cycle that raised the edge, so
moving the step from the first to the second changes no observation the machine
can make — but it does move the step against the channels, and that costs
same-suite `channel_1_sweep_restart_2`. SameBoy passes that row, so it outranks
a gambatte-derived intra-cycle order (never drop a row SameBoy passes).

**Two events move the grid**, both from gambatte `sound.cpp`:

| event | effect | source |
|---|---|---|
| NR52 power-on | grid trails by one cycle in single speed, in step in double | `PSG::reset`: `lastUpdate_ = ((lastUpdate_ + 3) & -4) - !ds` |
| STOP *leaving* double speed | grid moves one cycle across the CPU's counter | `PSG::speedChange`: `lastUpdate_ -= ds` (leaving only) |

Entering double speed moves the grid's remainder not at all; its re-pace is the
whole extra granule of `Apu::set_double_speed_lag` (above). With both re-anchors
disabled the whole model is byte-identical to the eager clock — that is how it
was verified, by running the gambatte matrix with the grid pinned in step.

Save state carries `lag` and the pending edge; `STATE_VERSION` is 18.

### Residual: the twelve `ch1_duty0_pos6_to_pos7_timing` rows

Eleven in `speedchange/` plus `sound/ch1_duty0_pos6_to_pos7_timing_ds_6`.

**Measured, per ROM** (probe: run the suite's 16 frames on CGB, read
`ch1.duty_pos` at the end — the kernel freezes it, so it *is* the verdict; duty 0
is high on position 7 only, `_outaudio1` ⇔ 7):

- all 22 `_1` rungs sit on 6 and all of them are correct;
- every `_2` rung is on 7 (pass) or 6 (fail) — nothing else, on any row.

So each failure is the same one 2 MHz cycle: the duty expiry lands *after* the
`_2` rung's retrigger instead of between the two rungs. That kills every uniform
shift a priori, without running one — advancing the duty unit a granule
everywhere puts all 22 `_1` rungs on 7 and fails them. The fix has to move the
expiry in the twelve failing configurations only, and the `_1` rungs are the
two-sided bracket that says how far: one machine cycle, no more.

The twelve are not separated by any single structural key. Ends-in-double-speed
holds for ten of them but `sound` `ds_2`/`ds_4` and all three `speedchange5`
rungs end there and pass; switch count splits 1/2/3 failing against 4/5 passing;
the `nop` position, the frequency ($C0 vs $E0) and the final grid parity each cut
across both sets.

The enter-side pace IS a live lever on this family — measured, not argued
(an earlier derivation here claimed otherwise and was wrong).
`Apu::set_double_speed_lag` gives the entering STOP one granule more than
gambatte's `generateSamples(cc + 8, old speed)` jump does. Moving it:

| entering pace | score | what moves |
|---|---|---|
| one granule less (gambatte's literal jump) | −6 / +0 | fails `speedchange{2,4,5}*_2` |
| shipped | — | — |
| one granule more | +9 / −8 | fixes `speedchange{,2,3}*_2`, fails `speedchange{3,4,5}*_1` |
| one granule more + the leave debt | +9 / −9 | — |

Read the +9/−8 row against the position data and the per-enter accounting comes
out exactly: an extra granule per entering switch moves the expiry one granule
earlier, and a family flips only when that crosses one of its rungs. `k` = 1 and
2 (one enter) move the expiry from "after `_2`" to "between the rungs" — a pure
fix. `k` = 3 (two enters) moves it two, from "after `_2`" to "before `_1`" —
`_2` gains, `_1` loses. `k` = 4 and 5 were already between their rungs, so any
advance only costs them.

So the corpus wants **+1 granule for `speedchange` 1/2/3 and 0 for 4/5**, and no
per-switch accumulating term generates that: `k` = 3 and `k` = 4 have the same
two entering switches and differ only by a trailing leave, while `k` = 2 and
`k` = 3 differ by an entering switch yet want the same delta. Every combination
of a per-enter and a per-leave term was tried against those five values and none
fits.

The frame that is probably wrong is "delta per switch count" itself. Each rung
carries its own `dec b` delay constant, tuned so the expiry lands between the
rungs on hardware, so two families with the same switch structure need not want
the same correction. **Disassemble the kernels and compare their delay constants
before fitting another term** (`/rom-disasm-gaps`); the twelve failures may not
share one cause at all.

The leave-side debt is the one lever that moves rows, and it is bracketed:

| leave re-anchor | score |
|---|---|
| moves the grid only (shipped) | +6 / −0 |
| with the debt (literal `lastUpdate_ -= 1`) | +10 / −4 |
| debt alternating in sign per leave | +10 / −1, no source, NOT shipped |

The debt form takes speedchange2/3 `_2` and breaks speedchange{4,5} `_1`: a
`_1`/`_2` pair is two granules apart in single speed, so one granule flips only
one of them. No constant (enter, leave, power-on) shift satisfies both families
— the nr52 rows force the leave to flip the grid's parity, and every
parity-flipping shift gives speedchange2..5 the same granule delta while the
duty rows want +1 for 2/3 and 0 for 4/5. Swept enter 0..4 x leave 0..3 (20
points): best net +6, and only the shipped shape is regression-free.

The remaining candidate named in gambatte's source is the frame-sequencer
re-base on the *entering* side — `cycleCounter_ = cc - divCycles/2 -
lastUpdate_ % 2` (`PSG::speedChange`) — but read it before spending a session on
it: the DIV reset immediately preceding it leaves `divCycles` at the four
granules the `cc + 8` jump just ran, so the whole expression reduces to pulling
the PSG counter back two granules plus the grid's parity, and it moves the frame
sequencer *relative to the channels*. The duty rows measure the channels alone,
so it is unlikely to be their lever — it belongs to the length/envelope side,
which is already green.

### What is already ruled out

Do not retry these; each is measured, with numbers in the baseline notes:
deferring the length clock a machine cycle, deferring the whole APU step,
snapshotting pre-clock bits at the read (plain, cycle-keyed, speed-scoped and
granule-scoped), shifting the APU's divider view, moving the div_write edge,
suppressing the entering-side div event, and stepping the frame sequencer at
T-cycle granularity. The last of those is the useful negative: resolution is not
the problem, the observation point is.
