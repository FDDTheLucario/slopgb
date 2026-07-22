//! Subsystem coprocessor plugins (tier 3): a whole chip a plugin hosts — e.g.
//! the SGB SPC700 or the 65C816 — running its own RAM inside the sandbox and
//! driven by the host through a clock + comm-port interface. The coprocessor's
//! internal bus never crosses the boundary; only the comm ports (and, for audio
//! chips, drained PCM) do, so a 1 MHz chip costs a handful of crossings per
//! frame, not one per memory access.

/// The [`crate::__emit`] `kind` a coprocessor uses to push drained PCM: a flat
/// buffer of interleaved little-endian `i16` `L,R` sample pairs.
pub const EMIT_KIND_PCM: i32 = 2;
/// The [`crate::__emit`] `kind` `slopgb_save_state` uses to push the chip's
/// serialized state (opaque bytes) back to the host.
pub const EMIT_KIND_STATE: i32 = 3;
/// The [`crate::__emit`] `kind` `slopgb_read_ram` uses to push a read chunk of
/// the chip's internal memory back to the host.
pub const EMIT_KIND_RAM: i32 = 4;
/// The [`crate::__emit`] `kind` `slopgb_manifest` uses to push the coprocessor's
/// self-describing manifest (line-based UTF-8 text, see [`Coprocessor::MANIFEST`])
/// back to the host.
pub const EMIT_KIND_MANIFEST: i32 = 5;

/// Read the host→guest **mailbox** — the bytes a game (or the frontend) last
/// deposited for this coprocessor, e.g. a streaming-audio play-request written
/// through `DATA_SND`. A resident coprocessor polls this each `run_until` and
/// edge-detects a change. Empty when nothing is queued.
///
/// The host writes into a guest-owned buffer through wasmi's bounds-checked
/// `Memory` and reports the true length, so this grows + retries on overflow —
/// no `unsafe`, no raw pointer (the ABI's guest-scratch pattern).
#[must_use]
pub fn recv_mailbox() -> Vec<u8> {
    let mut buf = vec![0u8; 64];
    loop {
        let need = crate::abi::host_recv(buf.as_ptr() as i32, buf.len() as i32).max(0) as usize;
        if need <= buf.len() {
            buf.truncate(need);
            return buf;
        }
        buf.resize(need, 0);
    }
}

/// Read up to `buf.len()` bytes of the host-owned file identified by `key`,
/// starting at byte `offset`, into `buf`. Returns the number of bytes actually
/// written (`0` when `key` names no file or `offset` is at/past the end) — the
/// caller streams by advancing `offset` until it gets a short/zero read.
///
/// The host owns the file (a track `.pcm`, a data `.msu` — far larger than the
/// comm ports can carry); the plugin pulls it in chunks. The host writes into
/// `buf` through the bounds-checked `Memory`, so there is no `unsafe` and no raw
/// pointer. `key`'s meaning is a host↔plugin convention (e.g. MSU-1: the audio
/// track number, or a reserved key for the data ROM).
pub fn read_file(key: u32, offset: u32, buf: &mut [u8]) -> usize {
    let n = crate::abi::host_file(
        key as i32,
        offset as i32,
        buf.as_ptr() as i32,
        buf.len() as i32,
    );
    (n.max(0) as usize).min(buf.len())
}

/// A chip a plugin hosts. Implement this, then invoke
/// [`slopgb_coprocessor_plugin!`](crate::slopgb_coprocessor_plugin).
pub trait Coprocessor {
    /// Capabilities; subsystem hosting is the tier-3 gate.
    const CAPABILITIES: crate::Capabilities = crate::Capabilities::SUBSYSTEM;

    /// A self-describing manifest the host reads at load to bind this chip by
    /// declared identity/role rather than by filename. Line-based UTF-8, one
    /// record per line, TAB-separated, first field = record type; unknown record
    /// types are ignored so the schema can grow without an ABI break:
    ///
    /// ```text
    /// id\t<stable-token>            e.g. "msu1" — logical identity + role key
    /// name\t<display name>          human label for UI / logs
    /// provides\t<role>             (0..n) a capability slot this chip can fill
    /// flag\t<name>\t<arg>\t<help>  (0..n) a CLI flag this plugin contributes
    /// ```
    ///
    /// Default: empty — an undeclared coprocessor (the host reports no manifest).
    /// The generated `slopgb_manifest` export ships it over the emit channel as
    /// [`crate::EMIT_KIND_MANIFEST`].
    const MANIFEST: &'static str = "";

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

    /// Take the stereo PCM synthesized since the last drain, oldest first. The
    /// host pulls this each frame and mixes it (like the built-in `mix_into`).
    /// Default: none — a non-audio coprocessor (e.g. the 65C816 CPU) makes no
    /// PCM. The generated `slopgb_drain_pcm` export ships it over the emit
    /// channel as [`EMIT_KIND_PCM`] and returns the pair count.
    fn drain_pcm(&mut self) -> Vec<(i16, i16)> {
        Vec::new()
    }

    /// Redirect the chip's program counter to `addr` (its own address space —
    /// e.g. a 24-bit `bank<<16 | pc` for the 65C816, a 16-bit `pc` for the
    /// SPC700). Lets an orchestrating host install firmware or apply an SGB
    /// `JUMP`. Default: ignored (a chip with no host-settable PC).
    fn set_pc(&mut self, addr: u32) {
        let _ = addr;
    }

    /// Write `bytes` into the chip's internal memory starting at `addr`. Lets a
    /// host install resident firmware or deposit a data block (e.g. SGB
    /// `DATA_SND` / `SOU_TRN`) the sandboxed chip then runs. Default: ignored.
    fn write_ram(&mut self, addr: u32, bytes: &[u8]) {
        let _ = (addr, bytes);
    }

    /// Read `len` bytes of the chip's internal memory starting at `addr`.
    /// Observability (a debugger peek, or a host confirming a firmware/transfer
    /// landed); `&mut` because some chips (the SPC700) expose RAM only through a
    /// mutable handle. Default: zeros (a chip that exposes no RAM).
    fn read_ram(&mut self, addr: u32, len: usize) -> Vec<u8> {
        let _ = addr;
        vec![0u8; len]
    }

    /// Serialize the chip's full volatile state to bytes for a host save-state.
    /// The format is private to the chip; [`Self::load_state`] is its inverse.
    /// Default: empty (a stateless / non-persisted coprocessor).
    fn save_state(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Restore chip state previously produced by [`Self::save_state`]. A
    /// malformed / foreign buffer should leave the chip usable (best-effort).
    /// Default: ignored.
    fn load_state(&mut self, bytes: &[u8]) {
        let _ = bytes;
    }
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

        /// Ship the coprocessor's self-describing manifest to the host over the
        /// emit channel (kind [`EMIT_KIND_MANIFEST`]); returns the byte count.
        /// Empty for a chip that declares none.
        ///
        /// [`EMIT_KIND_MANIFEST`]: $crate::EMIT_KIND_MANIFEST
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_manifest() -> i32 {
            let m = <$ty as $crate::Coprocessor>::MANIFEST;
            $crate::__emit($crate::EMIT_KIND_MANIFEST, m.as_bytes());
            m.len() as i32
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

        /// Drain the coprocessor's stereo PCM to the host over the emit channel
        /// (interleaved LE `i16` L,R pairs, kind [`EMIT_KIND_PCM`]); returns the
        /// pair count. The bytes are the guest's own buffer, read synchronously
        /// by the host within this call.
        ///
        /// [`EMIT_KIND_PCM`]: $crate::EMIT_KIND_PCM
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_drain_pcm() -> i32 {
            __SLOPGB_COP.with_borrow_mut(|c| {
                let pcm = $crate::Coprocessor::drain_pcm(c);
                let mut bytes = ::std::vec::Vec::with_capacity(pcm.len() * 4);
                for (l, r) in &pcm {
                    bytes.extend_from_slice(&l.to_le_bytes());
                    bytes.extend_from_slice(&r.to_le_bytes());
                }
                $crate::__emit($crate::EMIT_KIND_PCM, &bytes);
                pcm.len() as i32
            })
        }

        /// Redirect the chip's program counter (host → guest scalar).
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_set_pc(addr: i32) {
            __SLOPGB_COP.with_borrow_mut(|c| $crate::Coprocessor::set_pc(c, addr as u32));
        }

        /// Write host-supplied bytes into the chip's memory at `addr`. The bytes
        /// arrive through the mailbox channel (the host sets it, the guest pulls
        /// it here), so no raw pointer crosses.
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_write_ram(addr: i32) {
            let bytes = $crate::recv_mailbox();
            __SLOPGB_COP
                .with_borrow_mut(|c| $crate::Coprocessor::write_ram(c, addr as u32, &bytes));
        }

        /// Read `len` bytes of the chip's memory at `addr` back to the host over
        /// the emit channel ([`EMIT_KIND_RAM`]); returns the byte count.
        ///
        /// [`EMIT_KIND_RAM`]: $crate::EMIT_KIND_RAM
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_read_ram(addr: i32, len: i32) -> i32 {
            __SLOPGB_COP.with_borrow_mut(|c| {
                let bytes = $crate::Coprocessor::read_ram(c, addr as u32, len.max(0) as usize);
                $crate::__emit($crate::EMIT_KIND_RAM, &bytes);
                bytes.len() as i32
            })
        }

        /// Serialize the chip state to the host over the emit channel
        /// ([`EMIT_KIND_STATE`]); returns the byte count.
        ///
        /// [`EMIT_KIND_STATE`]: $crate::EMIT_KIND_STATE
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_save_state() -> i32 {
            __SLOPGB_COP.with_borrow(|c| {
                let bytes = $crate::Coprocessor::save_state(c);
                $crate::__emit($crate::EMIT_KIND_STATE, &bytes);
                bytes.len() as i32
            })
        }

        /// Restore chip state the host staged in the mailbox (host → guest bulk).
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn slopgb_load_state() {
            let bytes = $crate::recv_mailbox();
            __SLOPGB_COP.with_borrow_mut(|c| $crate::Coprocessor::load_state(c, &bytes));
        }
    };
}
