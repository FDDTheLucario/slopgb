//! Coprocessor plugins (tier 3): a whole chip a plugin hosts (the SGB SPC700 or
//! 65C816), driven by the host through reset / clock / comm-port calls. The
//! chip's internal RAM stays inside the sandbox; only the comm ports cross.

use slopgb_plugin_api::{ABI_VERSION, Capabilities};
use wasmi::{Engine, Module, Store, TypedFunc};

use crate::LoadError;
use crate::host::{HostState, build_linker};
use crate::snapshot::Snapshot;

/// One instantiated coprocessor plugin and the entry points the host drives it
/// with.
pub struct LoadedCoprocessor {
    store: Store<HostState>,
    reset: TypedFunc<(), ()>,
    run_until: TypedFunc<i64, i64>,
    port_write: TypedFunc<(i32, i32), ()>,
    port_read: TypedFunc<i32, i32>,
}

impl LoadedCoprocessor {
    /// Instantiate a coprocessor plugin, enforcing the ABI + capability gate
    /// (tier 3 requires the `SUBSYSTEM` capability).
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
        // Subsystem hosting (optionally with introspection); anything beyond
        // (e.g. MUTATE) is not served on this path.
        let allowed = Capabilities::SUBSYSTEM.union(Capabilities::INTROSPECTION);
        if !allowed.contains(Capabilities::from_bits(caps)) {
            return Err(LoadError::UnsupportedCapabilities { requested: caps });
        }

        let reset = instance
            .get_typed_func::<(), ()>(&store, "slopgb_reset")
            .map_err(|_| LoadError::MissingExport("slopgb_reset"))?;
        let run_until = instance
            .get_typed_func::<i64, i64>(&store, "slopgb_run_until")
            .map_err(|_| LoadError::MissingExport("slopgb_run_until"))?;
        let port_write = instance
            .get_typed_func::<(i32, i32), ()>(&store, "slopgb_port_write")
            .map_err(|_| LoadError::MissingExport("slopgb_port_write"))?;
        let port_read = instance
            .get_typed_func::<i32, i32>(&store, "slopgb_port_read")
            .map_err(|_| LoadError::MissingExport("slopgb_port_read"))?;

        Ok(Self {
            store,
            reset,
            run_until,
            port_write,
            port_read,
        })
    }

    /// Power-on / reset the chip.
    pub fn reset(&mut self) -> Result<(), LoadError> {
        self.reset.call(&mut self.store, ())?;
        Ok(())
    }

    /// Advance the chip to at least `target_cycle`; returns the cycle reached.
    pub fn run_until(&mut self, target_cycle: u64) -> Result<u64, LoadError> {
        let target = i64::try_from(target_cycle).unwrap_or(i64::MAX);
        let got = self.run_until.call(&mut self.store, target)?;
        Ok(u64::try_from(got).unwrap_or(0))
    }

    /// Push a host-side write to comm `port`.
    pub fn port_write(&mut self, port: u8, val: u8) -> Result<(), LoadError> {
        self.port_write
            .call(&mut self.store, (i32::from(port), i32::from(val)))?;
        Ok(())
    }

    /// Read the chip's current value on comm `port`.
    pub fn port_read(&mut self, port: u8) -> Result<u8, LoadError> {
        let v = self.port_read.call(&mut self.store, i32::from(port))?;
        Ok((v & 0xFF) as u8)
    }
}
