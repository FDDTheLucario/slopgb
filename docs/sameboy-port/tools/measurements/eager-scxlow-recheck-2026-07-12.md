# Adversarial re-crack of `m3_scx_low_3_bits` [Dmg] — the 8th floor falls (2026-07-12, #11em)

Task: `rom-diff-weld` the ONE eager DMG SCX mealybug regression #11el declared
"airtight REFUTED as genuine render-LENGTH — eff.scx IS the length, no
discriminator." #11el swept a UNIFORM pre-match SCX debt (the exact false-weld
error the skill warns of) and stopped at "write dot 87, scx&7 2/4/7 overlap"
without ever A/B-ing the axis the mealybug ROM is literally named for: the SCX
VALUE.

**Result: CRACKED. `m3_scx_low_3_bits` PASSES eager (320px → 0px), ZERO gambatte
drops, golden byte-identical.** #11el's refutation was WRONG at its premise —
the write is NOT length-coupled; it is a pure coarse shift whose eager cc+0
commit RE-OPENS the fine-scroll comparator. The discriminator is a window/glitch
render-state flag, not even the scx value.

## The premise #11el got wrong: the hunt is ALREADY locked before the write

The DMG mode-3 length is emergent from the fine-scroll comparator hunt
(`Render::hunt_done` at `hunt_match_dot`). #11el asserted m3_scx_low writes
"during the hunt (`!hunt_done`) → it feeds the emergent length." The full trace
(probe on `stage_write` FF43 + the hunt) shows the opposite:

| clock | SCXW dot | scx 0→2 commit | **hunt match** |
|---|---|---|---|
| OFF (pass) | 88, staged 2 | dot 90 | dot 89 (mode3_dot 5), **scx=0, discard=0** |
| tier2 (pass) | 87, staged 3 | dot 90 | dot 89, **scx=0, discard=0** |
| **eager (fail)** | 84, staged 6 | **dot 87** | **dot 91 (mode3_dot 7), scx=2, discard=2** |

A bare BG line begins SCX=0, so the comparator matches IMMEDIATELY at mode3_dot 5
(hunt_idx 0 == scx&7 0) → discard 0. OFF/tier2 commit the write to scx=2 at dot
90, one dot AFTER that lock → no effect on the discard (pure coarse shift, correct
= reference). The eager write stages at cc+0 (dot 84) and commits at dot 87,
BEFORE the dot-89 hunt, so the comparator runs with scx=2 the whole time and
RE-MATCHES at mode3_dot 7 → **discard 2**, a wrong length → 320px. `!hunt_done`
at stage time is TRUE for all three, but the OLD-SCX=0 match makes the write
post-effective-lock. #11el read `!hunt_done` as "feeds the length"; it doesn't.

## Every gambatte "length row" #11el welded with is separable — POST-match, glitch, or window

Traced all the SS-DMG SCX-during-m3 gambatte rows under the coherent eager clock:

| row | dir | SCXW dot | scx→new | hunt_done | glitch | wy_trig_sb | class |
|---|---|---|---|---|---|---|---|
| **m3_scx_low_3_bits** (target) | mealybug | 84 | 0→2 | false | **false** | **false** | pre-match BG |
| m2int_scx2/3/5_m3stat | m2int_m3stat/scx | **152** | 0→N | **TRUE** | false | false | POST-match (arm above) |
| ly0_late_scx7_m3stat_scx0_2 | enable_display | 80 | 0→7 | false | **TRUE** | false | glitch re-open |
| late_scx_late_disable_0/1/2 | window | 84 | 0→4 | false | false | **TRUE** | window line |

* **m2int_scxN** write at dot **152** with `hunt_done=TRUE` → already the existing
  post-match arm (`hunt_done && dot > hunt_match_dot => 6`). Not in the pre-match
  bucket at all — #11el's uniform sweep only *looked* like it welded them.
* **ly0_late_scx7** is the CGB LCD-enable **glitch line** — it WANTS the comparator
  re-open (the tier2 glitch re-open law at `regs.rs`). `!glitch_line` excludes it.
* **late_scx_late_disable** is a **window line** (`wy_trig_sb`=TRUE, LCDC win
  enable=TRUE). The window masks the SCX fine-scroll discard, so the eager early
  commit is already CORRECT there (out0 both eager and OFF); the debt would BREAK
  it. `!wy_trig_sb` excludes it. This is the sole row that drops under a
  value-blind debt — the "3rd drop" #11el saw.

The scx VALUE (2 vs 4 vs 7) that #11el skipped is a real distinguisher, but the
*physical* discriminator is cleaner: **the render-state flags `glitch_line` /
`wy_trig_sb`** — the window and glitch lines are genuinely length/re-open-coupled;
the bare BG line is not.

## The fix — a PRE-match SCX render debt, BG-line-scoped (`Ppu::stage_write`, FF43 DMG SS)

```rust
0xFF43 if !self.ds
    && !self.render.hunt_done
    && !self.glitch_line
    && !self.wy_trig_sb => 6,
```

`eager_value`-scoped, `!is_cgb` + `!ds`. `6` = the render-frame debt (3 base dots
×2 + 6 = 12 half-dots → commit at dot 84+6 = **dot 90**, exactly the OFF/tier2
commit, past the dot-89 hunt lock). Same `6` as the post-match arm. Swept
unique-optimal on m3_scx_low pixels: 3→320px, **4/5/6/7→PASS**; `6` is the
principled OFF-frame alignment.

`regs.rs` was at 998 lines; the arm needed a SPLIT FIRST — the mode-3 write-strobe
trio (`stage_write`/`commit_eff`/`strobe_tick`, ~270 lines) moved to a
`regs/strobe.rs` sibling (byte-identical commit, second `impl Ppu` via
`use super::*`, same pattern as the `stat_irq/*` splits). regs.rs 727 / strobe.rs
304, both <1000.

## Gates (all hold)

* **golden_fingerprint byte-identical** (9020 cases) — split commit AND arm commit
  (render-path, `eager_value`-scoped).
* mealybug pixel EV: **m3_scx_low_3_bits PASS** (was 320px), m3_scx_high still PASS.
* **gambatte EV DMG 38 (ZERO new drops, ZERO fixed — the row is mealybug, not in
  the gambatte rowlist), EV CGB 287** (both rowlists; `!is_cgb`-scoped, CGB frame
  untouched).
* tier2 two-bin **CGB 291 / DMG 116** unchanged.
* mooneye **93/93 × 3** (OFF / `SLOPGB_MOONEYE_EAGER` / `SLOPGB_MOONEYE_RECLOCK`).
* mealybug_matrix + age_matrix (OFF) green; age/mealybug eager writecommit pins
  green; clippy `-D warnings` clean.
* Red-before-green pin `mealybug_eager_dmg_m3_scx_low_writecommit_passes` (FAILS at
  320px with the arm `6`→`0`, PASSES restored; the high-bits pin is independent).

## Lesson

#11el's uniform pre-match SCX sweep shifted the bare-line write AND the
window/glitch writes equally, so a separable BG-line row and two genuinely-coupled
window/glitch rows looked identically welded — and its "scx&7 overlaps" glance
never A/B'd the value/state axis. The comparator was ALREADY locked (old SCX=0
matched at mode3_dot 5); the eager early commit merely re-opened it. Gating the
render-frame debt on `!glitch_line && !wy_trig_sb` recovers the one row that was
never length-coupled and leaves the two that are. 8th "structural floor" of the
endgame cracked by the same method: find the representable discriminator before
believing a uniform-sweep weld.
