//! Assemble a `.spc` (SPC700 Sound File, v0.30) snapshot from a live SPC700 +
//! S-DSP — the 256-byte header (with the CPU registers), the 64 KB APU RAM, and
//! the 128-byte DSP register file. A snapshot taken while a song is playing is
//! self-sustaining: an SPC player resumes from the saved PC with the loaded song
//! and keyed voices. (Format: nocash *fullsnes* "SPC File Format".)

use crate::dsp::SDsp;
use crate::spc700::Spc700;

/// Total `.spc` size: 256 header + 64 KiB ARAM + 128 DSP regs + 64 unused + 64
/// extra-RAM = 66048.
pub const SPC_FILE_LEN: usize = 0x1_0200;

const MAGIC: &[u8; 33] = b"SNES-SPC700 Sound File Data v0.30";

/// Build a complete `.spc` file from the current chip state.
pub fn build_spc_file(spc: &Spc700, dsp: &SDsp) -> Vec<u8> {
    let mut f = vec![0u8; SPC_FILE_LEN];
    // Header ($00-$FF).
    f[..33].copy_from_slice(MAGIC);
    f[0x21] = 0x1A;
    f[0x22] = 0x1A;
    f[0x23] = 0x1A; // header contains ID666 info
    f[0x24] = 30; // minor version
    // CPU registers.
    f[0x25] = spc.pc as u8;
    f[0x26] = (spc.pc >> 8) as u8;
    f[0x27] = spc.a;
    f[0x28] = spc.x;
    f[0x29] = spc.y;
    f[0x2A] = spc.psw.to_byte();
    f[0x2B] = spc.sp;
    // Minimal ID666: dumper name ($6E, 16 bytes) + emulator = "unknown" ($D2).
    let dumper = b"slopgb";
    f[0x6E..0x6E + dumper.len()].copy_from_slice(dumper);
    f[0xD2] = 0;
    // 64 KB APU RAM at $100.
    f[0x100..0x100 + 0x1_0000].copy_from_slice(spc.apu_ram());
    // The $F0-$FF I/O registers live in struct fields, not APU RAM, so the copy
    // above left them zero. Overwrite with the real control/timer-target state —
    // a timer-paced driver stays frozen (silent) if $F1/$FA-$FC come back 0.
    f[0x100 + 0xF0..0x100 + 0x100].copy_from_slice(&spc.io_snapshot());
    // 128 DSP registers at $10100 (raw file, not the live-computed read()).
    f[0x1_0100..0x1_0100 + 128].copy_from_slice(dsp.regs());
    // Extra RAM at $101C0: the 64 bytes shadowed by the IPL ROM region
    // ($FFC0-$FFFF). We don't page the IPL ROM in, so this mirrors ARAM.
    f[0x1_01C0..0x1_0200].copy_from_slice(&spc.apu_ram()[0xFFC0..]);
    f
}

#[cfg(test)]
#[path = "spc_file_tests.rs"]
mod tests;
