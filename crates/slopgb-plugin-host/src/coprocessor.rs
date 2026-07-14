//! Coprocessor plugins (tier 3): a whole chip a plugin hosts (the SGB SPC700 or
//! 65C816), driven by the host through reset / clock / comm-port calls. The
//! chip's internal RAM stays inside the sandbox; only the comm ports cross.

use slopgb_plugin_api::{ABI_VERSION, Capabilities, EMIT_KIND_PCM, EMIT_KIND_RAM, EMIT_KIND_STATE};
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
    set_pc: TypedFunc<i32, ()>,
    write_ram: TypedFunc<i32, ()>,
    read_ram: TypedFunc<(i32, i32), i32>,
    save_state: TypedFunc<(), i32>,
    load_state: TypedFunc<(), ()>,
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
        let set_pc = instance
            .get_typed_func::<i32, ()>(&store, "slopgb_set_pc")
            .map_err(|_| LoadError::MissingExport("slopgb_set_pc"))?;
        let write_ram = instance
            .get_typed_func::<i32, ()>(&store, "slopgb_write_ram")
            .map_err(|_| LoadError::MissingExport("slopgb_write_ram"))?;
        let read_ram = instance
            .get_typed_func::<(i32, i32), i32>(&store, "slopgb_read_ram")
            .map_err(|_| LoadError::MissingExport("slopgb_read_ram"))?;
        let save_state = instance
            .get_typed_func::<(), i32>(&store, "slopgb_save_state")
            .map_err(|_| LoadError::MissingExport("slopgb_save_state"))?;
        let load_state = instance
            .get_typed_func::<(), ()>(&store, "slopgb_load_state")
            .map_err(|_| LoadError::MissingExport("slopgb_load_state"))?;

        Ok(Self {
            store,
            reset,
            run_until,
            port_write,
            port_read,
            drain_pcm,
            set_pc,
            write_ram,
            read_ram,
            save_state,
            load_state,
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

    /// Redirect the chip's program counter to `addr` (its own address space).
    pub fn set_pc(&mut self, addr: u32) -> Result<(), LoadError> {
        self.set_pc.call(&mut self.store, addr as i32)?;
        Ok(())
    }

    /// Write `bytes` into the chip's internal memory at `addr`. The bytes ride
    /// the mailbox channel (staged, then pulled by the guest inside the call).
    pub fn write_ram(&mut self, addr: u32, bytes: &[u8]) -> Result<(), LoadError> {
        self.store.data_mut().mailbox = bytes.to_vec();
        self.write_ram.call(&mut self.store, addr as i32)?;
        Ok(())
    }

    /// Read `len` bytes of the chip's internal memory at `addr` (the guest ships
    /// them over the emit channel; this decodes them). Short on a chip that
    /// exposes less.
    pub fn read_ram(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, LoadError> {
        self.store.data_mut().emitted = None;
        let n = self
            .read_ram
            .call(
                &mut self.store,
                (addr as i32, i32::try_from(len).unwrap_or(i32::MAX)),
            )?
            .max(0) as usize;
        let bytes = match self.store.data_mut().emitted.take() {
            Some((EMIT_KIND_RAM, buf)) => buf,
            _ => return Ok(Vec::new()),
        };
        Ok(bytes.into_iter().take(n).collect())
    }

    /// Snapshot the chip's full state to bytes (guest ships them over the emit
    /// channel). Pair with [`Self::load_state`].
    pub fn save_state(&mut self) -> Result<Vec<u8>, LoadError> {
        self.store.data_mut().emitted = None;
        self.save_state.call(&mut self.store, ())?;
        Ok(match self.store.data_mut().emitted.take() {
            Some((EMIT_KIND_STATE, buf)) => buf,
            _ => Vec::new(),
        })
    }

    /// Restore chip state from bytes produced by [`Self::save_state`] (staged in
    /// the mailbox, pulled by the guest inside the call).
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), LoadError> {
        self.store.data_mut().mailbox = bytes.to_vec();
        self.load_state.call(&mut self.store, ())?;
        Ok(())
    }
}
