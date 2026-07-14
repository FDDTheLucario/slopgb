//! Tool plugins: request/response wasm modules the host calls on demand (the
//! shape MCP debug tools take). Unlike the per-frame [`PluginHost`], a tool is
//! invoked with an argument string and returns text or an image.

use slopgb_core::GameBoy;
use slopgb_plugin_api::{ABI_VERSION, Capabilities, ToolResult};
use wasmi::{Engine, Memory, Module, Store, TypedFunc};

use crate::LoadError;
use crate::host::{HostState, build_linker};
use crate::snapshot::Snapshot;

/// One instantiated tool plugin: its advertised name and the entry points to
/// pass arguments in and pull a result out.
pub struct LoadedTool {
    name: String,
    store: Store<HostState>,
    memory: Memory,
    arg_alloc: TypedFunc<i32, i32>,
    call_tool: TypedFunc<i32, i32>,
}

impl LoadedTool {
    /// Instantiate a tool plugin, enforcing the ABI + capability gate and
    /// reading its advertised name.
    pub fn load(bytes: &[u8]) -> Result<Self, LoadError> {
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
        let caps = instance
            .get_typed_func::<(), i32>(&store, "slopgb_capabilities")
            .map_err(|_| LoadError::MissingExport("slopgb_capabilities"))?
            .call(&mut store, ())? as u32;
        if !Capabilities::INTROSPECTION.contains(Capabilities::from_bits(caps)) {
            return Err(LoadError::UnsupportedCapabilities { requested: caps });
        }

        let memory = instance
            .get_memory(&store, "memory")
            .ok_or(LoadError::MissingExport("memory"))?;
        let arg_alloc = instance
            .get_typed_func::<i32, i32>(&store, "slopgb_arg_alloc")
            .map_err(|_| LoadError::MissingExport("slopgb_arg_alloc"))?;
        let call_tool = instance
            .get_typed_func::<i32, i32>(&store, "slopgb_call_tool")
            .map_err(|_| LoadError::MissingExport("slopgb_call_tool"))?;

        // The name is pushed as a text result by slopgb_tool_name().
        let name_fn = instance
            .get_typed_func::<(), ()>(&store, "slopgb_tool_name")
            .map_err(|_| LoadError::MissingExport("slopgb_tool_name"))?;
        store.data_mut().emitted = None;
        name_fn.call(&mut store, ())?;
        let name = match store.data_mut().emitted.take() {
            Some((_, bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
            None => return Err(LoadError::MissingExport("slopgb_tool_name")),
        };

        Ok(Self {
            name,
            store,
            memory,
            arg_alloc,
            call_tool,
        })
    }

    /// The tool's advertised name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Invoke the tool against the live machine. Captures a fresh snapshot so
    /// the tool's `GameBoyView` reads resolve, writes `args` into the guest's
    /// scratch, runs it, and returns whatever it emitted.
    pub fn call(&mut self, args: &str, gb: &GameBoy) -> Result<ToolResult, LoadError> {
        self.store.data_mut().snap = Snapshot::capture(gb);
        self.store.data_mut().emitted = None;

        let len = i32::try_from(args.len()).unwrap_or(i32::MAX);
        let ptr = self.arg_alloc.call(&mut self.store, len)?;
        self.memory
            .write(
                &mut self.store,
                ptr as usize,
                &args.as_bytes()[..len as usize],
            )
            .map_err(|e| LoadError::Wasm(wasmi::Error::new(e.to_string())))?;
        self.call_tool.call(&mut self.store, len)?;

        Ok(match self.store.data_mut().emitted.take() {
            Some((1, bytes)) => ToolResult::Image(bytes),
            Some((_, bytes)) => ToolResult::Text(String::from_utf8_lossy(&bytes).into_owned()),
            None => ToolResult::Text(String::new()),
        })
    }
}
