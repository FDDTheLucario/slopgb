# slopgb-w65c816

Clean-room WDC 65C816 CPU core — the SNES-side CPU the Super Game Boy runs.
Bus-generic (`Bus` trait) so the same code is tested natively against vectors and
hosted as a slopgb coprocessor plugin (`Coprocessor`, comm-port bus).
`forbid(unsafe_code)` while it's a pure core; switches to the plugin-api `deny` +
`slopgb_coprocessor_plugin!` when wrapped for wasm.

## Clean-room rule (non-negotiable)

**Never read an emulator's source.** Implement from:
- **TomHarte `ProcessorTests/65816`** — 10k per-opcode JSON vectors (initial →
  final regs+RAM+cycles). The primary TDD oracle: test data, not code.
- **Klaus `65C816_extended_opcodes_test`** ROM — disassemble + run to its
  success trap as an independent cross-check.
- Docs: **WDC W65C816S datasheet**, **Eyes & Lichty "Programming the 65816"**,
  the opcode matrix.

Cite the datasheet/vector source in a comment for any non-obvious behavior.

## The width/mode crux

E (emulation) / M (acc width) / X (index width) govern every instruction's width
and wrapping (`regs.rs`). Settle these before any opcode: XCE swaps C↔E and forces
8-bit + page-1 stack; REP/SEP mask P (M/X can't clear in emulation); SEP X drops
index high bytes.

## Acceptance gates

- All 256 opcodes × TomHarte vectors pass, **cycle-exact** (16-bit +1, DP-nonzero
  +1, page/bank-cross, decimal +1).
- The Klaus extended-opcodes ROM reaches its self-loop success PC.

## Test

```sh
cargo test -p slopgb-w65c816
```
