# APU

## Post-boot warmup

- The APU is warmed ~1 emulated second post-boot so the boot beep's envelope is decayed at hand-off: PCM12/FF76 reads `$00`, and NR52 keeps the ch1 status bit.
- Parked: "simplifying" the warmup away — it is load-bearing for the post-boot PCM12/NR52 state. Do keep the ~1 s warmup.

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
| `freq_change_timing` revision variants | revision-dependent |
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
