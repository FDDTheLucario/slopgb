use super::*;

#[test]
fn rejects_non_riff_bytes() {
    assert!(parse(b"not a riff file at all").is_err());
}

#[test]
fn rejects_wrong_form_type() {
    let bytes = crate::riff::write_chunk(b"RIFF", b"WAVEjunk");
    assert!(parse(&bytes).is_err());
}

#[test]
fn round_trips_through_the_writer() {
    // Build minimal input structs directly (mirrors what writer_tests does)
    // and confirm the reader recovers the same sample/instrument shape.
    let sample = Sf2Sample {
        name: "tone".to_string(),
        pcm: vec![0, 1000, 2000, 1000, 0, -1000, -2000, -1000],
        loop_start: 0,
        loop_end: 8,
        sample_rate: 32000,
        original_pitch: 60,
        pitch_correction: 0,
    };
    let inst = Sf2Instrument {
        name: "tone_inst".to_string(),
        sample_index: 0,
        loops: true,
        root_key_override: None,
        coarse_tune: 0,
        fine_tune: 0,
        initial_attenuation_cb: 0,
        vol_env: VolEnv::default(),
        key_range: None,
    };
    let bytes = crate::writer::write(&[sample], &[inst]);
    let parsed = parse(&bytes).expect("writer output must parse");
    assert_eq!(parsed.samples.len(), 1);
    assert_eq!(parsed.instruments.len(), 1);
    assert_eq!(
        parsed.samples[0].pcm,
        vec![0, 1000, 2000, 1000, 0, -1000, -2000, -1000]
    );
    assert!(parsed.instruments[0].loops);
}
