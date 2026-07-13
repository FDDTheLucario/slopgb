# `cdl-ranges` MCP tool — plan

A ninth MCP tool that lists the **continuous ranges of addresses the CDL has
logged** (any `r`/`w`/`x` flag set — i.e. non-`.`). It does not summarise what
was logged, only *that* a contiguous span was touched. Companion to `cdl`
(which dumps the per-byte access words for a caller-chosen range); `cdl-ranges`
answers "where is there anything to look at?" with no arguments.

## Output

One range per line, CPU-address form, same `AAAA` / `BB:AAAA` convention as the
other tools (banked regions carry the bank on *both* endpoints):

```
21a0-3fff
c000-c010
01:d000-01:d210
04:6000-04:650a
02:4001-02:4001
```

Empty output (nothing but a trailing nothing) when the log is off or nothing has
been logged yet. Ranges are inclusive and never cross a region **or** bank
boundary — that boundary is exactly where the CPU-address form / bank prefix
changes, so each line is one region, one bank.

Order is deterministic **physical/region order**: ROM0, then ROMX banks
ascending, VRAM banks, SRAM banks, WRAM0, WRAMX banks, tail (`FE00-FFFF`). The
example's mixed order is illustrative only — grouped order is clearer and
stable across calls.

## Why a core method (not pure frontend)

The logged set lives in the bank-aware physical CDL buffer
(`Interconnect::cdl`), whose layout (`cdl_layout`: `ROM | VRAM(0x4000) | SRAM |
WRAM | tail(0x200)`) and bank→CPU-address mapping are core-private. Replicating
the ROM/VRAM/SRAM/WRAM banking inverse in the frontend would duplicate
`cdl_index`/`cdl_flag_banked` knowledge and need bank-count accessors that don't
exist. Instead core walks its own buffer once and yields structured CPU-address
ranges; the frontend only formats them into strings. Read-only `&self`,
golden-safe (mirrors `cdl_flag_banked`).

### Physical → (bank, CPU addr) decode (inverse of `cdl_index`)

Walk each region's slice, bank by bank; within one bank the CPU addresses are
contiguous, so a maximal run of non-zero bytes = one range. Each segment is
`(phys_base, span, cpu_start, bank)`:

| Region | segments | cpu_start | form |
|---|---|---|---|
| ROM bank 0 | `[0, 0x4000)` | `0x0000` | bare |
| ROM banks `1..rom_len/0x4000` | `n*0x4000` | `0x4000` | `BB:` |
| VRAM banks `0..2` (`0x4000/0x2000`) | `vram + b*0x2000` | `0x8000` | `BB:` |
| SRAM banks `0..ram_len/0x2000` | `sram + b*0x2000` | `0xA000` | `BB:` |
| WRAM bank 0 | `wram + 0`, span `0x1000` | `0xC000` | bare |
| WRAM banks `1..wram_len/0x1000` | `wram + b*0x1000`, span `0x1000` | `0xD000` | `BB:` |
| tail | `wram + wram.len()`, span `0x200` | `0xFE00` | bare |

This decode is the exact inverse of the `cdl_flag_banked` forward mapping
(verified against ROM `rom_offset`/`ram_index` linearity and `wram_index`), so a
range's bytes read back identically through the existing `cdl` tool. Bare
regions carry `bank = 0` (ignored on formatting).

## Changes

1. **core** `crates/slopgb-core/src/interconnect/debug.rs`
   - `pub struct CdlRange { pub bank: u16, pub start: u16, pub end: u16 }`.
   - `pub fn cdl_logged_ranges(&self) -> Vec<CdlRange>` — the segment walk above,
     using a private `push_runs(buf, base, span, cpu_start, bank, &mut out)`
     helper. Empty vec when `cdl` is `None`.
2. **core** `crates/slopgb-core/src/lib.rs` — `pub use interconnect::CdlRange;`.
3. **core** `crates/slopgb-core/src/lib/debug.rs` — `GameBoy::cdl_logged_ranges`
   passthrough to `self.bus`.
4. **frontend** `crates/slopgb/src/mcp/tools.rs`
   - `Call::CdlRanges` variant (no args).
   - `dispatch` arm → `format_cdl_ranges(gb)`: one line per range, banked via
     `addr::Region::of(start).banked()`:
     `BB:AAAA-BB:AAAA` (banked) or `AAAA-AAAA` (bare), lowercase hex to match the
     example.
5. **frontend** `crates/slopgb/src/mcp/server.rs` — `build_call("cdl-ranges")`
   (no args), and a `tool("cdl-ranges", …, &[])` entry in `tool_defs`.
6. **docs** `docs/ui-state/mcp-server.md` — add the row to the tool table.

## Tests (failable checks)

- **core** `lib_tests/cdl.rs`: load a crafted buffer with known runs in ROM0,
  ROMX bank 1, WRAM0, WRAMX, SRAM; assert `cdl_logged_ranges()` yields the exact
  `(bank,start,end)` set. Include a single-byte run (`start==end`) and two
  disjoint runs in one bank (a `.` gap splits them). Off → empty.
- **frontend** `mcp/tools_tests.rs`: `Call::CdlRanges` off → empty string; after
  loading flags, assert exact lines incl. a `BB:` line and a bare line and a
  single-byte `nnnn-nnnn`.
- **golden-safe**: `cargo test -p slopgb-core --test gbtr golden_fingerprint`
  byte-identical (read-only add, defaults untouched). clippy `-D warnings`.
