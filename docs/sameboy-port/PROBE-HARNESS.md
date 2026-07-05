# Port measurement harness (`port_probe`)

The SameBoy cc-exact port is measured with a set of `SLOPGB_*` traces and
constant-sweep knobs wired into the timing core. They are **off by default and
compile to nothing** — the production build has zero instrumentation and stays
byte-identical (golden frame-hash unchanged, mooneye 91/91). Arm them only for
measurement, behind the `port_probe` Cargo feature.

Implementation: [`crates/slopgb-core/src/probe.rs`](../../crates/slopgb-core/src/probe.rs).

## Arming

Two independent switches — the **Cargo feature** (compiles the harness in) and
the **env vars** (turn individual probes on at runtime):

```sh
# nothing compiled unless the feature is on:
cargo test -p slopgb-core --features port_probe ...
cargo run  --features port_probe -- game.gb
cargo run  --features port_probe --example run_gambatte -- <rom>
```

Without `--features port_probe` the env vars below are dead — `probe!` discards
its body and every `tune_*` hook returns the production default.

Most traces only fire on the **tier2 reclock** path, so pair the run with the
reclock switch (`SLOPGB_MOONEYE_RECLOCK=1`, or `new_with_reclock` /
`SLOPGB_TIER2` in the example harnesses). Reading the plain production path
traces nothing new.

## Trace probes

Two env vars gate the trace families. Presence of the var (any value) enables
it; it is read **once** via `OnceLock` on first use, so export it *before* the
run. Output goes to **stderr**, one line per event — expect millions of lines on
a full run, so always filter (`2>&1 | grep '^SLOPGB winmatch'`).

| Env var | Family | Line format |
|---|---|---|
| `SLOPGB_S5DBG` | read / wake / render / write traces | `SLOPGB <tag> ly=.. dot=.. <fields>` |
| `SLOPGB_ISRTRACE` | ISR dispatch traces, filtered to `ly∈134..=138 ∪ ly≤3` | `SL2 <tag> a=<addr> ly=.. dot=.. clk=.. pend=..` |

`SLOPGB_S5DBG` tags, by subsystem:

| Tag | Fires in | Traces |
|---|---|---|
| `wake[w0\|g2\|w2\|plain\|first]` | interconnect/speed.rs | halt-wake sampler at each wake grid point (clk/pend/intf/late/hold) |
| `stop` / `leave` / `vec` / `hentry` | interconnect/speed.rs | STOP dance, DS→SS leave advance, interrupt-vector, halt-entry |
| `m0rise` / `halt-hdma` | interconnect/tick.rs | mode-0 STAT rise instant; HDMA state while halted |
| `pal` / `ff0f` / `palw` | interconnect/cycle.rs | deferred palette read, FF0F/IF read peek, palette write |
| `hdmarun` / `wff55` | interconnect/hdma.rs | HDMA block run; FF55 arm write |
| `wlyc` | ppu/lyc.rs | FF45 LYC write edge |
| `wytrigset` / `visexit` | ppu/engine.rs | WY window trigger latch; visible mode-3 exit dot |
| `wlcdc` / `vramw` / `oamw` / `wwy` | ppu/regs.rs | FF40 LCDC, VRAM, OAM, FF4A WY writes (mode-3 strobe grid) |
| `hunt` | ppu/render.rs | sprite-hunt / fetch decisions |
| `visflip` | ppu/render/mode0.rs | mode-3→0 flip projection (proj/lead/early_lead) |
| `winmatch` | ppu/render/window.rs | WX comparator match (lx/wx/wy_ok/en/active) |
| `dispatch` | ppu/stat_irq/reclock.rs | STAT-IF dispatch reclock, four variants |

`SLOPGB_ISRTRACE` tags (`SL2`): `rd` (ISR read), `wr` (ISR write), `na`
(dispatch, no address), `ob` (opcode byte). Line-filtered to the vblank-edge
window and the first scanlines to keep the volume usable.

Example — capture the window comparator on the tier2 path:

```sh
SLOPGB_S5DBG=1 SLOPGB_MOONEYE_RECLOCK=1 \
  cargo test --features port_probe -p slopgb-core --test mooneye acceptance_ppu \
  -- --nocapture 2>&1 | grep '^SLOPGB winmatch' | head
```

## Override / sweep knobs

These replace a production constant with an env value **only** when set;
unset (or feature off) folds back to the exact production default, so a run
without the var is byte-identical. Use them to A/B a candidate timing value
across the baselined ROM battery.

| Env var | Type | Overrides | Production default |
|---|---|---|---|
| `SLOPGB_STOPADV` | `u32` | STOP realignment leave-advance `k` | `dsa7==4 ? 6 : 2` |
| `SLOPGB_LCDPH` | `i16` | injected LCD-phase offset on a non-DS line | `0` (no-op) |
| `SLOPGB_P2TBL` | 4-char digit string | halt LY-phase carry table, indexed `(cc-1)&3` | `HALT_LY_PHASE_BY_CC` = `1201` |
| `SLOPGB_P2HH` | `u8` | mode-0 halt-hold value | `1` on DMG, `0` on CGB |
| `SLOPGB_NOXLINE` | presence | **disables** the cross-line window mode-3 exit arm | arm fires |

`SLOPGB_P2TBL` takes a 4-character string of digits, e.g. `SLOPGB_P2TBL=1201`
reproduces the default; sweep by varying the digits. `SLOPGB_NOXLINE` is a
toggle — set it to disable the arm, unset to keep it.

Example — sweep the STOP advance across the battery:

```sh
for k in 2 4 6 8; do
  echo "== STOPADV=$k =="
  SLOPGB_STOPADV=$k SLOPGB_TIER2=1 \
    cargo test --features port_probe -p slopgb-core --test gbtr gambatte_matrix 2>&1 | tail -1
done
```

## Adding a new probe

- **Trace point** — put `probe!(self.trace_x(args));` at the site and define
  `fn trace_x(&self, ...)` in a `#[cfg(feature = "port_probe")] impl Type {}`
  block near the state it reads (gate the body on `crate::probe::s5dbg_on()` or
  `isrtrace_on()`). `probe!` discards the call when the feature is off, so the
  method need not exist off-feature and nothing is wasted.
- **Sweep knob** — add a `tune_<name>(default, ...) -> T` pair to `probe.rs`
  (`#[cfg(feature)]` reads the env; `#[cfg(not)]` `#[inline(always)]` returns
  `default`) and call it inline: `let v = crate::probe::tune_<name>(<default>);`.
  The default arm keeps the production path byte-identical.

Verify any change both ways: default build clippy-clean + golden unchanged, and
`--features port_probe` clippy-clean.
