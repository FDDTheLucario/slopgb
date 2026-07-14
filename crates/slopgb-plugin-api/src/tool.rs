//! Request/response tool plugins — the shape an MCP-style debug tool takes.
//! A tool plugin is called on demand (not per frame) with an argument string
//! and returns text or an image.

use crate::abi;

/// What a tool call produces.
pub enum ToolResult {
    /// UTF-8 text (the common case: a dump, a table, an evaluated expression).
    Text(String),
    /// Raw image bytes (e.g. a PNG capture).
    Image(Vec<u8>),
}

/// A tool a plugin exposes. Implement this, then invoke
/// [`slopgb_tool_plugin!`](crate::slopgb_tool_plugin).
///
/// The host queries [`name`](ToolPlugin::name) once at load, then routes each
/// matching request to [`call`](ToolPlugin::call).
pub trait ToolPlugin {
    /// Capabilities this tool needs; defaults to read-only introspection.
    const CAPABILITIES: crate::Capabilities = crate::Capabilities::INTROSPECTION;

    /// Construct the tool. Called once, when the host instantiates the module.
    fn new() -> Self
    where
        Self: Sized;

    /// The tool's name (what the host advertises and matches a request against).
    fn name(&self) -> &str;

    /// Handle one request. `args` is the request payload; `gb` is the same
    /// read-only view tier-1 plugins get.
    fn call(&mut self, args: &str, gb: &crate::GameBoyView) -> ToolResult;
}

/// Push a result to the host: `kind` 0 = text, 1 = image. The bytes are the
/// guest's own (`as_ptr`/`len`); the host reads them through wasmi's
/// bounds-checked `Memory`.
#[doc(hidden)]
pub fn __emit(kind: i32, bytes: &[u8]) {
    abi::host_emit(kind, bytes.as_ptr() as i32, bytes.len() as i32);
}

/// Export a [`ToolPlugin`] as a loadable tool module: generates the ABI /
/// capability / name query and the argument-in, result-out call entry points.
#[macro_export]
macro_rules! slopgb_tool_plugin {
    ($ty:ty) => {
        ::std::thread_local! {
            static __SLOPGB_TOOL: ::core::cell::RefCell<$ty> =
                ::core::cell::RefCell::new(<$ty as $crate::ToolPlugin>::new());
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
            <$ty as $crate::ToolPlugin>::CAPABILITIES.bits() as i32
        }

        /// Emit the tool's name (as a text result the host captures at load).
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_tool_name() {
            __SLOPGB_TOOL
                .with_borrow(|t| $crate::__emit(0, $crate::ToolPlugin::name(t).as_bytes()));
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

        /// Run the tool against the `args_len` bytes now in the scratch and emit
        /// the result. Returns 0 on success.
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_call_tool(args_len: i32) -> i32 {
            let args = __SLOPGB_ARG.with_borrow(|b| {
                let n = (args_len.max(0) as usize).min(b.len());
                ::std::string::String::from_utf8_lossy(&b[..n]).into_owned()
            });
            __SLOPGB_TOOL.with_borrow_mut(|t| {
                let view = $crate::GameBoyView::__new();
                match $crate::ToolPlugin::call(t, &args, &view) {
                    $crate::ToolResult::Text(s) => $crate::__emit(0, s.as_bytes()),
                    $crate::ToolResult::Image(v) => $crate::__emit(1, &v),
                }
            });
            0
        }
    };
}
