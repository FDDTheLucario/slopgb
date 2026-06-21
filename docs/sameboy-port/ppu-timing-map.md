# SameBoy cycle-exact PPU timing — port map for slopgb

Source: `/tmp/sbbuild/SameBoy-1.0.2/Core/` (SameBoy 1.0.2). Every claim below is grounded
in `file:line` against that tree. Read this top-to-bottom: §0 establishes the clock/unit
model that every other section depends on, and §6 (the kernel pair) is the payoff that the
rest exists to support.

---

## 0. Clock, units, and the coroutine model (read this first)

SameBoy's PPU is **not** a "switch on dot, advance 1" loop. It is a **stackless coroutine**
written as straight-line C with `GB_SLEEP` yield points, driven by an accumulator.

### Unit system
- `GB_advance_cycles(gb, cycles)` (`timing.c:432`) receives **4 MHz T-cycles** (what the CPU
  burned this M-cycle).
- Single speed doubles them, double speed does not:
  ```c
  // timing.c:478-480
  if (unlikely(!gb->cgb_double_speed)) {
      cycles <<= 1;
  }
  ```
  So the PPU's internal accumulator counts **"8 MHz units" = half-dots**. The comment at
  `display.c:2206` confirms ("8MHz units").
- Then `GB_display_run(gb, cycles, false)` is called (`timing.c:510`).
- The display state machine uses **divisor 2**:
  ```c
  // display.c:1601
  GB_BATCHABLE_STATE_MACHINE(gb, display, cycles, 2, !force) {
  ```
  and `GB_SLEEP(gb, display, state, N)` subtracts `N * divisor = N*2` from `display_cycles`
  (`timing.h:25-32`). So **one `GB_SLEEP` "cycle" = 1 dot = 2 internal half-dot units.**

Net cadence:
| Mode | 1 CPU T-cycle advances PPU by | resolution exposed |
|---|---|---|
| Single speed | `<<1` → 2 units → **1 dot** | whole-dot |
| Double speed | 1 unit → **½ dot** | **half-dot** |

This is the single most important porting fact: **at double speed the PPU has half-dot
resolution relative to the CPU**, and SameBoy relies on it (see §7, and the `_ds` test legs).

### The coroutine / `GB_SLEEP` mechanism
```c
// timing.h:25-32
#define GB_SLEEP(gb, unit, state, cycles) do {\
    (gb)->unit##_cycles -= (cycles) * __state_machine_divisor; \
    if (unlikely((gb)->unit##_cycles <= 0)) {\
        (gb)->unit##_state = state;\
        return;\                 /* yield: save resume label, bail out */
        unit##state:; \          /* resume target (computed goto) */
    }\
} while (0)
```
`GB_STATE(gb, display, N)` is `case N: goto displayN;` (`timing.h:56`). On entry the big
`switch (display_state)` (`display.c:1601-1644`) jumps straight back to the `GB_SLEEP` that
yielded. So the PPU "remembers where it was" by an integer `display_state` plus the signed
`display_cycles` balance. There is **no per-dot dispatch**; a whole `MODE2_LENGTH` or a whole
mode-3 can be consumed in one straight-line pass, and `GB_BATCHPOINT` (`timing.h:34-40`) lets
quiescent lines be skipped wholesale.

### Force-sync — the hinge for CPU/PPU coherence
```c
// display.h:51
#define GB_display_sync(gb) GB_display_run(gb, 0, true)
```
`force=true` runs the coroutine forward to *exactly the current cycle* with batching
disabled. **Every CPU access that can observe or perturb PPU state calls `GB_display_sync`
first** (see §3/§6), so the value the CPU sees is the PPU's state at the precise T-cycle (or
half-dot, at double speed) of that access's bus M-cycle.

### Constants
```c
// display.c:158-164
#define MODE2_LENGTH (80)
#define LINE_LENGTH  (456)
#define LINES        (144)
#define WIDTH        (160)
#define VIRTUAL_LINES (LCDC_PERIOD / LINE_LENGTH)   // = 154
// gb.h:258
#define LCDC_PERIOD 70224
```

### The decisive design note (verbatim)
```c
// display.c:1529-1532
/*
 TODO: It seems that the STAT register's mode bits are always "late" by 4 T-cycles.
       The PPU logic can be greatly simplified if that delay is simply emulated.
 */
```
SameBoy does **not** take that shortcut; instead it bakes the lateness into the explicit
ordering of `GB_SLEEP` boundaries (§2, §6). The visible STAT mode lags the interrupt-facing
mode by ~1 M-cycle, and the *sign* of that lag differs between mode entries — which is what
the whole kernel pair turns on.

---

## 1. The PPU advance loop & the mode-3 → mode-0 transition

`GB_display_run` (`display.c:1533`) is the whole PPU. Its shape per visible line:

1. **Pre-amble / wraparound guards** (`display.c:1535-1599`): WY-check scheduling, the
   end-of-line overrun split (`display.c:1563-1574` — if this batch crosses `LINE_LENGTH`,
   recurse for the first chunk, then force `display_state = 9`), the delayed glitch HBlank
   IRQ (`1575-1580`), and STOP handling.
2. **State dispatch** (`display.c:1601-1644`): jump to the saved resume point.
3. **Line 0 mode-2 special case** (`display.c:1664-1714`): hand-unrolled because the OAM
   interrupt does *not* fire 1 cycle early on line 0.
4. **Per-line loop** `for (; gb->current_line < LINES; ...)` (`display.c:1760`):
   - **Mode 2 (OAM search), 80 dots**: `display.c:1767-1836`. Sets `oam_write_blocked`,
     `oam_read_blocked`, walks 40 OAM entries 2 dots each (`GB_SLEEP state 8`,
     `display.c:1812`), then arms mode 3.
   - **Mode 3 (pixel transfer)**: `mode_3_start:` (`display.c:1845`) through the
     `while (true)` fetcher/FIFO loop (`display.c:1872-2042`). Either the batched fast path
     (`display.c:1856-1870`) or the slow per-pixel path. Each emitted pixel /
     fetcher step does `gb->cycles_for_line++; GB_SLEEP(...,1)`.
   - **Mode 0 (HBlank)**: entered right after the pixel loop breaks (`position_in_line==160`,
     `display.c:2034`).
5. **VBlank** lines 144-152 (`display.c:2152-2215`) and line 153 (`display.c:2217-2244`).

### Mode 3 → Mode 0 transition (quote)
The pixel loop exits when the 160th pixel is placed:
```c
// display.c:2032-2034
render_pixel_if_possible(gb);
advance_fetcher_state_machine(gb, &cycles);
if (gb->position_in_line == 160) break;
```
Then HBlank is entered in **two decoupled steps** (single speed):
```c
// display.c:2090-2108
if (!gb->cgb_double_speed) {
    gb->io_registers[GB_IO_STAT] &= ~3;     // (A) VISIBLE mode -> 0  (no STAT_update!)
    gb->mode_for_interrupt = 0;
    gb->oam_read_blocked = gb->model >= GB_MODEL_CGB_D;
    gb->vram_read_blocked = false;
    gb->oam_write_blocked = false;
    gb->vram_write_blocked = false;
}
gb->cycles_for_line++;
GB_SLEEP(gb, display, 22, 1);                // (B) 1 dot passes
gb->io_registers[GB_IO_STAT] &= ~3;
gb->mode_for_interrupt = 0;
gb->oam_read_blocked = false;
gb->vram_read_blocked = false;
gb->oam_write_blocked = false;
gb->vram_write_blocked = false;
GB_STAT_update(gb);                          // (C) mode-0 STAT IRQ fires HERE
```
**Key:** the *visible* STAT mode becomes 0 at (A), but the *mode-0 STAT interrupt* fires one
dot later at (C). (Holds the symmetric-but-opposite relationship to mode-2 entry; see §2/§6.)

---

## 2. `mode` (CPU-visible) vs `mode_for_interrupt`

Two distinct fields:

| Field | Decl | Meaning | Read by CPU? |
|---|---|---|---|
| `io_registers[GB_IO_STAT] & 3` | — | **Visible** mode the CPU reads from FF41 | **yes** (`memory.c:630`) |
| `gb->mode_for_interrupt` | `gb.h:612` (`uint8_t`) | mode that *feeds the STAT IRQ line* | no (internal) |

CPU read of FF41 returns the **visible** field only:
```c
// memory.c:629-630
case GB_IO_STAT:
    return gb->io_registers[GB_IO_STAT] | 0x80;
```
(reached after `sync_ppu_if_needed(gb, addr)` at `memory.c:623`, so it's exact).

The STAT IRQ line is computed **solely** from `mode_for_interrupt`:
```c
// display.c:545-550
switch (gb->mode_for_interrupt) {
    case 0: gb->stat_interrupt_line = gb->io_registers[GB_IO_STAT] & 8;    break;
    case 1: gb->stat_interrupt_line = gb->io_registers[GB_IO_STAT] & 0x10; break;
    case 2: gb->stat_interrupt_line = gb->io_registers[GB_IO_STAT] & 0x20; break;
    default: gb->stat_interrupt_line = false;
}
```
`mode_for_interrupt == -1` (i.e. `0xFF`, `default`) is a deliberate "no source" state used to
force the line low between transitions (e.g. `display.c:1799-1800`).

### The offset between them is NOT a fixed constant — its sign flips per mode entry
- **Mode-2 entry (lines 1-143):** the IRQ-facing mode goes to 2 **one dot before** the visible
  mode does:
  ```c
  // display.c:1778-1798
  /* The OAM STAT interrupt occurs 1 T-cycle before STAT actually changes, except on line 0.
     PPU glitch? */
  if (gb->current_line != 0) {
      gb->mode_for_interrupt = 2;            // IRQ mode -> 2 now
      gb->io_registers[GB_IO_STAT] &= ~3;    // ...but visible mode still 0 here
  }
  ...
  GB_STAT_update(gb);                        // mode-2 IRQ can fire
  GB_SLEEP(gb, display, 7, 1);               // 1 dot
  ...
  gb->io_registers[GB_IO_STAT] |= 2;         // visible mode -> 2, one dot LATER
  ```
- **Mode-0 entry:** the visible mode goes to 0 **one dot before** the IRQ fires (the (A)→(C)
  ordering in §1).

So: at the mode-2 boundary the interrupt leads the visible byte by +1 dot; at the mode-0
boundary the interrupt trails it by −1 dot. That 2-dot total swing is the crux of §6.

---

## 3. Accessibility (OAM / VRAM / palette blocking)

Five CPU-facing block flags (`gb.h:582-585, 614`):
`oam_read_blocked, vram_read_blocked, oam_write_blocked, vram_write_blocked,
cgb_palettes_blocked`. (Distinct `*_ppu_blocked` flags at `gb.h:617-619` are for the PPU's
*own* OAM/VRAM fetches during DMA conflicts — not the CPU path.)

### Read paths consult the flags AFTER a force-sync
- **VRAM (8000-9FFF):**
  ```c
  // memory.c:294-309
  static uint8_t read_vram(GB_gameboy_t *gb, uint16_t addr) {
      if (likely(!GB_is_dma_active(gb))) {
          GB_display_sync(gb);
      }
      ...
      if (unlikely(gb->vram_read_blocked && !gb->in_dma_read)) {
          return 0xFF;
      }
  ```
  (plus a mode-3 "data bus" replay path at `memory.c:310-335` for the `display_state == 22`
  edge.)
- **OAM (FE00-FE9F):** `read_high_memory` (`memory.c:546-619`): `GB_display_sync(gb)` at
  `memory.c:547`, then
  ```c
  // memory.c:560
  if (gb->oam_read_blocked) { ...corruption on DMG... return 0xFF; }
  ...
  return GB_read_oam(gb, addr);   // memory.c:619
  ```
- **CGB palettes (FF69 BGPD / FF6B OBPD):**
  ```c
  // memory.c:699-712
  case GB_IO_BGPD:
  case GB_IO_OBPD: {
      if (!gb->cgb_mode && gb->boot_rom_finished) return 0xFF;
      if (gb->cgb_palettes_blocked) return 0xFF;
      ...
  }
  ```
  (reached after `sync_ppu_if_needed` at `memory.c:623`).

### Per-line block timeline (single-speed DMG/CGB; see §7 for DS deltas)
Walking one visible line in `display.c`:
| Dot region | Code | oam_rd | oam_wr | vram_rd | vram_wr | pal |
|---|---|---|---|---|---|---|
| mode-2 first 2 dots | `1767-1770` | open | CGB:blk | open | open | open |
| mode-2 body | `1771-1822` | blk (`1790`) | blk (`1794`) | open until idx37 | open | open |
| mode-3 arm | `1827-1844` | blk | blk | **blk** (`1830`) | **blk** (`1831`) | open→blk (`1842`) |
| mode-3 body | pixel loop | blk | blk | blk | blk | blk |
| **mode-0 (A)** | `2090-2097` | model-dep | **open** | **open** | **open** | blk |
| mode-0 +1 dot (C) | `2102-2108` | **open** | open | open | open | blk |
| mode-0 +3 dots | `2111-2113` | open | open | open | open | **blk** (2-dot pulse) |
| mode-0 +5 dots | `2119-2121` | open | open | open | open | **open** (`2121`) |

So **OAM/VRAM unblock at the visible mode-3→0 boundary** (`display.c:2090-2096`, step (A)),
i.e. on the same dot the visible mode reads 0. CGB palettes are special: they *stay* blocked
across the boundary and get a separate **2-dot blocked pulse** in early HBlank
(`display.c:2113` sets `cgb_palettes_blocked = !cgb_double_speed`, `display.c:2121` clears it).
VRAM/OAM *block* points in mode 2/3 are at `display.c:1790-1834`.

---

## 4. The STAT IRQ line (`GB_STAT_update`)

```c
// display.c:523-560
void GB_STAT_update(GB_gameboy_t *gb) {
    if (!(gb->io_registers[GB_IO_STAT] & 0x10 ... ENABLE)) return;       // LCD off -> no-op
    if (GB_is_dma_active(gb) && (gb->io_registers[GB_IO_STAT] & 3) == 2) // OAM-DMA hides mode 2
        gb->io_registers[GB_IO_STAT] &= ~3;

    bool previous_interrupt_line = gb->stat_interrupt_line;

    /* LY==LYC bit */
    if (gb->ly_for_comparison != (uint16_t)-1 || (gb->model <= GB_MODEL_CGB_C && !gb->cgb_double_speed)) {
        if (gb->ly_for_comparison == gb->io_registers[GB_IO_LYC]) {
            gb->lyc_interrupt_line = true;  gb->io_registers[GB_IO_STAT] |= 4;
        } else { ... gb->io_registers[GB_IO_STAT] &= ~4; }
    }

    switch (gb->mode_for_interrupt) {                       // mode sources
        case 0: gb->stat_interrupt_line = gb->io_registers[GB_IO_STAT] & 8;    break;
        case 1: gb->stat_interrupt_line = gb->io_registers[GB_IO_STAT] & 0x10; break;
        case 2: gb->stat_interrupt_line = gb->io_registers[GB_IO_STAT] & 0x20; break;
        default: gb->stat_interrupt_line = false;
    }
    if ((gb->io_registers[GB_IO_STAT] & 0x40) && gb->lyc_interrupt_line)       // LYC source
        gb->stat_interrupt_line = true;

    if (gb->stat_interrupt_line && !previous_interrupt_line)                   // RISING EDGE
        gb->io_registers[GB_IO_IF] |= 2;
}
```
- The IRQ fires only on a **0→1 rising edge** of `stat_interrupt_line` (`display.c:557-559`);
  the line itself is the OR of (the selected mode source) | (LYC source). This is the
  classic STAT-blocking model: if the line is already high from one source, a second source
  going high produces no new IRQ.
- `ly_for_comparison` (`gb.h:587`, `uint16_t`, `-1` = "don't compare this dot") is the
  *delayed* LY used for LYC, distinct from `current_line` / `io[LY]`.
- The **mode-0 STAT IRQ** fires at `display.c:2108` — i.e. **1 dot after** the visible mode
  reads 0 (`display.c:2091`) and after the pixel pipe has fully ended
  (`position_in_line==160`, `display.c:2034`). There is also a separate
  `delayed_glitch_hblank_interrupt` path (`display.c:1575-1580, 1742-1745, 2164-2167`) for the
  pos-in-line≥156 overrun corner, and a `wx_166_interrupt_glitch` early mode-0 IRQ
  (`display.c:2038-2041`).

---

## 5. Mode-0 entry timing & mode-3 length

Mode 0 begins the dot the pixel loop places pixel 160 (`display.c:2034`). Everything that
lengthens mode 3 therefore *delays* mode 0. The closed-form base (used by the batching fast
path, and matched dot-for-dot by the slow fetcher loop) is:
```c
// display.c:1493  (no objects, no active window)
if (gb->n_visible_objs == 0 && !(gb->wy_triggered && (gb->io_registers[GB_IO_LCDC] & GB_LCDC_WIN_ENABLE)))
    return 167 + (gb->io_registers[GB_IO_SCX] & 7);
```
So **base mode 3 = 167 + (SCX & 7) dots**, and:
- **SCX fine scroll** (`SCX & 7`): the first `SCX&7` pixels are discarded by
  `render_pixel_if_possible` before `position_in_line` advances out of the prologue
  (`display.c:686-704`), and `line_has_fractional_scrolling` (`gb.h` / `display.c:702`) records
  the fractional case. Each discarded pixel still costs a dot → +`SCX&7`.
- **Sprites**: each visible object stalls the FIFO; the object-fetch block
  (`display.c:1946-2026`) burns `cycles_for_line` (`+1` at `1958/1967/2001`, `+2` at
  `1979/1994`) per object, gated by `x_for_object_match` (`display.c:1508-1513`). Penalty is
  position-dependent (5–11 dots/object), so SameBoy can't shortcut it — `mode3_batching_length`
  returns 0 when objects exist *and* a STAT/HBlank IRQ could observe the exact length
  (`display.c:1499-1505`), forcing the slow path.
- **Window**: triggering the window mid-line (`display.c:1883-1931`) refills the BG FIFO
  (`fifo_clear`, `display.c:1915`) and re-fetches, adding ~6 dots; WX==0 with `SCX&7` adds an
  extra dot (`display.c:1917-1920`). Window presence also forces `mode3_batching_length`→0
  (`display.c:1479-1490`).

The CPU never reads a "mode 3 length" register; it observes the boundary purely by *when*
the visible STAT flips 3→0 at step (A). Because that flip is placed by the exact accumulated
`cycles_for_line`, **mode-3 length must be cycle-exact for §6 to land.**

---

## 6. THE KERNEL PAIR — how SameBoy makes two identical `ldh a,(FF41)` reads disagree

**The tests** (gambatte / Sindre Aamås, in `test-roms/.../gambatte/`):
- `m2int_m3stat_1_dmg08_cgb04c_out3` — anchor off a **mode-2** STAT IRQ, then read FF41;
  expects **mode 3**.
- `m0int_m3stat_2_dmg08_cgb04c_out0` — anchor off a **mode-0** (HBlank) STAT IRQ, then read
  FF41; expects **mode 0**.

Both ISRs reduce to the same opcode `ldh a,(FF41)`, and slopgb's whole-dot model computes
both reads onto the byte-identical bus M-cycle, both reading raw mode 0 — so slopgb cannot
satisfy `out3 ∧ out0` and concludes the only discriminator is "which ISR is on the stack."
**That conclusion is an artifact of two missing degrees of freedom in slopgb's model, not a
real hardware fact.** SameBoy passes both with no CPU-context awareness whatsoever. Three
cooperating mechanisms do it:

### (i) Force-sync places each read at its true PPU cycle
The FF41 read runs `sync_ppu_if_needed(gb, GB_IO_STAT)` →
```c
// memory.c:471-498
static inline void sync_ppu_if_needed(GB_gameboy_t *gb, uint8_t register_accessed) {
    switch (register_accessed) {
        ...
        case GB_IO_STAT:
        ...
            GB_display_sync(gb);   // GB_display_run(gb, 0, true)
            break;
    }
}
```
which advances the coroutine to the **exact** T-cycle of that read's bus M-cycle. The PPU is
not sampled "as of the last batch"; it is run forward to the read instant. So whatever
visible mode the coroutine holds at that dot is what the CPU gets. There is no rounding to a
batch boundary that could merge the two reads.

### (ii) Visible mode and interrupt mode are *separate fields* updated on *different dots*
Per §2/§4: the CPU reads `io[STAT] & 3` (`memory.c:630`); the IRQ that *woke the test* came
from `mode_for_interrupt` (`display.c:545-559`). These are updated at distinct `GB_SLEEP`
boundaries. A model that derives "the mode the CPU reads" and "the mode that fired the IRQ"
from one variable (slopgb's `observed=(O<E?3:0)`) literally cannot represent the gap; SameBoy
represents it as two fields with two write sites.

### (iii) The two anchor IRQs sit on OPPOSITE sides of their visible-mode edges
This is the resolver. Each test counts a *fixed* interrupt→read latency (dispatch = 5 M, then
the ISR's `ldh a,(FF41)` read M-cycle). What differs is **where the anchoring IRQ fired
relative to the visible STAT edge it is near**:

- **mode-2 anchor** (`m2int`): the IRQ fires at `display.c:1787`, which is **1 dot *before***
  the visible mode is written to 2 (`display.c:1792`) — the "OAM int 1 T-cycle early" glitch
  (`display.c:1778-1779`). The test then counts forward into the *same line's* mode 3 and
  samples while visible STAT still reads 3 → **out3**.
- **mode-0 anchor** (`m0int`): the IRQ fires at `display.c:2108`, which is **1 dot *after***
  the visible mode was written to 0 (`display.c:2091`). The countdown therefore begins already
  on the far side of the visible 3→0 edge, so the same-shaped read samples mode 0 → **out0**.

Because one anchor *leads* its visible edge by +1 dot and the other *trails* its visible edge
by −1 dot, "the same number of cycles after my anchor IRQ" reaches the visible 3→0 boundary
with a **2-dot relative offset** between the two tests. Combine that with (i) exact sampling
and the cycle-exact mode-3 length from §5 (`167 + (SCX&7)` …), and the two reads land on
opposite sides of the visible 3→0 edge. SameBoy outputs 3 for `m2int_m3stat_1` and 0 for
`m0int_m3stat_2` **deterministically from PPU dot position alone** — the call stack is never
consulted.

### Why slopgb sees a contradiction
slopgb collapses (ii) into one mode variable and lacks the half-dot/decoupled-edge structure
of (iii), so both anchors map to the *same* effective dot `E=4` and the *same* read offset
`O=4`, demanding `O<E ∧ O≥E`. The fix is **not** to tag the ISR — it is to (a) keep
`mode_for_interrupt` as a field separate from the visible STAT mode, (b) write each at the
SameBoy `GB_SLEEP` boundary (mode-2 IRQ-mode set 1 dot *before* visible byte at
`display.c:1781`+`1792`; mode-0 visible byte set 1 dot *before* IRQ at
`display.c:2091`+`2108`), and (c) force-sync the PPU to the exact cycle on every FF41 read.
With those three, the two reads naturally fall on different dots and the contradiction
evaporates.

### Porting checklist for the pair
1. PPU is a cycle-addressable object; CPU FF41/FF44/OAM/VRAM/pal access runs it forward to the
   access cycle first (`GB_display_sync`).
2. Keep `mode_for_interrupt` (drives STAT IRQ) and the visible STAT mode bits as **two**
   fields.
3. Mode-2 entry (lines 1-143 only): set IRQ-mode=2 and raise the IRQ **one dot before** you
   set visible mode=2.
4. Mode-0 entry: set visible mode=0 **one dot before** you set IRQ-mode=0 and raise the IRQ.
5. Mode-3 length = `167 + (SCX&7)` + sprite + window penalties, accumulated exactly so the
   visible 3→0 edge sits on the right dot.

---

## 7. Double speed

### Cadence
Only single speed doubles the input cycles (`timing.c:478-480`, quoted in §0). Consequence:
- **Single speed:** 1 CPU T-cycle = 1 PPU dot (whole-dot grid).
- **Double speed:** 2 CPU T-cycles = 1 PPU dot; one CPU T-cycle advances the PPU by **½ dot**.
  The `display_cycles` accumulator (divisor 2) carries that half-dot so a force-sync can land
  *between* dots. This is why the `_ds` test legs exist and why a whole-dot port fails them.

### Double-speed-gated accessibility / timing differences (grep `cgb_double_speed` in
`display.c`)
- VRAM is blocked during **mode 2** only at double speed, on line 0:
  ```c
  // display.c:1698-1699
  gb->vram_read_blocked = gb->cgb_double_speed;
  gb->vram_write_blocked = gb->cgb_double_speed;
  ```
- OAM write/read block edges shift by model & speed:
  ```c
  // display.c:1767
  gb->oam_write_blocked = GB_is_cgb(gb) && !gb->cgb_double_speed;
  // display.c:1775
  gb->oam_read_blocked = !gb->cgb_double_speed || gb->model >= GB_MODEL_CGB_D;
  ```
- The **mode-3→0 two-step (A)** runs only at single speed (`if (!gb->cgb_double_speed)`,
  `display.c:2090`); at double speed the visible-mode clear and the IRQ collapse toward the
  later step, changing the §6 offsets — hence separate `_ds_1/_ds_2` expectations.
- The early-HBlank palette pulse is single-speed only:
  `gb->cgb_palettes_blocked = !gb->cgb_double_speed;` (`display.c:2113`).
- LY=LYC comparison gating includes a `!gb->cgb_double_speed` clause for `model <= CGB_C`
  (`display.c:532`), and `ly_for_comparison` at line 153 differs by speed (`display.c:2232`).
- The WY-check modulo phase differs by speed/model (`display.c:1542-1549`).
- A double-speed-only STAT-write edge case re-pulses the mode-2 IRQ
  (`memory.c:1545-1553`).

---

## Appendix A — state-field cross reference (`gb.h`)

| Field | Decl | Role |
|---|---|---|
| `mode_for_interrupt` | `gb.h:612` | mode feeding STAT IRQ (`-1`/`0xFF` = none) |
| `stat_interrupt_line` | `gb.h:569` | current STAT line level (edge-detected) |
| `lyc_interrupt_line` | `gb.h:613` | LY==LYC latch |
| `ly_for_comparison` | `gb.h:587` (`u16`) | delayed LY for LYC (`-1` = skip) |
| `current_line` | `gb.h:586` | PPU's true line counter |
| `position_in_line` | `gb.h:568` | pixel X in mode 3 (`-16..160`, wrapping `uint8_t`) |
| `cycles_for_line` | `gb.h:590` (`u16`) | dots consumed this line (drives mode-3 length) |
| `oam/vram_read/write_blocked` | `gb.h:582-585` | CPU-facing access gates |
| `cgb_palettes_blocked` | `gb.h:614` | CPU-facing palette gate |
| `oam/vram/cgb_palettes_ppu_blocked` | `gb.h:617-619` | PPU-own access gates (DMA conflict) |
| `display_state` / `display_cycles` | (via `GB_UNIT`, `timing.h:59`) | coroutine resume label + half-dot balance |
| `delayed_glitch_hblank_interrupt` | `gb.h:630` | pos-in-line≥156 overrun mode-0 IRQ |

## Appendix B — every PPU force-sync site
- `read_vram` `memory.c:298`; OAM/IO read `read_high_memory` `memory.c:547`;
  `sync_ppu_if_needed` for STAT/LY/LCDC/SCX/SCY/LYC/DMA/BGP/OBP/WX/WY/HDMA/palette regs
  `memory.c:471-498` (called at `memory.c:623` for FF00-FF7F reads, and on the write side);
  OAM-bug trigger `memory.c:98`; write paths `memory.c:1010, 1350, 1766`.
- All are `GB_display_sync(gb)` = `GB_display_run(gb, 0, true)` (`display.h:51`).
