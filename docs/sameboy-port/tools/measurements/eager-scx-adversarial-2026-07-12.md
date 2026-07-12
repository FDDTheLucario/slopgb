# Adversarial re-examination of the 2 eager DMG SCX mealybug regressions (2026-07-12, #11el)

Task: `rom-diff-weld` the 2 `m3_scx_high_5_bits` / `m3_scx_low_3_bits` [Dmg]
eager pixel regressions that #11ej refuted as "genuine render-LENGTH, eff.scx IS
the length, no discriminator." #11ej swept a UNIFORM SCX render-debt (the exact
false-weld error) and never checked the SCX write's relation to the fine-scroll
comparator lock.

**Result: 1/2 CRACKED (`m3_scx_high_5_bits`, +1, zero drops), 1/2 airtight
REFUTED (`m3_scx_low_3_bits`) with a per-debt A/B table. #11ej's blanket "no
discriminator" was WRONG for the post-match ROM, RIGHT for the pre-match one.**

## The discriminator #11ej missed: the fine-scroll comparator lock (`hunt_done`)

The DMG mode-3 length is EMERGENT from slopgb's render pipeline; the fine-scroll
`SCX&7` discard hunt locks (`Render::hunt_done`, at `hunt_match_dot`) early in
mode 3. A mid-mode-3 SCX write is one of two physically distinct things:

* **POST-match** (write after the lock): the discard is already fixed → the write
  is a pure COARSE / pixel tile shift with **no mode-3-length effect**.
* **PRE-match** (write during the hunt): it changes the very quantity the
  emergent length grows from → it moves both the pixels AND the FF41 length.

`cmp -l` showed the two ROMs are DIFFERENT programs (131 byte diffs), not a
NOP-shift pair — so this is the lone-ROM shape (skill step 1b: trace the eager
frame). Tracing the FF43 commit under the coherent eager clock:

| ROM (target) | write dot | scx old→new | `hunt_done` @ write | `hunt_match_dot` |
|---|---|---|---|---|
| **m3_scx_high_5_bits** | 111 | 0→1..7 (+high) | **TRUE** (post-match) | 89 |
| **m3_scx_low_3_bits**  | 87  | 0→2            | **FALSE** (mid-hunt)  | 0  |
| gambatte late_scx4_1               | 84 (line-start) | 4→4 | true (STALE) | 89 |
| gambatte ly0_late_scx7_m3stat_scx0_2 | 82 | 0→7 | false | 0 |
| gambatte late_scx_late_disable_0   | 87 | 0→4 | false | 0 |

**`m3_scx_high` is the ONLY post-match writer; every gambatte length row writes
pre-match (or at line-start).** That is the clean separation #11ej's uniform
sweep could not see.

## The fix — a POST-match SCX render debt (`Ppu::stage_write`, FF43 DMG SS)

```rust
0xFF43 if !self.ds
    && self.render.hunt_done
    && self.dot > self.render.hunt_match_dot => 6,
```

`eager_value`-scoped, `!is_cgb` + `!ds`. `6` swept unique-optimal on
`m3_scx_high_5_bits`: 4→41px, **6→PASS (0px)**, 8→35px.

### The `dot > hunt_match_dot` guard (the line-start trap)

The gate was NOT `hunt_done` alone: under the eager clock a LINE-START SCX write
(dot 80, mode-3 entry) stages BEFORE `render_init` resets the hunt, so it sees the
PREVIOUS line's stale `hunt_done=true` / `hunt_match_dot=89`. `late_scx4_1` does
exactly this and DROPPED with the naive `hunt_done`-only gate (EV DMG 38→39). The
match dot is always ≥85 (early mode 3), a line-start write is at dot 80, so
`dot > hunt_match_dot` rejects the stale case and admits only genuine
this-line-post-match writes. With the guard: EV DMG **38, zero drops.**

## m3_scx_low — airtight REFUTATION (genuine pre-match length coupling)

`m3_scx_low` writes scx 0→2 at dot 87 **during** the fine-scroll hunt
(`hunt_done=false`) → it feeds the emergent length. A/B sweep of a PRE-match SCX
debt (post-match held at 6), measuring m3_scx_low pixels vs new gambatte EV-DMG
drops:

| pre-debt | m3_scx_low | new gambatte drops |
|---|---|---|
| 0 | 320px (fail) | 0 |
| 2 | 320px (fail) | 2 (ly0_late_scx7_m3stat, late_scx4) |
| 4 | **PASS**     | 3 (+ late_scx_late_disable) |
| 6/8 | PASS       | 3 |

The gambatte m3stat/late_scx length rows DROP at debt≥2 — **before m3_scx_low even
improves** (still 320px at debt 2). No pre-match discriminator (write dot 87/82,
scx&7 2/4/7, window-free vs window all overlap). eff.scx genuinely IS the length
for a mid-hunt write. Kept zero-debt (`_ => 0`).

## Gates (all hold)

* **golden_fingerprint byte-identical** (render-path change, `eager_value`-scoped).
* mealybug pixel_probe EV: `m3_scx_high` PASS (was 159px), `m3_scx_low` unchanged.
* **gambatte EV DMG 38 (ZERO drops), EV CGB 287** (both rowlists; `!is_cgb`-scoped).
* tier2 two-bin **CGB 291 / DMG 116** unchanged.
* mooneye **93/93 × 3** (OFF / `SLOPGB_MOONEYE_EAGER` / `SLOPGB_MOONEYE_RECLOCK`).
* mealybug_matrix + age_matrix (OFF) green; clippy clean; regs.rs 998 (<1000).
* Red-before-green pin `mealybug_eager_dmg_m3_scx_high_writecommit_passes`
  (FAILS at 159px with the arm's `6`→`0`, PASSES restored).

## Lesson

#11ej's uniform SCX sweep shifted ALL writes equally, so the post-match ROM
(separable) and the pre-match ROM (coupled) looked identically welded. The
fine-scroll comparator lock (`hunt_done`, guarded by `dot > hunt_match_dot`
against line-start staleness) is the representable discriminator: it recovers the
one that was never length-coupled and confirms the one that is.
