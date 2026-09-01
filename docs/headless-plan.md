# Headless mode + slopscript

**Status: design settled, nothing built.** This is the plan document. When it
lands, the per-area state moves to `ui-state/headless.md` (CLAUDE.md's rule:
dedicated dirs hold *built* subsystems) and this file records what was decided
and why.

A non-interactive `slopgb --headless <script.slp>` that boots a ROM, drives it
from a script, asserts against memory, and exits with a meaningful code — so a
game-side change can be verified in CI without a hand-written C harness.

## Why

The motivating case is a game under development — a disassembly or homebrew ROM
— where a source change needs verifying against real emulated hardware. Today
that means a bespoke C harness compiled against another emulator's core objects:
boot a ROM with a prepared battery file, tap through menus until a memory value
changes, idle for a load, dump the RAM of interest, write the battery back out.
Every run needs its own recompile, and none of that work is specific to the game
being tested.

`slopgb-core` is already headless — `run_frame`, `press`/`release`,
`save_data`/`load_save_data`, `save_state`/`load_state`, `debug_read_banked`.
What is missing is a way to *drive* it without a window.

## Shape

```sh
slopgb --headless case.slp game.gbc in.sav out.sav
slopgb --headless -            # script on stdin, runs at EOF
```

- The script is the whole test case. Arguments after it land in `argv`.
- Exit **0** = ran to the end, every check held. **1** = a check or assert
  failed — the game did the wrong thing. **2** = a script bug (unbound symbol,
  bad address, type error) or an I/O failure.
- With at least one check, the runner prints a one-line summary (`4 checks
  passed`). A run with no checks is valid and silent — driving a ROM and writing
  a battery for external comparison is a legitimate use.
- A wall-clock cap for a CI suite is `timeout 120 slopgb --headless …`.
  Coreutils already does that job; the language does not.

## slopscript

A small, dependency-free language in its own crate (`crates/slopscript`),
mirroring `slopfp`: no `winit`, no emulator types, testable standalone.

Lua-inspired in a few surface details only — `--` comments, `repeat`/`until`,
`..` for concatenation — and deliberately **not** Lua: blocks are braces, and
consistency inside the language won every case where the two pulled apart. Do
not assume Lua semantics carry over.

### Values and structure

Integers, strings, byte-lists, booleans. `let` variables, procedures,
`if`/`else`. Decimal literals by default, `0x`/`$` for hex. Byte-lists are
`[1, 2, 3]`, and indexing is `argv[1]` — so `{ }` means a block and nothing else.
Procedures are `proc name(a, b) { … }`, called as `name(1, 2)`.

Fixed sets are **constants, not strings**: buttons are `BTN_A`, `BTN_START`, …,
registers are `A`, `BC`, `PC`, …, and models are `DMG`, `CGB`, `SGB`, ….
Buttons carry the prefix because the register set claims the bare `A` and `B`
first, and a `gb:tap(A, …)` that silently meant the accumulator would read
perfectly and do the wrong thing. A string means a name that is late-bound to a
particular build — a symbol, or a path. Since `let` is the only way to introduce
a name, unbound identifiers are caught **at parse time**, so `STRAT` fails before
frame 1 rather than 6000 frames in. Same philosophy as the `within` rule.

**Ceiling, deliberate:** no maps, no closures, no modules, no string library
beyond formatting helpers. The trigger to reconsider is a second game needing a
shared boot-to-overworld procedure, not a stylistic preference.

### Blocks

Every block is `{ }`. There is no `do`, no `then`, no `end` — the braces are the
only delimiter, so there is one rule and no exception to remember.

```
if cond { … } else { … }
while cond { … }                    -- pre-test, may run zero times
repeat { … } until cond             -- post-test, runs at least once
for i = 1, n { … }                  -- counted, terminates by construction
within 6000 { … }                   -- bounded scope (see below)
within 6000 { … } else { … }        -- with an expiry handler
```

`until cond` is a trailing clause on the already-closed `repeat` block, the
mirror of `within N` as a leading one.

### Whitespace

Insignificant everywhere. Allman, K&R, or all on one line parse identically;
newlines are not statement terminators and `;` is optional.

That works because every statement is self-delimiting — a `let`, an assignment,
a call, or a block construct — with one restriction it forces: **a statement may
not begin with `(` or `[`.** Either would otherwise glue onto the previous line
— a leading `(` reads as calling the previous statement's result, a leading `[`
as indexing it. Neither is a useful statement on its own, so the restriction
costs nothing and removes the only ambiguity insignificant whitespace
introduces.

### Bounds

`while` and `repeat` can run forever, so **both must appear lexically inside a
`within` block**; a `for` loop cannot hang and is exempt. The check is lexical
containment, decided at parse time — an unbounded loop is a parse error, not a
CI hang. A loop inside a procedure carries its own `within`.

`within N` is in **frames**, tracked internally in cycles so `wait_cycles` also
consumes the budget. Nested bounds **clamp**: an inner `within` can only tighten,
never extend, so a procedure's own bound cannot defeat its caller's.

On expiry the block **breaks out and continues**; the optional `else` branch runs
so the script can say what the bound meant. A `within` with no `else` really does
just carry on — that is the point of it, and it is why the `else` branch exists.

Expiry is never silent, though: it always prints a line to stderr naming the
block, `else` or not. Without that, a blown bound produces a confusing exit 1
several checks later, pointing at the wrong thing.

There is no global watchdog. It was in an earlier draft and mandatory `within`
made it redundant: a bound that only fires when someone forgot to write one is
better replaced by refusing to parse the program that forgot.

### The `gb:` / global split

`gb:` is the machine. Bare globals are the runner.

| | |
|---|---|
| `gb:load_rom(p [, model])` | model is bound at construction, so it is an argument, not a setting |
| `gb:load_battery(p)` `gb:load_state(p)` | explicit, never inferred |
| `gb:set_rtc(epoch)` | sets **and pins** the clock; call it *after* `load_battery` |
| `gb:save_battery(p)` `gb:save_state(p)` | the only ways a file is written |
| `gb:load_symbols(p)` | bgb/rgbds `.sym` |
| `gb:read(…)` | overloaded, see below |
| `gb:press(b)` `gb:release(b)` `gb:tap(b, n)` | `b` is a constant: `BTN_A BTN_B BTN_START BTN_SELECT BTN_UP BTN_DOWN BTN_LEFT BTN_RIGHT` |
| `gb:wait_frames(n)` `gb:wait_cycles(n)` | |
| `gb:reg(r)` | read one register: `A F B C D E H L AF BC DE HL SP PC` |
| `gb:poke(…)` `gb:set_reg(r, v)` | mutating; `poke` takes all four `gb:read` shapes |
| `argv` `print` `hex` `assert` `check` | runner-side |

`assert` aborts on failure (exit 1). `check` records and keeps running, so one
run reports every broken field rather than only the first: a harness's value is
its whole results table, not its first failure. Runtime errors always abort with
exit 2 — a script bug must never look like a test failure.

`gb:read` dispatches on argument type, all four shapes:

```
gb:read("wGameState")               -- symbol; bank + address from the .sym
gb:read("wResultTable", 8)          -- symbol, 8 bytes -> byte-list
gb:read(1, 0xAD0C)                  -- bank + CPU address
gb:read(1, 0xDC8B, 4)               -- bank + address, 4 bytes -> byte-list
```

A symbol is passed as a *string* because it names something that can change
between builds. An unknown symbol is exit 2.

`gb:poke` takes the same four shapes. A stale `.sym` corrupts a symbol-addressed
*read* exactly as badly as a write — it returns the wrong value and the check
passes for the wrong reason — so there is no case for allowing one and not the
other. The blast radius differs, but it is already contained: `gb:save_battery`
is the only thing that persists, and the script has to ask.

`hex()` formats uppercase, space-joined, no `0x` (`1B 02 0A 04`) — the same
format `mcp/tools.rs` already uses for `peek` dumps. One convention in the
binary, not two.

Byte-lists compare by value, which is what turns a table check into an assertion
instead of a dump to eyeball:

```
check(gb:read(1, 0xDC8B, 4) == [27, 2, 10, 4], "result table")
```

### Frames

`gb:wait_frames(n)` is `n` × `GameBoy::run_frame`, which runs to the next vblank
**or** `CYCLES_PER_FRAME` (70224), whichever comes first (`lib.rs`, `run_slice`).
With the LCD off `frame_count` never advances and the cycle deadline ends the
slice, so a frame is well-defined in every case.

Consequence worth knowing: `run_frame` stops at the *next* vblank, so a
`wait_frames` following a `wait_cycles` starts mid-frame and is shorter than
70224 cycles. Fully deterministic, but frame boundaries are no longer at
multiples of 70224 once the two are mixed.

## Worked example

A generic shape: boot with a prepared battery, drive the game until a flag in
cart RAM changes, then assert on the result. One file per case; the same file
covers a control run and a patched run by taking the ROM from `argv`.

```
-- Usage: slopgb --headless case.slp <rom.gbc> <in.sav> <out.sav>

gb:load_rom(argv[1], CGB)
gb:load_battery(argv[2])
gb:set_rtc(1767225600)   -- pin the clock: a host-clock-dependent run is flaky by design
gb:load_symbols("game.sym")

-- Drive the title screen until the game reports it has processed the save.
within 6000 {
  repeat {
    gb:tap(BTN_START, 3); gb:wait_frames(17)
    gb:tap(BTN_A, 3);     gb:wait_frames(17)
  } until gb:read("sSaveVersion") == 2
} else {
  check(false, "save was never processed")
}

-- Through the confirmation prompt, then let the map load.
for i = 1, 8 {
  gb:tap(BTN_A, 3); gb:wait_frames(37)
}
gb:wait_frames(600)

let table = gb:read("wResultTable", 4)
print("table=" .. hex(table) .. " state=" .. gb:read("wGameState"))

check(table == [27, 2, 10, 4], "result table")
check(gb:read("wGameState") == 0, "game state after load")

gb:save_battery(argv[3])
```

Every construct is exercised: `within`/`else`, `repeat`, `for`, both `gb:read`
forms, byte-list comparison, `check` accumulating so one run reports every
broken field, `argv`, and explicit battery I/O on both ends.

## Host side

- **`--headless` lives in `crates/slopgb`**, the frontend binary. `symbols.rs`
  (`.sym` parsing), `session.rs` (machine + persistence + plugin wiring),
  `cli.rs`, and `app_boot.rs` (boot ROM / SGB BIOS / plugin resolution) all
  already exist there. `winit` is linked but never initialised — the CLI is
  parsed before any `EventLoop` is created. If CI ever objects to linking the
  display libraries, the escape hatch is lifting headless into its own crate.

- **`slopscript` knows nothing about Game Boys.** `Session` lives in the
  frontend *binary* crate, so the language crate cannot see it — the dependency
  only points one way. `slopscript` exposes a `register_builtin(name, closure)`
  registry and the frontend registers every `gb:` method into it. Less code than
  a trait with fifteen methods, and adding a builtin touches only the frontend.

- **`Session::load` splits in two:** `load_rom` (everything except the battery
  restore) plus `restore_battery`, with today's `load` as the two composed, so
  the frontend path is unchanged. `load` currently derives
  `sav_path = rom.with_extension("sav")` and restores it if present; a headless
  run must not, or a control run silently inherits the previous run's output.

  Nothing else needs touching: `autosave`, `flush_save` and `capture_rewind` are
  all caller-driven from `app_handler.rs` / `main.rs` / `app_run.rs`, and
  headless never constructs an `App`. `save_battery(path)` needs no split —
  `save_image()` + `write_atomic()` already take an explicit path.

## Golden-safe

Reads are `debug_read_banked` and friends — `&self`, no cycle advanced. The
mutating surface (input, battery/state load, `poke`) is the third class the
golden-safe law already allows: explicit, user-initiated, never on a passive
frame loop. Headless is off unless `--headless` is passed, so no existing path
changes.

## Rejected

**Scripts as wasm plugins.** Tempting, because `slopgb-plugin-host` and the guest
SDK already exist and a plugin build measures ~2 s. It fails on direction: the
plugin ABI is *pull* — `on_frame(view)`, `call(args, view)`, coprocessor ticked by
the host — and every import is read-only observation (`host_read`, `host_reg`,
`host_read_banked`, `host_disasm`). A harness needs the inverse. It would take
`ABI_VERSION` 8 with six mutating imports, a `DRIVE` capability bit, a fourth
loader alongside the documented three tiers, and a host-side path allowlist
because a driver writes files. That makes the plugin ABI mutating — the widest
blast radius in the tree, against a law whose whole point is that plugins
observe — and costs a cargo crate per test case instead of a file. It becomes
right only if harness scripts must be written in a language slopgb does not ship.

**Embedding real Lua.** `mlua`/`hlua` are C FFI, and `unsafe_code = "forbid"` is
set workspace-wide; vendored Lua also puts a C compiler in CI. Pure-Rust Lua VMs
are experimental — you own their bugs *and* do not control the language.

**A declarative input timeline** (`@40 press a`). Cannot poll, so it cannot
express the central idiom (mash until a memory value changes). That shape is a
TAS harness, which is a different scope.

**Excluding `poke` from v1** was considered and rejected. The argument for
excluding it: planting state by preparing the save file is the more honest test
— poking WRAM mid-run exercises a state the game cannot reach on its own. The argument that won: it is
explicit, user-initiated mutation, which the golden-safe law already permits, and
withholding it buys nothing.

## Glossary

**Headless run**: one non-interactive execution of a script against one machine,
producing an exit code. Not "batch mode".

**slopscript**: the language. Files are `.slp`. Brace-delimited, whitespace-
insensitive, Lua-inspired in a few surface details and not Lua.

**Bound** (`within`): a scoped frame budget. Not "timeout" — nothing times out;
the block ends early and execution continues.

**Check** vs **assert**: `check` records and continues, `assert` aborts. Both
mean the game did the wrong thing (exit 1), as distinct from a **script error**,
which means the script is wrong (exit 2).

**Symbol**: a name from a bgb/rgbds `.sym`, carrying bank and address. Passed as
a string because it is late-bound to a particular build.
