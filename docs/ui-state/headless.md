# Headless mode + slopscript

`slopgb --headless <script.slp>` runs a `slopscript` (`.slp`) case
non-interactively against one machine and exits with a code a CI step can
check. Design rationale, the rejected alternatives, and the language spec
prose live in [`docs/headless-plan.md`](../headless-plan.md) (now a decision
record); this file is the built state.

## CLI surface

`--headless <PATH>` (`crates/slopgb/src/cli.rs`, `Options::headless`). `-`
reads the script from stdin (at EOF); any other path is read whole. With
`--headless` present, every bare argument becomes the script's `argv`
(`Options::argv`) and `Options::rom` stays `None` — the script names its own
ROM via `gb:load_rom`. `--headless` may appear before or after the bare
arguments; a missing value is `"--headless requires a script path"`.

## Exit codes

- **0** — ran to the end and every check held. A run with `checks >= 1` prints
  a one-line summary (`4 checks passed`, `1 check passed`); zero checks is
  valid and silent.
- **1** — a `check` or `assert` failed: the game did the wrong thing. Prints
  `N of M checks failed` to stderr, unless the only failure is an aborting
  `assert` (which never adds to `checks` and has already printed its own
  `assert failed: <msg>` line) — then nothing further is printed
  (`headless.rs`, `exit_code`/`failure_summary`).
- **2** — a parse error, a runtime error, or an I/O failure: the script (or
  its invocation) is wrong. Prints `slopgb: <message>` to stderr.

## Where the branch sits

`crates/slopgb/src/main.rs`: the `--headless` branch runs right after
`session.set_sgb_bios(...)` and before `EventLoop::new()`, and calls
`process::exit` — no `winit::event_loop::EventLoop` and no `App` are ever
constructed on this path. `winit` stays linked but uninitialised.

## The `slopscript` crate

`crates/slopscript` — std-only, `#![forbid(unsafe_code)]`, no external deps
(mirrors `slopfp`'s liftable `Cargo.toml`: a concrete, not
workspace-inherited, version so the directory can be copy-pasted into another
project). It knows nothing about Game Boys; the frontend links it and
registers every `gb:` builtin/constant into an `Interp`.

Modules: `lib.rs` (public API), `lexer.rs` (tokenizer), `ast.rs` (pure data,
`pub(crate)` only), `parser.rs` (recursive-descent, enforces every parse-time
rule), `eval.rs` + `eval/{exec,expr,calls,format}.rs` (tree-walking
evaluator, each an `impl Ctx` block via `use super::*`). Tests are externalized
siblings: `lexer_tests.rs`, `parser_tests.rs`, `eval_tests.rs` +
`eval_tests/{checks,control_flow,helpers,indexing,procs,values,within,worked_example}.rs`.

### Public API (`lib.rs`)

- `Value` — `Int(i64) | Str(String) | Bytes(Vec<u8>) | Bool(bool) | Nil`.
- `Outcome` — `Ok { checks } | Failed { checks, failures } | Error(String)`,
  the type `headless.rs`'s `exit_code` maps to a process exit code.
- `Interp::new()`, then, **before** `parse_only`/`run` (an unregistered call
  name is a parse-time error):
  - `register_builtin(name, closure)` — `name` may carry a `"ns:name"`
    prefix, matching `ns:name(...)` call syntax.
  - `register_const(name, Value)`.
  - `set_argv(Vec<String>)` — 1-based; `argv[1]` is the first.
  - `set_clock(ticks_per_unit, || -> u64)` — the `within` budget clock;
    default `(1, || 0)`, a clock that never advances (so `within` never
    expires in a standalone `slopscript` test).
- `parse_only(src)` — every parse-time rule checked, nothing executed; stashes
  the parsed `Program` for a following `run`, or drops any stashed program on
  failure (a stale parse can never silently run).
- `run(src)` — `parse_only` then walks the program, returning `Outcome`.

## The language as shipped

**Values:** `Int`, `Str`, `Bytes` (a byte-list, indexed 1-based:
`argv[1]`, `[1,2,3][1]`), `Bool`, `Nil`. Integer literals are decimal, or hex
via `0x`/`$`. Strings are double-quoted with `\n \t \\ \"` escapes; any other
escape or an unterminated string is a lex error. Comments are `-- to end of
line`. Whitespace/newlines are fully insignificant — no statement terminator,
`;` optional — enforced by parse-time rule 3 below.

**Statements:** `let name = expr`, `name = expr` (assignment — only to an
already-`let`-bound name), a call as a statement (`name(...)` /
`ns:name(...)`), `if cond { } else if cond { } else { }`, `while cond { }`
(pre-test), `repeat { } until cond` (post-test, runs body at least once),
`for var = from, to { }` (inclusive, step 1, zero iterations if `to < from`,
counted so exempt from the `within` requirement), `within N { } [else { }]`,
`proc name(a, b) { }` (top-level only, called as a statement only — never in
expression position).

**Operators**, loosest to tightest: `or`, `and`, `not` (prefix), comparison
(`== != < <= > >=`, non-associative — at most one per expression), `..`
(concat), `+ -`, `* / %`, unary `-`. Strict typing throughout: `==`/`!=`
comparing two different `Value` variants is a runtime error, never `false`;
arithmetic/comparison operators require two `Int`s; `..` accepts `Str`/`Int`
(rendered decimal) but rejects `Bytes` (format with `hex()` first),
`Bool`, `Nil`. All arithmetic is checked (`checked_add`/`_sub`/`_mul`/`_div`/
`_rem`/`_neg`) — overflow and division/modulo by zero are runtime errors, not
wraparound or a panic.

### The four parse-time rules (`parser.rs`)

1. **Unbound identifier is a parse error.** A bare name is bound only via
   `let`, a `for`-loop variable, a proc param, a registered constant, a
   registered/intrinsic bare builtin, or `argv` — checked by `Parser::is_bound`
   before the name is ever evaluated.
2. **`while`/`repeat` must be lexically inside a `within`.** Tracked as a
   depth counter (`Parser::within_depth`), reset to 0 entering a proc body (a
   loop in a proc needs its own `within`); `for` is exempt (a counted loop
   can't hang).
3. **A statement may not begin with `(` or `[`.** Either would glue onto the
   previous statement now that newlines carry no meaning (a leading `(` reads
   as calling the previous result, `[` as indexing it).
4. **An unregistered, non-proc call name is a parse error** — checked
   separately for a bare call (`Parser::check_bare_call`: must be a proc or a
   registered/intrinsic bare builtin) and a namespaced call
   (`Parser::check_ns_call`: must be a registered `"ns:name"` builtin — a
   namespaced call can never be a proc).

Plus (documented in `parser.rs`'s module doc, not separately numbered): a
proc used in expression position (`let x = my_proc()`) is a parse error —
procs have no `return` and produce no value; a proc body sees only its own
params (**a scope barrier — no closures**), the whole lexical scope stack is
replaced entering a proc, `within_depth` resets too.

**Runtime-only checks** (can't be resolved without executing): 1-based
index-out-of-range, wrong-type operand, the `MAX_CALL_DEPTH` = 256 recursion
cap (`eval.rs`), and the `within` budget itself.

## The `gb:` / global builtin table (as implemented)

Registered by `crates/slopgb/src/headless.rs::build_interp`, one module each:

| Module | Builtins | Notes |
|---|---|---|
| `consts.rs` | registers `BTN_A BTN_B BTN_START BTN_SELECT BTN_UP BTN_DOWN BTN_LEFT BTN_RIGHT` (tagged `0x100+n`), `A F B C D E H L AF BC DE HL SP PC` (tagged `0x200+n`), `AUTO DMG CGB SGB SGB2` (tagged `0x300+n`) | every constant is a tagged `Value::Int`, so a bare number can never be mistaken for one; a decode function names the expected set on a tag mismatch |
| `io.rs` | `gb:load_rom(path [, MODEL])`, `gb:load_battery(path)`, `gb:save_battery(path)`, `gb:save_state(path)`, `gb:load_state(path)`, `gb:load_symbols(path)` | see below |
| `mem.rs` | `gb:read(...)`, `gb:poke(...)` | four overloaded shapes each, see below |
| `input.rs` | `gb:press(BTN)`, `gb:release(BTN)`, `gb:tap(BTN, n)`, `gb:wait_frames(n)`, `gb:wait_cycles(n)` | `tap` presses, runs `n` frames, releases; the script supplies its own release gap after |
| `regs.rs` | `gb:reg(R)`, `gb:set_reg(R, v)` | `R` an 8-bit half splices into its pair via `DebugReg::{Af,Bc,De,Hl}` (`DebugReg` only writes 16-bit pairs); `Sp`/`Pc` write directly |
| `rtc.rs` | `gb:set_rtc(epoch)` | see below |

Plus the runner-side globals `argv`, `print`, `hex`, `check`, `assert` (always
valid call targets — `parser::INTRINSIC_CALLS`, no registration needed).

Every wrong-shape call is a `Result::Err(String)` naming the expected shapes,
wrapped by `call_value` into a `Signal::Runtime` → `Outcome::Error` (exit 2) —
never a panic. A wrong argument *type* (e.g. `gb:press("A")` instead of
`gb:press(BTN_A)`) fails the same way, via each module's own decode helper
(`consts::decode_button`/`decode_reg`/`decode_model`) or the
`slopscript::Value::kind` mismatch message in `regs.rs`.

### `gb:load_rom` / battery / state / symbols (`io.rs`)

- `gb:load_rom(path [, MODEL])` calls `Session::load_rom` (never
  `Session::load`) — see "The `load_rom`/`restore_battery` split" below. Model
  defaults to `ModelChoice::Auto`. Re-applies the boot ROM / SGB BIOS /
  plugins dir `main` already resolved into `Machine`, plus
  `effective_plugin_flags(registry)`, to the freshly loaded session — the same
  treatment an interactive (re)load gives it.
- `gb:load_battery(path)` reads the file and calls
  `GameBoy::load_save_data`; a `false` return (wrong size, or the cartridge
  has no battery) is a runtime error, never a silent skip.
- `gb:save_battery(path)` writes `GameBoy::save_data()` — the canonical
  timestamp-free image — via `session::write_atomic`. Deliberately **not**
  `Session::save_image()`'s VBA-footer variant (used by the interactive
  path), which stamps the host wall clock and would make a headless run's
  output non-reproducible.
- `gb:save_state(path)` / `gb:load_state(path)` call
  `Session::save_state_to`/`load_state_from` — the same on-disk savestate
  format as the game-window State submenu
  ([`save-states-and-link.md`](save-states-and-link.md)).
- `gb:load_symbols(path)` parses a bgb/rgbds `.sym` via `SymbolTable::parse`
  into `Machine.syms`.

### `gb:read` / `gb:poke` (`mem.rs`)

Both dispatch on argument shape, backed by `GameBoy::debug_read_banked` /
`debug_write_banked` (`&self` / gated `&mut self` debug introspection):

```
gb:read("wGameState")               -- symbol; bank+addr from the loaded .sym
gb:read("wResultTable", 8)          -- symbol, n bytes -> Bytes
gb:read(1, 0xAD0C)                  -- bank + address, one byte
gb:read(1, 0xDC8B, 4)               -- bank + address, n bytes -> Bytes
```

`gb:poke` takes the mirror four shapes, the last argument an `Int` 0..=255 or
a `Bytes` run. An unknown symbol, an out-of-range bank/address, a byte count
that would wrap past `0xFFFF`, a multi-byte run that would leave the memory
region its start address is in (reusing `mcp::addr::Region`, the same table
the `peek`/`disassemble` MCP tools check a range against — e.g.
`gb:read(1, 0x7FFE, 8)` starting in ROMX and running on into VRAM), or a poke
value outside `0..=255` are all runtime errors (exit 2). A stale `.sym`
corrupts a symbol-addressed *read* exactly as badly as a *write* (wrong
value, check passes for the wrong reason), so both shapes get identical
resolution/range-checking — there is no case for treating one more carefully
than the other.

### `gb:set_rtc(epoch)` (`rtc.rs`)

`slopgb-core`'s MBC3 RTC is already purely cycle-driven and never reads the
host clock, so pinning it needed **zero `slopgb-core` changes** — only
*setting* it from an epoch was new, done entirely frontend-side: rewrite the
trailing 16-byte RTC block of `GameBoy::save_data()`'s output (live
S,M,H,DL,DH; latched S,M,H,DL,DH; a little-endian sub-second T-cycle counter
`u32`; the last latch-register write; one pad byte —
`crates/slopgb-core/src/cartridge/save.rs`), then `load_save_data` the patched
image back in. Decomposition: `S = epoch % 60`, `M = epoch/60 % 60`,
`H = epoch/3600 % 24`, `days = epoch/86400`, `DL = days as u8`, `DH` bit 0 =
day bit 8, `DH` bit 7 (the sticky day-counter carry) set when `days > 511`.
Live and latched registers both get the same values; the sub-second counter,
last-latch-write byte, and pad byte are zeroed, matching a fresh latch taken
exactly at `epoch`.

**Consequence:** the MBC3 day counter is 9 bits, so any realistic Unix epoch
overflows it — seconds/minutes/hours round-trip exactly, the day field is
`days mod 512` with the carry flag set, which is what the hardware register
can physically represent. `gb:set_rtc` errs if the cartridge has no RTC
(`GameBoy::rtc_state()` is `None`) or the save image is shorter than the
16-byte trailer.

## `within` budget semantics

`within N { body } [else { }]` is a frame budget, tracked internally in clock
ticks (`Interp::set_clock(CYCLES_PER_FRAME, || gb.cycles())` —
`headless::build_interp`), so `gb:wait_cycles` also consumes it, not only
`gb:wait_frames`.

- Checked before every statement inside the body (`Ctx::exec_block`), and
  again once the body finishes (`exec_within`'s `body_overran` check) — a
  block whose own last statement is what pushed the clock past budget (e.g. a
  lone trailing `gb:wait_cycles`) still counts as expired.
- **Nested bounds clamp**: pushing a `within` computes
  `min(own_deadline, enclosing_deadline)` (`within_stack`) — an inner `within`
  can only tighten the budget, never extend it, and the stack is **not** reset
  on a proc call, so a caller's bound still applies inside the callee.
- On expiry the block breaks out and continues; the optional `else` runs
  unconditionally (the block's own now-popped budget can't re-trigger it, but
  it still answers to whatever enclosing `within` remains).
- Expiry is never silent: it always prints
  `slopgb: within {n} (line {line}) expired` to stderr, `else` or not — `line`
  is the 1-based source line the `within` keyword started on.

## `Session::load` → `load_rom` + `restore_battery`

`Session::load` (`crates/slopgb/src/session.rs`) splits into `load_rom`
(everything except restoring `<rom>.sav`) and `restore_battery` (reads
`<rom>.sav` if present); `load` is the two composed, unchanged for the
interactive path. `gb:load_rom` calls `load_rom` only — a headless run must
never auto-restore `<rom>.sav`, or a control run would silently inherit the
previous run's output. `gb:load_battery` is the only way a headless run's
battery RAM is populated, and it is always explicit, never inferred.

## Golden-safe position

- Reads (`gb:read`, `gb:reg`) are `&self` `debug_read_banked`/`cpu_regs` —
  read-only introspection, same accessors the MCP server and viewers use.
- The mutating surface (`gb:poke`, `gb:press`/`release`/`tap`,
  `gb:load_battery`/`load_state`/`set_reg`/`set_rtc`) is the **third class**
  the golden-safe law already allows: explicit, user-initiated mutation
  (here, script-initiated in place of a human clicking), never something that
  runs on a passive frame loop.
- Headless is inert unless `--headless` is passed: with it absent, `main.rs`
  never reaches the branch, and every other startup path is unchanged.

## Test inventory

16 tests exercise this area:

- `crates/slopgb/src/headless_tests.rs` (10): the worked-example fixture
  byte-matches the plan's fenced block; the fixture parses against the real
  registered `gb:`/const table; an end-to-end run against a synthetic ROM;
  headless never auto-restores `<rom>.sav` (and `gb:load_battery` does bring
  it in when asked); every exit code (`check` fail, `assert` fail, parse
  error, unknown-symbol runtime error, zero-check success, unreadable script
  path); the failure-summary line is omitted when no `check` ran;
  `gb:set_rtc` round-trips its decomposed registers against an
  independently-computed expectation; `gb:read`/`gb:poke` round-trip all four
  shapes; a multi-byte `gb:read`/`gb:poke` run crossing a region boundary
  (e.g. ROMX into VRAM) is a runtime error, while one that stays inside its
  starting region still works; a ROM-only (no-battery) cart runs fine with no
  battery ops.
- `crates/slopgb/src/cli_tests.rs` (6, `headless`-prefixed): `--headless`
  collects trailing args into `argv` and clears `rom`; `--headless` works
  whether it appears before or after the bare arguments; `-` is accepted as
  the stdin marker; a missing `--headless` value errors; pre-`--headless`
  rom/extra-argument behaviour is unchanged when `--headless` is absent;
  `argv`/`headless` default to empty/`None`; `--help` documents `--headless`.
- `crates/slopscript` (88 tests total, standalone — lexer/parser/eval, incl.
  `eval_tests/worked_example.rs`, which runs the same fixture through a bare
  `Interp` with a stub `gb:`-shaped host, and `eval_tests/within.rs`'s
  spin-loop / last-statement-expiry coverage).

## Known limitations

**A `within`-bound spin loop is caught by a back-edge counter, not a
wall-clock or instruction budget.** `within` itself is checked against the
clock `Interp::set_clock` supplies — here, `gb.cycles()` — so a loop whose
body advances no emulated cycles can never deplete that budget on its own
(e.g. `within 2 { while true { n = n + 1 } }`). `eval.rs`'s `MAX_SPIN_STEPS`
(1,000,000) counts consecutive `while`/`repeat` back-edges taken with the
clock frozen, resetting the instant the clock advances; tripping it is a
`Signal::Runtime` → `Outcome::Error` (exit 2, a script error, not a `within`
expiry), with the message `loop inside the 'within' on line N turned 1000000
times without advancing the clock — it can never reach its bound`. A loop
that waits on the machine (any `gb:wait_frames`/`gb:wait_cycles` or other
clock-advancing call in its body) resets the counter every time the clock
moves and is unaffected however long it runs — only a genuine spin is
caught. `for` is exempt (it terminates by construction, per the parse-time
rule). This is a fixed cap, not configurable — see the `ponytail:` comment on
`MAX_SPIN_STEPS`.

**A `within` body that runs to its last statement is never reported as
expired**, even when that statement itself spends the final cycle of the
budget — only a body actually cut short (`Flow::WithinExpired`) counts.
Landing exactly on the deadline is what a `wait_frames`/`wait_cycles` sized to
the bound always does, and treating it as an expiry would run `else` on a
script that did everything it was asked (previously, `within 1 { repeat {
gb:wait_cycles(70224) } until true } else { check(false, …) }` printed an
expiry line and ran `else`, turning a pass into a CI failure).

**Model ceiling:** only `AUTO DMG CGB SGB SGB2` are registered as model
constants. `Session::load_rom` takes a `windows::options::ModelChoice`, whose
`from_model` folds `Mgb`/`Dmg0` to `Dmg` and `Agb` to `Cgb` — `MGB`/`DMG0`/
`AGB` constants could not do what their names say, so they are not
registered. Naming one of them in a script is already a parse-time unbound-
identifier error, which is the honest failure over silently folding.
