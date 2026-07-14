//! Subsystem coprocessor plugins (tier 3): a whole chip a plugin hosts — e.g.
//! the SGB SPC700 or the 65C816 — running its own RAM inside the sandbox and
//! driven by the host through a clock + comm-port interface. The coprocessor's
//! internal bus never crosses the boundary; only the comm ports (and, for audio
//! chips, drained PCM) do, so a 1 MHz chip costs a handful of crossings per
//! frame, not one per memory access.

/// The [`crate::__emit`] `kind` a coprocessor uses to push drained PCM: a flat
/// buffer of interleaved little-endian `i16` `L,R` sample pairs.
pub const EMIT_KIND_PCM: i32 = 2;

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
    };
}
