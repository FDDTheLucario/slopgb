# EAGER `ppu_sprite0_scx{2,6}_b` [Dmg] — #11eg's "rphd-512 weld" REFUTED by the rom-diff-weld method; both rows SHIPPED clean (2026-07-11, #11eh)

Base: `finish-port-halfdot @ 3e2e8b1` (= #11eg). Fix SHIPPED (`read_laws_exit.rs`
+ pin `eager_dmg_sprite0_passes`); golden byte-identical, all gates hold.

## Task

Re-examine the 2 `gbmicrotest ppu_sprite0_scx{2,6}_b` [Dmg] eager flip
regressions that #11eg
(`eager-nongambatte-relatch-2026-07-11.md`) refuted as an "A/B-PROVEN rphd-512
weld" with `gambatte m2int_m3stat_1` / `late_scx4_1` — a verdict reached by a
READ-FRAME sweep (`SLOPGB_ARM8BIAS`), exactly the lever the `rom-diff-weld`
skill warns produces a FALSE floor. Apply the skill's method instead.

## STEP 1 — `cmp -l` the siblings: a whole-M-cycle NOP shift (representable)

```
cmp -l ppu_sprite0_scx2_a.gb ppu_sprite0_scx2_b.gb
  336 161 150      # expected-value constant (0x71 → 0x68): _b wants a diff mode
  537 360   0      # a single 0x00 (NOP) inserted at 537 in _b …
  538 101 360      # … shifting the whole downstream run one byte later
  ...
```

A single NOP inserted in `_b` = one whole M-cycle (4 T) that delays `_b`'s
measurement `ldh a,(FF41)` read by 4 dots vs `_a`. REPRESENTABLE — the skill's
signature, NOT the "sub-M-cycle 1-T weld" #11eg assumed.

## STEP 2 — full-trace the CPU FF41 read (regs.rs probe, reverted): the read DOT is the whole test

Decisive measurement read on line 1 (`{ly,dot,rphd,vm,carr,rdcarr,nspr,flip}`):

| ROM (want) | clock | dot | rphd | carr | rdcarr | nspr | flip | exit | vm | verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| scx2_a ($83, mode 3) | OFF | 252 | 504 | 0 | false | 0 | 256 | 514 | 3 | PASS |
| scx2_b ($80, mode 0) | OFF | **256** | 512 | 0 | false | 0 | 256 | 514 | **0** | PASS |
| scx2_b ($80, mode 0) | EAGER | **252** | 512 | 0 | false | 0 | 256(proj) | 514 | **3** | **FAIL** |
| scx6_b ($80, mode 0) | OFF | 260 | 520 | 0 | false | 0 | 260 | 522 | 0 | PASS |
| scx6_b ($80, mode 0) | EAGER | 256 | 520 | 0 | false | 0 | 260(proj) | 522 | 3 | FAIL |

The `_a`/`_b` NOP brackets the flip: `_a` reads at dot 252 (< flip 256 → mode 3),
`_b` at dot 256 (= flip → mode 0). Under eager the CPU dispatch moves `_b`'s
read one M-cycle early (256 → 252) but the `+8hd` read-debt keeps
`read_pos_hd = 512 = 2*flip`. **Production reads mode 0 AT `flip_dot`** (the flip
is inclusive → rphd `2*flip` is already mode 0), but the bare-exit arm's emergent
`2*flip + 2` (= 514) holds mode 3 for 2 extra hd → reads mode 3 → `$83`, wrong.

## STEP 3 — the render-FSM discriminator: sprite0 is a POLLED read; the weld-partners are CARRIED

The task's core suspicion, VERIFIED by tracing BOTH the sprite0 rows AND #11eg's
claimed weld-partners at their DECISIVE reads:

| ROM (want) | rphd | **carr** | **rdcarr** | proj flip | eff.scx | exit arm |
|---|---|---|---|---|---|---|
| **sprite0_scx2_b** (mode 0) | 512 | **0** | **false** | 256 | 2 | `2*256 + 2 − 0` = 514 |
| `late_scx4_1` (mode 3) | 512 | **4** | **true** | 258 | 4 | `2*258 + 2 − 4` = 514 |
| `m2int_m3stat_1` (mode 3) | **504** | 4 | true | 254 | 0 | `2*254 + 2 − 4` = 506 |

- `nspr` is 0 on sprite0's measured line (line 1) — #11eg's "bare, sprite-free"
  claim is CORRECT (the sprite's timing effect is baked into the ROM's dispatch,
  the measured line renders bare) — so `bare_sprite_free()` / arm-8 DO fire. But
  that was never the discriminator.
- The real discriminator: **`read_carried`** (equivalently `carr`). sprite0's
  measurement is a POLLED read (`carr = 0`); both weld-partners are mode-2-ISR
  **carried** reads (`carr = 4`). The exit's `- carry` term ALREADY lands the
  carried partners at `2*flip − 2`; they arrive at 514 via `2*258 + 2 − 4`, a
  DIFFERENT path from sprite0's `2*256 + 2 − 0`.
- `m2int_m3stat_1`'s decisive read is at rphd **504**, not 512 — #11eg's
  "IDENTICAL rphd 512" is simply wrong for it (its rphd-512 read reads mode 0).

**#11eg's `ARM8BIAS` sweep lowered `2*flip + bias` UNIFORMLY**, hitting polled AND
carried reads equally — so of course sprite0's `−2` dropped the carried partners.
That is the read-frame-sweep false-weld the skill names. A DISCRIMINATOR EXISTS:
`read_carried`.

## STEP 4 — the fix: drop the emergent `+2` for the eager-DMG POLLED bare exit

`read_laws_exit.rs` arm-8 SS bare branch:

```rust
let over = if self.eager_value && !self.model.is_cgb() && !self.read_carried {
    0            // polled read at rphd 2*flip is ALREADY mode 0 (flip inclusive)
} else {
    2            // carried reads owned by `- carry`; tier2/production keep +2
};
fold(&mut exit, 2 * i32::from(flip) + over - carry - phase);
```

`eager_value` + `!is_cgb` + `!read_carried` scoped → tier2 + production
byte-identical by construction; the carried weld-partners keep exit 514 (their
`- carry` owns them) and still read mode 3. Swept unique-optimal: `over ∈ {0}`
recovers both, `{2}` = the bug, no intermediate.

## Gates (all HELD)

- `golden_fingerprint` byte-identical — 9020 cases match HEAD.
- **sprite0 recovery:** `ppu_sprite0_scx{2,6}_b` eager PASS ($80); `scx{2,6}_a`
  still PASS ($83). Whole scx0–7 family: no new drops (scx3_b/scx7_b fail OFF too
  — documented `gbmicrotest.txt` "hardware divergence" baseline rows, untouched).
- **EV DMG 38 → 38, byte-identical fail-SET** (`comm` both ways = ∅) — zero
  gambatte-OCR drops (sprite0 is off-rowlist; the arm change touches no gambatte
  DMG row).
- **EV CGB 287 unchanged** (`!is_cgb`-scoped).
- **tier2 CGB 291 / DMG 116** unchanged.
- **mooneye 93×3** (OFF / `SLOPGB_MOONEYE_EAGER` / `SLOPGB_MOONEYE_RECLOCK`).
- **Red-before-green pin** `gbmicrotest::eager_dmg_sprite0_passes`: FAILS with
  `+2` restored (FF82=0xFF, got $83), passes with the fix.
- clippy `-D warnings` clean; `read_laws_exit.rs` 748 lines (< 1000).

## Verdict

#11eg's rphd-512 weld was a read-frame-sweep artifact. The eager sprite0 flip
regressions were a POLLED-read `+2` over-hold, separable from the carried
weld-partners by `read_carried`. Both rows SHIPPED clean. The eager non-gambatte
residual set drops 15 → 13 (the 2 sprite0 rows cleared; the 7 render-LENGTH + 6
`ly_lyc_153` remain, per #11eg).
