# SameBoy cycle-exact CPU/memory timing — port map for slopgb

Source: SameBoy 1.0.2, `/tmp/sbbuild/SameBoy-1.0.2/Core/`. Every claim below is grounded in
`file:line` against that tree. This document is the porting spec: an engineer should be able to
implement the Rust model from this alone.

The headline difference from slopgb is in §2 and §7. Read those first if short on time.

---

## 0. Mental model in one paragraph (the "deferred-commit" / lazy-advance clock)

SameBoy does **not** advance the machine and then access memory. Instead the CPU carries a debt
counter `gb->pending_cycles` (gb.h:666, type `unsigned`). Every bus primitive (`cycle_read`,
`cycle_write`, `cycle_oam_bug`) does three things in this fixed order:

1. **Pay the debt of the *previous* M-cycle first** — `GB_advance_cycles(gb, gb->pending_cycles)`.
   This is what actually ticks PPU/timer/APU/DMA forward.
2. **Sample / commit the bus at the current clock position** — `GB_read_memory` /
   `GB_write_memory`, which internally `GB_display_sync`s the PPU to exactly *now*.
3. **Set a fresh debt of 4** — `gb->pending_cycles = 4` — representing this M-cycle's own 4
   T-cycles, *which have not been advanced yet*. They are deferred to the next access.

So a memory read **latches the byte at the leading edge of its own M-cycle (cc+0)** — i.e. after
the *prior* M-cycle's 4 cycles have elapsed but **before** its *own* 4 cycles elapse. The trailing
4 cycles are charged later, by the next bus op or by `flush_pending_cycles` at the end of the
instruction. This is the entire ballgame for FF41/OAM/VRAM boundary reads.

---

## 1. The advance-cycles spine

### 1.1 `GB_advance_cycles(gb, cycles)` — timing.c:432

This is the single chokepoint that ticks every peripheral. It is called only from the `cycle_*`
primitives in sm83_cpu.c (plus a handful of direct calls in the HALT/interrupt path, §6).

Order of operations inside it (timing.c:432–516):

```c
void GB_advance_cycles(GB_gameboy_t *gb, uint8_t cycles)
{
    if (unlikely(gb->speed_switch_countdown)) { ... }   // 434: speed-switch boundary, may recurse
    gb->apu.pcm_mask[0] = gb->apu.pcm_mask[1] = 0xFF;    // 450
    gb->dma_cycles = cycles;                             // 452: OAM-DMA quantum  (CPU units)

    timers_run(gb, cycles);                              // 454: DIV/TIMA/serial/APU-divider — *UNDOUBLED*
    camera_run(gb, cycles);                              // 455
    ... speed_switch_halt_countdown, debugger ticks ...

    if (gb->speed_switch_freeze) { ... return; }         // 469

    if (unlikely(!gb->cgb_double_speed)) {               // 478  <-- THE DOUBLE-SPEED FACTOR
        cycles <<= 1;                                     // 479: single speed -> PPU sees 2x
    }
    ... double_speed_alignment += cycles (489) ...
    gb->cycles_since_last_sync += cycles;                // 492: in 8MHz units (gb.h:701)
    ... data_bus_decay ...

    GB_joypad_run(gb, cycles);                            // 508  } all see the *doubled* count
    GB_apu_run(gb, false);                                // 509  } (PPU/APU run in 8MHz ticks)
    GB_display_run(gb, cycles, false);                    // 510  <-- PPU advanced here
    if (unlikely(!gb->stopped)) GB_dma_run(gb);           // 512
    ir_run(gb, cycles);                                   // 514
    rtc_run(gb, cycles);                                  // 515
}
```

Key structural facts to port:

- **`cycles` is in CPU "T-cycles" where 4 == one M-cycle, regardless of speed.** One M-cycle of
  debt is always `pending_cycles = 4`.
- **The timer/serial/APU-divider domain (`timers_run`) receives the *undoubled* value** (line 454,
  before the `<<= 1`). That is why DIV advances 4 per M-cycle in *both* single and double speed
  (timing.c:278 `GB_set_internal_div_counter(gb, gb->div_counter + 4)` inside the per-4 `GB_SLEEP`
  loop). The timer clock is fixed relative to the CPU.
- **The PPU/APU/DMA/IR/RTC domain receives the *doubled* value in single speed** (lines 508–515,
  after `cycles <<= 1`). The PPU runs on the ~8.388 MHz "8MHz tick" / half-dot grid; one single-speed
  M-cycle = 8 of those = 4 dots; one double-speed M-cycle = 4 of those = 2 dots. The `// In 8MHz
  units` and `// Time passed in 8MHz ticks` comments (gb.h:701, display.c:805) confirm the unit, and
  `offset >>= 1; // Convert to T-cycles` (display.c:809) confirms 8MHz-tick / 2 = CPU T-cycle.

### 1.2 When the CPU accesses memory, in what order are peripherals advanced vs the bus sampled?

**Peripherals first, then sample.** But — and this is the trap — the peripherals advanced "first"
are the *previous* M-cycle's cycles, not this one's. See `cycle_read` (sm83_cpu.c:85), quoted in §2.
The advance pays off `pending_cycles` (the debt left by the *prior* access), `GB_read_memory` then
samples at the now-current clock, and a new debt of 4 is parked. The current M-cycle's 4 cycles are
advanced only on the *next* access. So the data byte is sampled at the **leading edge** of its
M-cycle.

`GB_read_memory` (memory.c:776) and the IO path (`read_high_memory`, memory.c:540) do **not** touch
the clock — they only `GB_display_sync(gb)` (= `GB_display_run(gb, 0, true)`, display.h:51) to drag
the PPU forward to the exact current `cycles_since_last_sync` position before reading the register.
So the value reflects PPU state at exactly cc+0 of the access M-cycle.

---

## 2. `cycle_read` — deferred-commit, line by line (sm83_cpu.c:85)

```c
static uint8_t cycle_read(GB_gameboy_t *gb, uint16_t addr)
{
    if (gb->pending_cycles) {
        GB_advance_cycles(gb, gb->pending_cycles);   // (A) pay PREVIOUS M-cycle's debt
    }
    gb->address_bus = addr;                          // (B) drive address
    uint8_t ret = GB_read_memory(gb, addr);          // (C) SAMPLE the byte NOW (cc+0 of this M-cycle)
    gb->pending_cycles = 4;                          // (D) park this M-cycle's 4 cycles as debt
    return ret;
}
```

**How many cycles advance BEFORE vs AFTER latching the byte?**

- **Before latch (step A):** `pending_cycles` — normally **4** (the previous access set it to 4), or
  **0** for the very first access of an instruction (the end-of-instruction `flush_pending_cycles`,
  sm83_cpu.c:1718, zeroed it). For a `cycle_no_access`-padded internal step it can be 8 (4 parked +
  4 added; §4).
- **At latch (step C):** the byte is sampled with the PPU/timer synced to the clock position reached
  by step A — i.e. the **start of this M-cycle**, T-cycle 0 of the 4.
- **After latch (step D):** **zero** cycles are advanced. The 4 cycles of *this* M-cycle are stored
  as debt and not advanced until the next `cycle_*` op (or the trailing `flush_pending_cycles`).

**Answer to "leading-edge or advance-then-sample?":** It is **sample-at-the-leading-edge, advance-deferred.**
The byte is latched at cc+0 of the M-cycle; the remaining T-cycles are advanced afterwards (lazily).
It is emphatically **not** "advance the M-cycle then sample." (That is slopgb's model — see §7.)

There is **no `cycle_read_inc_oam_bug`** in this version of SameBoy (confirmed: `grep cycle_read_inc_oam_bug
sm83_cpu.c` → no hits). The OAM-bug coupling is done by a separate primitive, `cycle_oam_bug` (§4),
and by `GB_trigger_oam_bug` calls in the dispatcher.

### 2.1 Worked example — the two `cycle_read`s of `LDH A,(a8)` (`ld_a_da8`, sm83_cpu.c:1284)

```c
static void ld_a_da8(GB_gameboy_t *gb, uint8_t opcode) {
    gb->af &= 0xFF;
    uint8_t temp = cycle_read(gb, gb->pc++);          // M2: fetch the FF-page offset
    gb->af |= cycle_read(gb, 0xFF00 + temp) << 8;     // M3: read FF00+temp  (e.g. FF41 = STAT)
}
```

Let cc be the clock (in CPU T-cycle units) and C0 the clock at instruction entry (`pending=0`).

| step | code | advance (A) | clock at sample | what is sampled |
|------|------|-------------|-----------------|-----------------|
| M1 opcode fetch | GB_cpu_run:1704 `cycle_read(pc++)` | +0 (pending was 0) | C0 | opcode `F0` |
| M2 imm fetch | `cycle_read(pc++)` :1287 | +4 (pay M1) | C0+4 | `temp` |
| M3 data read | `cycle_read(0xFF00+temp)` :1288 | +4 (pay M2) | **C0+8** | **STAT @ FF41** |
| trailing flush | GB_cpu_run:1718 | +4 (pay M3) | C0+12 | — |

So **STAT is sampled at C0+8 = the leading edge of M3**, a full M-cycle (4 dots single-speed)
*earlier* than a tick-then-access emulator that would advance M3 first and read at C0+12.

---

## 3. `cycle_write` — commit timing and the conflict-staging map (sm83_cpu.c:113)

`cycle_write` is the asymmetric twin of `cycle_read`. A plain write commits like a read does, but
many IO registers have **conflict semantics**: the value the PPU/APU latches depends on *which*
T-cycle within the M-cycle the write lands, because the bus is driven late and the peripheral may
sample the old value, the new value, or a transient. SameBoy models this by splitting the M-cycle
into pre-/post-write advances and writing once or twice.

Dispatch (sm83_cpu.c:116–129): for addresses in `0xFF00..0xFF7F` it looks up a per-model conflict
table (`cgb_conflict_map` 31, `cgb_double_conflict_map` 44, `dmg_conflict_map` 56, `sgb_conflict_map`
71); everything else (and unmapped IO) is `GB_CONFLICT_READ_OLD`.

The baseline case — **`GB_CONFLICT_READ_OLD` (sm83_cpu.c:131)** — is the exact mirror of `cycle_read`:

```c
case GB_CONFLICT_READ_OLD:
    GB_advance_cycles(gb, gb->pending_cycles);   // pay previous M-cycle
    GB_write_memory(gb, addr, value);            // commit at cc+0 (leading edge)
    gb->pending_cycles = 4;                       // park this M-cycle
    break;
```

So an ordinary write **also commits at the leading edge** and parks 4. The "data bus is driven in the
2nd half of the M-cycle" effect is expressed **relative to that baseline** by shifting where the
`GB_write_memory` lands and adjusting the parked debt so the total stays 4 per M-cycle. The staging
patterns (all from sm83_cpu.c) — note each splits the nominal 4 into `pending±k` before + `k` after,
and re-parks so the running total per M-cycle is conserved:

| conflict | pre-advance | commit pattern | re-park | meaning |
|---|---|---|---|---|
| `READ_OLD` (131) | `pending` | write `value` once | `=4` | component reads OLD value (write is late) |
| `READ_NEW` (137) | `pending-1` | write `value` | `=5` | write lands 1 T early → component reads NEW value |
| `WRITE_CPU` (143) | `pending+1` | write `value` | `=3` | CPU wins a same-cycle write (e.g. IF) — lands 1 T late |
| `STAT_DMG` (150) | `pending` | write `0xFF` (or `~0x20` at display_state 7), **+1 cyc**, write `value` | `=3` | DMG STAT-write bug: STAT reads FF for one T-cycle |
| `STAT_CGB` (168) | `pending` | write `(old&0x40)|(value&~0x40)`, **+1**, write `value` | `=3` | LYC bit latches a cycle late |
| `STAT_CGB_DOUBLE` (180) | `pending` | write `(value&~8)|(old&8)`, **+1**, write `value` | `=3` | mode-0 bit transient |
| `PALETTE_DMG` (195) | `pending-2` | write `value|old`, **+1**, write `value` | `=5` | palette OR-glitch, 2 T early |
| `PALETTE_CGB` (205) | `pending-2`/`-1` | write `value` | `=6`/`5` | CGB ≥D vs ≤C palette write delay |
| `DMG_LCDC` (219) | `pending-2` | masked write, **+1**, write `value` | `=5` | LCDC.0/.1 FIFO-vs-fetch conflict hacks |
| `SGB_LCDC` (248) | `pending-2` | write `value`, write `old`, **+1**, write `value` | `=5` | object-fetch abort hack |
| `WX_DMG` (262) | `pending` | write `value`, set `wx_just_changed`, **+1**, clear | `=3` | WX latch |
| `LCDC_CGB` (271) | `pending` | write `value` (+tile_sel_glitch for 1 T if TILE_SEL falling) | `=3`/`4` | tile-select glitch |
| `LCDC_CGB_DOUBLE` (288) | `pending-2` | masked write, **+2**, write `value` | `=4` | double-speed LCDC |
| `SCX_DMG_AND_CGB_DOUBLE` (302) | `pending-2` | write `value` | `=6` | SCX fine-scroll latches 2 T early |
| `NR10_CGB_DOUBLE` (308) | `pending-1` | sweep-disable flag for 1 T, write `value` | `=4` | APU sweep quirk |

After the switch, `gb->address_bus = addr;` (sm83_cpu.c:318) regardless of branch.

**Port note:** the `pending-2`/`pending+1` arithmetic assumes the incoming `pending_cycles` is large
enough (≥2). It always is for a normal write because the write is never the *first* access of an
instruction without a preceding fetch having parked 4 (asserted at sm83_cpu.c:115
`assert(gb->pending_cycles)`). Reproduce the staging exactly; the conserved-total invariant (sum of
pre-advance + post-advance over the M-cycle, plus the final re-park consumed by the next op, equals
the instruction's nominal T-count) is what keeps overall instruction timing correct while letting the
*sub-M-cycle* sample point vary.

---

## 4. Every distinct `cycle_*` primitive

| primitive | file:line | advances before | samples / commits | parks | purpose |
|---|---|---|---|---|---|
| `cycle_read(addr)` | 85 | `pending` (if nonzero) | `GB_read_memory` at cc+0 (leading edge), sets `address_bus` | `=4` | normal bus read; the workhorse |
| `cycle_write_if(value)` | 102 | `pending` (asserts nonzero) | writes IF at cc+0, returns **old** `IF & 0x1F`; sets `address_bus = FF0F` | `=4` | ISR-only IF write; used in interrupt dispatch so the pushed-PC-low write can also mutate IF and feed back into vector selection (§6) |
| `cycle_write(addr,value)` | 113 | varies by conflict | conflict-staged commit (§3) | varies (3–6) | normal/IO write with PPU/APU conflict modelling |
| `cycle_no_access()` | 321 | — (no advance, no bus op) | nothing | `pending += 4` | an **internal execution M-cycle** (no memory touched): 16-bit INC/DEC, taken cond. jump/call, PUSH pre-decrement, `LD SP,HL`, etc. The +4 debt is paid by the *next* real access, so peripherals still tick for it, just lazily. |
| `cycle_oam_bug(register_id)` | 326 | `pending` (if nonzero) | sets `address_bus = reg`, calls `GB_trigger_oam_bug(gb, reg)` instead of reading memory | `=4` | an M-cycle that drives a 16-bit register onto the address bus near OAM (`INC rr`/`DEC rr`/`LD (HL±),A` family) and may corrupt OAM during mode 2; the data-less analogue of `cycle_read` |
| `flush_pending_cycles()` | 336 | `pending` (if nonzero) | nothing | `=0` | drains remaining debt and parks **0**; called at the end of every `GB_cpu_run` (1718) and before special sequences (STOP 387/410, interrupt vector retiming 1691) |

There is **no** `cycle_read_inc_oam_bug` and no `cycle_oam_bug_read` primitive in 1.0.2; the OAM-bug
read path lives in `read_high_memory` (memory.c:550 `GB_trigger_oam_bug_read`) and is reached through
the ordinary `cycle_read` → `GB_read_memory` chain.

---

## 5. M-cycle granularity & the double-speed factor

- **The machine is advanced one M-cycle (4 CPU T-cycles) at a time, in lumps, not per-T-cycle.**
  `cycle_read`/baseline `cycle_write` always advance exactly `pending_cycles` (= 4) at a time. The
  *only* place sub-M-cycle (1- or 2-T) advances happen is inside the conflict-staged writes (§3),
  where `GB_advance_cycles(gb, 1)` / `(gb, 2)` split a single M-cycle to model the bus-driven-late
  effect. So the granularity is "4 at a time, except IO-write conflicts which split into e.g. 2+1+1
  or 3+1." There is no free-running per-T-cycle loop.
- **Double-speed factor is applied once, centrally, in `GB_advance_cycles` (timing.c:478–480):**

  ```c
  if (unlikely(!gb->cgb_double_speed)) {
      cycles <<= 1;
  }
  ```

  In **single speed** the incoming CPU T-cycle count is **doubled** for the PPU/APU/DMA/IR/RTC
  (they run on the 8MHz half-dot grid). In **double speed** it passes through unscaled. The
  timer/serial/APU-divider domain (`timers_run`, called at line 454 *before* the shift) is **never**
  scaled — it is locked to the CPU. Net effect: per M-cycle, DIV always +4; PPU advances 8 half-dots
  (= 4 dots) single-speed or 4 half-dots (= 2 dots) double-speed.
- The actual speed *switch* (mid-stream toggling of `cgb_double_speed`) is handled by the
  `speed_switch_countdown` block at the top of `GB_advance_cycles` (timing.c:434–449), which can
  split the advance across the toggle and recurse once.

---

## 6. Interrupt dispatch timing

The whole decision is made **once per `GB_cpu_run`, at the instruction boundary, before any cycles
of the new instruction are advanced** (sm83_cpu.c:1606). At that point `pending_cycles == 0` (the
previous instruction's trailing `flush_pending_cycles` zeroed it, line 1718).

Sampling of the pending set (sm83_cpu.c:1629):

```c
uint8_t interrupt_queue = gb->interrupt_enable & gb->io_registers[GB_IO_IF] & 0x1F;
```

`interrupt_enable` is the FFFF IE register (gb.h:374); `io_registers[GB_IO_IF]` is FF0F. This is read
**before** the instruction runs, i.e. it reflects IE/IF as of the end of the previous instruction.

IME edge (the delayed-EI flop), sm83_cpu.c:1636–1640:

```c
bool effective_ime = gb->ime;
if (gb->ime_toggle) {            // EI/DI scheduled a flip last instruction (gb.h:384)
    gb->ime = !gb->ime;
    gb->ime_toggle = false;
}
```

`effective_ime` (the value *before* applying this instruction's pending toggle) is what gates
dispatch — so `EI` enables interrupts only after the following instruction, matching hardware.

Three mutually-exclusive branches:

- **HALT wake without dispatch** (1643): `halted && !effective_ime && interrupt_queue` → just clear
  `halted`, run a DMA quantum, no vector. (HALT-bug territory; the `halt` opcode at 1032 sets
  `halt_bug` when `IF&IE` already nonzero with IME off.)
- **Interrupt dispatch** (1654): `effective_ime && interrupt_queue`. The 5-M-cycle ISR sequence:

  ```c
  gb->halted = false;
  gb->dma_cycles = 4; GB_dma_run(gb);          // 1660-1661
  uint16_t call_addr = gb->pc;
  cycle_read(gb, gb->pc++);                     // 1665  internal (dummy fetch), parks 4
  cycle_oam_bug(gb, GB_REGISTER_PC);            // 1666  internal, address bus = PC
  gb->pc--;
  GB_trigger_oam_bug(gb, gb->sp);               // 1668
  cycle_no_access(gb);                          // 1669  internal M-cycle
  cycle_write(gb, --gb->sp, (gb->pc) >> 8);     // 1671  push PC high
  interrupt_queue = gb->interrupt_enable;       // 1672  RE-SAMPLE IE here!
  if (gb->sp == GB_IO_IF + 0xFF00 + 1) {        // 1674  pushing low byte onto FF0F
      gb->sp--;
      interrupt_queue &= cycle_write_if(gb, gb->pc & 0xFF);  // 1676 push *and* read old IF
  }
  else {
      cycle_write(gb, --gb->sp, gb->pc & 0xFF); // 1679  push PC low
      interrupt_queue &= gb->io_registers[GB_IO_IF] & 0x1F;  // 1680  RE-SAMPLE IF here!
  }
  if (interrupt_queue) {                         // vector still pending after the pushes?
      ... find lowest set bit -> interrupt_bit ...
      assert(gb->pending_cycles > 2);
      gb->pending_cycles -= 2;                    // 1690  retime: vector latched 2 T early
      flush_pending_cycles(gb);                   // 1691
      gb->pending_cycles = 2;                     // 1692
      gb->io_registers[GB_IO_IF] &= ~(1 << interrupt_bit);  // ack
      gb->pc = interrupt_bit * 8 + 0x40;          // 1694  jump to vector
  }
  else {
      gb->pc = 0;                                 // 1697  interrupt cancelled -> PC=0000
  }
  gb->ime = false;                                // 1699
  ```

  The decisive cycle-exact details to port:
  - **IE is re-read at 1672 and IF re-read at 1680** (or merged into the IF push at 1676), *after*
    the PC-high push. This is the "interrupt cancellation" window: a write that clears IE/IF during
    the two push M-cycles can drop the interrupt (`interrupt_queue` becomes 0 → `pc = 0`).
  - The **vector is selected 2 T-cycles before the final M-cycle completes** — `pending_cycles -= 2;
    flush; pending_cycles = 2` (1690–1692). The total dispatch cost is 5 M-cycles (20 T), but the
    IF-acknowledge/vector-latch lands at the 2-T-early point, not the M-cycle end.
- **Normal run** (1703): fetch opcode via `cycle_read(gb->pc++)` (1704), then `opcodes[opcode](gb,
  opcode)` (1715).

Every branch ends with `flush_pending_cycles(gb)` (1718).

HALT itself adds `GB_advance_cycles` calls *outside* the `pending_cycles` mechanism (1626/1632) to
model the idle HALT clocking (2 or 4 T depending on model and `just_halted`).

---

## 7. The decisive contrast: SameBoy vs slopgb's "tick-then-access"

**slopgb today:** advances a *full* M-cycle (4 dots) and *then* performs the access. A read therefore
observes peripheral state at **cc+4** — the **trailing edge / end** of the access M-cycle.

**SameBoy:** the access primitive pays only the *previous* M-cycle's debt, then samples, then parks
this M-cycle's 4 cycles as debt. A read observes peripheral state at **cc+0** — the **leading edge /
start** of the access M-cycle. The 4 trailing cycles are advanced *after* the byte is latched,
lazily, by the next access.

Net: **for the identical instruction, SameBoy samples FF41/OAM/VRAM exactly one M-cycle (4 dots in
single speed; recall the PPU sees `pending<<1` = 8 half-dots) earlier than slopgb.** Concretely, for
`LDH A,(FF41)` (§2.1) SameBoy latches STAT at C0+8 while slopgb latches at C0+12.

SameBoy itself notes the PPU side of this offset: *"It seems that the STAT register's mode bits are
always 'late' by 4 T-cycles"* (display.c:1530). Combined with leading-edge sampling, that 4-T phase
is exactly what places a STAT read on the correct side of a mode-3→mode-0 flip.

### 7.1 Why two identical `ldh a,(FF41)` reads can land in different M-cycles

Because the byte is committed at the **leading edge** and the M-cycle's own 4 cycles are **deferred**,
the clock position at which a given read resolves is **not** a property of that read's opcode — it is
the running sum of every preceding access's deferred commit. Two textually-identical
`ldh a,(FF41)` instructions resolve at **different cc** whenever the code path *before* them parked a
different amount of debt or inserted a different number of internal M-cycles. The two biggest sources
of such divergence:

1. **Interrupt dispatch retiming (§6).** A dispatched ISR runs `cycle_no_access` (1669) and the
   `pending_cycles -= 2 … = 2` retime (1690–1692). An instruction that executes *after* an ISR enters
   its FF41 read with the clock phase shifted by that 2-T retime relative to the same instruction
   reached without an ISR. The FF41 read's leading edge therefore lands 2 T earlier/later — enough to
   cross a mode boundary.

2. **Different predecessor instruction timing / conflict-write parking.** Because a conflict write
   re-parks 3/5/6 instead of 4 (§3), and `cycle_no_access` parks +4 with no bus op, the *phase* of the
   clock entering the next instruction depends on the exact predecessor. A `ldh a,(FF41)` preceded by
   an instruction that left the PPU at half-dot phase X resolves in mode 3; the same `ldh` preceded by
   a 1-T-different predecessor resolves at phase X±1 → mode 0.

This is the mechanism slopgb's tick-then-access model cannot reproduce: by advancing the full M-cycle
*before* reading, slopgb collapses both predecessors' phases into the same cc+4 sample point, so both
reads see the post-flip value (`0`). The slopgb floor note's `m2int_m3stat_1` (wants out3) vs
`m0int_m3stat_2` (wants out0) contradiction — "same `ldh a,(FF41)` on the byte-identical bus M-cycle,
both read 0" — is precisely the symptom of sampling at the trailing edge. SameBoy resolves it because
the two scenarios reach that `ldh` with **different deferred-commit phase** (one came through an ISR
dispatch, one did not), so the leading-edge sample lands on opposite sides of the mode-3→mode-0 flip.

### 7.2 What a faithful port must replicate (minimum set)

1. A `pending_cycles` debt counter; bus ops advance `pending` *then* sample *then* set the new debt.
   **Never advance the current M-cycle before sampling.**
2. `flush_pending_cycles` at the end of each instruction (and the STOP/interrupt special points).
3. `cycle_no_access` as `pending += 4` for internal M-cycles — do **not** advance immediately.
4. The conflict-staging `cycle_write` table (§3) for IO registers, with the exact pre/post split and
   re-park values, including the per-model maps.
5. The double-speed factor applied centrally and only to the PPU/APU/DMA/IR/RTC domain, with the
   timer/serial/APU-divider domain locked to the CPU (timing.c:454 vs 478–480).
6. The interrupt-dispatch re-sampling of IE (1672) and IF (1680/1676) after the PC-high push, and the
   `pending_cycles -= 2 … = 2` vector retime (1690–1692).
7. Reads/writes that `GB_display_sync` the PPU to the exact current clock before touching the
   register (memory.c:547, etc.) — i.e. the PPU must be queryable at arbitrary sub-line cc, not only
   at M-cycle boundaries.

---

## Appendix: relevant `GB_gameboy_t` fields (gb.h)

| field | gb.h | role |
|---|---|---|
| `unsigned pending_cycles` | 666 | the deferred-commit debt counter (the whole model hinges on this) |
| `uint16_t address_bus` | 395 | last address driven; set by every `cycle_*` |
| `uint8_t data_bus` | 396 | cart MAIN data bus (open-bus decay), set in `GB_read_memory` (memory.c:816) |
| `uint8_t interrupt_enable` | 374 | the FFFF IE register |
| `io_registers[GB_IO_IF]` | — | FF0F IF register |
| `bool ime` / `bool ime_toggle` | —/384 | IME and the delayed EI/DI flop |
| `bool halted` / `bool just_halted` / `bool halt_bug` | 381/386/385 | HALT state machine |
| `bool cgb_double_speed` | 380 | the `<<= 1` selector in `GB_advance_cycles` |
| `uint64_t cycles_since_last_sync` | 701 | PPU/APU clock accumulator, **in 8MHz (half-dot) units** |
| `uint8_t double_speed_alignment` | 533 | even/odd-mode phase tracker |
| `uint16_t dma_cycles` | 410 | OAM-DMA per-advance quantum (CPU units) |

Unit reminder: `pending_cycles` and the `cycles` arg to `GB_advance_cycles` are **CPU T-cycles**
(4/M-cycle). Inside `GB_advance_cycles` they become **8MHz half-dot ticks** for the PPU/APU via
`cycles <<= 1` in single speed (timing.c:479; `>>= 1` to convert back, display.c:809).
