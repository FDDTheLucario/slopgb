# Plugin API — design + phased build (runtime-loaded, wasmi-hosted)

A plugin API third parties can target to extend slopgb without forking it:
plugins authored in Rust, compiled to `wasm32`, loaded at runtime. Sized to
eventually re-home two existing surfaces (the MCP debug tools and the SGB
SPC700+DSP audio chip) as plugins. User-facing guide:
[`docs/ui-state/plugin-api.md`](ui-state/plugin-api.md).

## The three constraints (and the one architecture they force)

1. **No unsafe** — no hand-written `unsafe` in slopgb's crates or in a plugin
   author's source. Unsafe inside a dependency (the wasm engine) or in a
   linkage marker is acceptable (winit/cpal already carry internal unsafe).
2. **Fast enough for the SPC700** — a 1 MHz coprocessor clocked per CPU
   instruction (`GameBoy::step`, ~1M cycles/s).
3. **No compile-time** — adding a plugin must not rebuild slopgb.

Safe Rust cannot load native foreign code (`dlopen`/`libloading` are `unsafe` at
the boundary, no stable ABI), so the only path satisfying all three is: compile
the plugin to `wasm32` ahead of time, load the `.wasm` at runtime into a wasm
engine with a **safe host embedding API**.

**Engine: wasmi** (pure-Rust interpreter, safe host API, MIT/Apache). It is
*not* the fastest engine — wasmtime's JIT is ~2–4× faster on sustained execution
— but the SPC700 is a tiny workload (single-digit % CPU under wasmi, ~100×
headroom), and wasmi is a fraction of wasmtime's dependency weight, has no
cranelift/JIT/mmap, is deterministic, and has a conservative MSRV. It fits a
codebase that hand-rolls its own JSON/PNG codecs to avoid deps. wasmtime is held
in reserve only for the tier-3 SPC700 case, if a perf spike ever shows wasmi is
marginal there.

## The golden-safe invariant

`--plugins <dir>` / `SLOPGB_PLUGINS_DIR` is **off by default**, mirroring
`--mcp-port`. No flag ⇒ no plugins ⇒ no snapshot, no imports, no calls ⇒ the
emulation path is byte-identical to golden (pinned by `golden_fingerprint` +
the mooneye matrix). Tier 1 is read-only; a trapping plugin is logged and left
in place, never corrupting the machine.

## Two crates (core stays zero-dep)

- **`slopgb-plugin-api`** — the guest SDK a plugin author depends on. `Plugin`
  trait, `Capabilities`, `Reg`, `GameBoyView`, and the `slopgb_plugin!` macro.
  Crate-level `deny(unsafe_code)` with a scoped `allow` on the `abi` module
  (the sole `unsafe extern` block) and on the macro's `#[unsafe(no_mangle)]`
  exports — no `unsafe` blocks, no raw pointers. Builds for `wasm32` (real
  target) and host (to share the wire-contract constants).
- **`slopgb-plugin-host`** — the only crate that depends on `wasmi`.
  `slopgb-core` never sees it (zero-dep rule intact); the frontend depends on it
  like it already depends on `slopfp`.

## The two design choices that keep the host safe under wasmi

1. **Owned-snapshot store data.** wasmi host functions get `Caller<'_, T>` with
   `T: 'static`, so no borrowed `&GameBoy` can be handed in. Before each call the
   host copies the observable state (64 KB bank-0 image via `debug_read`, the
   exposed registers) into an owned `Snapshot` held as store data; imports read
   the snapshot. Fully safe, `'static`, negligible cost, paid only when plugins
   are loaded.
2. **Scalar-and-own-pointer ABI, no `from_raw_parts`.** Host→guest is one scalar
   per import call (`host_read`/`host_reg`). Guest→host strings pass the guest's
   own `(ptr, len)` (safe `str::as_ptr`/`len`), which the host reads through
   wasmi's bounds-checked `Memory::read`. Neither side reconstructs a slice from
   a foreign pointer, so the whole surface stays safe without `wit-bindgen`.

## Call site

A wasm call is in-process, so — unlike MCP's TCP-thread→UI-thread job channel —
the host runs synchronously: `PluginHost::pump(&GameBoy)` is called once per
rendered frame-batch from the `if frames > 0` block in `about_to_wait`
(`app_handler.rs`), next to the redraw request. No channel, no timeout.

## Capability tiers

1. **Introspection (built).** `on_frame(&GameBoyView)` per frame, read-only,
   mirrors the read-only MCP tool set.
2. **Mutation (designed, deferred).** `debug_write`/`debug_set_reg`/breakpoint,
   behind a second opt-in (`--plugins-allow-mutation`) plus a declared `MUTATE`
   capability — mirrors `breakpoint` being MCP's lone mutating tool. Applied to
   the real `&mut GameBoy` after the call, not via the snapshot.
3. **Subsystem hosting (ABI shape designed, SPC700 gated on a perf spike).** A
   plugin hosts a whole subsystem via a **cycle-stamped, demand-driven** event
   stream (host feeds comm-port writes in, pulls PCM + a port-value timeline
   out, catching the coprocessor up on the GB's comm-port reads). NOT a
   frame-batched port state — a review killed that: the SPC700 handshake is
   instruction-granular, so a frame-latched port would go stale mid-handshake.
   The SPC700's ~1M cycles/s run inside wasm without a per-cycle boundary cross.
   Open question: confirm wasmi sustains it at realtime before building.

## Rejected approaches

- **Compile-time static trait plugins** — fastest, zero unsafe, but violates
  "no compile-time" (a plugin rebuilds slopgb).
- **Native `cdylib` + `libloading`** — native speed + runtime load, but the load
  boundary is `unsafe` in slopgb's own code, and Rust has no stable ABI.
- **Hand-rolled `extern "C"` wasm ABI with a slopgb-authored guest shim** —
  needs `slice::from_raw_parts` in the guest SDK → hand-written unsafe. The
  scalar/own-pointer ABI avoids it entirely.
- **wit-bindgen + component model** — would relocate the linkage markers into
  generated code, but wasmi has no component-model support, and it worsens
  author UX (write a `.wit` world) for no memory-safety gain.
- **wasmtime as the primary engine** — faster, but a heavy cranelift/JIT
  dependency; overkill for the SPC700's tiny workload.
- **Per-instruction host↔guest crossing / frame-batched port state** for tier 3
  — a perf cliff and a staleness bug respectively; resolved by the demand-driven
  reconcile.

## Verification

- `cargo test -p slopgb-plugin-api` — `Capabilities`/`Reg` units + the crate
  doctest (the macro expands and type-checks a real plugin).
- `cargo test -p slopgb-plugin-host` — `Snapshot` fidelity, load gating (ABI
  mismatch / unsupported capability), the `host_log` guest-memory read, and the
  `roundtrip` integration test (builds the `frame-probe` fixture to `wasm32`,
  loads it, and asserts its register/memory reads and log resolve against a live
  `GameBoy`).
- `cargo clippy -p slopgb-plugin-api --target wasm32-unknown-unknown -- -D
  warnings` — the guest lints clean for its real target (CI + pre-commit run
  this; the workspace run does not cross-compile).
- `cargo test -p slopgb-core --test gbtr` (`golden_fingerprint`) — default
  (no `--plugins`) is byte-identical.
