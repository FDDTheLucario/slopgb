//! [`PluginHost`] — loads wasm plugins, serves their host imports from a
//! per-frame [`Snapshot`], and drives their `on_frame` export.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use slopgb_core::GameBoy;
use slopgb_plugin_api::{ABI_VERSION, Capabilities, Reg};
use wasmi::{Caller, Config, Engine, Extern, Linker, Module, Store, TypedFunc};

use crate::snapshot::Snapshot;

/// Fuel budget for one per-frame `on_frame` (tier 1). Sized far above any
/// legitimate introspection pass; a runaway guest (an infinite loop) exhausts it
/// and traps — logged and skipped by [`PluginHost::pump`] — instead of hanging
/// the host thread with no escape but `kill -9`.
/// ponytail: tunable ceiling — raise if a heavy legitimate plugin ever trips it.
const FRAME_FUEL: u64 = 20_000_000;

/// Fuel for one on-demand tool call (tier 2) and for the one-shot load-time
/// setup calls (start fn + ABI/capability/metadata probes) on both loaders.
/// Larger than [`FRAME_FUEL`] — these run on demand or once at load, not per
/// frame, so a looser bound is fine.
pub(crate) const CALL_FUEL: u64 = 200_000_000;

/// A wasmi engine that meters guest execution, so a runaway plugin traps with
/// `OutOfFuel` (surfaced as a normal call `Err`, handled like any trap) rather
/// than spinning forever. Used by the tier-1 (`--plugins`) and tier-2 (tool/MCP)
/// loaders, whose modules are arbitrary opt-in user wasm. The tier-3 coprocessor
/// deliberately uses the plain [`Engine::default`] — its modules are first-party
/// staged wasm on the host-clocked >=66fps path where per-instruction metering
/// isn't worth the cost (see `coprocessor.rs`).
pub(crate) fn metered_engine() -> Engine {
    let mut cfg = Config::default();
    cfg.consume_fuel(true);
    Engine::new(&cfg)
}

/// wasmi store data: the frame snapshot the imports read, the log lines the
/// guest emitted this frame, and the last result a tool plugin pushed via
/// `host_emit` (kind, bytes). Owned and `'static`, so no `GameBoy` is borrowed.
///
/// `mailbox` + `files` back the v4 coprocessor bulk channels: `host_recv` serves
/// the mailbox (a game-written play-request), `host_file` serves a chunk of a
/// keyed host-owned file (a track `.pcm` / data `.msu`). Both are empty for a
/// per-frame plugin that never touches them.
pub(crate) struct HostState {
    pub(crate) snap: Snapshot,
    pub(crate) log: Vec<String>,
    pub(crate) emitted: Option<(i32, Vec<u8>)>,
    pub(crate) mailbox: Vec<u8>,
    // ponytail: linear-scanned (key, bytes); a coprocessor holds a handful of
    // files (one data ROM + a few tracks). A map only if that ever grows large.
    pub(crate) files: Vec<(u32, Vec<u8>)>,
}

impl HostState {
    /// A store state with no snapshot, log, mailbox, or files (the load-time
    /// default before the first frame / file registration).
    pub(crate) fn empty() -> Self {
        HostState {
            snap: Snapshot::empty(),
            log: Vec::new(),
            emitted: None,
            mailbox: Vec::new(),
            files: Vec::new(),
        }
    }
}

/// Why a plugin failed to load.
#[derive(Debug)]
pub enum LoadError {
    /// The wasm was malformed or an expected export was missing/mistyped.
    Wasm(wasmi::Error),
    /// A required export (`slopgb_abi_version` / `_capabilities` / `_on_frame`)
    /// was absent.
    MissingExport(&'static str),
    /// The plugin targets a different ABI than this host.
    AbiMismatch { found: i32, expected: i32 },
    /// The plugin asked for a capability this host does not yet serve.
    UnsupportedCapabilities { requested: u32 },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Wasm(e) => write!(f, "invalid plugin module: {e}"),
            LoadError::MissingExport(name) => write!(f, "plugin missing export `{name}`"),
            LoadError::AbiMismatch { found, expected } => {
                write!(f, "plugin ABI {found} != host ABI {expected}")
            }
            LoadError::UnsupportedCapabilities { requested } => {
                write!(f, "plugin requests unsupported capabilities {requested:#b}")
            }
        }
    }
}

impl std::error::Error for LoadError {}

impl From<wasmi::Error> for LoadError {
    fn from(e: wasmi::Error) -> Self {
        LoadError::Wasm(e)
    }
}

/// One instantiated plugin and its private store.
pub struct LoadedPlugin {
    name: String,
    caps: Capabilities,
    /// Whether `pump` drives this plugin. On by default at load; the UI toggles
    /// it (a disabled plugin's `on_frame` stops firing but it stays resident).
    enabled: bool,
    store: Store<HostState>,
    on_frame: TypedFunc<(), i32>,
}

impl LoadedPlugin {
    /// The plugin's advertised name (its `.wasm` file stem).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The capabilities this plugin declared at load.
    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        self.caps
    }

    /// Whether `pump` currently drives this plugin.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// A loaded plugin's UI-facing metadata: its name, a human capability label, and
/// whether it is currently enabled. What the Options tab / right-click submenu
/// render (the frontend never touches the [`Capabilities`] type directly).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginInfo {
    pub name: String,
    pub capabilities: String,
    pub enabled: bool,
}

/// A human label for a capability bit set (e.g. `introspection`), for the UI.
fn caps_label(caps: Capabilities) -> String {
    let mut parts = Vec::new();
    for (bit, name) in [
        (Capabilities::INTROSPECTION, "introspection"),
        (Capabilities::MUTATE, "mutate"),
        (Capabilities::SUBSYSTEM, "subsystem"),
    ] {
        if caps.contains(bit) {
            parts.push(name);
        }
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join("+")
    }
}

/// Owns the loaded plugins and drives them once per frame. Empty by default, so
/// a host with no plugins is a no-op — the golden path is untouched.
#[derive(Default)]
pub struct PluginHost {
    plugins: Vec<LoadedPlugin>,
    log: Vec<String>,
    /// The directory the plugins were scanned from (set by [`Self::load_dir`]),
    /// so [`Self::reload`] can re-scan the same place. `None` for a host built
    /// plugin-by-plugin via [`Self::push`].
    dir: Option<PathBuf>,
    /// Valid plugins found in the scanned directory that this per-frame host
    /// does NOT drive — higher-tier ones (`SUBSYSTEM` / tool), which load through
    /// their own seams (`--sgb-coprocessor` / `--msu1` / the MCP host). Recorded
    /// so the UI can list every supported plugin, not silently drop them.
    discovered: Vec<PluginInfo>,
}

impl PluginHost {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load every `*.wasm` in `dir`. A file that fails to load is logged and
    /// skipped, so one bad plugin cannot stop the rest.
    pub fn load_dir(dir: &Path) -> std::io::Result<Self> {
        let mut host = Self::new();
        host.dir = Some(dir.to_owned());
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "wasm") {
                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                match fs::read(&path)
                    .map_err(LoadError::from_io)
                    .and_then(|b| Self::load_bytes(&name, &b))
                {
                    Ok(p) => host.push(p),
                    // A valid plugin of a higher tier (SUBSYSTEM / tool): this
                    // per-frame host doesn't drive it, but it is a real plugin —
                    // record it so the UI lists every supported subsystem rather
                    // than dropping it. It loads through its own seam.
                    Err(LoadError::UnsupportedCapabilities { requested }) => {
                        host.discovered.push(PluginInfo {
                            name: name.into_owned(),
                            capabilities: caps_label(Capabilities::from_bits(requested)),
                            enabled: false,
                        });
                    }
                    Err(e) => eprintln!("slopgb: skipping plugin {}: {e}", path.display()),
                }
            }
        }
        Ok(host)
    }

    /// Instantiate a plugin from raw wasm bytes, enforcing the ABI version and
    /// capability gate. Its own fresh engine keeps plugins independent.
    pub fn load_bytes(name: &str, bytes: &[u8]) -> Result<LoadedPlugin, LoadError> {
        let engine = metered_engine();
        let module = Module::new(&engine, bytes)?;
        let mut store = Store::new(&engine, HostState::empty());
        // Metered engine: every guest call must be fuelled or it traps at once.
        // Cover the start fn (run by `instantiate_and_start`) + the load probes.
        store.set_fuel(CALL_FUEL)?;
        let linker = build_linker(&engine);
        let instance = linker.instantiate_and_start(&mut store, &module)?;

        let version = instance
            .get_typed_func::<(), i32>(&store, "slopgb_abi_version")
            .map_err(|_| LoadError::MissingExport("slopgb_abi_version"))?
            .call(&mut store, ())?;
        if version != ABI_VERSION {
            return Err(LoadError::AbiMismatch {
                found: version,
                expected: ABI_VERSION,
            });
        }

        let caps_bits = instance
            .get_typed_func::<(), i32>(&store, "slopgb_capabilities")
            .map_err(|_| LoadError::MissingExport("slopgb_capabilities"))?
            .call(&mut store, ())? as u32;
        // This loader (the per-frame pump) serves introspection only; anything
        // else is refused up front.
        if !Capabilities::INTROSPECTION.contains(Capabilities::from_bits(caps_bits)) {
            return Err(LoadError::UnsupportedCapabilities {
                requested: caps_bits,
            });
        }

        let on_frame = instance
            .get_typed_func::<(), i32>(&store, "slopgb_on_frame")
            .map_err(|_| LoadError::MissingExport("slopgb_on_frame"))?;

        Ok(LoadedPlugin {
            name: name.to_owned(),
            caps: Capabilities::from_bits(caps_bits),
            enabled: true,
            store,
            on_frame,
        })
    }

    /// Add an already-loaded plugin.
    pub fn push(&mut self, plugin: LoadedPlugin) {
        self.plugins.push(plugin);
    }

    /// Whether any plugins are loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Capture a fresh snapshot and call every plugin's `on_frame`. A plugin
    /// that traps is logged and left in place. Call once per emulated frame.
    pub fn pump(&mut self, gb: &GameBoy) {
        let snap_src = gb;
        let Self { plugins, log, .. } = self;
        for p in plugins.iter_mut() {
            if !p.enabled {
                continue;
            }
            let data = p.store.data_mut();
            data.snap = Snapshot::capture(snap_src);
            data.log.clear();
            // Refill the per-frame fuel budget; a guest that overruns it traps
            // below instead of hanging this thread.
            let _ = p.store.set_fuel(FRAME_FUEL);
            if let Err(e) = p.on_frame.call(&mut p.store, ()) {
                log.push(format!("[{}] trapped: {e}", p.name));
                continue;
            }
            for line in p.store.data_mut().log.drain(..) {
                log.push(format!("[{}] {line}", p.name));
            }
        }
    }

    /// Take the log lines accumulated since the last drain (each prefixed with
    /// its plugin name). The frontend prints these; tests assert on them.
    #[must_use]
    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    /// The directory the plugins were scanned from, if any (set by
    /// [`Self::load_dir`]). The frontend persists this so plugins reload without
    /// re-passing `--plugins` on the next launch.
    #[must_use]
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// UI-facing metadata for every plugin found: the per-frame plugins this host
    /// drives (togglable), then the higher-tier ones it discovered but does not
    /// drive (`SUBSYSTEM` / tool), so the UI lists every supported subsystem.
    #[must_use]
    pub fn infos(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|p| PluginInfo {
                name: p.name.clone(),
                capabilities: caps_label(p.caps),
                enabled: p.enabled,
            })
            .chain(self.discovered.iter().cloned())
            .collect()
    }

    /// Enable or disable the plugin named `name` (a no-op if none matches). A
    /// disabled plugin is skipped by [`Self::pump`], so its `on_frame` stops
    /// firing while it stays resident.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        for p in &mut self.plugins {
            if p.name == name {
                p.enabled = enabled;
            }
        }
    }

    /// Re-scan the directory this host was loaded from ([`Self::load_dir`]),
    /// replacing the loaded set — so a new `.wasm` is picked up and a removed one
    /// dropped. Per-plugin enabled flags are preserved by name across the
    /// re-scan. A no-op for a host with no source directory.
    pub fn reload(&mut self) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let disabled: Vec<String> = self
            .plugins
            .iter()
            .filter(|p| !p.enabled)
            .map(|p| p.name.clone())
            .collect();
        match Self::load_dir(&dir) {
            Ok(fresh) => {
                self.plugins = fresh.plugins;
                self.discovered = fresh.discovered;
                for name in &disabled {
                    self.set_enabled(name, false);
                }
            }
            Err(e) => self
                .log
                .push(format!("plugin reload failed for {}: {e}", dir.display())),
        }
    }
}

impl LoadError {
    fn from_io(e: std::io::Error) -> Self {
        LoadError::Wasm(wasmi::Error::new(e.to_string()))
    }
}

/// Register the read-only host imports. All wasmi calls here are safe;
/// `host_log`/`host_emit` read guest memory through the bounds-checked
/// `Memory::read`.
pub(crate) fn build_linker(engine: &Engine) -> Linker<HostState> {
    let mut linker = Linker::new(engine);
    linker
        .func_wrap(
            "slopgb",
            "host_read",
            |caller: Caller<'_, HostState>, addr: i32| -> i32 {
                i32::from(caller.data().snap.read((addr & 0xFFFF) as u16))
            },
        )
        .and_then(|l| {
            l.func_wrap(
                "slopgb",
                "host_reg",
                |caller: Caller<'_, HostState>, which: i32| -> i32 {
                    match usize::try_from(which).ok().and_then(|i| Reg::ALL.get(i)) {
                        Some(&reg) => i32::from(caller.data().snap.reg(reg)),
                        None => -1,
                    }
                },
            )
        })
        .and_then(|l| {
            l.func_wrap(
                "slopgb",
                "host_log",
                |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                    let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
                        return;
                    };
                    let (Ok(off), Ok(n)) = (usize::try_from(ptr), usize::try_from(len)) else {
                        return;
                    };
                    // Clamp before allocating: a guest-supplied `len` can be up to
                    // i32::MAX; never allocate more than the guest's memory holds
                    // (an over-long read fails the bounds check below anyway).
                    let n = n.min(mem.data_size(&caller));
                    let mut buf = vec![0u8; n];
                    if mem.read(&caller, off, &mut buf).is_ok() {
                        if let Ok(s) = String::from_utf8(buf) {
                            caller.data_mut().log.push(s);
                        }
                    }
                },
            )
        })
        .and_then(|l| {
            l.func_wrap(
                "slopgb",
                "host_emit",
                |mut caller: Caller<'_, HostState>, kind: i32, ptr: i32, len: i32| {
                    let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
                        return;
                    };
                    let (Ok(off), Ok(n)) = (usize::try_from(ptr), usize::try_from(len)) else {
                        return;
                    };
                    // Clamp before allocating (see `host_log`): bound the alloc by
                    // actual guest memory size, not the guest-supplied `len`.
                    let n = n.min(mem.data_size(&caller));
                    let mut buf = vec![0u8; n];
                    if mem.read(&caller, off, &mut buf).is_ok() {
                        caller.data_mut().emitted = Some((kind, buf));
                    }
                },
            )
        })
        .and_then(|l| {
            // v4: hand the guest the mailbox (a game-written play-request). Writes
            // up to `out_cap` bytes into the guest scratch and returns the *true*
            // length, so the guest grows + retries a short buffer.
            l.func_wrap(
                "slopgb",
                "host_recv",
                |mut caller: Caller<'_, HostState>, out_ptr: i32, out_cap: i32| -> i32 {
                    // Clone so the mailbox stays set for the next poll (the guest
                    // edge-detects a change; it is not consumed on read).
                    let mailbox = caller.data().mailbox.clone();
                    write_guest(&mut caller, out_ptr, out_cap, &mailbox);
                    i32::try_from(mailbox.len()).unwrap_or(i32::MAX)
                },
            )
        })
        .and_then(|l| {
            // v4: serve a chunk of a keyed host-owned file (a track `.pcm` / data
            // `.msu`). Writes up to `out_cap` bytes of file `key` at `offset` and
            // returns the byte count actually written (0 = no file / past EOF).
            l.func_wrap(
                "slopgb",
                "host_file",
                |mut caller: Caller<'_, HostState>,
                 key: i32,
                 offset: i32,
                 out_ptr: i32,
                 out_cap: i32|
                 -> i32 {
                    let key = key as u32;
                    let Ok(off) = usize::try_from(offset) else {
                        return 0;
                    };
                    let cap = out_cap.max(0) as usize;
                    let chunk = caller
                        .data()
                        .files
                        .iter()
                        .find(|(k, _)| *k == key)
                        .map(|(_, bytes)| {
                            let end = off.saturating_add(cap).min(bytes.len());
                            bytes.get(off..end).unwrap_or(&[]).to_vec()
                        })
                        .unwrap_or_default();
                    write_guest(&mut caller, out_ptr, out_cap, &chunk)
                },
            )
        })
        .expect("host import names are unique and well-typed");
    linker
}

/// Write `bytes` (capped at `out_cap`) into the guest scratch at `out_ptr`
/// through wasmi's bounds-checked `Memory`, returning the byte count written. No
/// raw pointer crosses; a bad memory/bounds fails closed (returns 0).
fn write_guest(
    caller: &mut Caller<'_, HostState>,
    out_ptr: i32,
    out_cap: i32,
    bytes: &[u8],
) -> i32 {
    let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
        return 0;
    };
    let (Ok(off), Ok(cap)) = (usize::try_from(out_ptr), usize::try_from(out_cap)) else {
        return 0;
    };
    let n = bytes.len().min(cap);
    if mem.write(caller, off, &bytes[..n]).is_ok() {
        i32::try_from(n).unwrap_or(i32::MAX)
    } else {
        0
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
