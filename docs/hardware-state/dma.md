# OAM DMA & VRAM DMA

## OAM DMA bus conflicts

Bus conflicts follow gambatte-core memory.cpp exactly (`interconnect.rs` `DmaSrcKind` + page masks). gambatte `oamdma/` is the oracle; residual clusters are commented in the baseline.

- Conflicted writes derail into the in-flight OAM slot.
- Each copied byte commits to OAM at its cycle's *end* (via `dma_pending_oam`).
- FF46 rewrites retarget the transfer mid-flight.

Per-model / per-source handling:

| Model / source | Behavior |
|---|---|
| DMG, WRAM source | wire-ANDs (via `dma_pending_oam`) |
| CGB, WRAM-region access | redirected to the FF46-bit-4 page |
| CGB, source ≥ $E0 | reads $FF |
| CGB-C | keeps 24 B of mirrored extra RAM at FEA0-FEFF |
| AGB | nibble echo |

## OAM DMA × VRAM DMA composition

Handled by `Interconnect::oam_dma_bus_capture`. While a VRAM DMA owns the bus the OAM DMA makes no source reads:

- It advances once per stolen M-cycle — on the cycle's *last* byte; every byte in double speed.
- It latches the stolen bus byte into `OAM[hdma_src & $FF]` (≥ $A0 → CGB-C extra OAM RAM), skipping its own copies for those positions.
- It is frozen entirely while the core clock is gated (gambatte memory.cpp `dma()`).

### Catch-up M-cycle on resume

Whether a resume runs one OAM-DMA catch-up M-cycle before the CPU's first post-wake cycle depends on *how* the clock was paused:

| Resume context | Catch-up M-cycle? | Source / pin |
|---|---|---|
| halt-mode wake | Yes — one catch-up M-cycle | SameBoy `GB_cpu_run` halt exit `dma_cycles=4; GB_dma_run` |
| speed-switch pause exit | No (deliberately) | gambatte `oamdmasrcC0_speedchange_readC000` pins the un-caught-up resume |

## CGB VRAM DMA

A gambatte-shaped request engine (`Interconnect::vram_dma_req`):

- FF55 is the live register; cancel latches the *written* length `| $80`.
- The dot-exact mode-0 entry — led by one dot, `Ppu::hdma_trigger_level`, gambatte xpos `lcd_hres+7` — flags one block.
- Requests steal the bus at the head of the CPU's next bus op, with reads yielding to a same-cycle trigger while in-flight writes commit first (hdma_late_destl vs hdma_start `_1`/`_2` pairs).
- Each service ends with one teardown M-cycle.
- The 16-bit dest counter terminates at the 0x10000 crossing — no VRAM wrap.
- Enabling with the LCD off copies one block immediately.
- An LCD disable kills an armed transfer but leaves FF55 reading active.

### Parked / disproven

- **Parked: SameBoy-derived VRAM wrap** — the old wrap behavior; superseded by terminating at the 0x10000 crossing (no VRAM wrap), per gambatte `dma_dst_wrap_2`.
- **Parked: chasing the residual `_2`/`a-phase` parity rows with whole-dot timing** — these are documented swaps in `baselines/gambatte.txt`; they need sub-M-cycle phase, so whole-dot timing won't close them.
