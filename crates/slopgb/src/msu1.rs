//! MSU-1 streaming-audio coprocessor wired into a running Game Boy.
//!
//! MSU-1 is an open homebrew add-on (near/byuu): a memory-mapped register file
//! that streams a CD-quality `.pcm` track and exposes a `.msu` data ROM. slopgb
//! hosts the chip as a tier-3 wasm coprocessor plugin (`slopgb-msu1-plugin`,
//! driven through [`LoadedCoprocessor`]); this module is the frontend side that
//! makes a user-supplied pack (`--msu1 <dir>`) actually play:
//!
//! - **Register mapping.** On the SNES MSU-1 lives at `$2000-$2007`. slopgb is a
//!   Game Boy, so the eight registers are adapted to the Game Boy cartridge I/O
//!   window at **`$A000-$A007`** — the external-cartridge address space where MBC
//!   RAM / RTC registers already live, and where real MSU-1 hardware's SRAM sits.
//!   [`Msu1::write_reg`] / [`Msu1::read_reg`] map `$A000+n` ↔ plugin comm port `n`.
//! - **Live play.** Each rendered frame [`Msu1::pump_frame`] polls the cartridge
//!   register window (golden-safe `&self` [`GameBoy::debug_read`]) and edge-
//!   forwards the audio-control registers (track select / volume / control) to
//!   the chip, advances the plugin one frame of 44.1 kHz samples, drains its PCM,
//!   and resamples it to the core output rate for the audio pipe to mix in.
//! - **Golden-safe.** The whole peripheral is frontend-owned and only exists when
//!   a pack is loaded ([`crate::App`]'s `msu1: Option<Msu1>`). With no pack the
//!   core is untouched and the audio path is byte-identical — there is no core
//!   memory hook at all (a wasm store can't be cloned into the machine's
//!   save-state, so MSU-1 stays outside the emulated machine, like the audio
//!   pipe).
//!
//! The seek / data-ROM read ports (`$A000-$A003`) need a per-access live read
//! intercept in core, which the golden-safe design does not add; the frame poll
//! drives the audio-control registers, which is what makes a track play.

use std::fs;
use std::path::Path;

use slopgb_core::{CLOCK_HZ, CYCLES_PER_FRAME, DEFAULT_SAMPLE_RATE, GameBoy};
use slopgb_plugin_host::LoadedCoprocessor;

use crate::audio::Resampler;

/// Base Game Boy cartridge-RAM address of the MSU-1 register file
/// (`$A000-$A007`, the SNES `$2000-$2007` mapping adapted to the GB cart window).
pub(crate) const REG_BASE: u16 = 0xA000;
/// Number of MSU-1 comm registers (SNES `$2000-$2007`).
const NUM_REGS: u8 = 8;
/// The plugin's data-ROM host-file key — mirrors `slopgb_msu1_plugin::
/// DATA_FILE_KEY` (a reserved 32-bit key a 16-bit track number can't collide
/// with). Hardcoded to avoid pulling the plugin crate into the frontend deps.
const DATA_FILE_KEY: u32 = 0xFFFF_FFFF;
/// MSU-1 `.pcm` tracks stream at CD rate.
const PCM_RATE: u32 = 44_100;
/// Filename of the MSU-1 coprocessor plugin `.wasm` inside a pack directory.
const PLUGIN_WASM: &str = "msu1.wasm";
/// i16 full-scale → f32 at half amplitude, matching the SGB coprocessor's mix
/// level so MSU-1 music sits alongside the Game Boy APU without clipping.
const MIX_SCALE: f32 = 0.5 / 32768.0;
/// 44.1 kHz output samples owed per Game Boy frame — a fractional accumulator
/// advances the plugin by this each frame so the track stays locked to GB time.
const SAMPLES_PER_FRAME: f64 = PCM_RATE as f64 * CYCLES_PER_FRAME as f64 / CLOCK_HZ as f64;

/// MSU-1 register indices (== plugin comm ports; `$A000+n`).
const REG_TRACK_LO: u8 = 4;
const REG_TRACK_HI: u8 = 5;
const REG_VOLUME: u8 = 6;
const REG_CONTROL: u8 = 7;

/// A loaded MSU-1 pack driving the coprocessor plugin.
pub(crate) struct Msu1 {
    cop: LoadedCoprocessor,
    /// 44.1 kHz `.pcm` → core output rate (the pipe then resamples to the device).
    resampler: Resampler,
    /// 44.1 kHz output-sample cursor handed to the plugin's `run_until`.
    cycle: u64,
    /// Fractional 44.1 kHz samples owed but not yet advanced (frame-locked).
    frac: f64,
    /// Last register byte forwarded to each port, for edge detection. Seeded to
    /// the plugin's power-on register values.
    reg_shadow: [u8; NUM_REGS as usize],
    /// Scratch: i16 plugin PCM scaled to f32 (fed to the resampler).
    scaled: Vec<(f32, f32)>,
    /// Output: PCM resampled to the core output rate, mixed by the audio pipe.
    pending: Vec<(f32, f32)>,
}

impl Msu1 {
    /// Load an MSU-1 pack from `dir`: the coprocessor plugin (`dir/msu1.wasm`),
    /// every `*.pcm` audio track (keyed by its trailing track number), and an
    /// optional `*.msu` data ROM. Errors (missing plugin, bad wasm) are returned
    /// so the caller can log them and run without MSU-1.
    pub(crate) fn load(dir: &Path) -> Result<Self, String> {
        let wasm_path = dir.join(PLUGIN_WASM);
        let bytes = fs::read(&wasm_path)
            .map_err(|e| format!("cannot read MSU-1 plugin '{}': {e}", wasm_path.display()))?;
        let mut cop = LoadedCoprocessor::load(&bytes)
            .map_err(|e| format!("cannot load MSU-1 plugin: {e}"))?;
        cop.reset()
            .map_err(|e| format!("MSU-1 plugin reset failed: {e}"))?;

        let entries = fs::read_dir(dir)
            .map_err(|e| format!("cannot read MSU-1 pack '{}': {e}", dir.display()))?;
        let mut tracks = 0usize;
        // ponytail: every track's full bytes are read into host memory up front
        // (the plugin `set_file` model). Fine for a handful of tracks; stream a
        // track's file on demand if a large pack's memory ever matters.
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(track) = track_number(&name) {
                if let Ok(data) = fs::read(&path) {
                    cop.set_file(u32::from(track), data);
                    tracks += 1;
                }
            } else if name.ends_with(".msu") {
                if let Ok(data) = fs::read(&path) {
                    cop.set_file(DATA_FILE_KEY, data);
                }
            }
        }
        eprintln!(
            "slopgb: MSU-1 pack '{}' loaded ({tracks} track(s)); registers mapped at ${REG_BASE:04X}-${:04X}",
            dir.display(),
            REG_BASE + u16::from(NUM_REGS) - 1,
        );
        Ok(Self {
            cop,
            resampler: Resampler::new(PCM_RATE, DEFAULT_SAMPLE_RATE),
            cycle: 0,
            frac: 0.0,
            // Plugin power-on: seek/track 0, volume $FF, control 0. Seeding the
            // shadow here means a game that pre-set a control register before the
            // first poll still forwards it, while a no-RAM cart's constant $FF
            // only forwards a (missing) track number → the chip stays silent.
            reg_shadow: [0, 0, 0, 0, 0, 0, 0xFF, 0],
            scaled: Vec::new(),
            pending: Vec::new(),
        })
    }

    /// Write MSU-1 register `reg` (`$A000+reg`) → plugin comm port `reg`. The raw
    /// register↔port map; the live frame poll ([`Self::poll_registers`]) drives
    /// the write side from cart RAM, so this direct form is exercised only by the
    /// tests (and is where a future per-access core read/write intercept lands).
    #[cfg(test)]
    pub(crate) fn write_reg(&mut self, reg: u8, val: u8) {
        if reg < NUM_REGS {
            let _ = self.cop.port_write(reg, val);
        }
    }

    /// Read MSU-1 register `reg` (`$A000+reg`) ← plugin comm port `reg`
    /// (status / data-ROM byte / id). `$FF` for an out-of-range register or a
    /// plugin trap. Live register *reads* need a per-access core intercept (a
    /// running game reads these from its CPU, which the frontend cannot observe
    /// golden-safe), so this is currently the tested demonstration of the read
    /// direction, not a live path.
    #[cfg(test)]
    pub(crate) fn read_reg(&mut self, reg: u8) -> u8 {
        if reg < NUM_REGS {
            self.cop.port_read(reg).unwrap_or(0xFF)
        } else {
            0xFF
        }
    }

    /// Poll the cartridge register window and edge-forward the audio-control
    /// registers to the chip. Golden-safe: a read-only `&self` peek of GB memory
    /// (`debug_read`), no cycle advanced. Only changed registers are forwarded, so
    /// a game that writes a track + play once and leaves them set does not
    /// re-trigger every frame.
    pub(crate) fn poll_registers(&mut self, gb: &GameBoy) {
        let read = |reg: u8| gb.debug_read(REG_BASE + u16::from(reg));
        // Track select: writing `$A005` (hi) is the commit in the register
        // protocol, so re-commit whenever *either* select byte changed — a
        // low-byte-only change with a stable (often 0) high byte must still
        // re-select the track, which a per-byte value edge would miss.
        let (lo, hi) = (read(REG_TRACK_LO), read(REG_TRACK_HI));
        if lo != self.reg_shadow[usize::from(REG_TRACK_LO)]
            || hi != self.reg_shadow[usize::from(REG_TRACK_HI)]
        {
            self.reg_shadow[usize::from(REG_TRACK_LO)] = lo;
            self.reg_shadow[usize::from(REG_TRACK_HI)] = hi;
            let _ = self.cop.port_write(REG_TRACK_LO, lo);
            let _ = self.cop.port_write(REG_TRACK_HI, hi); // commits select_track
        }
        // Volume + control forward on a value change; control's play/stop is
        // edge-triggered, so writing it once and leaving it set does not restart.
        for reg in [REG_VOLUME, REG_CONTROL] {
            let v = read(reg);
            let slot = &mut self.reg_shadow[usize::from(reg)];
            if v != *slot {
                *slot = v;
                let _ = self.cop.port_write(reg, v);
            }
        }
    }

    /// Advance one Game Boy frame: poll the registers, step the plugin by a
    /// frame's worth of 44.1 kHz samples, drain its PCM, and resample it to the
    /// core output rate. Returns the core-rate samples for the audio pipe to mix
    /// into the Game Boy stream (empty while no track plays).
    pub(crate) fn pump_frame(&mut self, gb: &GameBoy) -> &[(f32, f32)] {
        self.poll_registers(gb);
        self.frac += SAMPLES_PER_FRAME;
        let advance = self.frac.floor();
        self.frac -= advance;
        self.cycle = self.cycle.saturating_add(advance as u64);
        let _ = self.cop.run_until(self.cycle);
        let raw = self.cop.drain_pcm().unwrap_or_default();
        self.scaled.clear();
        self.scaled.extend(
            raw.iter()
                .map(|&(l, r)| (f32::from(l) * MIX_SCALE, f32::from(r) * MIX_SCALE)),
        );
        self.pending.clear();
        self.resampler.run(&self.scaled, &mut self.pending);
        &self.pending
    }
}

/// The track number a `.pcm` filename ends with (`track_1.pcm` / `game-3.pcm` /
/// `5.pcm` → `1` / `3` / `5`), or `None` for a non-`.pcm` / un-numbered name.
/// The number is the plugin host-file key the game selects via `$A004`/`$A005`.
fn track_number(name: &str) -> Option<u16> {
    let stem = name.strip_suffix(".pcm")?;
    // Trailing digits are ASCII (1 byte each), so the char count is the byte
    // length of the run — slice it straight off the end.
    let run = stem.chars().rev().take_while(char::is_ascii_digit).count();
    stem.get(stem.len() - run..).and_then(|d| d.parse().ok())
}

#[cfg(test)]
#[path = "msu1_tests.rs"]
mod tests;
