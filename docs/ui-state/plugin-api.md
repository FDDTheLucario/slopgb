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

**Manifest (ABI v6).** A coprocessor self-describes through the `Coprocessor::MANIFEST`
associated const (default empty). The generated `slopgb_manifest` export ships it over
the emit channel (kind `EMIT_KIND_MANIFEST`); the host parses it in
`LoadedCoprocessor::manifest() -> Option<Manifest>`. The format is line-based UTF-8, one
record per line, TAB-separated, first field = record type; unknown record types are
ignored, so the schema grows without an ABI break. Records:

```text
id\t<stable-token>              logical identity + role key (e.g. "msu1")
name\t<display name>            human label
provides\t<role>               (0..n) a capability slot this chip can fill
flag\t<name>\t<arg>\t<help>    (0..n) a CLI flag this plugin contributes
menu\t<label>\t<export>\t<ext> (0..n) a main-menu row this plugin/mediator contributes
```

This lets a caller bind a chip by declared identity/role instead of by filename, and lets
the frontend surface a plugin's contributed flags. Optional and metadata-only: an absent
or empty manifest parses to `None` ("undeclared"), and its absence never fails a load —
so it is golden-neutral. Proof: `msu1_roundtrip::manifest_self_describes_the_chip_and_its_flag`.

**`menu` records and who declares them.** A `menu` row names a `label` to show, an
`export` entry point the row dispatches to, and the file `ext` to save the result
as — `MenuContribution { label, export, ext }`, collected by
`PluginRegistry::menus()`. The frontend's main menu (`crates/slopgb/src/
app_menu.rs` `build_plugin_menu_rows`) reads them off the **live engaged SGB
coprocessor** via `GameBoy::coprocessor_manifest` — a plain `&self` accessor over
`AudioCoprocessor::manifest`, not the registry scan — and splices one row per
declared record into the main menu (absent entirely when the manifest is empty,
greyed when `AudioCoprocessor::export_ready(export)` is false). The declaring
unit need not be a wasm plugin: `SgbCoprocessor` (a **native** mediator, not a
guest module) is the one declarer today, for exactly the reason `dump_spc`
(above) is plugin-owned but "Export SPC" is mediator-owned — the from-start `.spc`
snapshot is assembled by the mediator watching the resident engine's play
command, while a plugin's own export is necessarily live-only; letting the
plugin declare the row would silently downgrade the export to a mid-song dump.
`registry.menus()` (the scan-time, filesystem-driven table) stays available for
introspection/listing, but nothing dispatches through it today — doing so would
need a live `LoadedCoprocessor` handle the frontend doesn't hold for a
tier-1-scanned file; that plumbing is deliberately not built.

### CLI flags from manifests (`PluginRegistry`, present-iff)

`crates/slopgb-plugin-host/src/registry.rs`'s `PluginRegistry::scan` reads every `*.wasm`
in the resolved plugins dir and collects each manifest's `flag` records
(`FlagContribution { name, arg, help, default }`); `main` pre-scans argv/env/the persisted
setting for `--plugins` *before* the real CLI parse (a chicken-and-egg the raw pre-scan
breaks — the parse needs the very flag table this directory produces), builds the registry
from that directory, and threads its `flags()` into `cli::Options::parse` as the
`declared` table.

**A plugin-contributed flag exists iff its plugin is present in the resolved plugins dir.**
`--sf2` and `--msu1` are no longer built-in `Options` fields — they parse only when
`sf2.wasm` / `msu1.wasm` respectively are in that directory; otherwise they're a hard
`unknown option '--sf2'` error, same as any unrecognized flag, never a soft warning. This is
a **locked, accepted regression** for a user with a valid `<hash>.smpl` SF2 cache next to
their soundfont and no plugins dir configured: their cache hit used to need no plugin at all
(`session::load_or_import_sf2`'s cache path still works with no plugin); now `--sf2` itself
won't parse without `sf2.wasm` present. `--help` mirrors the same rule: `cli::usage`
splices in each declared flag's help line where `--sf2`/`--msu1` used to be hardcoded, and
omits a flag whose plugin isn't scanned.

The plugin consumes its own flag value — the frontend keeps no typed field for either.
`Options::plugin_flags: Vec<(String, String)>` carries the raw parsed values;
`app_boot::apply_plugin_flags` applies each declared flag's CLI value (else the generic
env fallback below) into the registry via `PluginRegistry::set_flag`;
`app_boot::effective_plugin_flags` reads back every flag's resolved value
(`PluginRegistry::flag` — explicit override, else the manifest's own default expanded
against the registry's `Context`, else absent) and hands the map to
`Session::set_plugin_flags`, which `apply_sgb_coprocessor` reads by name (`"sf2"`,
`"msu1"`) instead of a dedicated field.

**Env fallback is generic**, not per-flag: a declared flag named `name` falls back to
`SLOPGB_<NAME>` (uppercased, `-` → `_`) when no CLI value was given — `sf2` → `SLOPGB_SF2`,
`msu1` → `SLOPGB_MSU1` (today's actual names, preserved by the rule, not coincidence).

**Deferred validation, never a hard error at ROM load.** A flag whose plugin is present but
inapplicable this run (`--msu1` on a DMG ROM, or no SGB coprocessor loaded at all) parses
fine and `Session::apply_sgb_coprocessor` warns once per (re)load to stderr — it must not
hard-error, since a drag-drop ROM swap can change applicability at any time after the window
opens (`Session`'s `plugin_flags_warned` guard, reset by `set_plugin_flags`).

**A duplicate role is a scan-time hard error, fatal only at startup.** Two plugins in the
same directory both declaring the same `provides` role fail `PluginRegistry::scan` with
`RegistryError::DuplicateRole { role, first, second }`; `main`'s startup pre-scan treats
this as fatal (prints both file names, `process::exit(2)`) since nothing is running yet to
lose. `app_menu::rebuild_plugins` (a live plugins-dir change from Options → Plugins) treats
the same error as non-fatal — logged, falling back to an empty registry — because the
window is already open and exiting mid-session would be a worse failure than losing that
directory's contributed flags for the rest of it.

**Orchestration + snapshots (ABI v5, v7).** The exports the SGB coprocessor uses
to install firmware into and snapshot its chips: `set_pc` / `write_ram` /
`read_ram` (v5 — memory peek/poke; reads ride the emit channel as
`EMIT_KIND_RAM`) and `save_state` / `load_state` (v5 — opaque chip state as
`EMIT_KIND_STATE`). An audio chip additionally exports `dump_spc` (v7): it
assembles a `.spc` (SPC700 Sound File) from its ARAM + registers + DSP and ships
it over the emit channel under **its own `EMIT_KIND_SPC`** — a distinct kind from
the save-state so the payload's intent is unambiguous. `LoadedCoprocessor::
dump_spc` decodes that kind; the SGB coprocessor surfaces it as `export_spc` for
the frontend's "Export SPC" (see
[`docs/hardware-state/sgb-audio.md`](../hardware-state/sgb-audio.md)).

The guest half of `read_ram` is `Coprocessor::emit_ram`, defaulted to
`read_ram` + `__emit`. A chip whose bytes are already contiguous in guest memory
overrides it and hands the host that region instead — `__emit_words` for a `[u16]`
buffer, whose little-endian wasm memory already *is* the byte stream. No ABI
change: the export shape and emit kind are untouched, only what the guest passes
to `host_emit`. The SNES PPU's 112 KB framebuffer does exactly this; building
those bytes in the guest cost ~4.5 ms a frame against a ~4 µs host copy (see
[`docs/hardware-state/sgb-snes-ppu.md`](../hardware-state/sgb-snes-ppu.md)).

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
two coprocessor bulk imports (`host_recv` / `host_file`, above); **v5** added the
five orchestration exports (`set_pc` / `write_ram` / `read_ram` / `save_state` /
`load_state`); **v6** added the `slopgb_manifest` self-description export. The wat
test fixtures interpolate `ABI_VERSION`, so a bump auto-tracks; the Rust SDK macros
emit it too — no literal to chase.

## For the full contract

Run `cargo doc -p slopgb-plugin-api --open` — the SDK is the authoritative,
self-documenting interface.
