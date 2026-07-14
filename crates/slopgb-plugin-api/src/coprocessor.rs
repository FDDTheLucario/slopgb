//! Subsystem coprocessor plugins (tier 3): a whole chip a plugin hosts — e.g.
//! the SGB SPC700 or the 65C816 — running its own RAM inside the sandbox and
//! driven by the host through a clock + comm-port interface. The coprocessor's
//! internal bus never crosses the boundary; only the comm ports (and, for audio
//! chips, drained PCM) do, so a 1 MHz chip costs a handful of crossings per
//! frame, not one per memory access.

/// A chip a plugin hosts. Implement this, then invoke
/// [`slopgb_coprocessor_plugin!`](crate::slopgb_coprocessor_plugin).
pub trait Coprocessor {
    /// Capabilities; subsystem hosting is the tier-3 gate.
    const CAPABILITIES: crate::Capabilities = crate::Capabilities::SUBSYSTEM;

    /// Construct the coprocessor. Called once, when the host instantiates it.
    fn new() -> Self
    where
        Self: Sized;

    /// Power-on / reset.
    fn reset(&mut self);

    /// Advance the chip to at least `target_cycle` (its own cycle domain).
    /// Returns the cycle actually reached (`>= target_cycle`).
    fn run_until(&mut self, target_cycle: u64) -> u64;

    /// A host-side write to comm `port` (the GB/SNES → chip direction).
    fn port_write(&mut self, port: u8, val: u8);

    /// The chip's current value on comm `port` (the chip → GB/SNES direction).
    fn port_read(&mut self, port: u8) -> u8;
}

/// Export a [`Coprocessor`] as a loadable subsystem module: generates the ABI /
/// capability query and the reset / clock / comm-port entry points the host
/// drives.
#[macro_export]
macro_rules! slopgb_coprocessor_plugin {
    ($ty:ty) => {
        ::std::thread_local! {
            static __SLOPGB_COP: ::core::cell::RefCell<$ty> =
                ::core::cell::RefCell::new(<$ty as $crate::Coprocessor>::new());
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_abi_version() -> i32 {
            $crate::ABI_VERSION
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_capabilities() -> i32 {
            <$ty as $crate::Coprocessor>::CAPABILITIES.bits() as i32
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_reset() {
            __SLOPGB_COP.with_borrow_mut(|c| $crate::Coprocessor::reset(c));
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_run_until(target_cycle: i64) -> i64 {
            __SLOPGB_COP
                .with_borrow_mut(|c| $crate::Coprocessor::run_until(c, target_cycle.max(0) as u64))
                as i64
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_port_write(port: i32, val: i32) {
            __SLOPGB_COP
                .with_borrow_mut(|c| $crate::Coprocessor::port_write(c, port as u8, val as u8));
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_port_read(port: i32) -> i32 {
            i32::from(
                __SLOPGB_COP.with_borrow_mut(|c| $crate::Coprocessor::port_read(c, port as u8)),
            )
        }
    };
}
