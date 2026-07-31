//! SoundFont-2 (SF2) importer/exporter + SNES BRR codec for the SGB N-SPC
//! music engine.
//!
//! Two directions:
//! - [`export_sf2`]: an N-SPC sample bank (dir + instrument table + BRR data,
//!   read out of a parsed APU RAM image) -> a standard, playable SF2 file.
//! - [`import_sf2`]: a standard SF2 file -> the three N-SPC memory regions
//!   (dir/instrument table/BRR), ready to be uploaded to the fixed APU
//!   destinations an N-SPC-compatible driver expects.
//!
//! Std-only, `forbid(unsafe_code)`, zero external dependencies: RIFF/SF2
//! parsing and writing are hand-rolled ([`reader`]/[`writer`]/[`riff`]), the
//! BRR codec is a self-contained port ([`brr`]), and [`cache`] holds a
//! compact on-disk form of the imported regions so re-running an import
//! doesn't require re-encoding.

#![forbid(unsafe_code)]

pub mod brr;
pub mod cache;
pub mod mapping;
pub mod reader;
pub mod resample;
mod riff;
pub mod writer;

pub use cache::{read_cache, write_cache};
pub use mapping::{BRR_DEST, DIR_DEST, INSTR_DEST, Regions, export_sf2, import_sf2};
