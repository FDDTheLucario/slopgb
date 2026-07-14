//! [`PluginHost`] — loads wasm plugins, serves their host imports from a
//! per-frame [`Snapshot`], and drives their `on_frame` export.

use std::fmt;
use std::fs;
use std::path::Path;

use slopgb_core::GameBoy;
use slopgb_plugin_api::{ABI_VERSION, Capabilities, Reg};
use wasmi::{Caller, Engine, Extern, Linker, Module, Store, TypedFunc};

use crate::snapshot::Snapshot;

/// wasmi store data: the frame snapshot the imports read, the log lines the
/// guest emitted this frame, and the last result a tool plugin pushed via
/// `host_emit` (kind, bytes). Owned and `'static`, so no `GameBoy` is borrowed.
pub(crate) struct HostState {
    pub(crate) snap: Snapshot,
    pub(crate) log: Vec<String>,
    pub(crate) emitted: Option<(i32, Vec<u8>)>,
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
    store: Store<HostState>,
    on_frame: TypedFunc<(), i32>,
}

/// Owns the loaded plugins and drives them once per frame. Empty by default, so
/// a host with no plugins is a no-op — the golden path is untouched.
#[derive(Default)]
pub struct PluginHost {
    plugins: Vec<LoadedPlugin>,
    log: Vec<String>,
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
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "wasm") {
                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                match fs::read(&path)
                    .map_err(LoadError::from_io)
                    .and_then(|b| Self::load_bytes(&name, &b))
                {
                    Ok(p) => host.push(p),
                    Err(e) => eprintln!("slopgb: skipping plugin {}: {e}", path.display()),
                }
            }
        }
        Ok(host)
    }

    /// Instantiate a plugin from raw wasm bytes, enforcing the ABI version and
    /// capability gate. Its own fresh engine keeps plugins independent.
    pub fn load_bytes(name: &str, bytes: &[u8]) -> Result<LoadedPlugin, LoadError> {
        let engine = Engine::default();
        let module = Module::new(&engine, bytes)?;
        let mut store = Store::new(
            &engine,
            HostState {
                snap: Snapshot::empty(),
                log: Vec::new(),
                emitted: None,
            },
        );
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
        // Phase 1 serves introspection only; anything else is refused up front.
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
        let Self { plugins, log } = self;
        for p in plugins.iter_mut() {
            let data = p.store.data_mut();
            data.snap = Snapshot::capture(snap_src);
            data.log.clear();
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
                    let mut buf = vec![0u8; n];
                    if mem.read(&caller, off, &mut buf).is_ok() {
                        caller.data_mut().emitted = Some((kind, buf));
                    }
                },
            )
        })
        .expect("host import names are unique and well-typed");
    linker
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
