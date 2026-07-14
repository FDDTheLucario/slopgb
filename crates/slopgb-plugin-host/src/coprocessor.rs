//! Coprocessor plugins (tier 3): a whole chip a plugin hosts (the SGB SPC700 or
//! 65C816), driven by the host through reset / clock / comm-port calls. The
//! chip's internal RAM stays inside the sandbox; only the comm ports cross.

use slopgb_plugin_api::{ABI_VERSION, Capabilities, EMIT_KIND_PCM};
use wasmi::{Engine, Module, Store, TypedFunc};

use crate::LoadError;
use crate::host::{HostState, build_linker};

/// One instantiated coprocessor plugin and the entry points the host drives it
/// with.
pub struct LoadedCoprocessor {
    store: Store<HostState>,
    reset: TypedFunc<(), ()>,
    run_until: TypedFunc<i64, i64>,
    port_write: TypedFunc<(i32, i32), ()>,
    port_read: TypedFunc<i32, i32>,
    drain_pcm: TypedFunc<(), i32>,
}

impl LoadedCoprocessor {
    /// Instantiate a coprocessor plugin, enforcing the ABI + capability gate
    /// (tier 3 requires the `SUBSYSTEM` capability).
    pub fn load(bytes: &[u8]) -> Result<Self, LoadError> {
        let engine = Engine::default();
        let module = Module::new(&engine, bytes)?;
        let mut store = Store::new(&engine, HostState::empty());
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
        let drain_pcm = instance
            .get_typed_func::<(), i32>(&store, "slopgb_drain_pcm")
            .map_err(|_| LoadError::MissingExport("slopgb_drain_pcm"))?;

        Ok(Self {
            store,
            reset,
            run_until,
            port_write,
            port_read,
            drain_pcm,
        })
    }

    /// Power-on / reset the chip.
    pub fn reset(&mut self) -> Result<(), LoadError> {
        self.reset.call(&mut self.store, ())?;
        Ok(())
    }

    /// Set the host→guest **mailbox** (v4) — the bytes the coprocessor's next
    /// `host_recv` (`recv_mailbox`) returns. The frontend deposits a game-written
    /// play-request here; the resident plugin polls it each `run_until`.
    pub fn set_mailbox(&mut self, bytes: &[u8]) {
        self.store.data_mut().mailbox = bytes.to_vec();
    }

    /// Register (or replace) a host-owned **file** (v4) the coprocessor reads by
    /// `key` + offset via `host_file` (`read_file`) — a streaming track `.pcm` or
    /// data `.msu`. The bytes stay host-side; only the requested chunks cross.
    pub fn set_file(&mut self, key: u32, bytes: Vec<u8>) {
        let files = &mut self.store.data_mut().files;
        match files.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = bytes,
            None => files.push((key, bytes)),
        }
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

    /// Drain the stereo PCM the chip synthesized since the last drain, oldest
    /// first — the host mixes this into the Game Boy stream like the built-in
    /// `mix_into`. Empty for a non-audio coprocessor. The plugin ships the
    /// samples over the emit channel (interleaved LE `i16` L,R pairs); this
    /// decodes them back.
    pub fn drain_pcm(&mut self) -> Result<Vec<(i16, i16)>, LoadError> {
        self.store.data_mut().emitted = None;
        let pairs = self.drain_pcm.call(&mut self.store, ())?.max(0) as usize;
        let bytes = match self.store.data_mut().emitted.take() {
            Some((EMIT_KIND_PCM, buf)) => buf,
            // No PCM emitted (a silent/non-audio chip, or a foreign kind).
            _ => return Ok(Vec::new()),
        };
        let mut out = Vec::with_capacity(pairs);
        for c in bytes.chunks_exact(4) {
            out.push((
                i16::from_le_bytes([c[0], c[1]]),
                i16::from_le_bytes([c[2], c[3]]),
            ));
        }
        Ok(out)
    }
}
