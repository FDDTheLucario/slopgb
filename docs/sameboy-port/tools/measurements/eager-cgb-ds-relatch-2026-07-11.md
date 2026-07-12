# The 8 CGB double-speed STAT-bar rows — the #11ec/#11ed/#11ee method RE-CRACKS all 8, none is a floor (#11ef)

Base: `finish-port-halfdot @ 559678a` (= #11ee). **CODE SHIPPED** — five
`eager_value`-gated, `is_cgb`/DS-scoped mechanisms across `ff0f.rs`,
`read_laws.rs`, `read_laws_exit.rs` and `speed.rs`. EV CGB **295 → 287**, CLEAN
**+8/−0** (A/B comm: recovered = exactly the 8 targets, new-fails = ∅). All
measurement scaffolding (a `SLOPGB_FT` full read/write/ack trace on the eager
`Bus`/`ack_impl`, an `SLOPGB_SPRK` sprite-exit sweep, the `dbg_isr` intf dump)
was REVERTED; only the fixes + comments + the `eager_cgb_ds_relatch_passes` pin
remain.

## TL;DR — #11dk/#11dx's "dispatch/emission/floor" verdict is REFUTED

#11dk/#11dx floored these 8 with the DISCREDITED read-debt/dispatch reasoning
that also falsely floored the DMG rows (#11ec recovered ~16 of those). Applying
the CORRECT method — ROM-binary `cmp -l` → full-trace diff under `eager_value`
+ DS → the representable whole-M-cycle / half-dot latch → calibrate the
eager-DS-frame arm — every one cracks. Each `_1`/`_2` (or `_1`/`_2` cross-file)
sibling differs by a whole NOP (2 dots at DS) shifting a WRITE or a READ that
slopgb latches as a representable dot; the eager cc+0 DS frame records it one
M-cycle before the tier2 cc+4 frame the consuming law targets.

## The ROM-binary diffs (`cmp -l`) — every pair is a whole NOP

All 8 `_1`/`_2` (or sibling) pairs are a 1-byte `00` (NOP) insertion shifting the
subsequent bytes down one M-cycle (2 dots DS). Confirmed on every pair.

## The 5 mechanisms

### (a) glitch-line mode-0 read-view mask — `ff0f.rs` `ff0f_cgb_ds_glitch_m0_mask`
Rows: `ly0_m0irq_scx0/1_ds_1` (want E0), `frame0_m0irq_count_scx2/3_ds_1`
(want 90). On the LCD-enable glitch line the eager DS frame emits the mode-0
STAT source EARLY (`intf` bit1 set ~dot 19, well before the real mode-3→0 flip),
so a poll landing BEFORE the true rise R reads the set bit where SameBoy's cc+4
frame — polling short of R — reads clear. The eager cc+0 read's TRUE position is
`dot + 2` (the +2-dot DS read-debt); mask `IF_STAT` when `dot + 2 <= R` (R =
`m0_flip_dot`, the render's own flip/projection). `scx0_ds_1` reads dot 250
(true 252) < R 253 → clear; its `_2` dot 252 (true 254) → set. The DMG analogue
(`ff0f_dmg_m0_coincident_mask`) had no CGB DS twin — this is it.

### (b) DS carried mode-2 line-start read debt — `read_laws.rs` (the `read_carried` arm)
Row: `m2int_m0stat_ds_2` (want 2). The mode-2 ISR's line-start FF41 read: `_2`
reads (leading, cc+0) at ly136 dot 0 (rph 4 = true dot 2), `_1` at the PREVIOUS
line's dot 454 (`dot < 4` excludes it → native mode 0). The carried arm returned
`dot >= 2 ? 2 : 0` on the RAW dot → 0 at raw dot 0 (want 2). Fix: the
debt-adjusted `read_pos_hd() >= 4` (= true dot 2). For tier2 (`read_deferred`
advances `self.dot` to cc+4, no debt) `read_pos_hd = 2*dot` so `>= 4` ⟺ the old
`dot >= 2` — byte-identical there.

### (c) eager shifted-frame flip twin — `read_laws.rs` (lcd_offset arm)
Row: `offset1_lyc99int_m0stat_count_scx2_ds_1` (want 90/mode 3, both siblings).
On the STOP-shifted frame (`lcd_shift_dots != 0`) the whole-dot flip lands ON
the poll dot but the true flip is a half-dot past → the poll reads mode 3. The
existing `dot == flip_dot` arm caught the `_2` sibling (flipped, dot 257 ==
flip 257); the eager `_1` twin reads 2 dots earlier (dot 255) WHILE the render
has not flipped (raw mode still 3, `flip_dot == 0`) and the bare arm-8 exit
(2·256 = 512) wrongly drops it to 0. Its debt-adjusted position `read_pos_hd`
514 == `2 * projected_flip_dot` 514 (the projected flip 257) — add an
`eager_value` twin returning 3 when the read lands EXACTLY on the projected flip.

### (d) DS window+sprite emergent exit — `read_laws_exit.rs` Arm 8-spr
Rows: `10spritesPrLine_wx7_m3stat_ds_2` (want 0; +7 sibling `wx0..6` NOT reached,
see below). Arm 1 EXCLUDES sprite-laden DS lines (`!ds || n_sprites == 0`, its
closed form carries no sprite penalty) and arm 8 requires `bare_sprite_free`, so
a WINDOW+SPRITE DS line gets NO exit → raw mode, mis-verdicting the `_2` sibling
reading one M-cycle past the render's flip (dot 370 < flip 371, raw mode 3, want
0). The render's OWN flip bakes in the window+sprite cost → the EMERGENT exit
`2*flip + 1` (swept unique on the DS window+sprite set: `+0` drops 7 siblings,
`+1/+2` plateau recovers `wx7` clean, `+3` loses it). Scoped `win_active` +
`n_sprites > 0` + `exit.is_none()` (non-window sprite lines are raw-mode-correct,
untouched). NOTE: `wx0..6` share the SAME render flip 371 but SameBoy ends mode 3
wx-dependently earlier (~321..361) — those are a RENDER-length mismatch (the
projected flip is itself wrong), NOT a read-frame miss, so this READ arm cannot
reach them (a render fix, out of scope).

### (e) DS mode-0 ack-squash widened to window 4 — `speed.rs` (`stat_src_hblank`)
Row: `late_m0irq_retrigger_scx1_ds_2` (want E0). The manual `ldh (FF0F),02`
re-sets IF bit1 → a second STAT ISR dispatches + acks it; a mode-0 rise landing
just after the ack is DELIVERED (re-set → E2), at/before is folded+cleared (E0).
The DS ack-squash window was 3 (widening to 4 over-squashes 6 LYC/m2/m1/vblank
DS `_1` retriggers by −6). Measured DS ack→mode-0-rise gaps: `_ds_2` 3 (squash),
`_scx1_ds_2` **4** (want squash, window 3 UNDER-squashed → delivered → E2, the
bug), `_ds_1` 5 (deliver), `_scx1_ds_1` 6 (deliver) — the boundary is gap 4, so
mode-0 needs window 4. The 6 over-squashed families have a DIFFERENT enabled STAT
source (`eng_stat & STAT_SRC_HBLANK` is set ONLY for `late_m0irq_retrigger`;
the others es=20 OAM / es=40 LYC), so widen to 4 EXACTLY when
`stat_src_hblank()`. The mode-0 `_ds_1`/`_scx1_ds_1` at gap 5/6 stay outside
window 4 and still deliver.

## Gates (all hold)

| gate | value |
|---|---|
| `golden_fingerprint` (production, no port_probe) | **ok — byte-identical** (43s) |
| EV CGB | **295 → 287** (−8 clean; A/B comm recovered = 8 targets, new = ∅) |
| EV DMG | 38 (A/B comm byte-identical — all arms `is_cgb`/DS-scoped, DMG never DS) |
| tier2 CGB / DMG | 291 / 116 (unchanged — every change `eager_value`-gated) |
| mooneye OFF / RECLOCK / EAGER | **93 / 93 / 93** (intr_2 / di_timing incl.) |
| clippy `-D warnings` | clean |
| file cap | `stat_irq.rs` 893, `ff0f.rs` 317, `read_laws.rs` 415, `read_laws_exit.rs` 724, `speed.rs` 672 (all < 1000) |
| pin | `eager_cgb_ds_relatch_passes` (red before: all 8 in the baseline EV CGB fail set) |

## Do-not-re-chase ledger

- None of the 8 is a floor / needs a T-exact CPU core. Each `_1`/`_2` weld's
  discriminator is a whole-M-cycle NOP landing a WRITE/READ at a representable,
  latched eager-DS dot, mis-framed against the tier2 cc+4 frame — the same
  #11ec/#11ed/#11ee shape. #11dk/#11dx floored them from the read-debt-only sweep
  (which moves both siblings equally) WITHOUT the ROM-diff / full-trace.
- `10spritesPrLine_wx0..6_m3stat_ds_2` (want 0) are the SPRITE arm's residual:
  the render's projected flip (371) is itself wrong for them (SameBoy ends mode 3
  at ~321..361, wx-dependent). A RENDER-length fix, NOT a read-frame arm — the
  read cannot see a flip the render mis-projects. Parked (render lever).
- The DS ack-squash window stays 3 for the LYC/OAM/m1/vblank retrigger families
  (window 4 over-squashes them −6); the mode-0 (HBLANK) family alone takes 4, the
  enabled STAT source (`stat_src_hblank`) being the representable discriminator.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd11
cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/hd11/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# EV CGB 287 (was 295):
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/cgb_rowlist.txt \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['
# the pin:
SLOPGB_REQUIRE_ROMS=1 cargo test -p slopgb-core --test gbtr --release eager_cgb_ds_relatch_passes
# golden + mooneye x3 as in #11ee.
```
