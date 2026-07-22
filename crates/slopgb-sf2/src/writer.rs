//! Hand-rolled SF2 (`RIFF....sfbk`) writer.
//!
//! Emits a minimal but valid, playable standard SF2: `INFO` (`ifil`=2.01,
//! `isng`="EMU8000", `INAM`), `sdta`/`smpl` (all sample PCM concatenated,
//! each followed by the SF2-required 46 zero samples), and `pdta` with
//! `phdr`/`pbag`/`pgen`/`inst`/`ibag`/`igen`/`shdr` including the mandatory
//! terminal records. No private chunks; no modulators (only the required
//! empty-but-present `pmod`/`imod` terminal records).
//!
//! Each input sample becomes one SF2 sample; each input instrument becomes
//! one SF2 instrument plus one bank-0 preset (program numbers 0, 1, 2, ...)
//! with a single global-key zone pointing at that instrument. Generator IDs
//! are from the SoundFont 2.01 Technical Specification §8.1.2.

use crate::reader::{
    GEN_ATTACK_VOL_ENV, GEN_COARSE_TUNE, GEN_DECAY_VOL_ENV, GEN_DELAY_VOL_ENV, GEN_FINE_TUNE,
    GEN_HOLD_VOL_ENV, GEN_INITIAL_ATTENUATION, GEN_INSTRUMENT, GEN_OVERRIDING_ROOT_KEY,
    GEN_RELEASE_VOL_ENV, GEN_SAMPLE_ID, GEN_SAMPLE_MODES, GEN_SUSTAIN_VOL_ENV,
};
use crate::reader::{Sf2Instrument, Sf2Sample};
use crate::riff::{write_chunk, write_fixed_str, write_list};

fn gen_record(out: &mut Vec<u8>, id: u16, amount: i16) {
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&amount.to_le_bytes());
}

/// A NUL-terminated ASCII string padded to an EVEN total length using one or
/// two terminators (SF2 §5.2/5.3: "...one or two terminators of value zero,
/// so as to make the total byte count even" — the chunk's own *declared*
/// size must be even, not just outside-padded the generic RIFF way, or
/// fluidsynth rejects the file as "in violation of RIFF spec").
fn zstr_even(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    if v.len() % 2 != 0 {
        v.push(0);
    }
    v
}

fn build_info() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&write_chunk(b"ifil", &[2, 0, 1, 0])); // wMajor=2, wMinor=1
    body.extend_from_slice(&write_chunk(b"isng", &zstr_even("EMU8000")));
    body.extend_from_slice(&write_chunk(b"INAM", &zstr_even("slopgb-sf2")));
    write_list(b"INFO", &body)
}

/// A sample's span in the `smpl` block: (start, end, startloop, endloop), in
/// sample points.
type SampleSpan = (u32, u32, u32, u32);

fn build_sdta(samples: &[Sf2Sample]) -> (Vec<u8>, Vec<SampleSpan>) {
    let mut smpl = Vec::new();
    let mut spans = Vec::new(); // (start, end, startloop, endloop), in sample points
    for s in samples {
        let start = (smpl.len() / 2) as u32;
        for &v in &s.pcm {
            smpl.extend_from_slice(&v.to_le_bytes());
        }
        let end = (smpl.len() / 2) as u32;
        // The SF2 spec requires >= 46 zero-valued sample points after each sample.
        smpl.extend(std::iter::repeat_n(0u8, 46 * 2));
        spans.push((start, end, start + s.loop_start, start + s.loop_end));
    }
    (write_list(b"sdta", &write_chunk(b"smpl", &smpl)), spans)
}

fn build_shdr(samples: &[Sf2Sample], spans: &[SampleSpan]) -> Vec<u8> {
    let mut body = Vec::new();
    for (s, &(start, end, loopstart, loopend)) in samples.iter().zip(spans) {
        write_fixed_str(&mut body, &s.name, 20);
        body.extend_from_slice(&start.to_le_bytes());
        body.extend_from_slice(&end.to_le_bytes());
        body.extend_from_slice(&loopstart.to_le_bytes());
        body.extend_from_slice(&loopend.to_le_bytes());
        body.extend_from_slice(&s.sample_rate.to_le_bytes());
        body.push(s.original_pitch);
        body.push(s.pitch_correction as u8);
        body.extend_from_slice(&0u16.to_le_bytes()); // wSampleLink
        body.extend_from_slice(&1u16.to_le_bytes()); // sfSampleType = monoSample
    }
    // Terminal "EOS" record, all other fields zero.
    write_fixed_str(&mut body, "EOS", 20);
    body.extend_from_slice(&[0u8; 26]);
    write_chunk(b"shdr", &body)
}

/// One instrument's generator list (a single non-global zone), in the
/// required order (sampleModes/tuning/env first, `sampleID` always last).
fn instrument_generators(inst: &Sf2Instrument) -> Vec<u8> {
    let mut g = Vec::new();
    gen_record(&mut g, GEN_COARSE_TUNE, i16::from(inst.coarse_tune));
    gen_record(&mut g, GEN_FINE_TUNE, i16::from(inst.fine_tune));
    gen_record(&mut g, GEN_INITIAL_ATTENUATION, inst.initial_attenuation_cb);
    gen_record(&mut g, GEN_DELAY_VOL_ENV, inst.vol_env.delay_tc);
    gen_record(&mut g, GEN_ATTACK_VOL_ENV, inst.vol_env.attack_tc);
    gen_record(&mut g, GEN_HOLD_VOL_ENV, inst.vol_env.hold_tc);
    gen_record(&mut g, GEN_DECAY_VOL_ENV, inst.vol_env.decay_tc);
    gen_record(&mut g, GEN_SUSTAIN_VOL_ENV, inst.vol_env.sustain_cb);
    gen_record(&mut g, GEN_RELEASE_VOL_ENV, inst.vol_env.release_tc);
    gen_record(&mut g, GEN_SAMPLE_MODES, inst.loops as i16);
    if let Some(root) = inst.root_key_override {
        gen_record(&mut g, GEN_OVERRIDING_ROOT_KEY, i16::from(root));
    }
    gen_record(&mut g, GEN_SAMPLE_ID, inst.sample_index as i16);
    g
}

fn build_pdta(
    samples: &[Sf2Sample],
    instruments: &[Sf2Instrument],
    spans: &[(u32, u32, u32, u32)],
) -> Vec<u8> {
    // phdr: one preset per instrument (bank 0, program = index), each with a
    // single preset zone whose only generator is `instrument` (index-generator,
    // so it must be the zone's last/only generator).
    let mut phdr = Vec::new();
    for (i, inst) in instruments.iter().enumerate() {
        write_fixed_str(&mut phdr, &inst.name, 20);
        phdr.extend_from_slice(&(i as u16).to_le_bytes()); // wPreset
        phdr.extend_from_slice(&0u16.to_le_bytes()); // wBank
        phdr.extend_from_slice(&(i as u16).to_le_bytes()); // wPresetBagNdx
        phdr.extend_from_slice(&[0u8; 12]); // library/genre/morphology
    }
    write_fixed_str(&mut phdr, "EOP", 20);
    phdr.extend_from_slice(&0u16.to_le_bytes()); // wPreset
    phdr.extend_from_slice(&0u16.to_le_bytes()); // wBank
    phdr.extend_from_slice(&(instruments.len() as u16).to_le_bytes()); // wPresetBagNdx
    phdr.extend_from_slice(&[0u8; 12]); // library/genre/morphology

    let mut pbag = Vec::new();
    for i in 0..instruments.len() {
        pbag.extend_from_slice(&(i as u16).to_le_bytes()); // wGenNdx (1 gen/preset)
        pbag.extend_from_slice(&0u16.to_le_bytes());
    }
    pbag.extend_from_slice(&(instruments.len() as u16).to_le_bytes());
    pbag.extend_from_slice(&0u16.to_le_bytes());

    let mut pgen = Vec::new();
    for (i, _) in instruments.iter().enumerate() {
        pgen.extend_from_slice(&GEN_INSTRUMENT.to_le_bytes());
        pgen.extend_from_slice(&(i as u16).to_le_bytes());
    }
    pgen.extend_from_slice(&[0u8; 4]); // terminal

    let pmod = vec![0u8; 10]; // terminal record only: no preset modulators

    // inst: one instrument per input, each with a single instrument zone.
    let mut inst_ck = Vec::new();
    for (i, inst) in instruments.iter().enumerate() {
        write_fixed_str(&mut inst_ck, &inst.name, 20);
        inst_ck.extend_from_slice(&(i as u16).to_le_bytes());
    }
    write_fixed_str(&mut inst_ck, "EOI", 20);
    inst_ck.extend_from_slice(&(instruments.len() as u16).to_le_bytes());

    let mut ibag = Vec::new();
    let mut igen = Vec::new();
    for inst in instruments {
        let gens = instrument_generators(inst);
        ibag.extend_from_slice(&((igen.len() / 4) as u16).to_le_bytes());
        ibag.extend_from_slice(&0u16.to_le_bytes());
        igen.extend_from_slice(&gens);
    }
    ibag.extend_from_slice(&((igen.len() / 4) as u16).to_le_bytes());
    ibag.extend_from_slice(&0u16.to_le_bytes());
    igen.extend_from_slice(&[0u8; 4]); // terminal generator record

    let imod = vec![0u8; 10]; // terminal record only: no instrument modulators

    let shdr = build_shdr(samples, spans);

    let mut body = Vec::new();
    body.extend_from_slice(&write_chunk(b"phdr", &phdr));
    body.extend_from_slice(&write_chunk(b"pbag", &pbag));
    body.extend_from_slice(&write_chunk(b"pmod", &pmod));
    body.extend_from_slice(&write_chunk(b"pgen", &pgen));
    body.extend_from_slice(&write_chunk(b"inst", &inst_ck));
    body.extend_from_slice(&write_chunk(b"ibag", &ibag));
    body.extend_from_slice(&write_chunk(b"imod", &imod));
    body.extend_from_slice(&write_chunk(b"igen", &igen));
    body.extend_from_slice(&shdr);
    write_list(b"pdta", &body)
}

/// Build a complete SF2 file from samples + instruments (index `i` in
/// `instruments` refers to `samples[instruments[i].sample_index]`).
pub fn write(samples: &[Sf2Sample], instruments: &[Sf2Instrument]) -> Vec<u8> {
    let (sdta, spans) = build_sdta(samples);
    let mut body = Vec::new();
    body.extend_from_slice(b"sfbk");
    body.extend_from_slice(&build_info());
    body.extend_from_slice(&sdta);
    body.extend_from_slice(&build_pdta(samples, instruments, &spans));
    write_chunk(b"RIFF", &body)
}

#[cfg(test)]
#[path = "writer_tests.rs"]
mod tests;
