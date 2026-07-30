use super::*;
use crate::reader::{self, VolEnv};

fn synth_samples() -> Vec<Sf2Sample> {
    // A short sine-ish tone (looping) and a short decaying pluck (one-shot).
    let tone: Vec<i16> = (0..64)
        .map(|i| ((i as f64 * 0.2).sin() * 8000.0) as i16)
        .collect();
    let pluck: Vec<i16> = (0..32)
        .map(|i| (12000.0 * (-(i as f64) / 8.0).exp()) as i16)
        .collect();
    vec![
        Sf2Sample {
            name: "tone".to_string(),
            loop_start: 16,
            loop_end: tone.len() as u32,
            pcm: tone,
            sample_rate: 32000,
            original_pitch: 60,
            pitch_correction: 0,
        },
        Sf2Sample {
            name: "pluck".to_string(),
            loop_start: 0,
            loop_end: 0,
            pcm: pluck,
            sample_rate: 32000,
            original_pitch: 69,
            pitch_correction: -5,
        },
    ]
}

fn synth_instruments() -> Vec<Sf2Instrument> {
    vec![
        Sf2Instrument {
            name: "lead_tone".to_string(),
            sample_index: 0,
            loops: true,
            root_key_override: None,
            coarse_tune: 0,
            fine_tune: 0,
            initial_attenuation_cb: 20,
            vol_env: VolEnv::default(),
            key_range: None,
        },
        Sf2Instrument {
            name: "perc_pluck".to_string(),
            sample_index: 1,
            loops: false,
            root_key_override: Some(69),
            coarse_tune: -2,
            fine_tune: 10,
            initial_attenuation_cb: 0,
            vol_env: VolEnv {
                delay_tc: -12000,
                attack_tc: -12000,
                hold_tc: -12000,
                decay_tc: 200,
                sustain_cb: 400,
                release_tc: -2000,
            },
            key_range: None,
        },
    ]
}

#[test]
fn round_trips_pcm_loop_points_and_instrument_count() {
    let samples = synth_samples();
    let instruments = synth_instruments();
    let bytes = write(&samples, &instruments);

    let parsed = reader::parse(&bytes).expect("own reader must parse own writer output");

    assert_eq!(parsed.samples.len(), samples.len());
    assert_eq!(parsed.instruments.len(), instruments.len());

    for (original, got) in samples.iter().zip(&parsed.samples) {
        assert_eq!(got.pcm, original.pcm, "PCM must round-trip exactly");
        assert_eq!(got.loop_start, original.loop_start);
        assert_eq!(got.loop_end, original.loop_end);
        assert_eq!(got.sample_rate, original.sample_rate);
        assert_eq!(got.original_pitch, original.original_pitch);
        assert_eq!(got.pitch_correction, original.pitch_correction);
    }

    assert!(parsed.instruments[0].loops);
    assert!(!parsed.instruments[1].loops);
    assert_eq!(parsed.instruments[1].root_key_override, Some(69));
    assert_eq!(parsed.instruments[1].coarse_tune, -2);
    assert_eq!(parsed.instruments[1].fine_tune, 10);
    assert_eq!(parsed.instruments[1].vol_env.decay_tc, 200);
    assert_eq!(parsed.instruments[1].vol_env.sustain_cb, 400);
    assert_eq!(parsed.instruments[1].vol_env.release_tc, -2000);
}

#[test]
fn writer_output_starts_with_a_valid_riff_sfbk_header() {
    let bytes = write(&synth_samples(), &synth_instruments());
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"sfbk");
}
