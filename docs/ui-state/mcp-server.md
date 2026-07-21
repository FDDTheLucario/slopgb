# MCP server

An opt-in [Model Context Protocol](https://modelcontextprotocol.io) server so an
LLM agent can drive the debugger against the **live machine you're watching** —
befitting the name. Off by default. Start it at launch with `--mcp-port <N>` /
`SLOPGB_MCP_PORT=<N>`, or at runtime from the game-window right-click **MCP**
submenu (Start server… / Stop server) — mirrors the Link menu. The bound port
shows in the title bar (`MCP :<port>`, like the link status). Lives in
`crates/slopgb/src/mcp.rs` + `mcp/`.

## Wiring it to a client

```sh
slopgb --mcp-port 8123 game.gb        # you play; window stays open
claude mcp add --transport http slopgb http://127.0.0.1:8123/mcp
```

The server binds `127.0.0.1` only (never `0.0.0.0`) — localhost, not the network.

## The tools

| Tool | Args | Output |
|---|---|---|
| `disassemble` | `from`, `to` | `BB:AAAA<tab>label<tab>instruction<tab>cycles` per row (empty label → two tabs, bare cycle count). Symbol names substituted into branch/call operands. |
| `peek` | `from`, `to` | 16 hex bytes/row, `BB:AAAA<tab>…` |
| `cdl` | `from`, `to` | like `peek`, each byte → an `r`/`w`/`x` access word or `.` |
| `cdl-ranges` | — | the continuous address ranges the CDL has logged (non-`.`), one `AAAA-AAAA` / `BB:AAAA-BB:AAAA` per line; empty when off / nothing logged |
| `vram` | `view` (`bg`\|`win`\|`tile0`\|`tile1`\|`oam`\|`palette`), optional `scale` | a PNG (`image/png` content); `bg`/`win` game-paletted, Tiles grey-ramp |
| `screencap` | optional `scale` | the current 160×144 screen (`gb.frame()`) as a PNG — cross-reference against `vram *` |
| `breakpoint` | `address` | sets a PC breakpoint (the only mutating tool) |
| `registers` | — | `af=… bc=… … lcdc=… stat=… ly=… cnt=… ie=… if=… ime=… ima=… spd=… rom=… ram=… wave=…` |
| `coprocessor` | — | SGB coprocessor status: whether the SPC700 + 65C816 subsystem plugins are engaged (or the built-in HLE / not-SGB) |
| `expr` | `expression` | evaluates a bgb-style debugger expression (hex default, register names, `[addr]`) |
| `memdump` | `from`, `to`, `file` | writes the range's raw bytes to a local `file` (feeds `simulate`); text confirms `N bytes … to file` |
| `savestate` | `file` | writes a full savestate (CPU + VRAM + all machine state, **not** the ROM) to `file` — capture a checkpoint before a glitch |
| `simulate` | `memdump_file`, `in_from`/`in_to`, `out_from`/`out_to`, `start`, `budget`; optional `end`, `savestate_file` | forks the live machine and runs the fork in the background; returns a `job_id` at once (see [Fork](#fork-simulate)) |
| `sim-result` | `job` | polls a fork: `still running`, or `stop: <code>` + the `registers` line + an `out`-range hex dump |

### Image scale

`vram` and `screencap` take an optional `scale` (`2x`–`6x`, or a bare `2`–`6`);
omit it for native size. It nearest-neighbor magnifies the PNG so a model that
struggles with 160×144 pixel art can read it — `screencap` `3x` → 480×432. Only
the two image tools read it; parsing lives in `tools::parse_scale`.

### Plugin tools

The tool set is **pluggable**: a wasm tool plugin (see
[plugin-api.md](plugin-api.md) — tier 2, `ToolPlugin` / `slopgb_tools!`) loaded
via `--plugins` registers new MCP tools, which `tools/list` advertises and
`tools/call` dispatches alongside these built-ins. A plugin tool whose name
matches a built-in wins (so the reference plugins in
`crates/slopgb/reference-tools/` — the nine built-ins re-implemented on the
plugin ABI, parity-pinned byte-identical — can stand in for them).

The introspection built-ins stay native; the plugins call back into the same
`mcp::tools` formatters through a borrowed `plugin_host::FrontendToolContext`, so
a ported tool's output (text or PNG) is identical to the built-in's. The
file/fork tools (`memdump`, `savestate`, `simulate`, `sim-result`) are built-in
only — they have no reference-plugin counterparts. The socket
thread advertises plugin tools from a metadata snapshot taken at server start;
`tools/call` for a plugin name is forwarded to the UI thread like a built-in and
run against the live machine. Loading plugins is opt-in, so the default path is
unchanged.

### Address forms

`AAAA` (bank implied 0) or `BB:AAAA` (`BB` = hex bank). `AAAA` is legal only for
ROM0/WRAM0/echo+ (`0000-3FFF`, `C000-CFFF`, `E000-FFFF`); `BB:AAAA` for the banked
regions ROMX/VRAM/**SRAM**/WRAMX (`4000-7FFF`, `8000-9FFF`, `A000-BFFF`,
`D000-DFFF`). Cart SRAM banks with the mapper, so `peek`/`cdl` read an explicit
RAM bank there (raw chip bytes, folded to the RAM size; open-bus `FF` / CDL 0 with
no RAM chip). A range must stay inside one region and one bank, so
`03:7FF0 04:400F` is rejected — split it into two queries.

`simulate`'s `start`/`end` are **bare 16-bit hex** (a raw CPU PC, no bank);
`budget` is a **decimal** instruction count.

## Fork (simulate)

`simulate` runs a what-if without disturbing what you're watching. It **clones**
the live machine (`GameBoy::clone` — a full independent GB incl. VRAM / PPU /
banking / ROM), optionally rewinds that clone to a `savestate_file`
(`load_state`), overlays the `memdump_file`'s bytes into the `in_from..in_to`
range (`debug_write_banked`; the file's length must equal the range, else the
call errors), sets `PC = start`, and registers the fork. It returns a `job_id`
immediately — the fork never runs inline.

Why a fork you poll instead of a call that blocks: `tools/call` is served on the
UI thread inside a 5 s reply window, so a run of any length can't answer on its
own call. Instead `Mcp::pump` advances the fork **one bounded slice**
(`SIM_SLICE`, ~one frame of instructions) per event-loop wake — cooperative, so
a long run neither freezes the UI nor blocks the socket. `sim-result` polls by
`job_id`. One fork at a time: starting another while one is still running is
rejected; a finished-but-unpolled fork is replaced.

A fork stops with one of three codes (`run_until_breakpoint` on the clone, plus
`debug_undefined_hit`):

| Stop code | Meaning |
|---|---|
| `reached_end` | PC hit the optional `end` breakpoint |
| `runaway` | the CPU executed an undefined opcode and hard-locked (gbctr "undefined opcodes") |
| `timed_out` | the instruction `budget` ran out first (a `HALT`/legit-`STOP` with no wake folds here) |

The primary cap is the **emulated instruction budget**, not wall-clock: it
measures the fork's own progress, so the verdict is deterministic and immune to
UI/fast-forward contention starving the cooperative slices. `budget` is clamped
to `MAX_BUDGET` so a runaway argument can't keep a fork alive forever. Note the
banking caveat: the overlay writes and the `out`-range dump hit the **specified**
`BB` bank, while execution reads through the **live-mapped** bank — target the
bank you'll actually run from.

## Architecture

Mirrors [`link`](save-states-and-link.md) (the serial cable): a background thread
owns a `TcpListener` and speaks the MCP **streamable-HTTP** profile (POST
JSON-RPC → JSON response; `GET` → 405, no server-initiated SSE). Std-only — no
serde, no HTTP crate (a hand-rolled JSON codec in `mcp/json.rs`, a stored-DEFLATE
PNG encoder in `mcp/png.rs`), honoring the frontend's winit/softbuffer/cpal-only,
no-Cargo-dep rule.

- `initialize` / `tools/list` are answered on the socket thread (static metadata).
- `tools/call` is forwarded to the **UI thread** over an mpsc channel (a `Job`
  with a one-shot reply) and executed there against the live `GameBoy`; the socket
  thread blocks (bounded, 5 s) on the reply.
- `Mcp::pump` drains jobs at the **top** of `about_to_wait` — *before* the idle
  guard — so an agent can still inspect a paused / breakpoint-halted machine. When
  the server is up and the machine is idle, the event loop polls (`WaitUntil` 8 ms)
  instead of parking, so calls are served within a few ms while frozen.
- One connection at a time (an agent uses one keep-alive connection); a socket read
  timeout + the stop flag keep a stalled peer from wedging the thread or a `Drop`
  join. If concurrent clients ever matter, spawn a bounded worker per connection.

## Golden-safe

Every tool is read-only `&self` introspection except `breakpoint`, which toggles
the App-owned breakpoint set (not core state — and empty-by-default breakpoints
keep the run loop byte-identical). `memdump` / `savestate` add a local **file
write**, still `&self` on the machine. `simulate` mutates and steps only a
**clone** (`debug_write_banked` / `debug_set_pc` / `run_until_breakpoint` all run
on the fork) — the live machine never advances a cycle, verified by a unit test
that runs a fork and asserts the live `PC` is unmoved. Two core accessors back the banked tools —
`GameBoy::debug_read_banked` and `cdl_flag_banked` (both cover ROMX/VRAM/SRAM/
WRAMX via the cartridge `ram_read_banked` / `ram_offset_banked` helpers), all
read-only `&self`, verified golden byte-identical + mooneye 91/91. The whole server is opt-in and
inert by default, so no golden path is touched. See the golden-safe law in the
[README](README.md).
