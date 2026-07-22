//! Resident N-SPC install: two independent, composable axes.
//!
//! **Engine source** ([`Engine`]) — the ROM's own resident engine code
//! (`Engine::Rom`, uploaded verbatim to `$0400`) or the clean-room engine
//! ([`NSPC_ENGINE`], `Engine::CleanRoom`, uploaded over `$0400` in its place).
//!
//! **Sample source** (`Option<&SampleRegions>`) — the ROM's own sample bank
//! (`None`, the three regions come from the ROM's APU block table) or an
//! SF2-derived bank (`Some`, its three regions override the ROM's
//! `$4B00`/`$4C30`/`$4DB0`).
//!
//! The two axes compose freely: an SF2 bank can ride either engine, and either
//! engine can ride either sample source. [`SgbCoprocessor::install_sgb_bios`]
//! is the existing `--sgb-bios`-alone entry point, now a thin wrapper over
//! [`SgbCoprocessor::install_nspc`].

use super::*;

/// A sample bank derived from an SF2 soundfont, in the SPC700 APU-RAM layout
/// the N-SPC engines expect: sample directory, instrument table, BRR
/// waveform data. Overrides the ROM's own `$4B00`/`$4C30`/`$4DB0` blocks when
/// passed to [`SgbCoprocessor::install_nspc`].
pub struct SampleRegions {
    pub dir: Vec<u8>,
    pub instr: Vec<u8>,
    pub brr: Vec<u8>,
}

/// Which N-SPC engine code runs resident at `$0400`.
#[derive(Clone, Copy)]
pub enum Engine {
    /// The SGB system ROM's own engine (requires `rom`; authentic playback).
    Rom,
    /// The original clean-room engine ([`NSPC_ENGINE`]).
    CleanRoom,
}

/// Fixed APU dests of the resident sound DATA blocks in the SGB system ROM's
/// upload table (see [`SgbCoprocessor::install_nspc`]). `$4C10` (quant/velocity)
/// is an ENGINE table, not a sample region, and is not listed here — it always
/// uploads from the ROM when present.
const SAMPLE_DIR_DEST: u16 = 0x4B00;
const SAMPLE_INSTR_DEST: u16 = 0x4C30;
const SAMPLE_BRR_DEST: u16 = 0x4DB0;

/// Offset of the SGB system ROM's SPC700 APU upload table (LoROM `$06:8000`).
/// ponytail: fixed offset for the known SGB1/SGB2 dump; a different revision
/// would need the boot loader's own source pointer (`$00:AC43`).
const TABLE_OFF: usize = 0x3_0000;

impl SgbCoprocessor {
    /// Install SGB resident music playback from a user-supplied SGB system ROM
    /// (`--sgb-bios`), so games that ship only song data (Animaniacs et al.) play.
    /// The SGB stores its resident SPC700 program — engine, sample directory, and
    /// BRR soundfont — as a standard SNES APU block table (`[u16 len, u16 dest,
    /// len bytes]*` then `[0000, entry]`) at LoROM $06:8000. Returns whether a
    /// valid table was found (else the clean-room firmware stays).
    ///
    /// **Default: the ROM's own resident engine** — the authentic, accurate
    /// playback (upload every block, enter its entry point). Set
    /// `SLOPGB_NSPC_CLEANROOM` to instead run the original [`NSPC_ENGINE`]
    /// (uploaded over $0400, reading the ROM's sample data) — the upstreamable
    /// clean-room path, still being refined (see `nspc/README.md`).
    ///
    /// A thin wrapper over [`Self::install_nspc`]: the ROM supplies both the
    /// engine (unless the env var swaps in the clean-room one) and every
    /// sample region (`sf2 = None`).
    pub fn install_sgb_bios(&mut self, program_rom: &[u8]) -> bool {
        let engine = if std::env::var_os("SLOPGB_NSPC_CLEANROOM").is_some() {
            Engine::CleanRoom
        } else {
            Engine::Rom
        };
        self.install_nspc(Some(program_rom), engine, None)
    }

    /// Install the resident N-SPC engine + sample bank. `rom` (the SGB system
    /// ROM) is required for `Engine::Rom` and for ROM samples (`sf2 == None`).
    /// When `sf2` is `Some`, its three regions OVERRIDE the ROM's
    /// `$4B00`/`$4C30`/`$4DB0`. Returns false if a required ROM is
    /// missing/unparseable or an upload fails.
    ///
    /// `Engine::CleanRoom` with `sf2 == Some` needs no ROM at all — the engine
    /// and every sample region both come from elsewhere.
    pub fn install_nspc(
        &mut self,
        rom: Option<&[u8]>,
        engine: Engine,
        sf2: Option<&SampleRegions>,
    ) -> bool {
        let rom_required = matches!(engine, Engine::Rom) || sf2.is_none();
        let parsed = rom.and_then(parse_sgb_apu_blocks);
        let (entry, blocks) = match parsed {
            Some(p) => p,
            None if rom_required => return false,
            None => (0, Vec::new()),
        };
        let spc = self.spc.get_mut();
        for (dest, data) in &blocks {
            // The clean-room engine replaces the ROM's engine CODE at $0400
            // with its own; an SF2 bank replaces the ROM's three sample-data
            // blocks. $4C10 (quant/velocity, an ENGINE table) always uploads
            // when present — the clean-room engine bakes its own copy and
            // never reads it, so a stale upload is harmless.
            let is_engine = *dest == SPC_PROG_ORG;
            let is_sample = matches!(*dest, SAMPLE_DIR_DEST | SAMPLE_INSTR_DEST | SAMPLE_BRR_DEST);
            let skip =
                (matches!(engine, Engine::CleanRoom) && is_engine) || (sf2.is_some() && is_sample);
            if !skip && spc.write_ram(u32::from(*dest), data).is_err() {
                return false;
            }
        }
        if let Some(regions) = sf2 {
            if spc
                .write_ram(u32::from(SAMPLE_DIR_DEST), &regions.dir)
                .is_err()
                || spc
                    .write_ram(u32::from(SAMPLE_INSTR_DEST), &regions.instr)
                    .is_err()
                || spc
                    .write_ram(u32::from(SAMPLE_BRR_DEST), &regions.brr)
                    .is_err()
            {
                return false;
            }
        }
        let pc = match engine {
            Engine::Rom => entry,
            Engine::CleanRoom => {
                if spc.write_ram(u32::from(SPC_PROG_ORG), NSPC_ENGINE).is_err() {
                    return false;
                }
                SPC_PROG_ORG
            }
        };
        if spc.set_pc(u32::from(pc)).is_err() {
            return false;
        }
        self.nspc_resident = true;
        true
    }
}

/// Parse the SGB system ROM's SPC700 APU upload table, returning `(entry_pc,
/// blocks)`. The fixed [`TABLE_OFF`] wrapper around the private
/// [`parse_apu_blocks`], exposed for the xtask SF2 exporter (which needs the
/// ROM's own block layout without installing anything).
pub fn parse_sgb_apu_blocks(rom: &[u8]) -> Option<(u16, ApuBlocks)> {
    parse_apu_blocks(rom, TABLE_OFF)
}

/// A parsed SNES APU upload table: each `(dest, bytes)` block, in order.
type ApuBlocks = Vec<(u16, Vec<u8>)>;

#[cfg(test)]
#[path = "samples_tests.rs"]
mod tests;
