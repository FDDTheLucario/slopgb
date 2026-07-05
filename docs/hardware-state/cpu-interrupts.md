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
