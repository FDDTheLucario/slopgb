# Stage 2 (Part B) — the exact-half-dot deferred read: fp dual-trace gate (2026-07-02)

HALFDOT-BUILD-PLAN §6 step 2. Code: `phase-b-s7` (read-position API +
`read_deferred` display-sync formalization). Gate ROMs: the kernel pair (DMG)
+ `m2int_m3stat` DS legs (CGB).

## Result: GATE PASSES — the deferred FF41 read lands at SameBoy's frame

Trace pair: `SLOPGB ff41 ly= dot= ... dh= mclk=` (new fields: `dh` =
`Ppu::sub_dot()`, `mclk` = machine dots since boot) ↔ `SBREAD ff41 ly= cfl=
dc= mode= fp=`. In-line half-dot position: slopgb `2*dot + dh`, SameBoy
`2*cfl + dc` (dc folded — REQUIRED, see below).

| ROM (measurement read) | slopgb | hd | SameBoy | hd | Δ | verdict |
|---|---|---|---|---|---|---|
| `m2int_m3stat_1` (dmg) | ly1 dot252 dh0 | 504 | ly1 cfl256 dc0 | 512 | +8 | 3 = 3 ✓ |
| `m0int_m3stat_2` (dmg) | ly1 dot256 dh0 | 512 | ly1 cfl261 dc−2 | 520 | +8 | 0 = 0 ✓ |
| `m2int_m3stat_ds_1` (cgb) | ly135 dot252 dh0 | 504 | ly137 cfl256 dc0 | 512 | +8 | 3 = 3 ✓ |
| `m2int_m3stat_ds_2` (cgb) | ly135 dot254 dh0 | 508 | ly137 cfl259 dc−2 | 516 | +8 | 0 = 0 ✓ |

- **The frame mapping is a UNIFORM +8 half-dots (+4 dots), BOTH speeds.** The
  goal/plan's "DS ISR read offset is +3 not +4" is a **cfl-only reading
  artifact**: the DS reads land at `cfl X, dc=−2` — cfl alone says +3, the
  dc-folded half-dot position says +8 hd exactly. One frame constant, not two.
- The read-to-read spacing matches per pair: kernel 8 hd apart both emulators;
  DS legs 4 hd apart both. The reads are POSITIONED correctly; only
  boundary/verdict laws remain (stage 3).
- The DS line label diverges (slopgb ly135 ↔ SameBoy ly137, the #11an
  observation) — a loop-anchor difference, not a read-frame error; the in-line
  hd offset is the invariant.
- fp absolute deltas are consistent: DS `_1`→`_2` fp +6 = the extra
  instruction (+2 hd DS) shifting the read while the in-line delta is +4 hd
  (the enable anchor absorbs the rest).

## What stage 2 shipped (verdict-preserving; all gates green)

1. `Ppu::read_pos_hd()` = `2*dot + dhalf` — THE read-position API; `sub_dot()`
   consumed (dead_code dropped).
2. `Ppu::isr_read_carry_hd()` — the per-ISR carry (m2 +4 / m0 +2 SS, m0 −4 DS)
   extracted from the PORT-1 law; single source for stage 3's unified
   comparison.
3. `read_deferred` documented as the `GB_display_sync` analogue (the #11ba
   grain already resolves the PPU to the read's exact half-dot; the sample IS
   the true-half-dot state) + `dh=`/`mclk=` trace fields.

Gates at commit: 36 tier2 pins · lib 660 · mooneye flag-on 91/91 ·
full gbtr OFF · two-bin ON unchanged (411).

## Implication for stage 3

The read side needs NO further motion: `read_pos_hd + isr_read_carry_hd` is
the read's true position in slopgb's frame, and slopgb-frame boundary
constants relate to SameBoy's by the uniform +8 hd. Stage 3's single
comparison is `read_pos_hd + carry < exit_hd` with `exit_hd` the emergent
per-config visible exit in slopgb's frame (SameBoy exit − 8 hd).
