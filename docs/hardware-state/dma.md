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
- Requests steal the bus **after an opcode fetch** (`Bus::run_dma`), not at an
  arbitrary bus cycle: SameBoy calls `GB_hdma_run` only from `GB_cpu_run`'s run
  branch, so the copy is anchored to the instruction stream. A block flagged
  while the dispatch is pushing PC therefore reaches the bus at the handler's
  first fetch and copies the *pushed* bytes
  (`irq_precedence/late_hdma_vs_{ei,ie,tima}`, whose HDMA1/2 point at the pushed
  stack slot), with no separate hold needed. See "Where the HBlank block
  actually copies" for the M-cycle counts that pin it.
- The **halt wake** is the other seam: `vram_dma_unhalt` re-flags a deferred
  block and it takes the bus there, ahead of any dispatch
  (`late_hdma_vs_*_halt_1`, `hdma_vs_m0_scx2_halt`). Two consequences of the
  wake being folded into the idle cycle that observed it: the hblank window is
  re-evaluated on the wake's own sub-M-cycle view (`m0_stat_flip_reached`, the
  same one `halt_wake_mid_impl` peeked with), and the resumed opcode is
  re-sampled after the block (`Bus::refetch`) because the copy can land on the
  resume address — `hdma_transition_halt_hdmadst_unhalt` halts at `$7FFF` and
  resumes at `$8000`, inside the destination.
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

#### Where the HBlank block actually copies (2026-08-02)

Disassembled, not swept. The `hdma_late_*` and `hdma_start` rows share ONE
LYC=1 ISR (`$1000`: arm `HDMA5=$80`, NOP sled, one observation), so their
observations can be counted in M-cycles from the ISR body at `$1006` — `ldh
(n),a` commits on its third:

| row | observation | M |
|---|---|---|
| `hdma_late_disable_1` | 42 NOP · `xor a` · `ldh ($55)` cancel | 46 |
| `hdma_late_disable_2` | 43 NOP · `xor a` · `ldh ($55)` cancel | 47 |
| `hdma_start_1` | 45 NOP · `ld a,(hl)` VRAM read | 47 |
| `hdma_start_2` | 46 NOP · `ld a,(hl)` VRAM read | 48 |
| `hdma_late_{destl,length,wrambank}_1` | 43 NOP · `ld a,n` · `ldh (n)` write | 48 |

The three `hdma_late_*` rows are **two M-cycles later** than
`hdma_late_disable_1`, not the same rung. Exactly one placement satisfies all
seven: the copy lands **in M 48**, with the trigger flag already set by M 47.
`disable_1`'s cancel beats the flag and `disable_2`'s does not (the cancel races
the flag, not the copy — SameBoy's HDMA5 cancel never clears a pending
`hdma_on`); `hdma_start_1`'s read samples in M 47, before the copy;
`hdma_start_2`'s read is in M 48 and yields to it; and the `hdma_late_*` writes
in M 48 commit before it. The last two are the same M-cycle with opposite
outcomes — the read/write asymmetry [`Interconnect::service_vram_dma`] already
models at the trigger's own cycle, one cycle out.

**Resolved by moving the seam to the instruction stream.** Two bus-cycle rules
were built and measured first — copy in the trigger's own cycle after the read
(**+18/−3**, the three `hdma_late_*_1` lost) and copy in the next cycle with
writes never hosting the steal (all seven green, eight others lost) — and neither
covers both families, because the second family's count disagrees:

The eight were counted too. `late_hdma_vs_*` sets `SP = $CFF2`, pushes `$1234`
and pops it, leaving the value in the HDMA source page, then prints what the
block copied: `_1` wants `1234` (copied before the dispatch overwrote the slot)
and `_2` wants `11E9` (the pushed return address). Probed on `_1`: the copy runs
at ly 1 dot 256 with the slot still `1234`, and that cycle is the **last opcode
fetch before the timer dispatch** — deferring it one cycle lands it inside the
dispatch instead (`dma_dispatch_hold` true at dots 256 and 260, copy at 272),
where it takes the pushed PC and prints `11E9`, its own `_2` answer.

The two families demand different cycles from the same *bus* seam — M 48 for the
LYC kernel, the trigger's own cycle for the dispatch-adjacent one — and both are
derived, not fitted. They are the same rule once the seam is the **opcode
fetch**: the dispatch-adjacent copy sits right after the last fetch before the
dispatch, and the LYC kernel's M 48 is the fetch boundary its `ldh` runs from.
That is `Bus::run_dma`, and it scores +24 over the whole corpus while deleting
the per-access seams, the trigger-phase stamp and the `dma_dispatch_hold`
special case that approximated it.

Ruled out by build and measurement along the way, do not retry: servicing at the
trigger cycle's post-read seam, servicing from the next cycle with a write never
hosting the steal, non-stacking the dispatch hold with a trigger-cycle wait
(armed at the service attempt and per M-cycle in `tick_machine` — neither moves a
row), scoping the same-cycle sample order to VRAM addresses, scoping the wake
service by `VramDmaReq::HblankUnhalt` (either polarity, only swaps
`hdma_transition_ei_halt_late_unhalt_scx1_1` against its `_2`), and servicing at
the speed-switch pause's end (breaks that same `_1`).

#### The halt window trails the line wrap by two dots (2026-08-02)

`Ppu::hdma_period_halt` — `hdma_period_law` with the line wrap lagged **two raw
dots** — is the window both ends of a halt use for the armed HBlank block. At
the halt entry it decides whether the wake may re-trigger (`HaltHdmaState::High`
= already serviced, no wake block); at the halt wake it decides whether the
`Low` snapshot fires one. Both readings are the same statement: the hblank a
line just left is still open for two dots after the wrap.

##### The halt entry

The four `hdma_late_m0halt*` `_1`/`_2` pairs pin it two-sided at both speeds and
under an LCD-enable shift. All eight share one LYC=1 timer ISR (`$1000`: a NOP
sled, `HDMA5=$81` — two blocks — `xor a; ldh ($0F),a; HALT; nop; ldh a,($55)`),
`_2` being `_1` plus one sled NOP, so the arm and the halt sit one M-cycle later
while the timer-driven wake stays on the same absolute dot. The observable is the
FF55 read: `00` = one block retired, `FF` = both.

| rung | halt at (ly.dot) | want | second block |
|---|---|---|---|
| `_1` | 2.0 | `00` | deferred to ly 3 |
| `_2` | 2.4 | `FF` | at the ly-2 wake |
| `_ds_1` | 3.0 | `00` | deferred |
| `_ds_2` | 3.2 | `FF` | at the wake |
| `_ds_lcdoffset1_1` | 2.1 | `00` | deferred |
| `_ds_lcdoffset1_2` | 2.3 | `FF` | at the wake |
| `_lcdoffset3_1` | 1.455 | `00` | deferred |
| `_lcdoffset3_2` | 2.3 | `FF` | at the wake |

Dots 0 and 1 suppress, dot 2 onwards does not — the same two dots at both
speeds, so the lag is in dots and not M-cycles. `_lcdoffset3_1` halts at the
previous line's dot 455, already inside the un-shifted `hdma_period_law` window,
and `_lcdoffset3_2` halts at raw dot 3 under a 3-dot LCD shift and must still
re-trigger, so the lag is measured in **raw** dots, not the `law_pos` frame.

SameBoy latches the STAT *register*'s mode bits here instead (sm83_cpu.c
`halt()`: `allow_hdma_on_wake = io_registers[GB_IO_STAT] & 3`, checked by the
three wake sites in `GB_cpu_run`). That register holds mode 0 for a whole
M-cycle past the wrap, so it also suppresses `_ds_lcdoffset1_2`,
`_lcdoffset3_2` and `hdma_late_m3halt_m0unhalt_scx2_2` — traced with an FF55
read hook, SameBoy prints `00` where all three want `FF`. Porting `STAT & 3`
verbatim scores +3/−5; the two-dot window scores **+3/−0**.

##### The halt wake, and only the halt wake

`hdma_late_m0unhalt[_ds]_{1,2}` pin the same two dots from the other side. Their
ISR turns the LCD *on* (`LCDC = $91` at `$1110`), zeroes HDMA1-4, arms `HDMA5 =
$80` — one block — then `xor a; ldh ($0F),a; HALT; nop; ldh a,($55)`. `_2` is
`_1` plus one NOP *before* the LCD-on, so the PPU frame lags 4 dots against the
timer-anchored wake, putting the wake at ly 2 dot 0 for `_2` and dot 4 for `_1`.
`00` = no block by the read, `FF` = one. `_2` wants `FF`, so a wake at dot 0
fires the block it is leaving and a wake at dot 4 does not — the same boundary,
read as "still inside" instead of "already counted".

The lag applies to the halt wake only. `vram_dma_unhalt` also serves the
speed-switch pause exit, which lands at dot 1 of a visible line in
`hdma_late_m3speedchange_tima_scx1_ds_{1,2}`; their TIMA count needs the block
at that line's own mode-0 entry (probed: dot 257, not the exit's dot 5), so the
pause exit keeps the un-lagged `hdma_period_law`. The two exits already differ
the same way over the OAM-DMA catch-up M-cycle (see "Catch-up M-cycle on
resume"). Sharing the lagged window with the pause exit scores +2/−2; scoping it
to the halt wake scores **+2/−0**.

- **Refuted: a one-dot-lagged HBlank-window snapshot.** A `hdma_period_prev`
  shadow OR'd into the snapshot moved no row — one dot does not reach `_1`'s
  dot-0 halt from the previous line's still-open window. Two dots does.

#### The remaining halt-family rows sit behind a halt→wake duration gap (2026-08-02)

Measured, not swept. Two `gambatte/dma` clusters are left whose blocks and reads
were traced on both sides, and neither is a placement lever — both bottom out on
our halt lasting longer than the reference's from the same halt dot.

`hdma_transition_{,ei_}halt_late_unhalt_scx1_1` (ours `FF`, want `00`). Kernel:
LCD on at `$10D9`, `SCX=1`, `HDMA1-4 = $0000/$00F0`, `HDMA5 = $81` at `$1180`
(two blocks), `xor a; ldh ($0F),a; HALT` at `$1185`, a 74-NOP sled, `ldh a,($55)`
at `$11D0`. `_2` is `_1` plus one NOP before the LCD-on. Ours: arm ly 1 dot 232,
block 1 at ly 1 dot 256, halt at ly 1 dot 292 (`High`), wake ly 2 dot 224, block
2 at ly 2 dot 256, read at ly 3 dot 108 (`_1`) / 100 (`_2`) — `FF` for both.
SameBoy: arm ly 1 cfl 236, halt ly 1 cfl 260 with `allow_hdma_on_wake = 0`
(matching our `High`), wake ly 2 cfl 89, blocks at ly 2 and ly 3, read ly 3 cfl
112 / 104 — `00` for both, so SameBoy passes `_1` and fails `_2`. The read dots
agree to the frame offset; the block *lines* do not, and the wake dots differ by
135. No block placement splits `_1` from `_2`: their reads are 8 dots apart and
both blocks are more than a line earlier, so nothing can retire between them.

`hdma_late_{ei_,}m3halt_m2unhalt_ly_scx{1,2}_4` (ours `02`, want `03`). Six-rung
ladder alternating two insertion sites — one NOP in the post-halt sled, one
before the arm. `HDMA5 = $80`, one block; the observable is `ldh a,($44)`. Ours,
per rung: arm ly 1 dot 228/228/232/232/236/236, block at ly 2 dot 256 (rungs
1-2, the halt at dot 252 precedes the trigger so the flag is deferred) or ly 1
dot 256 (rungs 3-6), read at ly 2 dot 452 · ly 3 dot 0 · ly 2 dot 416 · ly 2 dot
420 · ly 2 dot 452 · ly 3 dot 0. Wants alternate 02/03; only rung 4 misses, and
it needs its read a whole block-stall (36 dots) later than rung 3's while rung 5
must stay before the boundary. SameBoy passes rungs 1-4 and **fails rung 5**
(reads `03`, want `02`), so its shape is not the target either.

Both clusters need the halt→wake duration reconciled first: rung 4's read sits
32 dots from rung 5's for a one-NOP difference that should move it 4, and the
post-wake sled is identical, so the gap is in the wake instant and not in the
DMA. Do not sweep the DMA seams for these rows.

Two rows needed the seam narrowed further, and each named its own mechanism.

`hdma_m0speedchange_late_m3wakeup_scx2_1` arms two blocks, `STOP`s into a speed
switch and reads FF55 straight after. Probed: the read lands at ly 58 dot 260
and the block at 262, so the ROM asks only whether the post-`STOP` read precedes
the block — its `scx1` sibling passes because its block lands at 256. A read
whose own M-cycle flags the block therefore yields to it, gated on
`hdma_trigger_edge` (the trigger's dot-END eighth plus the two-dot bus arrival)
against the read's `ACCESS_PHASE` sample: `hdma_start_1` samples first and reads
`$00`, this one samples after and sees FF55 retired.

`hdma_transition_oamdma_2` reads `(C000)` *while an OAM DMA is running from
`$C000`*, so the conflicted read returns the engine's in-flight byte and the
printed value IS its index (`$50 + idx`; want `$67`, we gave `$66`). Its gated
read is at `$116C` — the halt idle prefetch, which is rolled back while the CPU
sleeps and which nothing samples. Holding the block off the bus for that
pseudo-read costs the OAM DMA a byte of advance, so it is exempt
(`Bus::read_halt_idle`).

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

## Measured: the late-HDMA block fires ~36 dots early in the line (2026-08-06)

`dma/hdma_late_m3halt_m2unhalt_ly_scx{1,2}_*` is a six-rung ladder whose
reference alternates LY `02/03/02/03/02/03`; we produce `02/03/02/**02**/02/03`
— only rung 4 misses (same for the three-rung `_inc_scx{1,2}` ladder at rung 2).
The rungs move the FF44 read by one M-cycle each.

Differenced against SameBoy (`SB_TRACE`, absolute `fp`; note `--length 4`, the
length `classify_pixel.py` uses — at `--length 3` the tester reports "Boot ROM
did not finish" and never runs the kernel, which reads as a false floor):

| | SameBoy | slopgb |
|---|---|---|
| HBlank block end (rungs 3+4) | fp 26319612 | — |
| rung 3 read | fp 26319616 (+2 dots) → LY **2** | line 2 dot 416 → LY 2 |
| rung 4 read | fp 26319624 (+6 dots) → LY **3** | line 2 dot 420 → LY 2 |

So SameBoy's two reads sit 4 dots apart and STRADDLE the line 2→3 boundary: its
deferred block retires within ~6 dots of the line end. Ours retires ~36 dots
earlier, so both rungs land short of the boundary and rung 4 reads the old LY.
The error is not a sub-dot phase — it is where the halt-deferred HBlank block
lands inside the line (floor class B, the speed-switch/HDMA seam). Fixing it
means moving that retire dot, which the `dma` cycle/seam rows bracket from the
other side; do not sweep it without re-running the whole `dma` cluster.
