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

- **Disproven: a longer VRAM-DMA service.** Padding the service with extra teardown M-cycles is monotonically worse over `gambatte/dma/` (72 → 82 → 88 → 92 failures for +0..+3 M-cycles). Direct measurement then settled it: our GDMA spans exactly 36 dots for 16 bytes (`gdma_cycles_short`, ly 153 dot 68 → 104) and 4100 dots for 2048 (`gdma_cycles_long`, ly 144 dot 64 → ly 153 dot 60), matching SameBoy's `2 + 2N + 2` cc to the dot. The service length is CORRECT; the `_cycles` residual is not in it.

#### What the `{g,h}dma_cycles*_2` rows actually constrain (2026-07-31)

Disassembled: the three variants are **not one ladder** — they anchor on three
different LYC lines (`gdma_cycles_short` LYC=$99, `gdma_cycles_long` LYC=$90,
`hdma_cycles` LYC=$01), and only `hdma_cycles` uses the HBlank path at all. Each
ISR arms its transfer, runs a per-variant NOP sled and reads `STAT & 3` once
(`n = 1` FF41 read per run; the second `vis_mode_read` evaluation per read is the
leading-edge/trailing pair of the same access, and the leading one wins).

SameBoy lands **every** variant's read at the same place — `cfl` 256 (mode 3) for
`_1` and 261 (mode 0) for `_2`. Ours splits by anchor line: `gdma_cycles_short`
(LYC 153) reads at dot 248/252 and passes both rungs, while `gdma_cycles_long`
(LYC 144) reads at 244/248 — four dots earlier — and fails `_2`. Since the two
transfers' durations are exact, that four dots is the **LYC=144 versus LYC=153
STAT dispatch dot**, not anything in the DMA.

`gdma_cycles_long_2` and `gdma_cycles_short_1` are bit-identical in every tracked
render quantity at their read — ly 0, dot 248, native mode 3, `render.active`,
`!line_render_done`, both resolving through the native-mode path because
`vis_exit_hd` returns `None` on line 0 — and they want OPPOSITE answers. The
discriminator is therefore outside the render FSM, and it is the anchor line.

The two halves also take different code paths, so no single exit constant covers
them: the `gdma_cycles` rows are decided by the native `vis_mode()`, the
`hdma_cycles` rows by the `read_pos_hd() < vis_exit_hd()` comparison. For the
`hdma_cycles` half alone the four SCX rungs pin the boundary uniquely — reads at
244/248 (SCX 0, 2, 3) and 248/252 (SCX 5) with `exit = 510 + 2·SCX` half-dots
force the demanded read boundary to `B(SCX) = 245 + SCX`, against our 251 + SCX,
i.e. our CPU-visible exit is 6 dots late *for this anchor*.

#### What the `hdma_start` / `hdma_late_disable` rows actually constrain (2026-07-31)

Disassembled rather than swept. Both families share ONE kernel: the LYC=1 STAT
ISR arms `HDMA5 = $80` at `$1000`, then a NOP sled, then a single observation.
`hdma_late_disable` observes with `xor a; ldh ($55),a` (cancel); `hdma_start`
observes with `ld a,(hl)` from VRAM `$8000`. Both print `observed & 7`, with
`$8000` preset to `$00` and the DMA source `$C000` = `$01`, so **0 = readable and
untransferred · 1 = transferred · 7 = the read was BLOCKED** (`$FF & 7`).

The `scx` variants are not the same rung: they add `ld a,N; ldh ($43),a` before
the `HALT` *and* one extra sled NOP, so their observation sits one M-cycle later
than the `scx0` rung while their mode 3 lengthens by `SCX`.

`hdma_start_1` reads **7**, so it is not a DMA-timing row at all — it fails on
VRAM accessibility. Measured on line 1 (SCX=0, mode 3 nominally ending at 252):

| quantity | ours | demanded | Δ |
|---|---|---|---|
| VRAM read release (`line_render_done`) | dot 253 | ≤ 252 (`hdma_start_1` must read `$00`, not `$FF`) | −1 |
| HBlank DMA trigger (`hdma_lead`, lx 159) | dot 255 | 249..252 (after `hdma_late_disable_1`'s cancel at 248, at or before `_2`'s at 252) | −3..−6 |
| block serviced in the observing M-cycle | yes, for reads | must NOT be, for `hdma_start_1` | read-side asymmetry |

The two anchors need *different* shifts, which is why a uniform lead cannot land
them: swept K = 0..7 over all 458 `gambatte/dma/` rows (72, 73, 72, 75, 82, 83,
86, 89) and single-speed-scoped K = 1..4 (73, 73, 75, 78), every K trades — K = 2
recovers `hdma_late_disable_scx{2,3}_2` and four speedchange rows while breaking
`hdma_late_disable[_scx5]_ds_*`, `hdma_late_m3halt_m2unhalt_*` and the GREEN
`hdma_start_scx{2,3}_1`.

The third row of the table is the real blocker, and it is a genuine conflict:
`hdma_late_disable_2`'s cancel and `hdma_start_1`'s read both land at dot 252,
and the cancel must see an already-triggered block (SameBoy's `GB_IO_HDMA5`
cancel clears only `hdma_on_hblank`, never a pending `hdma_on`, so the block
still runs — traced: cancel at `cfl` 261, run after it) while the read must not
be stolen from. Inverting the same-cycle service asymmetry was measured both
ways: the full swap (reads stop yielding, writes start) scores 80, and dropping
only the read-side post-tick service scores 75 — the read side is pinned by
`hdma_start[_scx{2,3,5}]_2` and the write side by
`hdma_late_{destl,length,wrambank}_1`, and neither even recovers `hdma_start_1`.
Both directions are load-bearing, so the split is below the M-cycle: the FF55
write commits in its cycle's second half while the VRAM read samples at the end.

#### The HBlank service lag (2026-08-01)

The "genuine conflict" above was derived with the STAT dispatch 4 dots early — see
[`cpu-interrupts.md`](cpu-interrupts.md) "The LYC-anchor halt-wake dot". Traced
against SameBoy on the shared LYC=1 kernel, in `cfl` on line 1, SCX 0:

| event | SameBoy | ours | Δ |
|---|---|---|---|
| STAT dispatch (`SBACK`) | 30 | 24 | −6 |
| `HDMA5 = $80` arm | 64 | 60 | −4 |
| `hdma_late_disable_1` cancel | 256 | 252 | −4 |
| mode-0 entry | 257 | 251 | −6 |
| `hdma_late_disable_2` cancel | 261 | 256 | −5 |
| block runs (`SBWHDMA run`) | 264 | 256 | −8 |

Every event is a rigid −4..−6 translate of SameBoy's except the service, which is
−8: SameBoy runs the block **two** M-cycles after the trigger, we run it at the
head of the next bus operation. Both cancels straddle the trigger the same way in
either emulator, so the trigger dot itself is right — dropping the `hdma_lead`
one-dot lead recovers nothing. What is missing is the extra M-cycle between
flagging and stealing the bus, which is exactly what keeps `hdma_start_1`'s VRAM
read out of the block's cycle.

Holding a flagged HBlank request one extra M-cycle before `service_vram_dma` takes
it, **together with** the pure-LYC halt-late law, scores **+17/−11** and recovers
`hdma_start[_ds,_ly0,_scx5,_scx5_ds]_1` — the `ours = 7` blocked-read rows — plus
the four `lyc-anchor` SCX-0/5 rungs and five `hdma_late_disable/enable_2`. The
eleven lost are currently green: `irq_precedence/late_hdma_vs_{ei,ie,tima}_scx{1,2}`,
`hdma_vs_m0_scx1`, `hdma_transition_{oamdma_2,halt_hdmadst_unhalt}` and
`hdma_late_m3speedchange_tima_scx1_ds_{5,6}`.

- **Disproven: the loss is the dispatch hold double-counting.** The hold defers
  the steal to the handler's first opcode fetch, so a wait stacked on top of it
  would move the copy off the pushed stack slot those ROMs read back. Probed at
  the trigger on `late_hdma_vs_tima_scx1_1`, `hdma_vs_m0_scx1` and
  `hdma_transition_oamdma_2`: `dma_dispatch_hold` is **false** in all three (the
  trigger fires at line dot 256, outside any dispatch), and a rule that zeroes the
  wait under a hold scores the same +17/−11, changing no row. Do not build it.
- The residual is a frame offset, not a wait: our service lags mode-0 entry by 5
  dots against SameBoy's 7, so the true correction is **2 dots**, not a whole
  M-cycle. Two dots is not expressible by holding the request (M-cycle granular)
  and moving `hdma_lead` instead breaks the cancel race — the trigger would land
  past `hdma_late_disable_2`'s cancel dot, which must precede it. Closing this
  needs the line-1 timeline's whole −4..−6 offset resolved, not another lever.

- **Disproven: SameBoy's whole placement (service only after an opcode fetch).** It recovers the `late_hdma_vs_*` family and `hdma_{start_ly0,pc_7ffe}` but breaks the M-cycle-granular races the head placement models (`hdma_late_{destl,length,wrambank}_2`, `hdma_start[_scx*]_2`, `hdma_transition_{oamdma,halt_hdmadst_unhalt}`) — ~15 recovered for ~11 lost. Only the dispatch-hold half of it is kept above.
- **Parked: SameBoy-derived VRAM wrap** — the old wrap behavior; superseded by terminating at the 0x10000 crossing (no VRAM wrap), per gambatte `dma_dst_wrap_2`.
- **Parked: chasing the residual `_2`/`a-phase` parity rows with whole-dot timing** — these are documented swaps in `baselines/gambatte.txt`; they need sub-M-cycle phase, so whole-dot timing won't close them.
