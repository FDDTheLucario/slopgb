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

## Core public API

- Curated facade: `GameBoy`, `Registers`, `Button`, `CartridgeError`, `Model` + screen/clock consts.
- Keep internals `pub(crate)`; new integration-test escape hatches go behind `#[doc(hidden)]`.

## Audio frontend

- Hand-rolled lock-free SPSC ring (`crates/slopgb/src/audio.rs`).
- The cpal callback must never lock or allocate — keep it that way.
