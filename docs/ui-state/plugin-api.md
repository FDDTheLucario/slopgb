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

`--plugins <DIR>` (or `SLOPGB_PLUGINS_DIR=<DIR>`) loads **every** `*.wasm` in the
directory. A file that fails to load is logged and skipped, so one bad plugin
can't stop the rest. With no such flag, no plugin machinery runs at all.

## What a plugin can see

`GameBoyView` (handed to `on_frame`) reads a snapshot the host captures just
before each call, so reads are cheap and consistent for the whole frame:

| Method | Returns |
|---|---|
| `read(addr: u16) -> u8` | one byte of the CPU address space (`$0000..=$FFFF`, bank 0), no I/O side effects |
| `reg(Reg) -> u16` | one CPU register or I/O byte — `Af Bc De Hl Sp Pc Lcdc Stat Ly` |
| `registers() -> Registers` | all of the above at once |
| `log(&str)` | append a UTF-8 line to slopgb's stderr |

## Capability tiers

A plugin declares what it needs via `Plugin::CAPABILITIES` (default:
`INTROSPECTION`). The host refuses to load a plugin asking for more than it
currently serves.

| Tier | Bit | Status |
|---|---|---|
| Introspection (read-only) | `INTROSPECTION` | **served now** |
| Mutation (write regs/memory, breakpoints) | `MUTATE` | reserved — rejected at load |
| Subsystem hosting (e.g. be the SPC700) | `SUBSYSTEM` | reserved — rejected at load |

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
after an ABI bump.

## For the full contract

Run `cargo doc -p slopgb-plugin-api --open` — the SDK is the authoritative,
self-documenting interface.
