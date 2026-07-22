//! Tool plugins: request/response wasm modules the host calls on demand (the
//! shape MCP debug tools take). One module may expose several tools, addressed
//! by index.
//!
//! Unlike the per-frame [`PluginHost`](crate::PluginHost), whose imports read an
//! owned [`Snapshot`](crate::Snapshot), the tool host borrows the *live*
//! [`ToolContext`] for the duration of a call (wasmi lets a `Store` hold borrowed
//! data), so the imports call the exact same core/frontend code the native tools
//! do — no copy, and byte-identical results. A fresh instance is made per call so
//! the borrow lives only as long as the call.

use slopgb_core::GameBoy;
use slopgb_plugin_api::{
    ABI_VERSION, Capabilities, META_DESCRIPTION, META_NAME, META_SCHEMA, Reg, ToolResult,
};
use wasmi::{Caller, Engine, Extern, Linker, Memory, Module, Store, TypedFunc};

use crate::LoadError;
use crate::host::{CALL_FUEL, metered_engine};

/// The live machine and its debugger surface, supplied by whoever hosts the tool
/// plugins (the frontend). The scalar reads come off the [`GameBoy`] directly;
/// the formatted/rendered results delegate to the host's own tool code so a
/// ported plugin matches the built-in tool byte-for-byte.
///
/// Every method is read-only introspection except [`set_breakpoint`], the one
/// gated mutation (served only when the calling module declared the
/// [`MUTATE`](slopgb_plugin_api::Capabilities::MUTATE) capability).
///
/// [`set_breakpoint`]: ToolContext::set_breakpoint
pub trait ToolContext {
    /// The live machine, backing `read` / `read_banked` / `cdl_flag` / `reg`.
    fn gb(&self) -> &GameBoy;
    /// Set a PC breakpoint (the App-owned set, not core state).
    fn set_breakpoint(&mut self, addr: u16);
    /// The one-line CPU + LCD register readout (`af=… bc=… …`).
    fn registers(&self) -> String;
    /// The continuous ranges the code/data log has recorded, one per line.
    fn cdl_ranges(&self) -> String;
    /// Disassemble `[from, to]` of `bank`, with symbol substitution.
    fn disassemble(&self, bank: u16, from: u16, to: u16) -> String;
    /// A VRAM view as PNG bytes (empty on an unknown view name).
    fn vram_png(&self, view: &str, scale: u32) -> Vec<u8>;
    /// The current screen as PNG bytes, magnified by `scale`.
    fn screencap_png(&self, scale: u32) -> Vec<u8>;
    /// Evaluate a bgb-style debugger expression.
    fn eval_expr(&self, expr: &str) -> String;
}

/// One tool's advertised metadata, read from the module at load.
#[derive(Clone, Debug)]
pub struct ToolMeta {
    pub name: String,
    pub description: String,
    /// The tool's JSON input schema, verbatim (a `{"type":"object", …}` string).
    pub schema: String,
}

/// wasmi store data for the tool path: the borrowed context (absent while reading
/// metadata at load), the last emitted result, log lines, and whether the module
/// is allowed to mutate.
struct ToolStore<'a> {
    ctx: Option<&'a mut dyn ToolContext>,
    emitted: Option<(i32, Vec<u8>)>,
    log: Vec<String>,
    mutate: bool,
}

/// A loaded tool module: the compiled wasm plus the metadata of every tool it
/// exposes. Instantiated fresh per [`call`](Self::call).
pub struct LoadedTool {
    engine: Engine,
    module: Module,
    tools: Vec<ToolMeta>,
    mutate: bool,
}

impl LoadedTool {
    /// Compile a tool module, enforce the ABI + capability gate, and read the
    /// metadata of every tool it exposes.
    pub fn load(bytes: &[u8]) -> Result<Self, LoadError> {
        let engine = metered_engine();
        let module = Module::new(&engine, bytes)?;
        let empty = ToolStore {
            ctx: None,
            emitted: None,
            log: Vec::new(),
            mutate: false,
        };
        let mut store = Store::new(&engine, empty);
        // Metered engine: fuel the start fn + the load-time metadata probes.
        store.set_fuel(CALL_FUEL)?;
        let linker = build_tool_linker(&engine);
        let instance = linker.instantiate_and_start(&mut store, &module)?;

        let call0 = |store: &mut Store<ToolStore<'static>>, name: &'static str| {
            instance
                .get_typed_func::<(), i32>(&*store, name)
                .map_err(|_| LoadError::MissingExport(name))
                .and_then(|f| f.call(store, ()).map_err(LoadError::from))
        };

        let version = call0(&mut store, "slopgb_abi_version")?;
        if version != ABI_VERSION {
            return Err(LoadError::AbiMismatch {
                found: version,
                expected: ABI_VERSION,
            });
        }
        let caps = call0(&mut store, "slopgb_capabilities")? as u32;
        // The tool host serves read-only introspection plus the gated breakpoint
        // mutation; anything beyond is refused up front.
        let allowed = Capabilities::INTROSPECTION.union(Capabilities::MUTATE);
        if !allowed.contains(Capabilities::from_bits(caps)) {
            return Err(LoadError::UnsupportedCapabilities { requested: caps });
        }
        let mutate = Capabilities::from_bits(caps).contains(Capabilities::MUTATE);

        let count = call0(&mut store, "slopgb_tool_count")?.max(0);
        let meta_fn = instance
            .get_typed_func::<(i32, i32), ()>(&store, "slopgb_tool_meta")
            .map_err(|_| LoadError::MissingExport("slopgb_tool_meta"))?;
        let mut tools = Vec::new();
        for idx in 0..count {
            let read_field = |store: &mut Store<ToolStore<'static>>, field: i32| {
                store.data_mut().emitted = None;
                meta_fn.call(&mut *store, (idx, field))?;
                Ok::<String, LoadError>(match store.data_mut().emitted.take() {
                    Some((_, bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
                    None => String::new(),
                })
            };
            tools.push(ToolMeta {
                name: read_field(&mut store, META_NAME)?,
                description: read_field(&mut store, META_DESCRIPTION)?,
                schema: read_field(&mut store, META_SCHEMA)?,
            });
        }
        if tools.is_empty() {
            return Err(LoadError::MissingExport("slopgb_tool_count"));
        }

        Ok(Self {
            engine,
            module,
            tools,
            mutate,
        })
    }

    /// Every tool this module exposes, in index order.
    #[must_use]
    pub fn tools(&self) -> &[ToolMeta] {
        &self.tools
    }

    /// The index of the tool named `name`, if any.
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.tools.iter().position(|t| t.name == name)
    }

    /// Invoke tool `idx` against the live machine (`ctx`). A fresh instance is
    /// created borrowing `ctx`, `args` is written into the guest scratch, and
    /// whatever the tool emits is returned.
    pub fn call(
        &self,
        idx: usize,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolResult, LoadError> {
        let store_data = ToolStore {
            ctx: Some(ctx),
            emitted: None,
            log: Vec::new(),
            mutate: self.mutate,
        };
        let mut store = Store::new(&self.engine, store_data);
        // Metered engine: bound this call so a runaway tool traps instead of
        // hanging the caller (the frontend / MCP request thread).
        store.set_fuel(CALL_FUEL)?;
        let linker = build_tool_linker(&self.engine);
        let instance = linker.instantiate_and_start(&mut store, &self.module)?;

        let memory: Memory = instance
            .get_memory(&store, "memory")
            .ok_or(LoadError::MissingExport("memory"))?;
        let arg_alloc = instance
            .get_typed_func::<i32, i32>(&store, "slopgb_arg_alloc")
            .map_err(|_| LoadError::MissingExport("slopgb_arg_alloc"))?;
        let call_tool: TypedFunc<(i32, i32), i32> = instance
            .get_typed_func(&store, "slopgb_call_tool")
            .map_err(|_| LoadError::MissingExport("slopgb_call_tool"))?;

        let len = i32::try_from(args.len()).unwrap_or(i32::MAX);
        let ptr = arg_alloc.call(&mut store, len)?;
        memory
            .write(
                &mut store,
                ptr as usize,
                &args.as_bytes()[..len.max(0) as usize],
            )
            .map_err(|e| LoadError::Wasm(wasmi::Error::new(e.to_string())))?;
        call_tool.call(&mut store, (idx as i32, len))?;

        Ok(match store.data_mut().emitted.take() {
            Some((1, bytes)) => ToolResult::Image(bytes),
            Some((_, bytes)) => ToolResult::Text(String::from_utf8_lossy(&bytes).into_owned()),
            None => ToolResult::Text(String::new()),
        })
    }
}

/// A [`Reg`] value from the live machine (mirrors the `Snapshot` mapping).
fn reg_value(gb: &GameBoy, reg: Reg) -> u16 {
    let r = gb.cpu_regs();
    match reg {
        Reg::Af => r.af(),
        Reg::Bc => r.bc(),
        Reg::De => r.de(),
        Reg::Hl => r.hl(),
        Reg::Sp => r.sp,
        Reg::Pc => r.pc,
        Reg::Lcdc => u16::from(gb.debug_read(0xFF40)),
        Reg::Stat => u16::from(gb.debug_read(0xFF41)),
        Reg::Ly => u16::from(gb.debug_read(0xFF44)),
    }
}

/// Read `len` bytes of guest memory at `ptr` as a UTF-8 string (lossy). Used for
/// the string arguments a tool import takes (`vram` view, `expr`).
fn read_guest_str(caller: &Caller<'_, ToolStore>, ptr: i32, len: i32) -> String {
    let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
        return String::new();
    };
    let (Ok(off), Ok(n)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return String::new();
    };
    // Clamp before allocating: never allocate more than the guest's memory holds
    // (an over-long read fails the bounds check below anyway).
    let n = n.min(mem.data_size(caller));
    let mut buf = vec![0u8; n];
    if mem.read(caller, off, &mut buf).is_ok() {
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        String::new()
    }
}

/// Write a bulk result into the guest scratch: at most `out_cap` bytes at
/// `out_ptr`, returning the *true* length so the guest can grow + retry a short
/// buffer. All through wasmi's bounds-checked `Memory` (no raw pointer).
fn write_out(caller: &mut Caller<'_, ToolStore>, out_ptr: i32, out_cap: i32, bytes: &[u8]) -> i32 {
    let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
        return 0;
    };
    let (Ok(off), Ok(cap)) = (usize::try_from(out_ptr), usize::try_from(out_cap)) else {
        return 0;
    };
    let n = bytes.len().min(cap);
    if let Err(e) = mem.write(caller, off, &bytes[..n]) {
        eprintln!("slopgb: plugin host write to guest memory failed: {e}");
        return 0;
    }
    i32::try_from(bytes.len()).unwrap_or(i32::MAX)
}

/// Register the tool-host imports. Every closure reads the borrowed
/// [`ToolContext`] out of the store; while reading metadata at load the context
/// is absent, but those exports call only `host_emit`.
fn build_tool_linker<'a>(engine: &Engine) -> Linker<ToolStore<'a>> {
    let mut linker = Linker::new(engine);
    let r = (|| -> Result<(), wasmi::Error> {
        linker.func_wrap(
            "slopgb",
            "host_read",
            |caller: Caller<'_, ToolStore>, addr: i32| {
                caller
                    .data()
                    .ctx
                    .as_deref()
                    .map_or(0, |c| i32::from(c.gb().debug_read((addr & 0xFFFF) as u16)))
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_reg",
            |caller: Caller<'_, ToolStore>, which: i32| {
                let reg = usize::try_from(which)
                    .ok()
                    .and_then(|i| Reg::ALL.get(i).copied());
                match (caller.data().ctx.as_deref(), reg) {
                    (Some(c), Some(reg)) => i32::from(reg_value(c.gb(), reg)),
                    _ => -1,
                }
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_log",
            |mut caller: Caller<'_, ToolStore>, ptr: i32, len: i32| {
                let s = read_guest_str(&caller, ptr, len);
                caller.data_mut().log.push(s);
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_emit",
            |mut caller: Caller<'_, ToolStore>, kind: i32, ptr: i32, len: i32| {
                let Some(Extern::Memory(mem)) = caller.get_export("memory") else {
                    return;
                };
                let (Ok(off), Ok(n)) = (usize::try_from(ptr), usize::try_from(len)) else {
                    return;
                };
                // Clamp before allocating (see `read_guest_str`): bound the alloc
                // by actual guest memory size, not the guest-supplied `len`.
                let n = n.min(mem.data_size(&caller));
                let mut buf = vec![0u8; n];
                if mem.read(&caller, off, &mut buf).is_ok() {
                    caller.data_mut().emitted = Some((kind, buf));
                }
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_read_banked",
            |caller: Caller<'_, ToolStore>, bank: i32, addr: i32| {
                caller.data().ctx.as_deref().map_or(0, |c| {
                    i32::from(
                        c.gb()
                            .debug_read_banked((bank & 0xFFFF) as u16, (addr & 0xFFFF) as u16),
                    )
                })
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_cdl_flag",
            |caller: Caller<'_, ToolStore>, bank: i32, addr: i32| {
                caller.data().ctx.as_deref().map_or(0, |c| {
                    i32::from(
                        c.gb()
                            .cdl_flag_banked((bank & 0xFFFF) as u16, (addr & 0xFFFF) as u16),
                    )
                })
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_set_breakpoint",
            |mut caller: Caller<'_, ToolStore>, addr: i32| -> i32 {
                if !caller.data().mutate {
                    return -1; // module did not declare MUTATE: no-op
                }
                match caller.data_mut().ctx.as_deref_mut() {
                    Some(c) => {
                        c.set_breakpoint((addr & 0xFFFF) as u16);
                        0
                    }
                    None => -1,
                }
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_registers",
            |mut caller: Caller<'_, ToolStore>, out_ptr: i32, out_cap: i32| {
                let bytes = caller
                    .data()
                    .ctx
                    .as_deref()
                    .map(|c| c.registers().into_bytes());
                write_out(&mut caller, out_ptr, out_cap, &bytes.unwrap_or_default())
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_cdl_ranges",
            |mut caller: Caller<'_, ToolStore>, out_ptr: i32, out_cap: i32| {
                let bytes = caller
                    .data()
                    .ctx
                    .as_deref()
                    .map(|c| c.cdl_ranges().into_bytes());
                write_out(&mut caller, out_ptr, out_cap, &bytes.unwrap_or_default())
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_disasm",
            |mut caller: Caller<'_, ToolStore>,
             bank: i32,
             from: i32,
             to: i32,
             out_ptr: i32,
             out_cap: i32| {
                let bytes = caller.data().ctx.as_deref().map(|c| {
                    c.disassemble(
                        (bank & 0xFFFF) as u16,
                        (from & 0xFFFF) as u16,
                        (to & 0xFFFF) as u16,
                    )
                    .into_bytes()
                });
                write_out(&mut caller, out_ptr, out_cap, &bytes.unwrap_or_default())
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_screencap",
            |mut caller: Caller<'_, ToolStore>, scale: i32, out_ptr: i32, out_cap: i32| {
                let bytes = caller
                    .data()
                    .ctx
                    .as_deref()
                    .map(|c| c.screencap_png(scale.max(1) as u32));
                write_out(&mut caller, out_ptr, out_cap, &bytes.unwrap_or_default())
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_vram",
            |mut caller: Caller<'_, ToolStore>,
             view_ptr: i32,
             view_len: i32,
             scale: i32,
             out_ptr: i32,
             out_cap: i32| {
                let view = read_guest_str(&caller, view_ptr, view_len);
                let bytes = caller
                    .data()
                    .ctx
                    .as_deref()
                    .map(|c| c.vram_png(&view, scale.max(1) as u32));
                write_out(&mut caller, out_ptr, out_cap, &bytes.unwrap_or_default())
            },
        )?;
        linker.func_wrap(
            "slopgb",
            "host_expr",
            |mut caller: Caller<'_, ToolStore>,
             in_ptr: i32,
             in_len: i32,
             out_ptr: i32,
             out_cap: i32| {
                let expr = read_guest_str(&caller, in_ptr, in_len);
                let bytes = caller
                    .data()
                    .ctx
                    .as_deref()
                    .map(|c| c.eval_expr(&expr).into_bytes());
                write_out(&mut caller, out_ptr, out_cap, &bytes.unwrap_or_default())
            },
        )?;
        Ok(())
    })();
    r.expect("host import names are unique and well-typed");
    linker
}
