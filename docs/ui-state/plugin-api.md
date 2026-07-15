# Plugin API

slopgb loads **plugins written in Rust and compiled to WebAssembly**. A plugin
is a `.wasm` file dropped into a directory; slopgb loads it at runtime — no
recompile of slopgb, no `unsafe` in your plugin, and (in this first tier) no way
for a plugin to perturb emulation. It observes the live machine once per frame.

The runtime lives in `crates/slopgb-plugin-host` (wraps the pure-Rust
[`wasmi`](https://github.com/wasmi-labs/wasmi) interpreter); the SDK a plugin
author depends on is `crates/slopgb-plugin-api`.

## Why wasm, and why no unsafe

The three constraints that shaped this — plugins authored in Rust, loadable at
runtime without rebuilding slopgb, and fast enough to one day host the SPC700 —
have exactly one solution in safe Rust: compile the plugin to `wasm32` ahead of
time and run it in a sandboxed interpreter with a safe host API. Native dynamic
loading (`dlopen`/`libloading`) is `unsafe` at the boundary and has no stable
ABI; a wasm interpreter is neither.

No hand-written `unsafe` appears in slopgb or in your plugin. The guest SDK
carries only two linkage *markers* (an `unsafe extern` import block and the
`#[unsafe(no_mangle)]` on the generated exports) — no `unsafe` blocks, no raw
pointers. Host→guest data crosses as one scalar per call; guest→host strings
cross as the guest's own `(ptr, len)`, which the host reads back through wasmi's
bounds-checked `Memory`.

## Writing a plugin

```sh
cargo new --lib my-plugin
cd my-plugin
rustup target add wasm32-unknown-unknown        # once per machine
```

`Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
slopgb-plugin-api = { path = "…/slopgb/crates/slopgb-plugin-api" }
```

`src/lib.rs`:

```rust
use slopgb_plugin_api::{GameBoyView, Plugin, Reg, slopgb_plugin};

#[derive(Default)]
struct FrameCounter {
    frames: u32,
}

impl Plugin for FrameCounter {
    fn new() -> Self {
        Self::default()
    }

    fn on_frame(&mut self, gb: &GameBoyView) {
        self.frames += 1;
        let ly = gb.reg(Reg::Ly);
        let op = gb.read(gb.reg(Reg::Pc));
        gb.log(&format!("frame {} ly={ly} op@pc={op:02X}", self.frames));
    }
}

slopgb_plugin!(FrameCounter);
```

Build and run:

```sh
cargo build --release --target wasm32-unknown-unknown
slopgb --plugins target/wasm32-unknown-unknown/release game.gb
# each plugin's log lines print to stderr, prefixed with the plugin's file stem
```

> **Toolchain note.** `wasm32-unknown-unknown` must be installed for the exact
> toolchain that builds the crate. Inside the slopgb tree that is automatic
> (`rust-toolchain.toml` pins the target); a **standalone** plugin crate uses
> your default toolchain, so run `rustup target add wasm32-unknown-unknown`
> first — otherwise the build fails with a misleading "target may not be
> installed" even when `rustup` reports the target present for a *different*
> toolchain.

`--plugins <DIR>` (or `SLOPGB_PLUGINS_DIR=<DIR>`) loads **every** `*.wasm` in the
directory. A file that fails to load is logged and skipped, so one bad plugin
can't stop the rest. With no such flag, no plugin machinery runs at all.

## Compiling several plugins at once

Put each plugin in its own crate under one **plugin workspace**, so a single
build drops every `.wasm` into one directory you point `--plugins` at:

```
my-plugins/
  Cargo.toml            # [workspace] members = ["frame-counter", "pc-logger", …]
  frame-counter/{Cargo.toml, src/lib.rs}
  pc-logger/{Cargo.toml, src/lib.rs}
```

Share the SDK dependency once at the workspace root so each member just inherits
it:

```toml
# my-plugins/Cargo.toml
[workspace]
resolver = "3"
members = ["frame-counter", "pc-logger"]
[workspace.dependencies]
slopgb-plugin-api = { path = "…/slopgb/crates/slopgb-plugin-api" }
[profile.release]
opt-level = "z"
strip = true
```

Each member is a `cdylib` that inherits the shared dep:

```toml
# my-plugins/frame-counter/Cargo.toml
[lib]
crate-type = ["cdylib"]
[dependencies]
slopgb-plugin-api.workspace = true
```

Build them all with one command, then load the whole directory:

```sh
cargo build --release --target wasm32-unknown-unknown   # builds every member
slopgb --plugins my-plugins/target/wasm32-unknown-unknown/release game.gb
```

Every member emits one `.wasm` into `target/wasm32-unknown-unknown/release/`
(crate `frame-counter` → `frame_counter.wasm` — dashes become underscores).
Build a subset with `-p frame-counter -p pc-logger`. That release directory *is*
your plugins dir — no renaming needed for tier-1/2 plugins (they report their
name from wasm metadata); only the tier-3 coprocessor seams need fixed filenames
(handled by `cargo xtask stage-plugins <dir>`).

## What a plugin can see

`GameBoyView` (handed to `on_frame`) reads a snapshot the host captures just
before each call, so reads are cheap and consistent for the whole frame:

| Method | Returns |
|---|---|
| `read(addr: u16) -> u8` | one byte of the CPU address space (`$0000..=$FFFF`, bank 0), no I/O side effects |
| `reg(Reg) -> u16` | one CPU register or I/O byte — `Af Bc De Hl Sp Pc Lcdc Stat Ly` |
| `registers() -> Registers` | all of the above at once |
| `log(&str)` | append a UTF-8 line to slopgb's stderr |

## Tool plugins (MCP debug tools)

A **tool plugin** is called on demand instead of every frame: it takes a request
and returns text or an image, which is exactly the shape of an [MCP debug
tool](mcp-server.md). A plugin can register a new MCP tool that the built-in
server then advertises and dispatches alongside its own — third parties extend
the tool set without touching slopgb.

Implement `ToolPlugin` and list your tools in `slopgb_tools!` (a module may expose
several). The nine built-in tools are themselves ported to a reference plugin
(`crates/slopgb/reference-tools/`) as the dogfood/proof set — a parity test pins
each one byte-identical to its built-in.

```rust
use slopgb_plugin_api::{GameBoyView, ToolPlugin, ToolResult, args, slopgb_tools};

struct Peek;
impl ToolPlugin for Peek {
    fn new() -> Self { Peek }
    fn name(&self) -> &str { "peek" }
    fn description(&self) -> &str { "Dump memory bytes." }
    fn input_schema(&self) -> &str {
        r#"{"type":"object","properties":{"from":{"type":"string"}},"required":["from"]}"#
    }
    fn call(&mut self, req: &str, gb: &GameBoyView) -> ToolResult {
        // `req` is the MCP `arguments` object as JSON; pull fields with `args::field`.
        let from = args::field(req, "from").unwrap_or_default();
        // …parse, read via the view, format…
        ToolResult::Text(format!("{from}: {:02X}", gb.read(0)))
    }
}
slopgb_tools!(Peek);
```

Beyond the tier-1 accessors, `GameBoyView` gives a tool plugin the richer debug
surface (served only on the tool host):

| Method | Returns |
|---|---|
| `read_banked(bank, addr) -> u8` / `cdl_flag(bank, addr) -> u8` | a byte / its code-data-log flags in an explicit bank |
| `set_breakpoint(addr)` | set a PC breakpoint (needs `MUTATE`) |
| `registers_text()` / `cdl_ranges()` / `disassemble(bank, from, to)` / `expr(&str)` | the host's formatted text results |
| `screencap(scale)` / `vram(view, scale)` | PNG bytes |

The text/image bulk results cross host→guest through a guest-owned scratch buffer
the guest reads by safe indexing — no `unsafe`, no raw pointers.

`--plugins <DIR>` loads tool plugins from the same directory as tier-1 plugins
(each `*.wasm` is tried as both; the wrong shape is skipped). The MCP server picks
them up automatically; start it with `--mcp-port`. A plugin tool whose name
matches a built-in **wins** (so the reference plugins can transparently stand in
for the built-ins).

## Capability tiers

A plugin declares what it needs (`Plugin::CAPABILITIES` / `ToolPlugin::capabilities()`,
default `INTROSPECTION`). Each host refuses a plugin asking for more than it serves.

| Tier | Bit | Status |
|---|---|---|
| Introspection (read-only) | `INTROSPECTION` | **served on every host** |
| Mutation (write regs/memory, breakpoints) | `MUTATE` | **served on the tool host** (`set_breakpoint`); rejected on the per-frame host |
| Subsystem hosting (e.g. be the SPC700) | `SUBSYSTEM` | **served** via `LoadedCoprocessor` |

## Coprocessor plugins (tier 3)

A coprocessor plugin implements `Coprocessor` (invoke `slopgb_coprocessor_plugin!`)
and hosts a whole chip inside the sandbox: the chip's RAM never crosses the
boundary, only its comm ports (and, for audio chips, drained PCM) do. The host
drives it with `reset` / `run_until` (the chip's own cycle domain) / `port_write`
/ `port_read` / `drain_pcm` through `LoadedCoprocessor`. Two references:

- `crates/slopgb-w65c816-plugin` wraps the clean-room 65C816 (`slopgb-w65c816`)
  over a guest SNES-RAM + comm-port bus — the SNES-side CPU route for a full SGB.
  Proof: `slopgb-plugin-host/tests/w65c816_roundtrip.rs`.
- `crates/slopgb-spc700-plugin` wraps the SPC700 + S-DSP (`slopgb-snes-apu`, the
  *same* code the core built-in SGB audio path runs) — clocking it in wasm runs
  the real SPC700 IPL ROM (the `$AA`/`$BB` boot handshake) and the S-DSP
  synthesizes. Proof: `slopgb-plugin-host/tests/spc700_roundtrip.rs`.
- `crates/slopgb-msu1-plugin` is an **MSU-1 streaming-audio** chip: the eight
  MSU-1 registers (`$2000-$2007`) map 1:1 to comm ports `0..=7`, streaming a
  user-supplied `.pcm` track and reading a `.msu` data ROM through the v4 bulk
  channels below. Proof: `slopgb-plugin-host/tests/msu1_roundtrip.rs` (register
  select/seek/play, the data port, a looping track, and the mailbox mode). See
  [`docs/msu1-plugin-plan.md`](../msu1-plugin-plan.md).

**PCM drain (ABI v3).** `drain_pcm` (default: none, for a non-audio chip like the
65C816) returns the stereo samples synthesized since the last drain; the generated
`slopgb_drain_pcm` export ships them over the emit channel (interleaved LE `i16`
L,R pairs, kind `EMIT_KIND_PCM`) and the host decodes them in `LoadedCoprocessor::
drain_pcm` to mix like the built-in `mix_into`. Proof:
`spc700_roundtrip::spc700_pcm_drains_to_the_host`.

**Bulk channels (ABI v4).** Two host→guest imports let a *streaming* coprocessor
move more than the scalar comm ports can carry, both reusing the guest-scratch
pattern (the host writes into a guest-owned buffer through wasmi's bounds-checked
`Memory` — no `unsafe`, no raw pointer):

- `recv_mailbox() -> Vec<u8>` (import `host_recv`): read the **mailbox**, the bytes
  a game / the frontend last deposited via `LoadedCoprocessor::set_mailbox` — a
  play-request the game writes through `DATA_SND`. A resident coprocessor polls it
  each `run_until` and edge-detects a change (the "resident handler + polled
  mailbox" pattern). The **per-frame hook** is that already-pumped `run_until`
  itself; no extra export.
- `read_file(key, offset, buf) -> usize` (import `host_file`): read a chunk of a
  keyed **host-owned file** registered with `LoadedCoprocessor::set_file` — a track
  `.pcm` or data `.msu`, far larger than a comm port. The bytes stay host-side;
  only the requested chunk crosses. `key`'s meaning is a host↔plugin convention
  (MSU-1: the audio track number, or `DATA_FILE_KEY` for the data ROM).

## Golden-safe rules

The one invariant this project guards is that no UI/extension feature perturbs
emulation. For plugins that means: this tier is **read-only**, `--plugins` is
**off by default**, and with no plugins loaded the pump is a no-op — so the
golden frame-hash is byte-identical (pinned by `golden_fingerprint`). A plugin
that traps is logged and left in place; it cannot corrupt the machine.

## Managing plugins from the UI

Plugins load from `--plugins <dir>` / `SLOPGB_PLUGINS_DIR`, or — when neither is
given — the persisted `settings.plugins.dir` (whatever directory plugins last
loaded from is remembered, so they reload without re-passing the flag). Two UI
surfaces manage them; **Options → Plugins** is the primary home, the right-click
**Plugins** submenu is secondary live status.

**Options → Plugins tab** (`OptionsTab::Plugins`, `tabs::plugins`): one **enable**
checkbox per discovered plugin (`name [capabilities]`), the read-only plugins-dir
line, and an **allow-mutation** toggle. The tab reads `Settings.plugins`
(`PluginConfig` — `dir`, `allow_mutation`, `entries: Vec<PluginEntry>`); the entry
list is synced from the live `PluginHost::infos()` each time the dialog opens
(`App::sync_plugin_entries`). Toggling a checkbox edits `entries[i].enabled`;
OK/Apply pushes each flag to the host via `PluginHost::set_enabled`, so a disabled
plugin's `on_frame` is skipped by `pump` (it stays resident, just idle).

**Right-click → Plugins submenu** (`SubKind::Plugins`, `SubMenu::plugins`): a
status row per loaded plugin — check-marked while enabled, greyed while disabled,
non-clickable — then a live **Reload plugins** action (`SubChoice::ReloadPlugins`
→ `PluginHost::reload`) that re-scans the directory, picking up a newly-dropped
`.wasm` and dropping a removed one while preserving each plugin's enabled flag by
name.

**Persistence** (`settings_file/`): `PluginConfig` round-trips the `dir`,
`allow_mutation`, and the *disabled* plugin names (the enabled set's complement —
a new plugin defaults to enabled). Native `slopgb.conf` uses a `[plugins]` section
(`dir` / `allow_mutation` / `disabled = a, b`); bgb.ini uses the `Slopgb*` extras
(`SlopgbPluginsDir` / `SlopgbPluginsAllowMutation` / `SlopgbPluginsDisabled`), so
bgb's own keys survive verbatim. The capability label per entry is runtime-only
(refilled from the host on sync), not persisted.

`allow_mutation` is a persisted, default-off preference reserved for the (not-yet-
served) `MUTATE` tier — it currently gates nothing, keeping the golden path
byte-identical.

## ABI versioning

The guest exports `slopgb_abi_version()`; the host refuses a plugin whose version
differs from its own (`ABI_VERSION`). Rebuild a plugin against the matching SDK
after an ABI bump. History: v3 added the coprocessor PCM drain; **v4** added the
two coprocessor bulk imports (`host_recv` / `host_file`, above). The wat test
fixtures interpolate `ABI_VERSION`, so a bump auto-tracks; the Rust SDK macros
emit it too — no literal to chase.

## For the full contract

Run `cargo doc -p slopgb-plugin-api --open` — the SDK is the authoritative,
self-documenting interface.
