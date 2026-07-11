# CPU, interrupts, halt, speed switch

## Interrupt sampling (frozen)

- CPU interrupt sampling is FROZEN: sampled at end of opcode fetch; dispatch aborts the fetched instruction (mooneye-gb prefetch semantics).
- Recalibrate dependents (PPU IRQ anchors); do **not** move the sampling.

## Dispatch-ack source sync-ahead

`Interconnect::ack` + `ack_squash_*` (gambatte memory.cpp `Memory::ackIrq`). The dispatch's IF clear syncs the acked bit's source slightly past the ack point first, so a hardware re-set due just after the clear is consumed by it.

Per-source sync-ahead point:

| Source | Sync-ahead (gambatte `ackIrq`) |
|---|---|
| Serial | `updateSerial(cc+3+isCgb())` |
| Timer (TIMA) | `updateTimaIrq(cc+2+isCgb())` |
| LCD (STAT/VBlank) | `lcd_.update(cc+2)` |

In our grid, the following are swallowed when their bit was just acked — and only the acked source is swallowed; others just get flagged:

- Timer/serial sets produced by the next machine tick (next two on CGB/AGB).
- STAT/VBlank rises in the first 2 dots of the next tick (both families, both speeds — in ds that's the whole tick, which is what flips the `*_late_retrigger_ds_2` rows).

Pins the gambatte `*_late_retrigger_2/3` model splits: tima tc00 dmg08_outE4 / cgb04c_outE0, serial trigger_int8, irq_precedence late_m0irq.

**Don't** widen the LCD window to the line-anchored rises' single-speed second-half emission dots:

- m2int_m2irq_late_retrigger_1 + late_m0irq_retrigger_scx1_1 pin the keeps.
- m2int_m2irq_late_retrigger_ds_1 is the one documented ds swap.
- The single-speed lyc/m1/m2-synced retrigger rows still in the baseline ride on their sync-IRQ's own one-cycle anchor (PPU event ordering), not on this window.

## Halt wake sampling

Halt wake uses a separate, earlier intra-cycle sample (`Bus::pending_halt_wake`, both IME states):

- A timer IF committed in the second half of the M-cycle is missed for one cycle (SameBoy `GB_cpu_run`).
- The STAT bit joins the mask per event (the PPU's dot-0 pulse commits, via `take_stat_halt_late`), **not** wholesale — masking other PPU bits breaks mooneye `intr_2_*` / `halt_ime1_timing2-GS`.
- The CGB/AGB start-of-cycle staleness for first-half PPU commits stays unmodelled (gambatte `halt/*_cgb04c` split rows) pending a per-model widening of the mask.

**Parked: masking the whole CGB M-cycle's commits (halt-wake-phase fix)** — AXIS-1 probed + DISPROVEN 2026-06 (workflow wcwot9hvs, instrumented vs gambatte). It is a DOUBLE-COUNT of gambatte's `cc+=4`: our natural CGB wake already lands at gambatte's post-+4 phase (the seam $8000 read is dot-for-dot identical). Don't pursue a halt-wake-phase fix.

The 13 "halt" rows actually fail READ-side (CGB getLyReg LY+1-near-boundary + getStat line-start mode-2/3), entangled with the A/B-swept CGB-C LY/STAT timeline + the parked mode-3 +1-dot — see the class-H index note in `tests/gbtr/baselines/gambatte.txt`.

### Eager sub-M-cycle wake clock (`eager_value`, CGB single-speed) — #11dl

The eager halt idle loop samples the wake once per **whole** M-cycle, and the
PPU commits the mode-0 STAT IF at the END of the M-cycle containing the flip, so
two lines whose `projected_flip_dot` differ by <4 dots (an `SCX&7` delta) wake at
the same boundary — collapsing the wake **instant** tier2's 4k+2 sample
resolves. The 5 CGB halt bar rows (`late_m0int_halt_m0stat_scx{2,3}_3a`,
`late_m0irq_halt_dec_scx{2,3}_2`, `late_m0irq_halt_m0stat_scx3_3b`) turn on that
instant. Recovered (+14/−0, all SameBoy-PASS) by two coupled **pure value
peeks** — no machine advance, timer-safe:

- **`Ppu::m0_stat_flip_reached`** (`interconnect/speed.rs` wake): OR `IF_STAT` in
  when `self.dot ∈ [flip, flip+4)` (flip = `flip_dot`/`projected_flip_dot`), so
  the wake lands at the flip's M-cycle boundary (`pfd256`→256, `pfd257`→260) —
  tier2's sub-M-cycle instant. The `+4` upper bound stops it re-firing on the
  stale flip after the IME=1 halt rewind.
- **`Ppu::halt_refetch_read_override`** (applied at `regs.rs` FF41): the armed
  `halt_refetch` flag makes the IME=1 dispatch's first FF41 read return mode 2
  once `read_pos_hd >= LINE_DOTS*2` (SameBoy's cc+4 re-fetch in the next line's
  OAM); one-shot, cleared at the boundary read / next halt entry.

Coupled: the sub-M-cycle wake **separates the read position** (want-0 `_a` wakes
one M-cycle early → `read_pos_hd` 904 < 912 → stays mode 0), so the override has
zero collateral — where the entry peek alone (#11cw/#11cy) or the read shift
alone (#11cz, −9) each dropped a SameBoy-pass row. Map:
`docs/sameboy-port/tools/measurements/eager-wake-clock-port-2026-07-11.md`. This
is distinct from the DMG deferred sub-M-cycle sampler (`halt_wake_mid_impl`, the
tier2 `!is_cgb` mid-block) and from the parked CGB whole-cycle mask above.

## HALT/STOP clock gating

- HALT/STOP gate the CPU core clock via `Bus::set_halted`, engaging only *after* the post-HALT prefetch M-cycle; the OAM DMA engine freezes with it.
- While frozen, the OAM-scan freeze glitch is model-dependent:

| Model | Frozen-OAM-scan glitch |
|---|---|
| MGB | PPU's OAM scan renders the glitch sprite (`test-roms-src/madness/mgb_oam_dma_halt_sprites.s`) |
| Other models | unreferenced — they keep the $FF scan disconnect, which gambatte's dmg08-verified oamdma_late_halt_stat rows pin for selection |

- HBlank DMA also never proceeds while the gate is on: a pending block defers (`Interconnect::halt_hdma`, gambatte `haltHdmaState_`) and re-fires at the wake without its teardown M-cycle.

## CGB speed switch (STOP, KEY1 armed)

The whole tail lives in `Bus::stop(skipped_addr, interrupt_pending)`:

- The skipped byte costs a real read M-cycle.
- DIV resets, committing like a write in that slot (the gambatte tima/div a/b pairs pin the cell; `Apu::div_write` carries the frame-sequencer edge).
- The CPU then pauses 0x7FFF more M-cycles on the *new* clock while PPU/APU/timer run on.

Pause length — competing models:

| Approach | Pause length | Status |
|---|---|---|
| gambatte `cc+0x20000+4` | 0x7FFF more M-cycles on the new clock; leaving double speed costs twice the dots | Do (correct for cgb04c) |
| SameBoy flat 0x20008 | — | Parked: wrong for cgb04c |

Early-exit / pending-IRQ rules:

- IE&IF ends the pause early.
- A pending IRQ skips read+pause entirely (SameBoy gate, age spsw).

Pending HBlank block across the switch (`hdma_transition_speedchange` matrix):

| Transition | Pending HBlank block |
|---|---|
| Entering DS | aborts the block (FF55 \|= $80, count latched) |
| Leaving DS | defers it |
