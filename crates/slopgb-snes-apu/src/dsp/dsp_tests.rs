//! Whole-DSP tests: register access + mirror, live ENVX/OUTX/ENDX reads,
//! KON-driven synthesis end-to-end, the FLG mute, and a save-state round-trip.

use super::*;

/// APU RAM with a directory (entry 0 at `0x0200`) and a looping constant BRR
/// sample at `0x0210` (nibble 2, filter 0, shift 4 -> value 32).
fn ram_with_sample() -> Box<[u8; 0x1_0000]> {
    let mut ram = Box::new([0u8; 0x1_0000]);
    ram[0x0200] = 0x10;
    ram[0x0201] = 0x02; // start = 0x0210
    ram[0x0202] = 0x10;
    ram[0x0203] = 0x02; // loop  = 0x0210
    ram[0x0210] = 0x43; // shift 4, filter 0, loop+end
    for b in 1..9 {
        ram[0x0210 + b] = 0x22;
    }
    ram
}

#[test]
fn register_write_read_roundtrips_and_mirror_is_readonly() {
    let mut dsp = SDsp::new();
    dsp.write(0x00, 0x5A); // VOLL voice 0
    assert_eq!(dsp.read(0x00), 0x5A);
    // The $80+ mirror reads the same data but ignores writes.
    assert_eq!(dsp.read(0x80), 0x5A);
    dsp.write(0x80, 0x99);
    assert_eq!(dsp.read(0x00), 0x5A); // unchanged
}

#[test]
fn endx_write_clears_and_readback_is_live() {
    let mut dsp = SDsp::new();
    dsp.endx = 0xFF;
    assert_eq!(dsp.read(ENDX as u8), 0xFF);
    dsp.write(ENDX as u8, 0x00); // any write clears
    assert_eq!(dsp.read(ENDX as u8), 0x00);
}

#[test]
fn envx_and_outx_read_live_voice_state() {
    let mut dsp = SDsp::new();
    dsp.voices[1].env.level = 0x7F0;
    dsp.voices[1].outx = -5;
    assert_eq!(dsp.read(0x18), 0x7F); // voice 1 ENVX = level>>4
    assert_eq!(dsp.read(0x19), (-5i8) as u8); // voice 1 OUTX
}

#[test]
fn keyon_produces_audio_end_to_end() {
    let mut ram = ram_with_sample();
    let mut dsp = SDsp::new();
    // Leave the power-on FLG state ($E0: reset + mute + echo-write disable)
    // like every real driver does before keying a voice.
    dsp.write(FLG as u8, 0x00);
    // Voice 0: DIR page 2, SRCN 0, full VOL, GAIN direct max, pitch 0x1000.
    dsp.write(DIR as u8, 0x02);
    dsp.write(0x04, 0x00); // SRCN 0
    dsp.write(0x00, 0x7F); // VOLL
    dsp.write(0x01, 0x7F); // VOLR
    dsp.write(0x02, 0x00); // PL
    dsp.write(0x03, 0x10); // PH -> pitch 0x1000
    dsp.write(0x05, 0x00); // ADSR1 (ADSR off -> GAIN)
    dsp.write(0x07, 0x7F); // GAIN direct max
    dsp.write(MVOLL as u8, 0x7F);
    dsp.write(MVOLR as u8, 0x7F);
    dsp.write(KON as u8, 0x01); // key on voice 0

    let mut peak = 0i32;
    for _ in 0..64 {
        let (l, r) = dsp.sample(&mut ram);
        peak = peak.max(i32::from(l).abs()).max(i32::from(r).abs());
    }
    assert!(peak > 10, "expected audible output, peak {peak}");
}

/// ADSR-mode key-on produces audio — the register set Space Invaders' ARCADE
/// sound driver programs (ADSR1 $FF = ADSR on, fast attack; ADSR2 $E0;
/// FLG $20; medium volumes), which must synthesize like the GAIN path does.
#[test]
fn keyon_produces_audio_in_adsr_mode() {
    let mut ram = ram_with_sample();
    let mut dsp = SDsp::new();
    dsp.write(FLG as u8, 0x20);
    dsp.write(DIR as u8, 0x02);
    dsp.write(0x04, 0x00); // SRCN 0
    dsp.write(0x00, 0x40); // VOLL
    dsp.write(0x01, 0x40); // VOLR
    dsp.write(0x02, 0xCD); // PL
    dsp.write(0x03, 0x0F); // PH -> pitch 0x0FCD
    dsp.write(0x05, 0xFF); // ADSR1: ADSR enabled, attack rate 15
    dsp.write(0x06, 0xE0); // ADSR2: sustain level 7
    dsp.write(MVOLL as u8, 0x40);
    dsp.write(MVOLR as u8, 0x40);
    dsp.write(KON as u8, 0x01);
    let mut peak = 0i16;
    for _ in 0..2000 {
        let (l, r) = dsp.sample(&mut ram);
        peak = peak.max(l.abs()).max(r.abs());
    }
    assert!(peak > 0, "ADSR-mode voice must produce audio, peak={peak}");
}

/// Same ADSR key-on, but on voice 4 with KOF bits held for the OTHER voices
/// — the exact register pattern the ARCADE driver leaves while the march
/// plays (KOF $EF pulses, voice-4 bit clear).
#[test]
fn keyon_produces_audio_on_voice_4_with_others_keyed_off() {
    let mut ram = ram_with_sample();
    let mut dsp = SDsp::new();
    dsp.write(FLG as u8, 0x20);
    dsp.write(DIR as u8, 0x02);
    dsp.write(0x44, 0x00); // SRCN 0
    dsp.write(0x40, 0x40); // VOLL
    dsp.write(0x41, 0x40); // VOLR
    dsp.write(0x42, 0xCD); // PL
    dsp.write(0x43, 0x0F); // PH
    dsp.write(0x45, 0xFF); // ADSR1
    dsp.write(0x46, 0xE0); // ADSR2
    dsp.write(MVOLL as u8, 0x40);
    dsp.write(MVOLR as u8, 0x40);
    dsp.write(KOF as u8, 0xEF); // every voice but 4 keyed off
    dsp.write(KON as u8, 0x10); // key on voice 4
    dsp.write(KOF as u8, 0x00);
    let mut peak = 0i16;
    for _ in 0..2000 {
        let (l, r) = dsp.sample(&mut ram);
        peak = peak.max(l.abs()).max(r.abs());
    }
    assert!(
        peak > 0,
        "voice-4 ADSR key-on must produce audio, peak={peak}"
    );
}

/// Re-writing KON with the SAME bit set re-triggers the voice: each KON
/// write arms its set bits (Blargg SPC_DSP `new_kon`; fullsnes "KON") — the
/// register holding a bit does not, but a fresh write does, with no 0-write
/// in between. Space Invaders' march re-KONs $10 for every note.
#[test]
fn keyon_rewrite_retriggers_without_a_zero_write() {
    let mut ram = ram_with_sample();
    // Make the sample one-shot (END without LOOP), so the first note ends.
    ram[0x0210] = 0x41;
    let mut dsp = SDsp::new();
    dsp.write(FLG as u8, 0x20);
    dsp.write(DIR as u8, 0x02);
    dsp.write(0x04, 0x00);
    dsp.write(0x00, 0x40);
    dsp.write(0x01, 0x40);
    dsp.write(0x02, 0xCD);
    dsp.write(0x03, 0x0F);
    dsp.write(0x05, 0xFF);
    dsp.write(0x06, 0xE0);
    dsp.write(MVOLL as u8, 0x40);
    dsp.write(MVOLR as u8, 0x40);
    dsp.write(KON as u8, 0x01);
    let mut peak1 = 0i16;
    for _ in 0..600 {
        let (l, r) = dsp.sample(&mut ram);
        peak1 = peak1.max(l.abs()).max(r.abs());
    }
    assert!(peak1 > 0, "first note must sound, peak={peak1}");
    // The one-shot has ended; the driver keys the next note with the same
    // KON value.
    dsp.write(KOF as u8, 0x01);
    for _ in 0..40 {
        dsp.sample(&mut ram);
    }
    dsp.write(KOF as u8, 0x00);
    dsp.write(KON as u8, 0x01); // same value — no 0 write first
    let mut peak2 = 0i16;
    for _ in 0..600 {
        let (l, r) = dsp.sample(&mut ram);
        peak2 = peak2.max(l.abs()).max(r.abs());
    }
    assert!(peak2 > 0, "re-written KON must retrigger, peak={peak2}");
}

#[test]
fn flg_mute_silences_output() {
    let mut ram = ram_with_sample();
    let mut dsp = SDsp::new();
    dsp.write(DIR as u8, 0x02);
    dsp.write(0x00, 0x7F);
    dsp.write(0x01, 0x7F);
    dsp.write(0x03, 0x10);
    dsp.write(0x07, 0x7F);
    dsp.write(MVOLL as u8, 0x7F);
    dsp.write(MVOLR as u8, 0x7F);
    dsp.write(KON as u8, 0x01);
    dsp.write(FLG as u8, 0x40); // mute
    for _ in 0..64 {
        assert_eq!(dsp.sample(&mut ram), (0, 0));
    }
}

/// FNV-1a over the save-state byte stream, so a checkpoint pins the exact
/// voice/echo state without embedding ~800 literal bytes per checkpoint.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// Pins the full observable output stream (mixed samples + save-state hash at
/// checkpoints) across a KON -> sustain -> KOF -> long silent release
/// (loop+end BRR, so `ENDX` keeps toggling while muted) -> re-KON sequence on
/// two voices with voice 1 PMON-modulated by voice 0. This is the byte-
/// identity guard for the release/level-0 output fast path: any change that
/// alters BRR advance, ENDX timing, envelope evolution, or the zero output
/// voice 0 feeds into voice 1's pitch modulation must fail this test.
#[test]
fn sample_stream_is_byte_identical_across_silent_release_stretch() {
    let mut ram = ram_with_sample();
    let mut dsp = SDsp::new();
    dsp.write(FLG as u8, 0x00);
    dsp.write(DIR as u8, 0x02);
    // Voice 0: SRCN 0, full vol, GAIN direct max, pitch 0x1000.
    dsp.write(0x04, 0x00);
    dsp.write(0x00, 0x7F);
    dsp.write(0x01, 0x7F);
    dsp.write(0x02, 0x00);
    dsp.write(0x03, 0x10);
    dsp.write(0x05, 0x00);
    dsp.write(0x07, 0x7F);
    // Voice 1: SRCN 0, full vol, GAIN direct max, pitch 0x0800, PMON-modulated
    // by voice 0's output.
    dsp.write(0x14, 0x00);
    dsp.write(0x10, 0x7F);
    dsp.write(0x11, 0x7F);
    dsp.write(0x12, 0x00);
    dsp.write(0x13, 0x08);
    dsp.write(0x15, 0x00);
    dsp.write(0x17, 0x7F);
    dsp.write(PMON as u8, 0x02); // voice 1 modulated by voice 0
    dsp.write(MVOLL as u8, 0x7F);
    dsp.write(MVOLR as u8, 0x7F);
    dsp.write(KON as u8, 0x03); // key on voices 0 + 1

    let checkpoints = [5, 20, 40, 41, 60, 150, 260, 300, 399, 400, 401, 420, 440];
    let mut results = Vec::new();
    for i in 0..500 {
        if i == 40 {
            dsp.write(KOF as u8, 0x03); // key off both -> release
        }
        if i == 400 {
            // KOF is level-sensitive (sampled live every sample, not an edge)
            // — it must be cleared before KON or the still-held key-off
            // immediately re-releases the voice this same/next sample.
            dsp.write(KOF as u8, 0x00);
            dsp.write(KON as u8, 0x03); // re-key while still silent -> revive
        }
        let (l, r) = dsp.sample(&mut ram);
        if checkpoints.contains(&i) {
            let mut w = crate::state::Writer::new();
            dsp.write_state(&mut w);
            results.push((i, l, r, fnv1a(&w.into_vec())));
        }
    }

    // Values captured from the pre-optimization implementation (sample() /
    // voice::step() synthesizing every voice in full every sample, no
    // release/level-0 fast path) — pins byte-identity across the fast path.
    let expected: [(i32, i16, i16, u64); 13] = [
        (5, 0, 0, 0x4faf_4a25_a95d_7534),
        (20, 55, 55, 0x787a_8f0f_e93e_58f5),
        (40, 55, 55, 0xa9bb_dc3e_d59c_9aaf),
        (41, 55, 55, 0x8d40_b6e7_e5ca_5278),
        (60, 51, 51, 0x1120_0eda_6726_c8ad),
        (150, 29, 29, 0xd179_75ff_e84f_f330),
        (260, 3, 3, 0xf6d8_d970_43aa_563a),
        (300, 0, 0, 0x7c73_6fcc_9e8a_58b6),
        (399, 0, 0, 0x1306_5eb1_b1b2_a6da),
        (400, 0, 0, 0x3ac8_38ed_7eea_54f6),
        (401, 0, 0, 0xbfa4_6061_78be_5067),
        (420, 55, 55, 0x8f70_a0e4_c347_c322),
        (440, 55, 55, 0x7fc2_f28a_17d8_0495),
    ];
    assert_eq!(results, expected);
}

#[test]
fn save_state_round_trips() {
    let mut ram = ram_with_sample();
    let mut dsp = SDsp::new();
    dsp.write(DIR as u8, 0x02);
    dsp.write(0x00, 0x7F);
    dsp.write(0x03, 0x10);
    dsp.write(0x07, 0x7F);
    dsp.write(MVOLL as u8, 0x7F);
    dsp.write(KON as u8, 0x01);
    for _ in 0..20 {
        dsp.sample(&mut ram);
    }

    let mut w = crate::state::Writer::new();
    dsp.write_state(&mut w);
    let bytes = w.into_vec();

    let mut restored = SDsp::new();
    let mut r = crate::state::Reader::new(&bytes);
    restored.read_state(&mut r).unwrap();

    // Re-serializing the restored DSP yields identical bytes.
    let mut w2 = crate::state::Writer::new();
    restored.write_state(&mut w2);
    assert_eq!(bytes, w2.into_vec());

    // And it keeps synthesizing identically to the original from here.
    let mut ram2 = ram.clone();
    for _ in 0..16 {
        assert_eq!(dsp.sample(&mut ram), restored.sample(&mut ram2));
    }
}
