# Serial, SGB, MBC, public API, audio frontend

## Serial

### Master clock flip-flop

- Serial clock is a master flip-flop toggled by DIV falling edges; it shifts on the high→low toggle.
- **Any SC write resets the flip-flop** — the first bit then lands on the *second* edge after the write (SameBoy `GB_serial_master_edge`, gambatte serial/ fully green).
- FF04 writes reach the serial within the cycle via `Serial::div_write` — the sampled tick would miss the fast clock's reset edge.

### Clock source per speed mode

| Mode | DIV bit toggling the flip-flop |
|---|---|
| Normal speed | bit-7 |
| CGB fast (double speed) | bit-2 |

## SGB joypad

- `src/joypad.rs` `Sgb`: ICD2 command-packet receiver + MLT_REQ multiplexing.
- Gated on Sgb/Sgb2 *and* the header SGB flag (`Cartridge::supports_sgb`: $146=$03 ∧ $14B=$33).
- Joypad-ID increments on JOYP bit-5 rising edges.
- The glitched MLT_REQ mode 2 is pinned by SameSuite sgb/ (both green).
- Only MLT_REQ executes — other commands are SNES-side only.

## MBC30

- MBC30 = MBC3 cart with >2 MiB ROM or >32 KiB RAM (SameBoy detection): 8-bit ROM-bank register, 8 RAM banks.
- mbc3-tester [Dmg] green.
- Its [Cgb] reference PNG green contradicts the suite's own howto (asset defect, see `tests/gbtr/smallsuites.rs`).

## MBC6

- Type 0x20 (`cartridge/mbc6.rs`), per Pan Docs "MBC6": two 8 KiB ROM/flash
  windows (A: 4000-5FFF via regs 2000/2800, B: 6000-7FFF via 3000/3800, bit 3
  of the select maps the flash) + two 4 KiB RAM windows (A: A000 bank reg
  0400, B: B000 bank reg 0800), RAMG at 0000-03FF (MBC1-style low-nibble
  $0A), flash /CE at 0C00 bit 0, flash /WP at 1000 bit 0. Battery forced on
  (the type byte has no +BATTERY variant; the only cart, Net de Get, has one).
- MX29F008 flash model: 1 MiB (eight 128 KiB sectors) + hidden 256-byte
  region, JEDEC unlock at flash addresses 5555/2AAA (= 2:5555/1:4AAA through
  either window), full command set (ID C2/81, sector/chip erase, program,
  hidden erase/program/read, protect/unprotect sector 0). Program = AND
  (bits only clear); sector 0 is double-gated (/WP bit **and** the protect
  command, the latter reported in status bit 1).
- Persistence: the battery .sav is SRAM + a flash trailer (1 MiB array +
  256-byte hidden region + 1 protect byte, `save_data`/`load_mbc6_trailer`);
  a foreign SRAM-only .sav still imports (flash stays fresh). Save states
  carry the full chip too.
- Blocked operations (sector 0 with /WP low or command-protected, hidden
  region with /WP low) never start: the chip stays in read mode, no status
  byte appears — the exerciser relies on data reads after blocked ops. Any
  out-of-sequence command cycle resets the whole JEDEC machine including a
  pending two-cycle prefix.
- Programming follows the chip's page protocol: 128 data loads into a page
  buffer (any value is data, $F0 included), then a rewrite of the page's
  final address commits (trigger only — its value is not data; $F0 there
  aborts the page). AND semantics on commit; a partial page never lands.
  The in-flight buffer is serialized in save states.
- Embedded operations run on the emulated clock (`Cartridge::tick_time`,
  the seam the MBC3 RTC uses): status bit 7 reads 0 and bus writes are
  ignored until the operation's duration elapses (~1.5 ms page program /
  protect, ~0.5 s block erase, 8×that for chip erase — order-of-magnitude
  typical MX29F008-family figures; Pan Docs gives none). The timeout bit 4
  never rises: it reports exceeding the chip's internal retry limit, a
  failure a healthy modeled chip cannot have. The busy counter is
  serialized in save states.
- Audited against every Pan Docs claim (57 claims: all accurate or the
  maximal deterministic reading; SameBoy has no MBC6 support to
  cross-reference, so Pan Docs is the sole oracle).
- Decode choices where Pan Docs is silent: the /WP register (listed only as
  "1000") decodes at 0x1000-0x13FF, the 1 KiB granularity of its neighbors;
  writes to 1400-1FFF do nothing.
- Debug indicators use the coarse units of the banked debug consumers:
  `cur_rom_bank`/`cur_ram_bank` report window A's 16 KiB ROM pair / 8 KiB
  SRAM pair; `rom_bank_at(addr)` resolves window B's independent pair for
  the memory-viewer labels.
- Pinned by the committed `roms/mbc6` exerciser (21 tests, RGBDS + wla-dx
  twin sources, `tests/mbc6.rs` runs both on DMG + CGB) and the
  `cartridge_tests/mbc6.rs` unit suite. Exerciser + unit suite were
  mutation-checked (no-op erase / whole-chip erase mutants are killed).

## Core public API

- Curated facade: `GameBoy`, `Registers`, `Button`, `CartridgeError`, `Model` + screen/clock consts.
- Keep internals `pub(crate)`; new integration-test escape hatches go behind `#[doc(hidden)]`.

## Audio frontend

- Hand-rolled lock-free SPSC ring (`crates/slopgb/src/audio.rs`).
- The cpal callback must never lock or allocate — keep it that way.
