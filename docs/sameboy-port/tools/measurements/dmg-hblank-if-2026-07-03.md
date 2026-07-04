# DMG hblank_int IF-delivery two-latch — SHIPPED (+16) (2026-07-03, #11bk)

The §3b `hblank_int`/`hblank` DMG rows re-measured and the **mode-0 STAT-IF
read-frame two-latch BUILT and SHIPPED flag-gated** — `+16` gbmicrotest DMG
rows, zero SameBoy-pass drops, CGB two-bin 291/291 zero-drift, mooneye 91/91
flag-on AND flag-off, production byte-identical OFF. The #11bj "atomic /
single-edge-peek" verdict for the `if_c`/`if_d` legs is **corrected**: the
read frame decouples cleanly (like `vis_mode_read`), it just needed the
DELIVER *and* SERVICE-CLEAR edges (the two-latch), not one peek.

## The ground truth (all measured this session, not assumed)

94 `hblank*` DMG rows: **OFF 94/94 pass** (production), **ON 51/94** (43
reclock regressions — all pass OFF, so every one is a flip-blocker the
reclock introduces, not a slopgb-OFF bug). The 43 by mechanism:

### The `if_a/b/c/d` ladder — the mode-0 STAT-IF read frame (uniform scx0-7)

Each ROM: `EI; …NOP sled…; ldh a,(FF0F); DI; <verdict>`, with the STAT
mode-0 interrupt armed (`FF41=0x08`, `IE=0x02`). The read value flows into
`A`; the ISR at `$48` does `CP $XX; JR Z` (pass iff `A == XX`). Because a
raw `FF0F` read is always `≥0xE0`, the pass hinges on the read frame around
the counter-pinned mode-0 rise `R = 254 + SCX&7` (`m0rise ly=1 dot=254`,
scx0). The reclock samples the read at cc+0 (leading edge) — **4 dots before
production's cc+4 read of the same `ldh`** — so each leg reads one rung early:

| leg | read dot (scx0) | rd−R | dispatch? | want (correct/OFF) | ON got | mechanism |
|---|---|---|---|---|---|---|
| if_a | 244 | −10 | no  | E0 (empty), main `CP E0` | E0 ✓ | passes |
| if_b | 248 | −6  | **no** | E0 + dispatch (`ISR CP E0`) | E0, **main path** | dispatch LOST → PARK |
| if_c | 252 | −2  | yes | **E2** (delivered, `ISR CP E2`) | E0 | **DELIVER** → ship |
| if_d | 256 | +2  | yes | **00** (serviced, `ISR CP 00`) | E2 | **SERVICE-CLEAR** → ship |

`rd−R` is constant per leg across scx0-7 (both the read dots and `R` shift
with SCX). So ONE window fits every scx:

```
rd < R−4          → E0  (empty; if_a −9..−11, if_b −5..−7)     unchanged
R−4 ≤ rd < R      → E2  (deliver the imminent rise; if_c −1..−3)
R ≤ rd < R+4      → 00  (serviced; if_d +1..+3)
```

- **DELIVER (if_c, +8) — principled read-frame restoration.** The reclock
  read at `rd` observes the IF as of its TRUE cc+4 position `rd+4`; when
  `rd+4 ≥ R` the mode-0 bit has risen → return the STAT bit set (E2). This
  is the SameBoy "visible IF leads the dispatch" two-latch — the same fold
  `ff0f_stat_peek` arm (a) already does for CGB double-speed, generalised to
  DMG single-speed. Restores the value the cc+0 frame lost; `intf` and the
  `R` dispatch untouched.
- **SERVICE-CLEAR (if_d, +8) — the coincident-dispatch IF clear.** On
  hardware if_d's `ldh a,(FF0F)` is serviced at the read's own cycle: the
  mode-0 STAT dispatch clears IF as the read samples, so `A = 0` (the ISR
  compares `A == 0`). The reclock's deferred read committed the set bit
  instead. Return 0 when the read has crossed `R` **and the STAT interrupt is
  pending AND enabled** (`intf & ie & STAT`, the interconnect discriminator).

### The co-temporal discriminator (why the SERVICE-CLEAR needs `intf & ie`)

`hblank_scx2_if_a` polls `FF0F` at `R+1` and wants **E2** (delivered, NOT
cleared) — the OPPOSITE want to if_d at the same relative dot. It is a pure
poll: `DI` + `IE=0` (never re-enabled), no dispatch, so the bit stays set.
if_d has `EI` + `IE=0x02` → the dispatch services and clears. The
`intf & ie & STAT` gate (pending AND enabled — both known to the
interconnect) separates them exactly; the clear WITHOUT the gate dropped
`hblank_scx2_if_a` (measured). This is the "opposite-edge = co-temporal"
case the goal names — resolved by the enable gate rather than parked.

## The 27 parked (dispatch-frame, counter-pinned — NOT read-value)

- **`if_b` (8):** the read value is ALREADY correct (E0); the *dispatch is
  lost* — the reclock's cc+0 read completes before `R`, so no interrupt
  services (main path). The read law cannot create a dispatch; moving `R`
  hangs mooneye `intr_2_*` (B=42). PARK.
- **`nops_a/b` scx1-7 (14):** the ISR reads `FF04` (DIV) and asserts its
  value — the test needs the STAT interrupt to *dispatch*; the reclock loses
  it (same as if_b). Dispatch-frame. PARK.
- **`hblank_scx3_if_a/b/c` + `hblank_scx3_int_a` (4):** a mid-family the
  reclock dispatches (or fails to) at the wrong INC-A/poll boundary —
  dispatch-timing, not a read value. PARK.
- **`hblank_int_scx7` (1):** an INC-A counter ISR (`CP 2F`, got 2E) — the
  dispatch fires one INC-A early; dispatch-frame. PARK.

All 27 land with the flip's global dispatch reclock (the same conclusion the
#11bj engine-set classification reached for the family as a whole — now
refined: the `if_c`/`if_d` READ-frame half decouples and ships; the
dispatch-frame half stays atomic).

## Build + gates

- `Ppu::ff0f_stat_peek` arm (a-dmg) DELIVER + `Ppu::ff0f_dmg_service_clear` +
  `Ppu::dmg_m0_if_rise` anchor (`ppu/stat_irq/reclock.rs`); the read path
  applies the enable-gated clear (`interconnect/cycle.rs`). All tier2 +
  `!is_cgb` + SS scoped → production and CGB byte-identical.
- **gbmicrotest DMG flag-on 409 → 425 (+16, ZERO of 513 regressed)**; hblank
  family 51 → 67. Pin `tier2_dmg_hblank_if_passes` (16 legs).
- CGB two-bin 291/291 zero-drift; mooneye 91/91 flag-on AND flag-off (B=42
  dispatch counter-pin held); lib 660; gbtr OFF 237/0; clippy clean.
