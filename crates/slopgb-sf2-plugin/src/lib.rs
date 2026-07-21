//! Coprocessor plugin that converts a host-supplied SF2 soundfont into the
//! N-SPC sample-bank cache format (`slopgb-sf2`'s `.smpl`), inside a wasm
//! sandbox — not a clocked chip: the host hands over the raw SF2 bytes with
//! `set_file(SF2_FILE_KEY, ...)`, then drives one `run_until` to trigger the
//! (memoized) conversion, then reads the `.smpl` payload back with
//! `save_state`.

#![deny(unsafe_code)]

use slopgb_plugin_api::{Coprocessor, read_file, slopgb_coprocessor_plugin};

/// Host-file key the frontend uses in `set_file` to hand this converter the raw
/// SF2 bytes; the guest reads them back with `read_file`. There is only ever one
/// file, so a single fixed key suffices (mirror this in the frontend driver).
pub const SF2_FILE_KEY: u32 = 0;

/// Chunk size for pulling the (multi-MB) SF2 file across the host boundary.
const CHUNK: usize = 64 * 1024;

/// Convert raw SF2 bytes to the `.smpl` payload, or `None` on a parse failure
/// (an empty result signals failure to the host). Factored out of
/// [`Sf2Cop::run_until`] so the conversion logic is testable natively, off the
/// wasm-only `read_file` boundary.
fn convert(sf2: &[u8]) -> Option<Vec<u8>> {
    let regions = slopgb_sf2::import_sf2(sf2).ok()?;
    Some(slopgb_sf2::cache::serialize(&regions))
}

/// Read the entire host file at `key` by chunked [`read_file`] calls, advancing
/// the offset until a short (or zero) read marks the end.
fn read_whole_file(key: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut offset = 0u32;
    loop {
        let mut chunk = vec![0u8; CHUNK];
        let n = read_file(key, offset, &mut chunk);
        bytes.extend_from_slice(&chunk[..n]);
        if n < CHUNK {
            break;
        }
        offset += n as u32;
    }
    bytes
}

/// The SF2-to-`.smpl` converter, memoizing its one-shot conversion result.
struct Sf2Cop {
    /// The converted `.smpl` bytes, computed once on the first `run_until`
    /// after a `set_file`/reset. `None` before conversion or on a failed parse.
    payload: Option<Vec<u8>>,
}

impl Coprocessor for Sf2Cop {
    fn new() -> Self {
        Sf2Cop { payload: None }
    }

    /// The host-side `set_file` bytes persist across a reset; clearing the
    /// memoized payload just forces a re-convert on the next `run_until`.
    fn reset(&mut self) {
        self.payload = None;
    }

    /// Converter, not a clocked chip: no cycle domain of its own, so this just
    /// triggers the (memoized) conversion and echoes `target_cycle` back so the
    /// host's `reached >= target` check is satisfied trivially.
    fn run_until(&mut self, target_cycle: u64) -> u64 {
        if self.payload.is_none() {
            let bytes = read_whole_file(SF2_FILE_KEY);
            if !bytes.is_empty() {
                self.payload = convert(&bytes);
            }
        }
        target_cycle
    }

    fn save_state(&self) -> Vec<u8> {
        self.payload.clone().unwrap_or_default()
    }

    /// A converter has no comm ports; required by the trait, always inert.
    fn port_write(&mut self, _port: u8, _val: u8) {}

    /// A converter has no comm ports; required by the trait, always inert.
    fn port_read(&mut self, _port: u8) -> u8 {
        0
    }
}

slopgb_coprocessor_plugin!(Sf2Cop);

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
