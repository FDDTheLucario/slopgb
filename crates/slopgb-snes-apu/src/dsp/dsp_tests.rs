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
