//! Guest SDK for slopgb plugins. A plugin is a `wasm32` library that implements
//! [`Plugin`] and invokes [`slopgb_plugin!`]. The host is `slopgb-plugin-host`.
//! Guide: `docs/ui-state/plugin-api.md`.

mod abi;
pub mod args;
mod coprocessor;
mod tool;
mod view;

pub use abi::{ABI_VERSION, META_DESCRIPTION, META_NAME, META_SCHEMA, Reg};
pub use coprocessor::{
    Coprocessor, EMIT_KIND_PCM, EMIT_KIND_RAM, EMIT_KIND_STATE, read_file, recv_mailbox,
};
pub use tool::{__emit, ToolPlugin, ToolResult};
pub use view::{GameBoyView, Registers};

/// What a plugin is allowed to do, as a bit set. Tier 1 ([`INTROSPECTION`]) is
/// read-only; higher tiers are reserved and host-gated.
///
/// [`INTROSPECTION`]: Capabilities::INTROSPECTION
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities(u32);

impl Capabilities {
    /// Read-only observation of the live machine (tier 1).
    pub const INTROSPECTION: Self = Self(1 << 0);
    /// Reserved: writing registers/memory/breakpoints (tier 2).
    pub const MUTATE: Self = Self(1 << 1);
    /// Reserved: hosting a whole subsystem, e.g. the SPC700 (tier 3).
    pub const SUBSYSTEM: Self = Self(1 << 2);

    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// A slopgb plugin. Implement this, then invoke [`slopgb_plugin!`] to export it.
///
/// ```
/// use slopgb_plugin_api::{GameBoyView, Plugin, Reg, slopgb_plugin};
///
/// #[derive(Default)]
/// struct FrameCounter {
///     frames: u32,
/// }
///
/// impl Plugin for FrameCounter {
///     fn new() -> Self {
///         Self::default()
///     }
///     fn on_frame(&mut self, gb: &GameBoyView) {
///         self.frames += 1;
///         let ly = gb.reg(Reg::Ly);
///         gb.log(&format!("frame {} ly={ly}", self.frames));
///     }
/// }
///
/// slopgb_plugin!(FrameCounter);
/// # fn main() {}
/// ```
pub trait Plugin {
    /// Capabilities this plugin requires. Defaults to read-only introspection.
    const CAPABILITIES: Capabilities = Capabilities::INTROSPECTION;

    /// Construct the plugin. Called once, when the host instantiates the module.
    fn new() -> Self
    where
        Self: Sized;

    /// Called once per emulated frame with a read-only view of the machine.
    fn on_frame(&mut self, gb: &GameBoyView);
}

/// Export a [`Plugin`] as a loadable slopgb module: generates the ABI-version,
/// capability, and per-frame entry points the host calls, backed by a single
/// plugin instance.
#[macro_export]
macro_rules! slopgb_plugin {
    ($ty:ty) => {
        ::std::thread_local! {
            static __SLOPGB_PLUGIN: ::core::cell::RefCell<$ty> =
                ::core::cell::RefCell::new(<$ty as $crate::Plugin>::new());
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_abi_version() -> i32 {
            $crate::ABI_VERSION
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_capabilities() -> i32 {
            <$ty as $crate::Plugin>::CAPABILITIES.bits() as i32
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_on_frame() -> i32 {
            __SLOPGB_PLUGIN.with_borrow_mut(|p| {
                $crate::Plugin::on_frame(p, &$crate::GameBoyView::__new());
            });
            0
        }
    };
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
