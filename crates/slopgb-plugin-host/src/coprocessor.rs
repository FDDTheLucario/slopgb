//! Coprocessor plugins (tier 3): a whole chip a plugin hosts (the SGB SPC700 or
//! 65C816), driven by the host through reset / clock / comm-port calls. The
//! chip's internal RAM stays inside the sandbox; only the comm ports cross.

use slopgb_plugin_api::{
    ABI_VERSION, Capabilities, EMIT_KIND_MANIFEST, EMIT_KIND_PCM, EMIT_KIND_RAM, EMIT_KIND_SPC,
    EMIT_KIND_STATE,
};
use wasmi::{Engine, Instance, Module, Store, TypedFunc};

use crate::host::{HostState, build_linker};
use crate::{LoadError, Manifest};

/// One instantiated coprocessor plugin and the entry points the host drives it
/// with.
pub struct LoadedCoprocessor {
    store: Store<HostState>,
    /// Kept so [`Self::call_export`] can resolve a guest export by name on
    /// demand, unlike the typed funcs below, which are all resolved at load.
    instance: Instance,
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
    dump_spc: TypedFunc<(), i32>,
    /// Optional (v6): a chip that predates the manifest export, or a hand-rolled
    /// module without one, simply reports no manifest.
    manifest: Option<TypedFunc<(), i32>>,
}

impl LoadedCoprocessor {
    /// Instantiate a coprocessor plugin, enforcing the ABI + capability gate
    /// (tier 3 requires the `SUBSYSTEM` capability).
    pub fn load(bytes: &[u8]) -> Result<Self, LoadError> {
        // Plain (unmetered) engine, unlike the tier-1/tier-2 loaders: coprocessor
        // modules are first-party staged wasm (`cargo xtask stage-plugins`) driven
        // on the host-clocked >=66fps audio path, where per-instruction fuel
        // metering isn't worth the cost. The host bounds each `run_until` by a
        // cycle target, so a well-formed chip can't run unbounded here.
        // ponytail: if third-party subsystem plugins ever become loadable, switch
        // to `metered_engine()` + a per-`run_until` fuel budget (bench first).
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
        let dump_spc = instance
            .get_typed_func::<(), i32>(&store, "slopgb_dump_spc")
            .map_err(|_| LoadError::MissingExport("slopgb_dump_spc"))?;
        // Optional: manifest is metadata, so its absence never fails a load.
        let manifest = instance
            .get_typed_func::<(), i32>(&store, "slopgb_manifest")
            .ok();

        Ok(Self {
            store,
            instance,
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
            dump_spc,
            manifest,
        })
    }

    /// Read the coprocessor's self-describing [`Manifest`] (v6). `None` if the
    /// module exports none, declares an empty one, or emits a malformed blob —
    /// all of which mean "undeclared", so the caller falls back to its own
    /// wiring (e.g. filename convention).
    pub fn manifest(&mut self) -> Option<Manifest> {
        let func = self.manifest?;
        self.store.data_mut().emitted = None;
        func.call(&mut self.store, ()).ok()?;
        match self.store.data_mut().emitted.take() {
            Some((EMIT_KIND_MANIFEST, buf)) => Manifest::parse(&buf),
            _ => None,
        }
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
        let mut bytes = match self.store.data_mut().emitted.take() {
            Some((EMIT_KIND_RAM, buf)) => buf,
            _ => return Ok(Vec::new()),
        };
        bytes.truncate(n);
        Ok(bytes)
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

    /// Ask the chip for a `.spc` snapshot (an audio chip assembles the SPC700
    /// file from its ARAM + registers + DSP; a non-audio chip returns empty).
    pub fn dump_spc(&mut self) -> Result<Vec<u8>, LoadError> {
        self.store.data_mut().emitted = None;
        self.dump_spc.call(&mut self.store, ())?;
        Ok(match self.store.data_mut().emitted.take() {
            Some((EMIT_KIND_SPC, buf)) => buf,
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

    /// Call the guest export `slopgb_<name>` and take the blob it emitted.
    /// Unlike the typed funcs above (resolved once at load), this resolves the
    /// export on demand, so a manifest-declared menu row can dispatch to any
    /// `() -> i32` export the guest chooses to add without a new typed field
    /// here. Empty when the guest emitted nothing (whatever the emit kind).
    pub fn call_export(&mut self, name: &str) -> Result<Vec<u8>, LoadError> {
        let export = format!("slopgb_{name}");
        let func = self
            .instance
            .get_typed_func::<(), i32>(&self.store, &export)
            .map_err(|_| {
                LoadError::Wasm(wasmi::Error::new(format!(
                    "plugin missing export `{export}`"
                )))
            })?;
        self.store.data_mut().emitted = None;
        func.call(&mut self.store, ())?;
        Ok(match self.store.data_mut().emitted.take() {
            Some((_, buf)) => buf,
            None => Vec::new(),
        })
    }
}
