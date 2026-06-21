# slopgb-core timing core — source map (pre-SameBoy-port)

A precise, source-grounded map of the **current** timing core, for the
re-architecture toward a SameBoy cycle-exact model. All paths under
`crates/slopgb-core/src/`. Every claim is cited `file:line`; quotes are the
load-bearing lines.

The one-sentence summary up front:

> **A CPU read of FF41 samples the PPU mode at the END of the M-cycle (cc+4 /
> state(D)): `Bus::read` ticks all 4 (or 2 in DS) dots first, *then* calls
> `Ppu::read(0xFF41)` → `vis_mode()` over the already-advanced `self.dot`.**
> The single half-dot escape hatch is the per-M-cycle `Option<u8>` edge stamps
> (`stat_mode_edge`/`m0_access_edge`/`pal_access_edge`), which can hold one
> read back to the M-cycle's first half — but every CPU access observes at the
> one fixed `ACCESS_PHASE` (= MID = cc+2), so two reads in the same bus
> M-cycle are inseparable.

---

## 1. The tick-then-access contract

The CPU is the clock master; the `Bus` trait (`cpu/mod.rs:35-109`) advances the
machine one M-cycle **then** performs the access. The `Interconnect` impl:

`interconnect.rs:671-705`:
```rust
fn read(&mut self, addr: u16) -> u8 {
    self.service_vram_dma();
    self.tick_machine();                 // ← advance one whole M-cycle first
    self.service_vram_dma();             // same-cycle HDMA trigger steals bus
    self.maybe_oam_bug(addr, OamBugKind::Read);
    self.read_no_tick(addr)              // ← then sample
}
fn write(&mut self, addr: u16, value: u8) {
    self.service_vram_dma();
    if let 0xFF40 | 0xFF42 | 0xFF43 | 0xFF47..=0xFF4B = addr {
        let dots = if self.double_speed { 1 } else { 2 };
        self.ppu.stage_write(addr, value, dots);   // mid-cycle pipe view (see §7)
    }
    self.tick_machine();
    self.maybe_oam_bug(addr, OamBugKind::Write);
    self.write_no_tick(addr, value);
}
fn tick(&mut self) { self.service_vram_dma(); self.tick_machine(); }
```
`read_inc` / `tick_addr` are the same shape (`interconnect.rs:707-719`). So
**all** time advance is in `tick_machine`; the access is side-effect-free except
the OAM bug (`cpu/mod.rs:21-33`).

### Dots per M-cycle and the cc-reclock dot loop

`interconnect/tick.rs:9-11`:
```rust
pub(super) fn tick_machine(&mut self) {
    let dots: u64 = if self.double_speed { 2 } else { 4 };
    self.cycles += dots;
```
So **4 dots/M-cycle single speed, 2 dots/M-cycle double speed**. The dots are
not advanced in a `for i in 0..dots` loop any more — they are advanced one CPU
**cc** at a time, ticking a whole PPU dot only on cc's selected by
`dot_ticks_on_cc` (`tick.rs:46-153`):
```rust
for cc in 1..=4u8 {
    if !dot_ticks_on_cc(cc, self.double_speed, self.dot_phase) { continue; }
    ...
    self.intf |= self.ppu.tick() & IF_MASK & !dot_squash;   // one PPU dot
    ...
}
```
`dot_ticks_on_cc` (`interconnect.rs:75-78`): `!ds || cc % 2 == phase % 2` — single
speed ticks a dot every cc (4 dots); double speed ticks on the even cc `{2,4}`
at `dot_phase==0` (today's fixed alignment) or the odd cc `{1,3}` at phase 1.
`dot_phase` is a struct field **held at 0** (`interconnect.rs:313-327`,
`512-...`): phase 0 is bit-identical to the old loop
(`cc_grid_matches_dot_loop` test). `Ppu::tick()` (`ppu/mod.rs:648-700`) is the
single-dot advance; it returns the IF bits raised this dot, OR-ed into `intf` at
`tick.rs:59`.

Note `tick_machine` ticks **timer first** (`tick.rs:21`), then OAM DMA, then the
PPU dots, then APU/serial/joypad/RTC (`tick.rs:154-159`).

---

## 2. The cc-reclock + `event_phase` scaffold (the half-dot / eighth grid)

An M-cycle is divided into **8 eighths** (= 4 cc). Events commit at a sub-cc
*phase*; CPU observers sample at a sub-cc *phase*; a read is blocked iff the
observer phase precedes the commit phase. The machinery (all in
`interconnect.rs`):

| symbol | line | meaning |
|---|---|---|
| `MID_PHASE = 4` | 43 | the cc+2 observer phase — the M-cycle midpoint a tick-then-access read "effectively" samples 2 dots before the cc+4 end view |
| `END_PHASE = 8` | 49 | cc+4 = the whole-M-cycle block (commits past every observer; visible only next M-cycle) |
| `edge_eighth(i,dots)` | 56-62 | dot-END commit eighth of dot `i`: `((i+1)*8/dots)` → SS `{2,4,6,8}`, DS `{4,8}` |
| `cc_eighth(cc)` | 88-92 | `edge_eighth(cc-1,4)` — the cc grid IS the single-speed dot grid |
| `obs_pre_edge(obs,edge)` | 100-103 | `obs < edge` — observer sees the pre-commit (still-blocked) state |
| `stamp_blocks(stamp,obs)` | 115-118 | `stamp.is_some_and(|edge| obs_pre_edge(obs, edge))` |
| `EdgeKind` | 128-139 | `M0Rise` / `M0Access` / `PalAccess` / `StatMode` |
| `event_phase(kind,cc,lead_eighths)` | 154-189 | the commit phase of `kind`'s edge on cc `cc`, shifted by a signed `lead_eighths`, clamped `0..=8` |
| `ACCESS_PHASE = MID_PHASE` | 198 | **the single phase every CPU bus access observes at** |

`event_phase` (`interconnect.rs:154-189`) returns, per kind:
```rust
let base = match kind {
    EdgeKind::PalAccess => END_PHASE,   // whole-M-cycle block (INC-G3 task 5)
    EdgeKind::StatMode  => END_PHASE,   // whole-M-cycle block (INC-G3 task 6)
    _ => cc_eighth(cc),                 // M0Rise / M0Access: dot-END commit
};
(i16::from(base) + i16::from(lead_eighths)).clamp(0, i16::from(END_PHASE)) as u8
```
**`lead_eighths` is the reclock hook (S0+S1): a per-event signed sub-dot offset.
All leads are 0 today, so the scaffold is net-zero.** The roadmap-critical fact:
`ACCESS_PHASE` is **one constant** (`interconnect.rs:191-198`). The reverted G2c
attempt to give each read-chain its own observer phase (`obs_phase(addr)`) was
the wrong premise — all CPU accesses sample at the same cc-offset because
M-cycles are dot-aligned to the PPU. The discriminator is meant to be the
EVENT's phase, not the observer's.

### The `Option` flip-stamps — set per tick, consumed per access

Reset every tick at `tick.rs:30-32`:
```rust
self.m0_access_edge = None;
self.pal_access_edge = None;
self.stat_mode_edge = None;
```
Set inside the dot loop when the PPU reports a flip on that dot
(`tick.rs:93-136`), e.g.:
```rust
if let Some(lead) = self.ppu.take_m0_access_flip() {
    self.m0_access_edge = Some(event_phase(EdgeKind::M0Access, cc, lead));
}
if let Some(lead) = self.ppu.take_pal_access_flip() {
    self.pal_access_edge = Some(event_phase(EdgeKind::PalAccess, cc, lead));
}
if let Some(lead) = self.ppu.take_m0_stat_flip() {
    self.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, cc, lead));
}
```
The PPU produces these as `Option<i8>` (`Some(lead_eighths)` = flip fired this
dot) in `m0_flip_events`/`advance_lx` (see §4) and hands them over via the
takers in `ppu/stat_irq.rs:78-98`.

**Consumer sites (all `memory.rs` unless noted) — each overrides exactly one
CPU read/write to return the pre-flip value while the stamp blocks at
`ACCESS_PHASE`:**

- **OAM read** FE00-FE9F: `memory.rs:145` `if stamp_blocks(self.m0_access_edge, ACCESS_PHASE) { 0xFF }`
- **OAM write** FE00-FE9F: `memory.rs:209` (dropped while blocked)
- **VRAM read** 8000-9FFF: `memory.rs:132-137` `stamp_blocks(self.m0_access_edge, ACCESS_PHASE) && self.hdma_mode == HdmaMode::Disabled => 0xFF`
- **VRAM write** 8000-9FFF: `memory.rs:199` (dropped)
- **CGB FEA0-FEFF mirror read**: `memory.rs:73` (OAM-blocked OR `stamp_blocks(m0_access_edge, ACCESS_PHASE)`)
- **CGB palette FF69/FF6B read**: `memory.rs:271-275` `self.cgb_mode && stamp_blocks(self.pal_access_edge, ACCESS_PHASE) => 0xFF`
- **FF41 STAT-mode read (double speed only)**: `memory.rs:243-249`
  ```rust
  0xFF41
      if stamp_blocks(self.stat_mode_edge, ACCESS_PHASE)
          && self.double_speed
          && self.ppu.lcd_enabled() =>
  {
      self.ppu.read(0xFF41) | 0x03   // hold the pre-flip mode 3
  }
  ```

`M0Rise` is **not** stamped/consumed as a read override — it routes the halt-exit
mask only (`tick.rs:76-92`, `obs_pre_edge(MID_PHASE, event_phase(M0Rise,cc,0))`).

---

## 3. Where a CPU read of FF41 samples the mode (the sampling point)

Trace of `read(0xFF41)`:

1. `Bus::read` (`interconnect.rs:672`) → `service_vram_dma()` → **`tick_machine()`**
   advances all 4/2 dots, mutating `ppu.dot`, `ppu.line`, `ppu.line_render_done`,
   and setting `stat_mode_edge`.
2. → `read_no_tick(0xFF41)` (`memory.rs:152`) → `io_read(0xFF41)`.
3. `io_read` (`memory.rs:243-250`): the DS-only override arm
   (`memory.rs:243-249`), else `memory.rs:250` `0xFF40..=0xFF45 | ... => self.ppu.read(addr)`.
4. `Ppu::read(0xFF41)` (`ppu/regs.rs:119`):
   ```rust
   0xFF41 => 0x80 | self.stat_en | (u8::from(self.cmp) << 2) | self.vis_mode(),
   ```
5. `vis_mode()` (`ppu/stat_irq.rs:10-41`) computes the mode bits from the
   **already-advanced** state — `self.dot`, `self.line`, `self.line_render_done`,
   `self.glitch_line`:
   ```rust
   } else if self.dot < 4 { ... }
   else if self.dot < 84 { 2 }
   else if !self.line_render_done { 3 }
   else { 0 }
   ```

**So the mode is sampled at the END of the M-cycle — state(D), the cc+4 view —
because `tick_machine` ran first and `vis_mode` reads the post-tick `dot`.** The
only sub-dot adjustment is the DS `stat_mode_edge` override (`memory.rs:243-249`),
which ORs `0x03` to hold mode 3 for the whole straddle M-cycle.

The sibling accesses, same shape (tick-first, sample-after):

- **OAM (FE00)**: `memory.rs:141-150` → `Ppu::read` `oam_read_blocked()`
  (`ppu/regs.rs:111-117`, `ppu/blocking.rs:6-11`: blocked while
  `enabled && line<=143 && !line_render_done && ...`), MID-override at
  `memory.rs:145`.
- **VRAM (8000)**: `memory.rs:132-138` → `vram_read_blocked()`
  (`ppu/blocking.rs:38-52`: `dot >= 80 + late`), MID-override at `memory.rs:132`.
- **CGB palette (FF69/FF6B)**: `memory.rs:271-276` → `Ppu::read`
  `pal_ram_blocked()` (`ppu/regs.rs:131-145`, `ppu/blocking.rs:71-81`:
  `!render_finished && dot >= 84`), whole-M-cycle MID-override at `memory.rs:271`.

Every one of these reads the PPU's positional state **after** the dots have run.

---

## 4. Mode-0 flip / IRQ dot computation (`ppu/render/mode0.rs`)

`m0_flip_events` (`mode0.rs:82-175`), called once per mode-3 dot from `step_dot`
(`ppu/mod.rs:803`) **after** the render step. It projects the line's end and, when
the projection is within `lead`, raises the visible mode-0 flip (`m0_src`,
`line_render_done`), the STAT-IRQ source (`m0_rise_dot`), and stamps the
half-dot flip flags.

The projection (`mode0.rs:86-137`): `proj` = dots from now to pipe end =
`stall + (160 - lx)` plus FIFO-refill cost (`mode0.rs:90-99`), still-ahead sprite
fetch costs (`mode0.rs:103-121`), and a window-start cost (`mode0.rs:125-137`).

The **lead** — how many dots the flip/IRQ precedes the pipe end P
(`mode0.rs:143-144`):
```rust
let lead = (2 + u16::from(r.fetched != 0 && !self.model.is_cgb()) - u16::from(self.ds))
    .saturating_sub(u16::from(r.win_stalled) + u16::from(r.win_aborted));
```
i.e. **2 dots before the pipe end on a bare line; +1 (→3) on sprite-laden DMG
lines** (`r.fetched != 0 && !cgb`, because the 6-dot first OBJ fetch extends the
pipe one more dot while the flip stays on its mooneye/gbmicrotest dot); **−1 in
double speed** (`self.ds`); **−1 per window stall/abort**. The fire
(`mode0.rs:156-174`):
```rust
let bare_flip = r.fetched == 0 && !r.win_active && !self.glitch_line;
if proj <= lead {
    self.m0_src = true;
    self.m0_rise_dot = true;
    self.line_render_done = true;
    self.m0_access_flip = bare_flip.then_some(0i8);       // OAM/VRAM unblock edge
    self.m0_stat_flip   = (r.fetched != 0).then_some(0i8); // DS FF41 flip edge
}
```
State read: `r.stall`, `r.lx`, `r.bg_count`, `r.phase`, `r.fetched`,
`r.penalty_tiles`, `r.n_sprites`, `r.sprites[]`, `r.win_active/win_stalled/
win_aborted`, `self.eff.lcdc/scx/wx`, `self.ds`, `self.glitch_line`,
`self.model`. The two flip flags are **complementary-gated**: `m0_access_flip`
on **bare** lines only (`bare_flip`), `m0_stat_flip` on **sprite-extended**
lines only (`r.fetched != 0`).

The **pipe-end anchors** (one/two dots *later* than the flip) are set in
`advance_lx` (`mode0.rs:26-53`): `lx==159 → hdma_lead`; `lx==160 → render_finished`
+ `pal_access_flip` (`mode0.rs:39-41`, bare lines only) + a zero-lead safety-net
flip. The CGB palette block is therefore anchored at `render_finished` (pipe
end), the OAM/VRAM/STAT block at the m0 flip (pipe end − lead). All `lead_eighths`
threaded today are `0` (`mode0.rs:39,163,173`).

`m0_unflip` (`mode0.rs:185-191`): a late stall after the flip drops `m0_src`/
`line_render_done` back to mode 3 (combinational level on hardware).

---

## 5. STAT IRQ engine (`ppu/stat_irq.rs`)

There is **no wired-OR STAT line on the IRQ side**; each source is an *event*
gated by a predicate over the *other* sources' enables (through delayed FF41/FF45
copies). `stat_events_tick` (`stat_irq.rs:233-324`), called every dot from
`Ppu::tick` (`ppu/mod.rs:698`), fires:

- **m2 line-start pulse** (lines 1-144 dot 0): `stat_irq.rs:244-254`, gated by
  `m2_pulse_fires` (`stat_irq.rs:487-496`). Second-half commit → sets `stat_late`
  + `stat_halt_late`.
- **m2 line-0 pulse** (line 0 dot 4): `stat_irq.rs:255-265`.
- **DMG vblank-line OAM pulses** (145-153 dot 12): `stat_irq.rs:266-274`.
- **m1 vblank event** (144 dot 4): `stat_irq.rs:275-281`, with the VBlank IF.
- **m0 rise** (on `m0_rise_dot`, set by `m0_flip_events`): `stat_irq.rs:282-299`;
  sets `self.m0_rise = true` so the interconnect applies the half-cycle halt law.
- **LYC events** (value's line, dot 4 / 153:12): `stat_irq.rs:300-322`.

All firings OR into `self.pending_if` (`stat_irq.rs:323`). **IF is set** when
`Ppu::tick` returns `pending_if` (`ppu/mod.rs:699`) and the interconnect ORs it
into `intf` at `tick.rs:59`:
```rust
self.intf |= self.ppu.tick() & IF_MASK & !dot_squash;
```
Register-write-induced STAT rises take a parallel path: `Ppu::write` returns the
raised bits (`ppu/regs.rs:157,218-219,299`), OR-ed into `intf` immediately by the
io_write arms (`memory.rs:336-358`). The dispatch `ack` (`interconnect.rs:751-781`)
clears the IF bit and arms the `ack_squash_*` sync-ahead window.

The mode bits FF41 shows come from `vis_mode` (`stat_irq.rs:10-41`); `mode_bits`
(`stat_irq.rs:45-47`) is the same value re-exported for the interconnect's
prohibited-area gate.

---

## 6. The collapse: why `m2int_m3stat_1` and `m0int_m3stat_2` read identically

Both ROMs execute the **same opcode** `ldh a,(FF41)`, which reaches
`Bus::read(0xFF41)` (`interconnect.rs:672`) → `tick_machine` → `read_no_tick` →
`io_read` → `Ppu::read(0xFF41)` → `vis_mode()`. Measured (per
`docs/hardware-state/ppu-subdot-ladder.md:24`, verified by direct `run_gambatte`):
both reads land on the **byte-identical modeled bus M-cycle** — `cyc=772, ly=1,
dot=256, boundary eighth E=4`. Because `vis_mode` is a pure function of
post-tick PPU state (`self.dot`, `self.line`, `self.line_render_done`), and that
state is identical, **the emulator returns the same value (mode 0) to both**.
But the baseline wants `m2int_m3stat_1` → out3 (mode 3) and `m0int_m3stat_2` →
out0 (mode 0) — `gambatte.txt:444-446` vs the m0int sibling.

Any sub-cc model in this scaffold computes
`observed = (O < E ? mode3 : mode0)`, exactly `stamp_blocks(stat_mode_edge,
ACCESS_PHASE)`, where:
- **`O`** = the read's bus sub-cc observer offset = `ACCESS_PHASE` — a **per-bus-
  access constant** (`interconnect.rs:191-198`), so identical for two identical
  `ldh`;
- **`E`** = the boundary's sub-dot eighth = `event_phase(StatMode,…)` — a
  **per-PPU-state** value, so identical for the same bus M-cycle.

Imposing both wants gives `O < 4` (m2int wants mode 3) **and** `O ≥ 4` (m0int
wants mode 0) — **unsatisfiable for a pure function of (O, E)**.

**The single quantity that would have to differ for them to separate:** *which
ISR is on the call stack* — the mode-2 (OAM) source's ISR vs the mode-0 (HBlank)
source's ISR. That is CPU control-flow / dispatch context, **not** a bus offset
and **not** a PPU sub-dot — outside the entire half-dot/eighth model space.
(Equivalently, per the parity finding `docs/sameboy-parity-plan.md:21-36`:
SameBoy's T-cycle-exact grid lands the two reads in **different M-cycles** — the
mode-0 ISR's read is one M-cycle later, after the mode-3→0 boundary — so for
SameBoy `E`/the dispatch dot differs and no contradiction arises.) Tagging the
ISR context (the reverted G2c) lifts the gambatte side but **breaks** the
canonical mooneye `intr_2_mode0_timing` ×6 and the gbmicrotest FF0F oracle
(`ppu-subdot-ladder.md:12,26`) — forbidden under the never-drop-a-SameBoy-pass
rule.

The deeper reason the half-dot model can't fix this without the rewrite: the
floored reads are the **END-VIEW** — the mode-3→mode-0 boundary *dot* is itself
one whole dot late in those geometries (`m0_flip_events` projects the flip from
render state alone; `ppu-subdot-ladder.md:23`), and a sub-dot *lead* cannot move
a whole-dot-late boundary. Only moving the pipe-end **dot** (the full pixel-pop
reclock, which moves the mealybug hardware photos) or a SameBoy-style decoupled
`mode_for_interrupt` separates them.

---

## 7. Seams for the port (cleanest insertion points)

The current model hard-codes the **cc+4 end view** in exactly two structural
places, and that is what a SameBoy-style deferred-commit read must change.

### Where the cc+4 end-view is hard-coded
1. **`Bus::read`/`read_inc` order** (`interconnect.rs:672-681, 713-719`):
   `tick_machine()` runs *before* `read_no_tick()`. The read always sees
   fully-advanced PPU state. SameBoy's `cycle_read` samples the bus at the
   M-cycle **leading edge** (cc+0) and defers the commit. → **Seam A.**
2. **`vis_mode()` reads post-tick `self.dot`** (`ppu/stat_irq.rs:10-41`,
   `ppu/regs.rs:119`). The mode is recomputed live from the advanced dot; there
   is no separate "mode latched at read time" or "mode_for_interrupt". → **Seam B.**

The `event_phase`/`lead_eighths`/`ACCESS_PHASE`/`stamp_blocks` machinery
(`interconnect.rs:43-198`) and the three `Option<u8>` edge stamps are the
**hand-fit half-dot approximation** of (1)+(2). The parity plan
(`docs/sameboy-parity-plan.md:48-50`) calls for **retiring** them in S3.

### Cleanest insertion points

**Seam A — deferred-commit read (sample at the M-cycle leading edge).**
Introduce a read path that samples the bus value *before* `tick_machine`, then
ticks, then returns the pre-tick value — the SameBoy `cycle_read`
sample-then-defer shape. Functions that change:
- `Interconnect::read` / `read_inc` (`interconnect.rs:672-681, 713-719`) — reorder
  to sample-then-tick (or sample a snapshot, tick, return snapshot).
- `Interconnect::write` (`interconnect.rs:683-700`) already half-does this for
  rendering registers via `stage_write` (`ppu/regs.rs:19-45`, the mid-cycle
  `PipeRegs` view, `ppu/mod.rs:183-213`) — that is the existing precedent for
  "the CPU drives the bus mid-cycle", and the model to generalize to *reads*.
- The OAM-DMA conflict snapshot (`read_no_tick` `dma_conflict` block,
  `memory.rs:99-123`) already assumes the conflict state of the just-ticked
  cycle; a leading-edge read must snapshot it pre-tick.

**Seam B — decoupled `visible_mode` / `mode_for_interrupt`.**
Replace the single `vis_mode()` (`ppu/stat_irq.rs:10-41`) with two cycle-exact
views (SameBoy `display.c:1782-1799`): a `visible_mode` for FF41 CPU reads and a
`mode_for_interrupt` for the STAT-IRQ predicate, with the mode-0 boundary's
sub-dot exit stagger (STAT&3→0, VRAM/OAM unblock at X, palette re-block X+3,
unblock X+5) instead of the flip/`render_finished` two-anchor scheme. Functions
that change:
- `vis_mode` / `mode_bits` (`ppu/stat_irq.rs:10-47`) → split.
- `Ppu::read(0xFF41)` (`ppu/regs.rs:119`) → reads `visible_mode`.
- The STAT-IRQ event consumers in `stat_events_tick` (`stat_irq.rs:233-324`) and
  `stat_line_level` (`stat_irq.rs:103-163`) → read `mode_for_interrupt`.
- `m0_flip_events` (`mode0.rs:82-175`) — its `lead`/`proj` projection
  (`mode0.rs:143-144`) and the `m0_src`/`line_render_done`/`render_finished`
  anchors become the cycle-exact boundary dots; the three half-dot flip stamps
  (`m0_access_flip`/`pal_access_flip`/`m0_stat_flip`) are retired in favor of
  per-read accessibility back-dating.

**The atomic constraint** (from `docs/sameboy-parity-plan.md:46`): Seam A + Seam B
are **not** net-zero against the current boundaries — they require shifting the
mode-boundary / IRQ-dispatch dots to SameBoy's frame too, so the foundation and
the first boundary set must land together. The `event_phase` scaffold can stay as
a parallel net-zero path during the port, then be deleted at S3.

### Quick seam index
| seam | current site | port change |
|---|---|---|
| read sampling phase | `interconnect.rs:672` (`tick_machine` before `read_no_tick`) | sample bus at leading edge, defer commit |
| mode readout | `ppu/stat_irq.rs:10` (`vis_mode`) + `ppu/regs.rs:119` | split `visible_mode` / `mode_for_interrupt` |
| mode-0 boundary dots | `ppu/render/mode0.rs:143` (`lead`) + `advance_lx` anchors | cycle-exact boundary + exit stagger |
| access blocking | `ppu/blocking.rs` + the `stamp_blocks` overrides in `memory.rs` | per-read accessibility back-dating |
| STAT-IRQ dispatch | `ppu/stat_irq.rs:233` (`stat_events_tick`) | rising-edge IF on `mode_for_interrupt` |
| write timing | `ppu/regs.rs:19` (`stage_write`/`PipeRegs`) | port SameBoy `cycle_write` conflict map |
| edge-stamp scaffold (to retire) | `interconnect.rs:43-198` + the 3 `Option<u8>` fields | delete at S3 once back-dating subsumes it |
