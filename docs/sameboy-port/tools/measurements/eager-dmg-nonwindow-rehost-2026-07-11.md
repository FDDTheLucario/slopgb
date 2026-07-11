# The eager DMG non-window re-host — 3 FF0F BUG rows SHIPPED, the FF41/dispatch/welded families REFUTED (2026-07-11, #11dj)

Ports the non-window DMG re-host vein of the eager C3-flip: the 12 candidate
bar rows that pass tier2 (deferred-portable) but fail the eager clock. **3 of
the 12 recovered — all 3 are SameBoy-PASS BUG rows, ZERO regressions on either
model. The other 9 are refuted with measured traces (an A/B trade, dispatch
frame, or welded render flip-dot).** EV DMG **69 → 66**.

## Baselines reproduced (exact, base `finish-port-halfdot @ 732e4ee`)

`flagon_probe`: OFF DMG **103**, EV DMG **69**, tier2 DMG **116**, OFF CGB 486,
EV CGB **318**, tier2 CGB **291**. All exact.

## SHIPPED — the DMG FF0F IF-clear write-conflict borrow (`interconnect/bus.rs`)

The #11dd CGB write-conflict borrow (`Bus::write`: after `tick_machine`, borrow
the next M-cycle's first PPU dot so a `GB_CONFLICT_WRITE_CPU` engine write
commits at the WriteCpu dot D+1, folding a co-instant STAT rise into `intf`
FIRST) was scoped `self.model.is_cgb()`. **DMG is single-speed with the same
4-dot M-cycle and 1-T WriteCpu commit as CGB SS**, so the identical borrow
re-hosts the DMG FF0F IF-clear straddle — un-scoped from is_cgb, but **FF0F
only** (the `borrow_addr` split):

```rust
let borrow_addr = if self.model.is_cgb() {
    matches!(addr, 0xFF0F | 0xFF41 | 0xFF45)
} else {
    addr == 0xFF0F
};
```

The FF0F squash arm (`arm_ff0f_if_squash`) already keys off `borrow`, so the DMG
FF0F case now arms it too — needed for the `lycint152_lyc153irq_ifw_2` recovery.
`eager_value`-gated + `!double_speed` + `!lcd_shift_active` → production/tier2
(flag off / early-returned) + CGB byte-identical.

**3 BUG rows recovered** (`classify_dmg.py` → 3 BUG / 0 FLOOR):

```
m2int_m0irq/m2int_m0irq_scx3_ifw_2   want 0 (was 2)   FF0F IF-clear straddle
m2int_m0irq/m2int_m0irq_scx3_ifw_4   want 0 (was 8)   FF0F IF-clear straddle
ly0/lycint152_lyc153irq_ifw_2        want E0 (was E2)  line-153 LYC IF-write
```

## REFUTED — the FF41/FF45 WriteCpu borrow does NOT cross to DMG (measured A/B trade)

The first cut un-scoped the borrow for all three registers (FF0F|FF41|FF45).
Measured EV DMG 69 → 64, but **NOT zero-drop**: it recovered 9 and broke 4.

- **Un-scoped (FF0F|FF41|FF45)**: recovers 9 (3 FF0F + 6 FF41), breaks 4 —
  including **`m0enable/lycdisable_ff45_3` which is a SameBoy-PASS BUG**
  (`classify_dmg.py`: sb=2=want, borrow makes it 0). A dropped BUG → fatal.
- **FF0F|FF41 (drop FF45)**: recovers 9, breaks 3 — `m0enable/disable_scx3_2`,
  `disable_scx7_2`, `lycdisable_ff41_2`, all **FLOOR** (SameBoy-fails-want) but
  all currently green vs gambatte, so 3 new flagon fails. The FF41 recovered set
  is 5 BUG + 1 FLOOR (`m1/lyc143_late_m2enable_lycdisable_2`, sb=3≠want=1 — a
  move AWAY from SameBoy).

The FF41/FF45 WriteCpu commit on DMG is a one-sided A/B swap: the borrow that
helps the `lycEnable`/`m2enable` STAT-enable rows regresses the `m0enable`
lyc-disable siblings (the classic floor-class trade — a "fix" that flips
now-green siblings). It is NOT a clean whole-dot borrow like FF0F; it needs the
coupled treatment. **Parked** — FF0F-only ships zero-drop.

## REFUTED — the dispatch-frame rows (counter-pinned, C3-flip Part-A)

- `enable_display/frame1_m2stat_count_2` (want 90): EV got **00** (full-blank) —
  the mode-2 STAT interrupt counter never increments on the LCD-enable frame.
  That is the eager dispatch count on the enable glitch line, not a read peek.
- `ly0/lycint152_ly0stat_3` (want C2, EV C0): the S5DBG trace shows this row's
  IRQ is a `dispatch ly=152` LYC dispatch; the ly0 STAT-mode readback diverges by
  the mode bit (C2 vs C0). Read-frame is welded to the counter-pinned dispatch.

Both are the counter-pinned dispatch residual (the C3-flip Part-A retime), not
read-peek-portable — a `|| eager_value` on a read law cannot move a dispatch
count. Parked with the dispatch core.

## REFUTED — the welded / coupled render-flip-dot rows (per #11dg)

- `oam_access/postwrite_2_scx3` (want 1), `vram_m3/postread_scx3_2` (want 0) are
  the DMG siblings of the CGB rows #11dg PROVED welded to the `vis_early`/
  `vis_exit_hd` render flip-dot (extending the eager `vis_early` release is
  net-negative — breaks `intr_2_*_sprites`). Genuine Part-A.
- `m2int_m3stat/scx/late_scx4_2` (want 0) is the classic coupled-atomicity row
  (render-length ∧ read-position) — the render-length and read-position each an
  A/B swap, both required (the #11az `late_scx4` proof). Genuine Part-A.

## Gates (all green)

1. `golden_fingerprint` — byte-identical (42s).
2. EV DMG **69 → 66** ↓; EV CGB **318** unchanged; tier2 DMG **116** + tier2 CGB
   **291** unchanged.
3. Zero-regression A/B (`comm`): DMG recovered 3, **new-fails EMPTY**; CGB fail
   count 318 unchanged (CGB `Bus::write` path untouched).
4. mooneye `acceptance/ppu` green on all three clocks (off / `MOONEYE_RECLOCK` /
   `MOONEYE_EAGER`) — includes intr_2 mode0/mode3/sprites both models.
5. eager intr_2 gbtr pins (`tier2_intr_2_*`) PASS.
6. clippy `-D warnings` clean; `bus.rs` 299, `eager_web.rs` 225 (< 1000);
   `read_laws.rs` untouched (998).
7. Red-before-green pin `gambatte::eager_web::eager_dmg_ff0f_write_commit_passes`
   (3 rows) — FAILS with the DMG FF0F scope neutered, PASSES with it.

## Files

- `interconnect/bus.rs` — the `borrow_addr` DMG FF0F split.
- `tests/gbtr/gambatte/eager_web.rs` — the pin `eager_dmg_ff0f_write_commit_passes`.

## Endgame after #11dj

The DMG non-window read-frame vein is drained to the clean FF0F borrow. The
remaining 9 candidates are: the FF41/FF45 DMG A/B trade (needs coupled
treatment), 2 counter-pinned dispatch rows (C3-flip Part-A), and 3 welded/coupled
render-flip-dot rows (#11dg-class Part-A). All land with the coherent per-T
retime, not a flag-gated read-law port.
