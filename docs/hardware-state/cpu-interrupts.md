# CPU, interrupts, halt, speed switch

## Interrupt sampling (frozen)

- CPU interrupt sampling is FROZEN: sampled at end of opcode fetch; dispatch aborts the fetched instruction (mooneye-gb prefetch semantics).
- Recalibrate dependents (PPU IRQ anchors); do **not** move the sampling.

## Dispatch IF sample position

`cpu/execute.rs` `dispatch_interrupt`, after SameBoy `sm83_cpu.c`'s interrupt
block:

- **IE** is latched right after the PC-high push, so a high push landing on
  FFFF cancels or redirects the dispatch and a low push onto FFFF is already
  too late (mooneye `interrupts/ie_push`).
- **IF** is sampled one M-cycle later, *after* the PC-low push. A source whose
  flag rises inside that push cycle therefore still wins the vector:
  `irq_precedence/late_m0irq_vs_tima_scx{2,3}[_halt]_1` (out4) need the mode-0
  STAT rise to beat the already-pending TIMA overflow, and their `_2` siblings
  (out2, the write block one M-cycle later) need it not to.
- A low push onto **FF0F** reads the pre-push flags for the vector choice and
  the acknowledge then clears its bit out of the *pushed* value
  (`late_if_via_sp_if_1`, outFD = pushed `$7F` less bit 1).
- The acknowledge follows the low push. SameBoy clears IF two T-cycles into
  that M-cycle; the windows below are the remainder.
- The whole dispatch holds any flagged VRAM-DMA block — see
  [`dma.md`](dma.md).

## Dispatch-ack source sync-ahead

`Interconnect::ack` + `ack_squash_*` (gambatte memory.cpp `Memory::ackIrq`). The dispatch's IF clear syncs the acked bit's source slightly past the ack point first, so a hardware re-set due just after the clear is consumed by it.

Per-source sync-ahead point:

| Source | Sync-ahead (gambatte `ackIrq`) |
|---|---|
| Serial | `updateSerial(cc+3+isCgb())` |
| Timer (TIMA) | `updateTimaIrq(cc+2+isCgb())` |
| LCD (STAT/VBlank) | `lcd_.update(cc+2)` |

In our grid, the following are swallowed when their bit was just acked — and only the acked source is swallowed; others just get flagged:

- Timer/serial sets produced by the machine tick after the ack on CGB/AGB, none on the DMG family — `ack_squash_ticks`.
- STAT/VBlank rises inside the LCD squash window `ack_squash_dots`: **2** dots at single speed (the two T-cycles left of the low push's M-cycle), **6** on DMG when the ack lands on line 0 or 153 or on a line's first dot — the line-start LYC/OAM emissions there sit one M-cycle past our ack where SameBoy's clear already covers them (`ly0/lycint152_lyc{0,153}irq_late_retrigger`, `lyc153int_m2irq_late_retrigger`, `m1/lycint{143_m1irq,_vblankirq}_late_retrigger`) — **1** in double speed and **2** there when HBlank is the enabled STAT source (`Ppu::stat_src_hblank`, the `late_m0irq_retrigger` family).

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

**Parked: masking the whole CGB M-cycle's commits (halt-wake-phase fix)** — probed and DISPROVEN: it is a DOUBLE-COUNT of gambatte's `cc+=4`, because our natural CGB wake already lands at gambatte's post-+4 phase (the seam $8000 read is dot-for-dot identical). Don't pursue a halt-wake-phase fix.

The 6 baselined CGB `halt/` rows (`m1int_ly_2`, `lycirq_m2stat_2`, `m0int`/`m0irq_m0stat_scx{2,3}_ds_2`) actually fail READ-side (CGB getLyReg LY+1-near-boundary + getStat line-start mode-2/3), entangled with the A/B-swept CGB-C LY/STAT timeline + the parked mode-3 +1-dot — see the class-H index note in `tests/gbtr/baselines/gambatte.txt`.

### Sub-M-cycle wake peek (`Interconnect::halt_wake_mid_impl`, CGB single-speed)

The halt idle loop samples the wake once per **whole** M-cycle, and the PPU
commits the mode-0 STAT IF at the END of the M-cycle containing the flip, so two
lines whose `projected_flip_dot` differ by <4 dots (an `SCX&7` delta) commit —
and wake — at the same boundary, collapsing the sub-M-cycle wake instant the
hardware separates. The 5 CGB halt bar rows
(`late_m0int_halt_m0stat_scx{2,3}_3a`, `late_m0irq_halt_dec_scx{2,3}_2`,
`late_m0irq_halt_m0stat_scx3_3b`) turn on that instant. It is restored by two
coupled **pure value peeks** — no machine advance, timer-safe:

- **`Ppu::m0_stat_flip_reached`** (`interconnect/speed.rs` wake): OR `IF_STAT` in
  when `self.dot ∈ [flip, flip+4)` (flip = `flip_dot`/`projected_flip_dot`), so
  the wake lands at the flip's M-cycle boundary rather than at the M-cycle-end IF
  commit — a flip projected at dot 256 wakes at 256, one at 257 wakes at 260, so
  the resumed stream and its FF41 read separate by the `SCX&7` delta. The `+4`
  upper bound stops it re-firing on the stale flip after the IME=1 halt rewind.
- **`Ppu::halt_refetch_read_override`** (applied at `regs.rs` FF41): the armed
  `halt_refetch` flag makes the IME=1 dispatch's first FF41 read return mode 2
  once `read_pos_hd >= LINE_DOTS*2` (SameBoy's cc+4 re-fetch in the next line's
  OAM); one-shot, cleared at the boundary read / next halt entry.

The two are **coupled**: the wake peek separates the read position (want-0 `_a`
wakes one M-cycle early → `read_pos_hd` 904 < 912 → stays mode 0), which is what
leaves the override collateral-free — either peek on its own drops a
SameBoy-pass row. `halt_wake_mid_impl` carries only this CGB single-speed arm
(`is_cgb() && !double_speed`); the DMG half-M-cycle halt sampler SameBoy runs is
**not modelled** — `Bus::pending_halt_wake_mid`'s default is the plain
end-sampled view. Distinct from the parked CGB whole-cycle mask above.

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

## Parked: moving the dispatch dot off the read frame

Four approaches to separating the IRQ-dispatch dot from the read frame were each
built and measured during the SameBoy port, and each dropped a test SameBoy
passes. Kept here so none is re-attempted; the eager clock landed without them.

- **Advancing the PPU +4 at dispatch, alone.** Hangs mooneye `intr_2_*`
  (B=42) — the dispatch dot is counter-pinned by those tests and cannot move
  independently of the frame the ISR reads.
- **Imminent-rise fold** (dispatch a cycle late, reads at the leading edge). Won
  presence rows but lost dispatch-*count* rows plus the same `intr_2` hang: the
  two are incoherent, and on the shared mode-0 rise there is no bus-observable
  discriminator between a row that wants the move and one that does not.
- **Eager-PPU / deferred-CPU split.** Built in full and refuted: it traded
  coherent-count rows the wrong way and cost mooneye passes.
- **Folding the timer/serial completion into the read.** Even the minimal
  `FF0F`-OR fold drops `gambatte/tima/tc00_late_div_write_if_1a`. The timer
  read-state is identical for rows with opposite wants, so no read-time
  discriminator exists.

Two sweeps in the same area also measured inert: the power-on DIV offset across
{−4..12} changes nothing, and reading `FF0F` a cycle late is refuted.

Before concluding that a surviving row here is structurally welded, read
`.claude/skills/rom-diff-weld/SKILL.md` — that method recovered rows several
investigations had each called a floor.
