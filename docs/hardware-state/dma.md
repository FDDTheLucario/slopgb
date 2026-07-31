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
- The **interrupt dispatch holds** the steal (`Bus::set_dispatching`, the `dma_dispatch_hold` flag): SameBoy calls `GB_hdma_run` only from `GB_cpu_run`'s run branch after an opcode fetch, never inside the dispatch branch, so a block flagged while the dispatch is pushing PC waits for the handler's first opcode fetch and copies the *pushed* bytes (`irq_precedence/late_hdma_vs_{ei,ie,tima}`, whose HDMA1/2 point at the pushed stack slot; `hdma_vs_m0_scx2`). Scoped to the running CPU's dispatch — extending the hold to the halt-wake dispatch trades `late_hdma_vs_tima_scx{1,2}_halt_2` for their `_1` siblings and `hdma_vs_m0_scx2_halt`, all three currently green.
- Each service ends with one teardown M-cycle.
- The 16-bit dest counter terminates at the 0x10000 crossing — no VRAM wrap.
- Enabling with the LCD off copies one block immediately.
- An LCD disable kills an armed transfer but leaves FF55 reading active.

### Parked / disproven

- **Disproven: a longer VRAM-DMA service.** The `{g,h}dma_cycles*_2` rows read `STAT & 3` one M-cycle after their `_1` siblings and want mode 0 where we still read mode 3, and the SameBoy trace puts its FF41 read 4 dots later than ours on the same line — but padding the service with extra teardown M-cycles is monotonically worse over `gambatte/dma/` (72 → 82 → 88 → 92 failures for +0..+3 M-cycles): it recovers the plain `long`/`hdma` rows and breaks the `_scx{2,3,5}` siblings plus the `hdma_late_*` family. The residual is the service's *placement* against the CPU's own fetch, not its length.
- **Disproven: retiming the HBlank trigger anchor.** `hdma_late_disable[_scx*]_2` (want 1) need the block flagged BEFORE their cancel write: SameBoy sets `hdma_on` at its HBlank entry (`cfl` 257 on an SCX=0 line) with the `_1` cancel landing at 256 and the `_2` cancel at 261, while our `hdma_lead` rises at dot 255 — after both cancels, so the disable kills the block on both rungs. Leading the trigger (firing `hdma_lead` at `lx 159 - K`) was swept K = 0..7 over all 458 `gambatte/dma/` rows: 72, 73, 72, 75, 82, 83, 86, 89 — and single-speed-scoped (K = 1..4): 73, 73, 75, 78. Every K is a trade, not a lift: K = 2 recovers `hdma_late_disable_scx{2,3}_2` and four speedchange rows but breaks `hdma_late_disable[_scx5]_ds_1/2`, `hdma_late_m3halt_m2unhalt_*` and the GREEN `hdma_start_scx{2,3}_1`. A uniform anchor shift moves every row equally, so the `_1`/`_2` pairs never separate — the discriminator has to be a latched write dot or render-FSM term, not the anchor.
- **Disproven: SameBoy's whole placement (service only after an opcode fetch).** It recovers the `late_hdma_vs_*` family and `hdma_{start_ly0,pc_7ffe}` but breaks the M-cycle-granular races the head placement models (`hdma_late_{destl,length,wrambank}_2`, `hdma_start[_scx*]_2`, `hdma_transition_{oamdma,halt_hdmadst_unhalt}`) — ~15 recovered for ~11 lost. Only the dispatch-hold half of it is kept above.
- **Parked: SameBoy-derived VRAM wrap** — the old wrap behavior; superseded by terminating at the 0x10000 crossing (no VRAM wrap), per gambatte `dma_dst_wrap_2`.
- **Parked: chasing the residual `_2`/`a-phase` parity rows with whole-dot timing** — these are documented swaps in `baselines/gambatte.txt`; they need sub-M-cycle phase, so whole-dot timing won't close them.
