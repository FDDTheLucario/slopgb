//! Request/response tool plugins — the shape an MCP-style debug tool takes.
//! A tool plugin is called on demand (not per frame) with an argument string
//! and returns text or an image. One module may expose several tools.

use crate::abi;

/// What a tool call produces.
pub enum ToolResult {
    /// UTF-8 text (the common case: a dump, a table, an evaluated expression).
    Text(String),
    /// Raw image bytes (e.g. a PNG capture).
    Image(Vec<u8>),
}

/// A tool a plugin exposes. Implement this, then list it in
/// [`slopgb_tools!`](crate::slopgb_tools) (or the one-tool sugar
/// [`slopgb_tool_plugin!`](crate::slopgb_tool_plugin)).
///
/// At load the host reads each tool's [`name`](ToolPlugin::name),
/// [`description`](ToolPlugin::description), and
/// [`input_schema`](ToolPlugin::input_schema) (for MCP `tools/list`), then routes
/// each matching request to [`call`](ToolPlugin::call).
pub trait ToolPlugin {
    /// Construct the tool. Called once, when the host instantiates the module.
    fn new() -> Self
    where
        Self: Sized;

    /// Capabilities this tool needs; defaults to read-only introspection. A tool
    /// that calls [`GameBoyView::set_breakpoint`](crate::GameBoyView::set_breakpoint)
    /// must widen this to include [`MUTATE`](crate::Capabilities::MUTATE). (A
    /// method, not an associated const, so `ToolPlugin` stays object-safe and a
    /// module can hold a `Box<dyn ToolPlugin>` list.)
    fn capabilities(&self) -> crate::Capabilities {
        crate::Capabilities::INTROSPECTION
    }

    /// The tool's name (what the host advertises and matches a request against).
    fn name(&self) -> &str;

    /// One-line human description, surfaced in MCP `tools/list`. Default empty.
    fn description(&self) -> &str {
        ""
    }

    /// The tool's JSON input schema (a `{"type":"object", …}` string), surfaced
    /// in MCP `tools/list`. Default: an object taking no properties.
    fn input_schema(&self) -> &str {
        r#"{"type":"object","properties":{},"required":[]}"#
    }

    /// Handle one request. `args` is the request payload (the MCP `arguments`
    /// object serialized as JSON); `gb` is the read-only view.
    fn call(&mut self, args: &str, gb: &crate::GameBoyView) -> ToolResult;
}

/// Push a result to the host: `kind` 0 = text, 1 = image. The bytes are the
/// guest's own (`as_ptr`/`len`); the host reads them through wasmi's
/// bounds-checked `Memory`.
#[doc(hidden)]
pub fn __emit(kind: i32, bytes: &[u8]) {
    abi::host_emit(kind, bytes.as_ptr() as i32, bytes.len() as i32);
}

/// Push `words` as their little-endian byte image. wasm linear memory is
/// little-endian, so a `[u16]` already *is* that image: the host reads the
/// slice's own region and the guest never materializes the bytes. Use this
/// instead of [`__emit`] for a large word payload (a framebuffer) — building
/// the intermediate `Vec<u8>` costs an interpreted pass over every byte, which
/// dwarfs the host's single bulk copy.
#[doc(hidden)]
pub fn __emit_words(kind: i32, words: &[u16]) {
    abi::host_emit(kind, words.as_ptr() as i32, (words.len() * 2) as i32);
}

/// Export one or more [`ToolPlugin`]s as a loadable tool module: generates the
/// ABI / capability query, the per-tool metadata + count queries, and the
/// argument-in, result-out call entry points. Tools are addressed by their index
/// in the list.
///
/// ```
/// use slopgb_plugin_api::{GameBoyView, ToolPlugin, ToolResult, slopgb_tools};
///
/// struct Pc;
/// impl ToolPlugin for Pc {
///     fn new() -> Self { Pc }
///     fn name(&self) -> &str { "pc" }
///     fn call(&mut self, _args: &str, gb: &GameBoyView) -> ToolResult {
///         ToolResult::Text(format!("{:04X}", gb.registers().pc))
///     }
/// }
/// slopgb_tools!(Pc);
/// # fn main() {}
/// ```
#[macro_export]
macro_rules! slopgb_tools {
    ($($ty:ty),+ $(,)?) => {
        ::std::thread_local! {
            static __SLOPGB_TOOLS: ::core::cell::RefCell<
                ::std::vec::Vec<::std::boxed::Box<dyn $crate::ToolPlugin>>
            > = ::core::cell::RefCell::new(::std::vec![
                $( ::std::boxed::Box::new(<$ty as $crate::ToolPlugin>::new())
                   as ::std::boxed::Box<dyn $crate::ToolPlugin> ),+
            ]);
            // Scratch the host writes call arguments into; the guest reads it by
            // safe indexing (no pointer reconstruction).
            static __SLOPGB_ARG: ::core::cell::RefCell<::std::vec::Vec<u8>> =
                ::core::cell::RefCell::new(::std::vec::Vec::new());
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_abi_version() -> i32 {
            $crate::ABI_VERSION
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_capabilities() -> i32 {
            // The module needs the union of every tool's capabilities.
            __SLOPGB_TOOLS.with_borrow(|tools| {
                tools
                    .iter()
                    .fold($crate::Capabilities::INTROSPECTION, |acc, t| {
                        acc.union($crate::ToolPlugin::capabilities(t.as_ref()))
                    })
                    .bits() as i32
            })
        }

        /// How many tools this module exposes.
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_tool_count() -> i32 {
            __SLOPGB_TOOLS.with_borrow(|t| t.len() as i32)
        }

        /// Emit one metadata field of tool `idx` as a text result: `field` is
        /// `META_NAME`/`META_DESCRIPTION`/`META_SCHEMA`.
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_tool_meta(idx: i32, field: i32) {
            __SLOPGB_TOOLS.with_borrow(|tools| {
                let Some(t) = usize::try_from(idx).ok().and_then(|i| tools.get(i)) else {
                    return;
                };
                let s = match field {
                    $crate::META_DESCRIPTION => $crate::ToolPlugin::description(t.as_ref()),
                    $crate::META_SCHEMA => $crate::ToolPlugin::input_schema(t.as_ref()),
                    _ => $crate::ToolPlugin::name(t.as_ref()),
                };
                $crate::__emit(0, s.as_bytes());
            });
        }

        /// Reserve `len` bytes of argument scratch and hand the host its address.
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_arg_alloc(len: i32) -> i32 {
            __SLOPGB_ARG.with_borrow_mut(|b| {
                b.clear();
                b.resize(len.max(0) as usize, 0);
                b.as_ptr() as i32
            })
        }

        /// Run tool `idx` against the `args_len` bytes now in the scratch and
        /// emit its result. Returns 0 on success, -1 on a bad index.
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_call_tool(idx: i32, args_len: i32) -> i32 {
            let args = __SLOPGB_ARG.with_borrow(|b| {
                let n = (args_len.max(0) as usize).min(b.len());
                ::std::string::String::from_utf8_lossy(&b[..n]).into_owned()
            });
            __SLOPGB_TOOLS.with_borrow_mut(|tools| {
                let Some(t) = usize::try_from(idx).ok().and_then(|i| tools.get_mut(i)) else {
                    return -1;
                };
                let view = $crate::GameBoyView::__new();
                match $crate::ToolPlugin::call(t.as_mut(), &args, &view) {
                    $crate::ToolResult::Text(s) => $crate::__emit(0, s.as_bytes()),
                    $crate::ToolResult::Image(v) => $crate::__emit(1, &v),
                }
                0
            })
        }
    };
}

/// Export a single [`ToolPlugin`] as a loadable module — sugar for a one-tool
/// [`slopgb_tools!`](crate::slopgb_tools).
#[macro_export]
macro_rules! slopgb_tool_plugin {
    ($ty:ty) => {
        $crate::slopgb_tools!($ty);
    };
}
