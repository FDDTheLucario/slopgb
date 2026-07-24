//! Hand-rolled SF2 (`RIFF....sfbk`) reader.
//!
//! Parses just enough of the format for the N-SPC round-trip: samples (PCM16,
//! loop points, tuning) and instruments (one sample zone plus the vol-env /
//! attenuation / tuning generators). Preset-layer flattening is NOT built —
//! see the module doc on [`Sf2File`] for the ceiling.
//!
//! Generator enumerator IDs are from the SoundFont 2.01 Technical
//! Specification §8.1.2 (synthfont.com `SFSPEC21.PDF`).

use crate::riff;

/// SF2 generator enumerator IDs this reader/writer understands (§8.1.2).
pub(crate) const GEN_DELAY_VOL_ENV: u16 = 33;
pub(crate) const GEN_ATTACK_VOL_ENV: u16 = 34;
pub(crate) const GEN_HOLD_VOL_ENV: u16 = 35;
pub(crate) const GEN_DECAY_VOL_ENV: u16 = 36;
pub(crate) const GEN_SUSTAIN_VOL_ENV: u16 = 37;
pub(crate) const GEN_RELEASE_VOL_ENV: u16 = 38;
pub(crate) const GEN_INSTRUMENT: u16 = 41;
pub(crate) const GEN_KEY_RANGE: u16 = 43;
pub(crate) const GEN_INITIAL_ATTENUATION: u16 = 48;
pub(crate) const GEN_COARSE_TUNE: u16 = 51;
pub(crate) const GEN_FINE_TUNE: u16 = 52;
pub(crate) const GEN_SAMPLE_ID: u16 = 53;
pub(crate) const GEN_SAMPLE_MODES: u16 = 54;
pub(crate) const GEN_OVERRIDING_ROOT_KEY: u16 = 58;

/// One SF2 sample: PCM16 plus the loop points and tuning shdr carries.
pub struct Sf2Sample {
    pub name: String,
    pub pcm: Vec<i16>,
    /// Loop points, sample-relative (offsets into `pcm`, i.e. already
    /// rebased from the absolute `smpl`-chunk offsets in `shdr`).
    pub loop_start: u32,
    pub loop_end: u32,
    pub sample_rate: u32,
    pub original_pitch: u8,
    pub pitch_correction: i8,
}

/// Volume envelope generator amounts (raw SF2 units: timecents / centibels).
pub struct VolEnv {
    pub delay_tc: i16,
    pub attack_tc: i16,
    pub hold_tc: i16,
    pub decay_tc: i16,
    pub sustain_cb: i16,
    pub release_tc: i16,
}

impl Default for VolEnv {
    /// SF2 §8.1.3 defaults: all times -12000 timecents (~instant), sustain 0
    /// centibels (full level, i.e. no decay).
    fn default() -> Self {
        VolEnv {
            delay_tc: -12000,
            attack_tc: -12000,
            hold_tc: -12000,
            decay_tc: -12000,
            sustain_cb: 0,
            release_tc: -12000,
        }
    }
}

/// One SF2 instrument: its single sample zone (multi-zone/velocity-split
/// instruments are not supported — see [`Sf2File`]).
pub struct Sf2Instrument {
    pub name: String,
    pub sample_index: usize,
    pub loops: bool,
    pub root_key_override: Option<u8>,
    pub coarse_tune: i8,
    pub fine_tune: i8,
    pub initial_attenuation_cb: i16,
    pub vol_env: VolEnv,
    pub key_range: Option<(u8, u8)>,
}

/// Parsed SF2 contents: samples + instruments only. Full preset-layer
/// flattening (velocity/key-split zones, modulators, preset-level generator
/// offsets) is NOT built — instrument order is the `inst` sub-chunk order,
/// which is what this crate's own [`crate::writer`] produces 1:1 with
/// presets, so the round trip needs nothing from `phdr`/`pbag`/`pgen`.
pub struct Sf2File {
    pub samples: Vec<Sf2Sample>,
    pub instruments: Vec<Sf2Instrument>,
}

struct InstRec {
    name: String,
    bag_ndx: u16,
}

struct IbagRec {
    gen_ndx: u16,
}

struct IgenRec {
    oper: u16,
    amount: [u8; 2],
}

struct ShdrRec {
    name: String,
    start: u32,
    end: u32,
    startloop: u32,
    endloop: u32,
    sample_rate: u32,
    original_pitch: u8,
    pitch_correction: i8,
}

fn parse_shdr(data: &[u8]) -> Result<Vec<ShdrRec>, String> {
    if data.len() % 46 != 0 || data.len() < 46 {
        return Err(format!(
            "shdr chunk size {} is not a valid multiple of 46",
            data.len()
        ));
    }
    let mut out = Vec::new();
    for rec in data.chunks_exact(46) {
        out.push(ShdrRec {
            name: riff::read_cstr(&rec[0..20]),
            start: riff::read_u32_le(&rec[20..24]),
            end: riff::read_u32_le(&rec[24..28]),
            startloop: riff::read_u32_le(&rec[28..32]),
            endloop: riff::read_u32_le(&rec[32..36]),
            sample_rate: riff::read_u32_le(&rec[36..40]),
            original_pitch: rec[40],
            pitch_correction: rec[41] as i8,
        });
    }
    out.pop(); // drop the terminal "EOS" record
    Ok(out)
}

fn parse_inst(data: &[u8]) -> Result<Vec<InstRec>, String> {
    if data.len() % 22 != 0 || data.len() < 44 {
        return Err(format!(
            "inst chunk size {} is not a valid multiple of 22",
            data.len()
        ));
    }
    Ok(data
        .chunks_exact(22)
        .map(|rec| InstRec {
            name: riff::read_cstr(&rec[0..20]),
            bag_ndx: riff::read_u16_le(&rec[20..22]),
        })
        .collect())
}

fn parse_ibag(data: &[u8]) -> Result<Vec<IbagRec>, String> {
    if data.len() % 4 != 0 {
        return Err(format!(
            "ibag chunk size {} is not a multiple of 4",
            data.len()
        ));
    }
    Ok(data
        .chunks_exact(4)
        .map(|rec| IbagRec {
            gen_ndx: riff::read_u16_le(&rec[0..2]),
        })
        .collect())
}

fn parse_igen(data: &[u8]) -> Result<Vec<IgenRec>, String> {
    if data.len() % 4 != 0 {
        return Err(format!(
            "igen chunk size {} is not a multiple of 4",
            data.len()
        ));
    }
    Ok(data
        .chunks_exact(4)
        .map(|rec| IgenRec {
            oper: riff::read_u16_le(&rec[0..2]),
            amount: [rec[2], rec[3]],
        })
        .collect())
}

fn find_chunk<'a>(chunks: &[riff::RiffChunk<'a>], id: &[u8; 4]) -> Option<&'a [u8]> {
    chunks.iter().find(|c| &c.id == id).map(|c| c.data)
}

fn find_list<'a>(chunks: &[riff::RiffChunk<'a>], form: &[u8; 4]) -> Option<&'a [u8]> {
    chunks.iter().find_map(|c| {
        if &c.id != b"LIST" {
            return None;
        }
        let (f, body) = riff::form_and_body(c.data).ok()?;
        (&f == form).then_some(body)
    })
}

/// A generator amount as a signed 16-bit value (most generators).
fn gen_i16(gens: &std::collections::HashMap<u16, [u8; 2]>, id: u16, default: i16) -> i16 {
    gens.get(&id).map_or(default, |a| riff::read_i16_le(a))
}

/// A generator amount as an unsigned 16-bit value (index/flag generators).
fn gen_u16(gens: &std::collections::HashMap<u16, [u8; 2]>, id: u16, default: u16) -> u16 {
    gens.get(&id).map_or(default, |a| riff::read_u16_le(a))
}

/// Parse a standard SF2 file's samples + instruments.
pub fn parse(bytes: &[u8]) -> Result<Sf2File, String> {
    let top = riff::parse_chunks(bytes)?;
    let riff_chunk = top.first().ok_or("empty RIFF file")?;
    if &riff_chunk.id != b"RIFF" {
        return Err("not a RIFF file".to_string());
    }
    let (form, body) = riff::form_and_body(riff_chunk.data)?;
    if &form != b"sfbk" {
        return Err(format!("not an sfbk RIFF form (got {form:?})"));
    }
    let subs = riff::parse_chunks(body)?;

    let sdta = find_list(&subs, b"sdta").ok_or("missing sdta-list chunk")?;
    let sdta_subs = riff::parse_chunks(sdta)?;
    let smpl = find_chunk(&sdta_subs, b"smpl").unwrap_or(&[]);
    let all_samples: Vec<i16> = smpl.chunks_exact(2).map(riff::read_i16_le).collect();

    let pdta = find_list(&subs, b"pdta").ok_or("missing pdta-list chunk")?;
    let pdta_subs = riff::parse_chunks(pdta)?;
    let shdr = parse_shdr(find_chunk(&pdta_subs, b"shdr").ok_or("missing shdr chunk")?)?;
    let inst = parse_inst(find_chunk(&pdta_subs, b"inst").ok_or("missing inst chunk")?)?;
    let ibag = parse_ibag(find_chunk(&pdta_subs, b"ibag").ok_or("missing ibag chunk")?)?;
    let igen = parse_igen(find_chunk(&pdta_subs, b"igen").ok_or("missing igen chunk")?)?;

    let samples: Vec<Sf2Sample> = shdr
        .iter()
        .map(|s| {
            let start = s.start as usize;
            let end = (s.end as usize).min(all_samples.len());
            let pcm = if start <= end {
                all_samples[start..end].to_vec()
            } else {
                Vec::new()
            };
            Sf2Sample {
                name: s.name.clone(),
                pcm,
                loop_start: s.startloop.saturating_sub(s.start),
                loop_end: s.endloop.saturating_sub(s.start),
                sample_rate: s.sample_rate,
                original_pitch: s.original_pitch,
                pitch_correction: s.pitch_correction,
            }
        })
        .collect();

    let mut instruments = Vec::new();
    // inst has a terminal "EOI" record; real instruments are all but the last.
    for i in 0..inst.len().saturating_sub(1) {
        let bag_start = inst[i].bag_ndx as usize;
        let bag_end = inst[i + 1].bag_ndx as usize;
        if bag_end <= bag_start || bag_end > ibag.len() {
            return Err(format!("instrument {i} ('{}') has no zones", inst[i].name));
        }

        let mut global_gens: std::collections::HashMap<u16, [u8; 2]> =
            std::collections::HashMap::new();
        let mut sample_gens: Option<std::collections::HashMap<u16, [u8; 2]>> = None;
        let n_zones = bag_end - bag_start;

        for (j, zone) in (bag_start..bag_end).enumerate() {
            let gen_start = ibag[zone].gen_ndx as usize;
            let gen_end = ibag
                .get(zone + 1)
                .map(|b| b.gen_ndx as usize)
                .unwrap_or(gen_start);
            if gen_end > igen.len() || gen_start > gen_end {
                continue; // malformed zone: skip (SF2 error handling: ignore)
            }
            let zone_gens = &igen[gen_start..gen_end];
            let has_sample_id = zone_gens.last().is_some_and(|g| g.oper == GEN_SAMPLE_ID);

            if j == 0 && !has_sample_id && n_zones > 1 {
                for g in zone_gens {
                    global_gens.insert(g.oper, g.amount);
                }
                continue;
            }
            if has_sample_id && sample_gens.is_none() {
                let mut combined = global_gens.clone();
                for g in zone_gens {
                    combined.insert(g.oper, g.amount);
                }
                sample_gens = Some(combined);
                // Only the first sample zone is used — no multi-zone support.
                break;
            }
        }

        let Some(gens) = sample_gens else {
            return Err(format!(
                "instrument {i} ('{}') has no sample zone",
                inst[i].name
            ));
        };

        let sample_index = gen_u16(&gens, GEN_SAMPLE_ID, 0) as usize;
        let sample_modes = gen_u16(&gens, GEN_SAMPLE_MODES, 0);
        let root_key_amount = gen_i16(&gens, GEN_OVERRIDING_ROOT_KEY, -1);
        let root_key_override = (0..=127)
            .contains(&root_key_amount)
            .then_some(root_key_amount as u8);
        // keyRange amount: byte 0 = low key, byte 1 = high key (the struct's
        // own field order, byLo then byHi — the §8.1.2 prose describing
        // "LS byte = highest" is a known erratum; every real SF2 reader,
        // including fluidsynth, uses byLo = low key).
        let key_range = gens.get(&GEN_KEY_RANGE).map(|a| (a[0], a[1]));

        instruments.push(Sf2Instrument {
            name: inst[i].name.clone(),
            sample_index,
            loops: sample_modes == 1 || sample_modes == 3,
            root_key_override,
            coarse_tune: gen_i16(&gens, GEN_COARSE_TUNE, 0) as i8,
            fine_tune: gen_i16(&gens, GEN_FINE_TUNE, 0) as i8,
            initial_attenuation_cb: gen_i16(&gens, GEN_INITIAL_ATTENUATION, 0),
            vol_env: VolEnv {
                delay_tc: gen_i16(&gens, GEN_DELAY_VOL_ENV, VolEnv::default().delay_tc),
                attack_tc: gen_i16(&gens, GEN_ATTACK_VOL_ENV, VolEnv::default().attack_tc),
                hold_tc: gen_i16(&gens, GEN_HOLD_VOL_ENV, VolEnv::default().hold_tc),
                decay_tc: gen_i16(&gens, GEN_DECAY_VOL_ENV, VolEnv::default().decay_tc),
                sustain_cb: gen_i16(&gens, GEN_SUSTAIN_VOL_ENV, VolEnv::default().sustain_cb),
                release_tc: gen_i16(&gens, GEN_RELEASE_VOL_ENV, VolEnv::default().release_tc),
            },
            key_range,
        });
    }

    Ok(Sf2File {
        samples,
        instruments,
    })
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
