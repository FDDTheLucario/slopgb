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
rows pin separately. Measured — a warmup long enough to place the duty phase
scores +11/−3, winning three `ch2_init_env_counter_timing` rows on DMG and
losing the same three on CGB; advancing channel 1 alone scores **+8/−0**.

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
