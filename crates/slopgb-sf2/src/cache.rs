//! `.smpl` cache: a compact on-disk serialization of [`Regions`] so a
//! previously-imported soundfont doesn't need to be re-resampled/re-BRR-
//! encoded on every run.
//!
//! Format (little-endian throughout):
//! ```text
//! magic   : 4 bytes, b"SMP1"
//! version : u8, currently 1
//! then, for each of the 3 regions in order dir, instr, brr:
//!   dest : u16  (the fixed APU destination, informational — always the
//!                matching DIR_DEST/INSTR_DEST/BRR_DEST constant)
//!   len  : u32  (byte length of this region)
//!   data : `len` bytes
//! ```

use crate::mapping::{BRR_DEST, DIR_DEST, INSTR_DEST, Regions};
use std::fs;
use std::path::Path;

const MAGIC: &[u8; 4] = b"SMP1";
const VERSION: u8 = 1;

fn write_region(out: &mut Vec<u8>, dest: u16, data: &[u8]) {
    out.extend_from_slice(&dest.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

/// Serialize `regions` to the `.smpl` byte format (magic + version + 3 regions).
pub fn serialize(regions: &Regions) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    write_region(&mut out, DIR_DEST, &regions.dir);
    write_region(&mut out, INSTR_DEST, &regions.instr);
    write_region(&mut out, BRR_DEST, &regions.brr);
    out
}

/// Serialize `regions` to `path` in the `.smpl` cache format.
pub fn write_cache(path: impl AsRef<Path>, regions: &Regions) -> Result<(), String> {
    let path = path.as_ref();
    fs::write(path, serialize(regions))
        .map_err(|e| format!("writing cache {}: {e}", path.display()))
}

fn read_region(buf: &[u8], pos: &mut usize) -> Result<Vec<u8>, String> {
    if *pos + 6 > buf.len() {
        return Err("cache truncated in region header".to_string());
    }
    let _dest = u16::from_le_bytes([buf[*pos], buf[*pos + 1]]);
    let len =
        u32::from_le_bytes([buf[*pos + 2], buf[*pos + 3], buf[*pos + 4], buf[*pos + 5]]) as usize;
    *pos += 6;
    if *pos + len > buf.len() {
        return Err("cache truncated in region data".to_string());
    }
    let data = buf[*pos..*pos + len].to_vec();
    *pos += len;
    Ok(data)
}

/// Deserialize `.smpl` bytes produced by [`serialize`].
pub fn deserialize(buf: &[u8]) -> Result<Regions, String> {
    if buf.len() < 5 || &buf[0..4] != MAGIC {
        return Err("not a SMP1 cache file".to_string());
    }
    if buf[4] != VERSION {
        return Err(format!("unsupported SMP1 cache version {}", buf[4]));
    }
    let mut pos = 5usize;
    let dir = read_region(buf, &mut pos)?;
    let instr = read_region(buf, &mut pos)?;
    let brr = read_region(buf, &mut pos)?;
    Ok(Regions { dir, instr, brr })
}

/// Deserialize a `.smpl` cache file written by [`write_cache`].
pub fn read_cache(path: impl AsRef<Path>) -> Result<Regions, String> {
    let path = path.as_ref();
    let buf = fs::read(path).map_err(|e| format!("reading cache {}: {e}", path.display()))?;
    deserialize(&buf)
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
