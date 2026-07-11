# Eager-value construction boot divergence — DIAGNOSED + FIXED (#11ds)

**Base:** `finish-port-halfdot @ 7b7fe39`. Fix is flag-gated; committed tree
keeps `interconnect.rs` defaults **false** (golden-safe, production
byte-identical).

## Symptom (reproduced)

Temp-flipping the two eager defaults (the C3 flip) —
`interconnect.rs:546 leading_edge_reads: false→true`,
`:548 eager_value: false→true` — makes mooneye `acceptance_ppu` FAIL **22/62**
rom×model combos, all with regs `B=42` (the mooneye fail path):

- `intr_2_mode0_timing`, `intr_2_mode3_timing`, `intr_2_mode0_timing_sprites`
  on all 6 models (18), plus `lcdon_timing-GS` on the 4 GS-family models (4).

Meanwhile `SLOPGB_MOONEYE_EAGER=1` (which sets eager POST-boot via
`set_eager_value`, defaults un-flipped) **passes 91/91 including intr_2**. So the
eager *steady-state* clock is correct; only the construction path diverges.

## Root cause — NOT a boot DIV/frame shift; it is PPU-flag propagation

The parent's hypothesis was a boot DIV / PPU-frame-position divergence. **That is
refuted by measurement.** The actual mechanism:

- The PPU holds its **own** copies of the reclock flags (`ppu.eager_value`,
  `ppu.leading_edge_reads`, gating its `StatUpdate` engine + render view).
- `set_eager_value(on)` / `set_leading_edge_reads(on)` **propagate** to the PPU
  (`self.ppu.set_eager_value(on)` etc).
- Flipping the `interconnect.rs` **struct-literal** defaults sets only the
  *Interconnect's* fields. `Ppu::new` already ran, so `ppu.*` stay **false** and
  nothing re-propagates. Result: the machine runs **eager reads against a
  non-eager PPU** — an incoherent frame → intr_2/lcdon `B=42`.

### Discriminator that isolates it (the key experiment)

With defaults flipped, arming eager **coherently but BEFORE** the boot warm-up
(`set_eager_value(false); set_eager_value(true)` *before* `apply_post_boot_state`)
**also passes** `acceptance_ppu` 62/62. If the bug were a boot-frame/DIV shift,
arming-before-boot would still be wrong. It is not — so the frame/DIV is fine and
the sole defect is the un-propagated PPU flags.

DIV is provably untouched: the boot `+4` recalibration in
`interconnect/boot.rs` keys on `tier2_reclock`, which eager **never** sets
(`set_eager_value` does not imply `tier2_reclock`). Eager and production share the
identical boot `div_counter`.

## Boot-state delta at hand-off (eager-struct-flip vs eager-post-boot vs production)

| quantity | production | eager post-boot (`set_eager_value`) | eager struct-flip (broken) | eager struct-flip + fix |
|---|---|---|---|---|
| boot `div_counter` (+4?) | no | no | no | no |
| PPU line/dot at hand-off | native | native | native | native |
| `interconnect.eager_value` | false | true | **true** | true |
| `ppu.eager_value` | false | true | **false (BUG)** | true |
| `ppu.leading_edge_reads` | false | true | **false (BUG)** | true |
| intr_2 verdict | pass | pass | **B=42 FAIL** | pass |

The only diverging rows are the PPU's flag copies. Everything timing (DIV, frame
position, cycles) is identical across all four columns.

## Fix (flag-gated, production byte-identical)

`GameBoy::post_boot_inner` (`lib.rs`): when the eager default is on, **suppress
across boot, re-arm after** — exactly mirroring the proven `set_eager_value`
post-boot path (the frame every EV two-bin measurement rides):

```rust
let eager = bus.eager_value();      // the (possibly-flipped) construction default
if eager { bus.set_eager_value(false); }
bus.apply_post_boot_state();        // boot warm-up on the production frame
if eager { bus.set_eager_value(true); }  // propagate to the PPU after hand-off
```

`eager == false` in production → both branches skipped → byte-identical. The
re-arm after `apply_post_boot_state` matches `SLOPGB_MOONEYE_EAGER`'s post-boot
enable, so the whole EV measurement corpus (295/54/...) is preserved.

(Arm-before-boot also fixes intr_2, but arm-after is chosen because it is the
frame every existing EV two-bin was measured against — zero risk to the
boot-frame-sensitive render rows.)

### Red-before-green pin

`GameBoy::new_with_eager` (test/`port_probe`) drives the eager default *through
construction* via `Interconnect::arm_eager_construction_default` (sets the
Interconnect fields un-propagated, exactly as the raw flip does), then relies on
`post_boot_inner`'s re-arm to make it coherent. Test
`mooneye::eager_construction_intr_2_timing` runs the three intr_2 ROMs ×
{Dmg,Cgb} via that path. **GREEN with the fix; RED (B=42) when the
`post_boot_inner` re-arm is removed** (verified by temporarily deleting it).

## Gates (committed tree = defaults FALSE)

| gate | result |
|---|---|
| golden_fingerprint | byte-identical ✓ |
| acceptance_ppu WITH defaults temp-flipped | **62/62** (was 22-fail → 0-fail) ✓, then reverted |
| `SLOPGB_MOONEYE_EAGER=1` full mooneye | 93/93 ✓ (92 groups + new pin) |
| production full mooneye | 93/93 ✓ |
| flagon_probe EV CGB / EV DMG | fail **295 / 54** (unchanged) ✓ |
| flagon_probe tier2 CGB / DMG | fail **291 / 116** (unchanged) ✓ |
| flagon_probe OFF CGB | fail **486** (unchanged) ✓ |
| eager gbtr pins | 17/17 ✓; + di_timing/intr_2 tier2 pins ✓ |
| lib unit tests | 760/760 ✓ |
| clippy (prod + `--all-targets --features port_probe`) | `-D warnings` clean ✓ |
| every `.rs` < 1000 | lib.rs 969, cycle.rs 534, accessors 129 ✓ |
| red-before-green | GREEN w/ fix, RED w/o re-arm ✓ |

## C3-checklist implication

The eager construction path is now **intr_2-safe**: the C3 flip (flipping the two
`interconnect.rs` defaults) no longer breaks the 22 PPU-timing combos. This
removes the single biggest construction-side flip blocker. The fix is purely
flag-gated; production stays byte-identical. Remaining eager residual is the
steady-state read/render web (EV CGB 295 etc), tracked in the eager-convergence
maps — unaffected by this change.
