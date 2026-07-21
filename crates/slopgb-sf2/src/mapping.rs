//! N-SPC sample bank (dir + instrument table + BRR data) <-> SF2, both
//! directions.
//!
//! The N-SPC sample bank is three byte regions at fixed APU destinations:
//! - **dir** (`DIR_DEST = $4B00`): 64 entries x 4 bytes
//!   (`start_lo,start_hi,loop_lo,loop_hi`), absolute APU addresses.
//! - **instrument table** (`INSTR_DEST = $4C30`): 6 bytes/entry
//!   (`SRCN,ADSR1,ADSR2,GAIN,base16_hi,base16_lo` — base16 big-endian,
//!   default `$1000`).
//! - **BRR data** (`BRR_DEST = $4DB0`): the BRR-encoded samples themselves.
//!
//! See [`tuning`] for the `base16` <-> SF2 tuning convention and [`adsr`]
//! for the (lossy, documented) `ADSR1`/`ADSR2`/`GAIN` <-> volume-envelope
//! mapping.
//!
//! Not built (ceiling): multi-zone/velocity-split SF2 instruments (only the
//! first sample zone of each instrument is read/written), stereo samples
//! (mono only), 24-bit `sm24` samples, and SF2 modulators/LFOs (none of the
//! N-SPC fields have an equivalent).
//!
//! [`export_sf2`] dedups by the resolved `(start_addr, loop_addr)` BRR
//! waveform, not by SRCN: a densely-packed real bank's directory aliases
//! heavily (many SRCN slots pointing at the same waveform), and exporting
//! each alias as its own SF2 sample would re-encode the same BRR data once
//! per alias on re-import, blowing the fixed 64 KiB APU BRR budget. Aliased
//! SRCNs collapse to one shared SF2 sample referenced by every instrument
//! that used any of them.

mod adsr;
mod tuning;

use crate::reader::{self, Sf2Instrument, Sf2Sample};
use crate::{brr, resample, writer};
use std::collections::{HashMap, HashSet};

/// S-DSP sample directory destination in APU RAM.
pub const DIR_DEST: u16 = 0x4B00;
/// N-SPC instrument table destination in APU RAM.
pub const INSTR_DEST: u16 = 0x4C30;
/// BRR waveform data destination in APU RAM.
pub const BRR_DEST: u16 = 0x4DB0;
/// The instrument-table base-pitch default before a track's first
/// instrument-select command (`$E0`) — see `nspc/README.md`.
pub const DEFAULT_BASE16: u16 = 0x1000;

/// The three N-SPC memory regions, ready to be uploaded at their fixed
/// destinations ([`DIR_DEST`]/[`INSTR_DEST`]/[`BRR_DEST`]).
pub struct Regions {
    pub dir: Vec<u8>,
    pub instr: Vec<u8>,
    pub brr: Vec<u8>,
}

/// Export an N-SPC sample bank (read out of a parsed 64 KiB APU RAM image)
/// to a standard SF2 file.
///
/// `apu_ram` is the full APU address space (so the dir's absolute
/// start/loop pointers resolve directly — the caller assembles it from the
/// ROM's uploaded APU blocks). `dir_base`/`instr_base` are where the
/// directory and instrument table live in it; `n_dir` bounds valid SRCN
/// values (`0..n_dir`); `n_instr` is how many 6-byte instrument-table
/// entries (starting at `instr_base`) to read.
///
/// Only samples actually referenced by one of the `n_instr` instruments are
/// decoded/exported (not all `n_dir` directory slots).
pub fn export_sf2(
    apu_ram: &[u8; 0x1_0000],
    dir_base: u16,
    instr_base: u16,
    n_dir: usize,
    n_instr: usize,
) -> Result<Vec<u8>, String> {
    if n_instr == 0 {
        return Err("export_sf2: n_instr must be > 0".to_string());
    }
    let instr_base = instr_base as usize;
    if instr_base + n_instr * 6 > apu_ram.len() {
        return Err("export_sf2: instrument table runs off the end of APU RAM".to_string());
    }

    struct RawInstr {
        srcn: u8,
        adsr1: u8,
        adsr2: u8,
        gain: u8,
        base16: u16,
    }
    let raw_instrs: Vec<RawInstr> = (0..n_instr)
        .map(|i| {
            let e = instr_base + i * 6;
            RawInstr {
                srcn: apu_ram[e],
                adsr1: apu_ram[e + 1],
                adsr2: apu_ram[e + 2],
                gain: apu_ram[e + 3],
                base16: (u16::from(apu_ram[e + 4]) << 8) | u16::from(apu_ram[e + 5]),
            }
        })
        .collect();

    // Referenced SRCNs only, first-seen order.
    let mut srcn_order: Vec<u8> = Vec::new();
    let mut srcn_seen: HashSet<u8> = HashSet::new();
    for ri in &raw_instrs {
        if srcn_seen.insert(ri.srcn) {
            srcn_order.push(ri.srcn);
        }
    }

    // Directory entries alias heavily (many SRCNs share the same BRR
    // waveform), so dedup samples by resolved `(start_addr, loop_addr)`, not
    // by SRCN — otherwise aliased SRCNs re-encode the same waveform once per
    // alias on re-import, blowing the fixed 64 KiB BRR budget.
    let dir_base = dir_base as usize;
    let mut addr_to_sample: HashMap<(u16, u16), usize> = HashMap::new();
    let mut srcn_to_index: HashMap<u8, usize> = HashMap::new();
    let mut samples = Vec::new();
    for &srcn in &srcn_order {
        if srcn as usize >= n_dir {
            return Err(format!(
                "export_sf2: SRCN {srcn} is outside the {n_dir}-entry directory"
            ));
        }
        let e = dir_base + srcn as usize * 4;
        if e + 4 > apu_ram.len() {
            return Err("export_sf2: directory entry runs off the end of APU RAM".to_string());
        }
        let start_addr = u16::from_le_bytes([apu_ram[e], apu_ram[e + 1]]);
        let loop_addr = u16::from_le_bytes([apu_ram[e + 2], apu_ram[e + 3]]);
        let sample_index = match addr_to_sample.get(&(start_addr, loop_addr)) {
            Some(&idx) => idx,
            None => {
                let decoded = brr::decode(&apu_ram[..], start_addr as usize)?;
                // Loop point, in samples (16 PCM per 9-byte block): the block
                // index the loop address resolves to, times 16.
                let loop_sample = (decoded.loops && loop_addr >= start_addr)
                    .then(|| (((loop_addr - start_addr) as usize) / 9) * 16);
                let len = decoded.pcm.len() as u32;
                samples.push(Sf2Sample {
                    name: format!("srcn_{srcn:02X}"),
                    pcm: decoded.pcm,
                    loop_start: loop_sample.unwrap_or(0) as u32,
                    loop_end: if loop_sample.is_some() { len } else { 0 },
                    sample_rate: tuning::BRR_ENCODE_RATE,
                    original_pitch: tuning::SF2_ROOT_KEY,
                    pitch_correction: 0,
                });
                let idx = samples.len() - 1;
                addr_to_sample.insert((start_addr, loop_addr), idx);
                idx
            }
        };
        srcn_to_index.insert(srcn, sample_index);
    }

    let instruments: Vec<Sf2Instrument> = raw_instrs
        .iter()
        .enumerate()
        .map(|(i, ri)| {
            let sample_index = srcn_to_index[&ri.srcn];
            let (coarse_tune, fine_tune) = tuning::base16_to_coarse_fine(ri.base16);
            Sf2Instrument {
                name: format!("inst_{i:02X}"),
                sample_index,
                loops: samples[sample_index].loop_end > 0,
                root_key_override: None,
                coarse_tune,
                fine_tune,
                initial_attenuation_cb: adsr::gain_to_attenuation_cb(ri.gain),
                vol_env: adsr::adsr_to_vol_env(ri.adsr1, ri.adsr2),
                key_range: None,
            }
        })
        .collect();

    Ok(writer::write(&samples, &instruments))
}

/// Import a standard SF2 file into the three N-SPC memory [`Regions`].
pub fn import_sf2(sf2_bytes: &[u8]) -> Result<Regions, String> {
    let parsed = reader::parse(sf2_bytes)?;
    if parsed.samples.is_empty() {
        return Err("import_sf2: SF2 has no samples".to_string());
    }
    if parsed.samples.len() > 64 {
        return Err(format!(
            "import_sf2: {} samples exceeds the 64-entry N-SPC directory",
            parsed.samples.len()
        ));
    }
    for inst in &parsed.instruments {
        if inst.sample_index >= parsed.samples.len() {
            return Err(format!(
                "import_sf2: instrument '{}' references out-of-range sample {}",
                inst.name, inst.sample_index
            ));
        }
    }

    // A sample loops in the exported BRR if ANY instrument referencing it
    // does (the BRR loop flag is per-sample, not per-instrument).
    let mut sample_loops = vec![false; parsed.samples.len()];
    for inst in &parsed.instruments {
        sample_loops[inst.sample_index] |= inst.loops;
    }

    let mut dir = Vec::with_capacity(parsed.samples.len() * 4);
    let mut brr_region = Vec::new();
    for (i, sample) in parsed.samples.iter().enumerate() {
        let rate = sample.sample_rate.max(1);
        let resampled = if rate == tuning::BRR_ENCODE_RATE {
            sample.pcm.clone()
        } else {
            resample::resample(&sample.pcm, rate, tuning::BRR_ENCODE_RATE)
        };
        let scale = f64::from(tuning::BRR_ENCODE_RATE) / f64::from(rate);
        let loop_sample = sample_loops[i].then(|| {
            ((f64::from(sample.loop_start) * scale).round() as usize)
                .min(resampled.len().saturating_sub(1))
        });
        let encoded = brr::encode(&resampled, loop_sample);

        let start_addr = BRR_DEST
            .checked_add(brr_region.len() as u16)
            .filter(|&a| (a as usize) + encoded.bytes.len() <= 0x1_0000)
            .ok_or("import_sf2: BRR data overflowed the 64 KiB APU address space")?;
        let loop_addr = match encoded.loop_block {
            Some(b) => start_addr + (b * 9) as u16,
            None => start_addr,
        };
        dir.extend_from_slice(&start_addr.to_le_bytes());
        dir.extend_from_slice(&loop_addr.to_le_bytes());
        brr_region.extend_from_slice(&encoded.bytes);
    }

    let mut instr = Vec::with_capacity(parsed.instruments.len() * 6);
    for inst in &parsed.instruments {
        let sample = &parsed.samples[inst.sample_index];
        let (adsr1, adsr2) = adsr::vol_env_to_adsr(&inst.vol_env);
        let gain = adsr::attenuation_cb_to_gain(inst.initial_attenuation_cb);
        let base16 = tuning::to_base16(
            sample.sample_rate,
            sample.original_pitch,
            sample.pitch_correction,
            inst.root_key_override,
            inst.coarse_tune,
            inst.fine_tune,
        );
        instr.push(inst.sample_index as u8);
        instr.push(adsr1);
        instr.push(adsr2);
        instr.push(gain);
        instr.push((base16 >> 8) as u8);
        instr.push((base16 & 0xFF) as u8);
    }

    Ok(Regions {
        dir,
        instr,
        brr: brr_region,
    })
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod tests;
