# SGB ICD2 bridge (SNES↔GB interface chip) — spec + crossing design

The ICD2 is the SGB cartridge's SNES-side interface chip: the SNES CPU sees the
Game Boy through it. This file is the clean-room register spec (extracted from
nocash **fullsnes**, chapter "SNES Cart Super Gameboy") plus slopgb's design for
carrying it across the wasm plugin boundary. Everything here derives from
fullsnes and Pan Docs only — never an emulator source or the SGB BIOS
(clean-room law).

## Register map (fullsnes "SGB I/O Map (ICD2-R)")

Visible to the SNES CPU at `$6000-$7FFF` (the chip decodes only
`A0-A3, A11-A15, A22`, so the block mirrors across `xx6xxN/xx7xxN`; slopgb maps
the canonical addresses and treats the rest as open bus until a title proves
otherwise).

| Addr | R/W | Name | Bits |
|---|---|---|---|
| `$6000` | R | LCD character row + write-buffer | 7-3 current GB LCD char row (0..$11, $11 = last row / vblank); 2 zero; 1-0 current *write* buffer row (0..3) |
| `$6001` | W | Char-buffer read-row select | 1-0 select the *read* buffer row for `$7800`; write resets the `$7800` index to 0 |
| `$6002` | R | Packet available | bit 0: a 16-byte SGB command packet is readable at `$7000-$700F` |
| `$6003` | W | Reset / multiplayer / speed | 7 GB CPU reset (0=reset, 1=normal); 5-4 num_controllers (0,1,3 = 1,2,4 players); 1-0 SGB CPU speed (0..3 = 5/4/3/2.3 MHz, default 1) |
| `$6004-$6007` | W | Controller data, players 1-4 | active low: 7 Start, 6 Select, 5 B, 4 A, 3 Down, 2 Up, 1 Left, 0 Right |
| `$6008-$600E` | — | Unused | open bus / `$600F` mirror on some chips |
| `$600F` | R | Chip version | `$21` / `$61` = ICD2-R revisions (ICD2-N unknown); slopgb returns `$21` |
| `$7000-$700F` | R | 16-byte command packet | reading `$7000` (only) clears the `$6002` flag |
| `$7800` | R | Character buffer data | 320 bytes/row from the `$6001`-selected row; index auto-increments per read; 320..511 read `$FF`; wraps at 512; index reset by `$6001` write |
| `$7801-$780F` | R | `$7800` mirrors | not open bus |

Semantics worth pinning (each has a unit test):

- **Packet handshake:** `$6002` bit 0 sets when a packet lands; a `$7000` read
  clears it (reads of `$7001-$700F` do not). The SGB BIOS also forwards six
  reset-time packets carrying GB cart header bytes `$0104-$014F` — slopgb's HLE
  boot already emits these on the GB side, so the tee delivers them naturally.
- **Pad direction:** `$6004-$6007` is how the SNES *feeds* the GB joypad — the
  return path this whole phase exists for. The bit layout maps 1:1 onto the GB
  P1 matrix: low nibble = d-pad column (Down/Up/Left/Right = bits 3-0), high
  nibble = button column (Start/Select/B/A = bits 7-4), both active low —
  exactly `crates/slopgb-core/src/joypad.rs`'s `(buttons << 4) | dpad` shape.
- **Char buffer:** `$7800` is the GB screen path (the SNES DMAs 320-byte tile
  rows out of it). Four row buffers; `$6000` bits 1-0 name the one being
  written, `$6001` picks the one to read.
- **`$6003` GB reset** (bit 7 = 0) is a *GB-side mutation* — not wired in
  slopgb (golden-safe: the coprocessor never mutates the GB core); captured and
  logged so a title that needs it is visible. Multiplayer count is already
  handled GB-side by the HLE MLT_REQ path.

## JUMP / SNES-side notes (fullsnes "SGB Commands" details)

- `JUMP` always destroys the SNES NMI vector (even when the target is
  `$000000`); after `JUMP` all RAM is usable except `$0000BB-$0000BD` (the NMI
  vector). Only NMIs can be hooked — IRQ/COP/BRK vectors live in the (unshipped)
  BIOS ROM, so uploaded programs rely on NMI alone. This makes Phase-3 NMI the
  one interrupt that matters.
- `JUMP` can return via a 16-bit return address forcing program bank `$00`.
- The APU boot ROM can be re-entered via `MOV [$2140], $FE`.

## Crossing design (the one architecture decision)

Constraints: `LoadedCoprocessor` stays generic (`reset`/`run_until`/`port_*`/
`write_ram`/`read_ram`/`set_pc` only — no ABI widening, no host special-casing);
read side-effects (`$7000` clears `$6002`; `$7800` auto-increments) must happen
**synchronously with the SNES CPU's read**, which executes inside the wasm
sandbox where the host cannot intervene per-access.

**Decision: the ICD2 register block lives in the w65c816 plugin's bus** (an
`icd2` module in `slopgb-w65c816-plugin`, natively unit-testable), and the host
(`slopgb-sgb-coprocessor`) pumps it through *out-of-band host-window addresses*
on the existing `read_ram`/`write_ram` calls:

- The 65C816 bus is 24-bit, so any `u32` address `>= 0x0100_0000` can never be
  a CPU address. The plugin reserves a **host window** there:
  - `HOST_ICD2` (`0x0100_0000` +): packet deposit (16 bytes + an arm byte that
    sets the `$6002` flag), the four pad latches (host reads what the SNES
    wrote), the `$6000` LCD-row shadow (host writes it each pump), captured
    `$6003` writes, and the `$7800` row buffers (host writes GB tile rows).
  - Later phases extend the same window: an MMIO write-capture ring
    (`$21xx`/`$42xx`/`$43xx` writes as `(addr, val)` entries) and host-fed read
    shadows (RDNMI/HVBJOY/joypad autopoll), plus an NMI-request byte.
- `write_ram`/`read_ram` with addresses `< 0x0100_0000` keep their existing
  meaning (raw memory install — firmware, `DATA_SND`, `DATA_TRN`), so every
  existing caller is untouched and a generic host sees one uniform ABI.
- Host pump protocol (per `flush`): deposit the next teed GB packet **only
  when the guest-visible `$6002` flag is clear** (read via the window — never
  overwrite an unconsumed packet); read the pad latches and hand them to
  `AudioCoprocessor::joypad_feed`; refresh the LCD-row shadow.

Read-side-effect strategy, explicitly: the flag-clear on `$7000` and the
`$7800` index auto-increment run inside the plugin's `Bus::read` (synchronous,
correct); the host only observes their aftermath through the window. Registers
with *host*-side effects (`$6003` reset bit) are capture-only.

## Status

- **Core packet tee** (landed): every accepted 16-byte packet (MLT_REQ and
  mid-command packets included) is queued on the joypad's SGB state and
  drained via `SgbCommandSource::take_packet` (`sgb::SGB_PACKET_LEN`);
  bounded at 16, serialized (save-state v8). The HLE presentation path is
  untouched — a tee, not a takeover.
- **Core joypad return seam** (landed): `AudioCoprocessor::joypad_feed()
  -> Option<[u8; 4]>`, polled on `GameBoy::step`; `Some` installs the ICD2
  pad latches as the P1 line source (per-current-player, IRQ edges intact).
  Default `None` = local matrix live (golden-safe); the feed is transient
  across save states (a live coprocessor re-feeds next step).
- **Plugin ICD2 block** (landed): `slopgb-w65c816-plugin/src/icd2.rs`
  implements the register table above on the hosted CPU's bus (synchronous
  `$7000` flag clear + `$7800` auto-increment, the `[600Fh]=21h` garbage
  mirrors, sparse A0-A3/A11-A15 decode) with the host window at
  `HOST_WIN = 0x0100_0000` (`HW_PACKET`/`HW_PADS`/`HW_LCD_ROW`/`HW_CONTROL`/
  `HW_CHAR_ROWS`) on the unchanged `write_ram`/`read_ram` ABI. ICD2 state
  rides the plugin save state.
- **Real memory map** (landed): 128 KB WRAM at `$7E-$7F` + the bank-0/`$80`
  low-8K mirror, the `$8000-$FFFF` program area (one 32 KB RAM-backed image
  aliased across system banks — the unshipped BIOS ROM region the host
  installs clean-room firmware into), I/O space and HiROM banks open-bus;
  ports + ICD2 gated to A22=0 (system banks). DATA_TRN's `$7F:0100` target
  and JUMP's `$00:1800` (= `$7E:1800` via the mirror) now resolve correctly.
- **Host pump** (landed): `SgbCoprocessor::flush` deposits teed packets when
  the guest-visible `$6002` flag is clear, reads the pad latches back into
  `joypad_feed` (sticky-written gated), and maintains the `$6000` LCD-row
  shadow off the GB frame position.
- **MMIO capture + shadows** (landed): write-capture ring (`$2100-$213F`,
  `$2180-$2183`, `$4000-$44FF`) drained per flush; host-fed read shadows for
  `$4200-$421F` + `$4016/17` with guest-side RDNMI/TIMEUP read-clear.
- **NMI + frame clock** (landed): hardware NMI in `slopgb-w65c816`
  (datasheet vectors, both modes, WAI wake) + `HW_NMI` trigger; the GB frame
  position scaled onto 262 NTSC lines drives the vblank edge — RDNMI/HVBJOY
  shadows + one NMI per frame when the guest's own NMITIMEN bit 7 asks.
- **GP-DMA + WRAM B-bus port** (landed): see the section below.
- **Joypad autopoll** (landed): `AudioCoprocessor::set_input` pushes the
  GB-side physical matrix each step (default drops it — golden-safe);
  when the guest's NMITIMEN bit 0 asks, the vblank edge arms the HVBJOY
  busy bit and the next flush publishes the mapped `$4218/$4219` shadows
  as busy drops (values valid only after the window, fullsnes "AUTO
  JOYPAD READ"). GB A/B/Select/Start + d-pad → SNES namesakes; Y/X/L/R
  and JOY2-4 read 0 (one physical controller). Manual `$4016` serial
  bit-shifting is not modeled (shadows are static bytes) — extend if a
  title manual-polls.
- **Pilot probe** (headless, scratch): ARCADE-mode select on Space Invaders
  runs DATA_SND bootstrap → JUMP → dispatcher → DATA_TRN payload staging on
  the real wasm 65C816; the arcade program enters and now needs the PPU
  (`$21xx`) + joypad autopoll to survive its hardware init.
- **Next**: probe whether the pilot touches the multiplier/divider
  (`$4202-$4206`), then the SNES PPU plugin chain (`goal.md` T13-T18).

## GP-DMA (`$420B` / `$43x0-$43x6`) + the WRAM port (`$2180-$2183`)

The GP-DMA engine is **host-side** (`slopgb-sgb-coprocessor/src/dma.rs`),
executing off captured `$43xx`/`$420B` writes — the same consumer path a
future PPU gets (`bbus_write` routes B-bus bytes through `apply_mmio`, so
DMA feeds the PPU for free once `$21xx` routing lands).

- **Atomicity**: a nonzero `$420B` write stalls the plugin CPU (absorbing
  cycles like STP/WAI — hardware pauses the CPU during GP-DMA, fullsnes
  420Bh) until the host drains the ring, runs the transfer, and clears
  `HW_DMA_STALL`. Post-trigger guest code can never observe a half-applied
  transfer, despite the polled ring.
- Semantics per fullsnes "SNES DMA and HDMA Channel 0..7 Registers": unit
  modes 0-7 (5-7 = repeats of 1-3), A-bus increment/decrement/fixed, B→A
  direction, byte counter with `0 = $10000`, channel 0-first order, working
  registers left stepped (A1T final, DAS zero), constant bank byte.
- The `$2180` WMDATA / 17-bit WMADD state machine lives host-side; guest
  `$2180-$2183` *writes* are captured and applied in order. WRAM-to-WRAM
  DMA is suppressed byte-wise but completes (fullsnes 2183h "DMA Notes").
- Ceilings (deliberate): guest *reads* of `$2180` are open bus (no game
  observed doing this; the address machine is host-side); H-DMA (`$420C`)
  inert in the ring; DMA `$43xx` read-back unimplemented (host-side state
  only); per-byte wasm crossings (bulk when PPU uploads make size matter);
  a `$420B` entry dropped by ring overflow loses its transfer (the CPU is
  still un-stalled; only the overflow warning fires — unreachable for real
  write rates, see the `MMIO_RING_CAP` comment); the CPU multiplier/divider
  (`$4202-$4206` → `$4214-$4217`) is unimplemented — a byte-scan of the
  pilot's whole 4 KB arcade payload finds no absolute-addressed access to
  either block, and the resident BIOS surface is an RTS sled with no math
  services to call, so the `$4214-$4217` shadows read 0; implement when a
  probe shows a title touching them.
