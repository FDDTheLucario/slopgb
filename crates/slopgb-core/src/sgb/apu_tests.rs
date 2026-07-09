//! SgbApu tests: model gating, clocking/emission, the SOU_TRN uploader, the
//! SOUND→port seam, mixing, and a save-state round-trip.

use super::*;
use crate::Model;

#[test]
fn only_sgb_models_have_an_apu() {
    assert!(SgbApu::for_model(Model::Sgb).is_some());
    assert!(SgbApu::for_model(Model::Sgb2).is_some());
    assert!(SgbApu::for_model(Model::Dmg).is_none());
    assert!(SgbApu::for_model(Model::Cgb).is_none());
    assert!(SgbApu::for_model(Model::Agb).is_none());
}

#[test]
fn clock_emits_output_samples_at_the_rate() {
    let mut apu = SgbApu::new(48_000);
    // One frame of GB cycles ≈ 70224; at 48 kHz that is ~803 output samples.
    apu.clock(70_224);
    let expected = (70_224.0 / (f64::from(crate::CLOCK_HZ) / 48_000.0)).round() as i64;
    assert!(
        (apu.out.len() as i64 - expected).abs() <= 1,
        "emitted {}",
        apu.out.len()
    );
}

#[test]
fn sou_trn_upload_writes_apu_ram_and_starts_execution() {
    let mut apu = SgbApu::new(48_000);
    // One descriptor: dest 0x0400, len 3, data [0xAA,0xBB,0xCC].
    let block = [0x00, 0x04, 0x03, 0x00, 0xAA, 0xBB, 0xCC, 0x00, 0x00];
    apu.upload_transfer(&block, true);
    let ram = apu.spc.apu_ram();
    assert_eq!(ram[0x0400], 0xAA);
    assert_eq!(ram[0x0401], 0xBB);
    assert_eq!(ram[0x0402], 0xCC);
    assert_eq!(apu.spc.pc, 0x0400); // started at the load address
}

#[test]
fn sound_command_reaches_the_comm_ports() {
    let mut apu = SgbApu::new(48_000);
    apu.apply_sound(SgbSound {
        effect_a: 0x11,
        effect_b: 0x22,
        attenuation: 0x33,
        effect_bank: 0x44,
    });
    assert_eq!(apu.spc.apu_port_in(0), 0x11);
    assert_eq!(apu.spc.apu_port_in(1), 0x22);
    assert_eq!(apu.spc.apu_port_in(2), 0x33);
    assert_eq!(apu.spc.apu_port_in(3), 0x44);
}

#[test]
fn mix_into_adds_and_drains() {
    let mut apu = SgbApu::new(48_000);
    apu.out = vec![(0.5, -0.5), (0.25, -0.25)];
    let mut gb = vec![(0.1f32, 0.1f32), (0.2, 0.2), (0.3, 0.3)];
    apu.mix_into(&mut gb);
    assert!((gb[0].0 - 0.6).abs() < 1e-6);
    assert!((gb[1].0 - 0.45).abs() < 1e-6);
    assert_eq!(gb[2], (0.3, 0.3)); // no SGB sample left for this one
    assert!(apu.out.is_empty()); // both consumed
}

#[test]
fn end_to_end_synthesis_produces_audio() {
    let mut apu = SgbApu::new(48_000);
    // Set up a looping constant BRR sample + directory directly in APU RAM.
    {
        let ram = apu.spc.apu_ram_mut();
        ram[0x0200] = 0x10;
        ram[0x0201] = 0x02; // dir[0].start = 0x0210
        ram[0x0202] = 0x10;
        ram[0x0203] = 0x02;
        ram[0x0210] = 0x43; // shift 4, filter 0, loop+end
        for b in 1..9 {
            ram[0x0210 + b] = 0x22;
        }
    }
    {
        let mut dsp = apu.dsp.borrow_mut();
        dsp.write(0x5D, 0x02); // DIR
        dsp.write(0x00, 0x7F); // VOLL
        dsp.write(0x01, 0x7F); // VOLR
        dsp.write(0x03, 0x10); // PH -> pitch 0x1000
        dsp.write(0x07, 0x7F); // GAIN direct max
        dsp.write(0x0C, 0x7F); // MVOLL
        dsp.write(0x1C, 0x7F); // MVOLR
        dsp.write(0x4C, 0x01); // KON voice 0
    }
    // Clock several frames' worth of cycles.
    apu.clock(70_224 * 4);
    let peak = apu
        .out
        .iter()
        .fold(0.0f32, |m, &(l, r)| m.max(l.abs()).max(r.abs()));
    assert!(peak > 0.0, "expected audible SGB output");
}

#[test]
fn save_state_round_trips() {
    let mut apu = SgbApu::new(48_000);
    apu.spc.apu_ram_mut()[0x1234] = 0x99;
    apu.dsp.borrow_mut().write(0x0C, 0x55);
    apu.clock(12_345);

    let mut w = crate::state::Writer::new();
    apu.write_state(&mut w);
    let bytes = w.into_vec();

    let mut restored = SgbApu::new(48_000);
    let mut r = crate::state::Reader::new(&bytes);
    restored.read_state(&mut r).unwrap();

    let mut w2 = crate::state::Writer::new();
    restored.write_state(&mut w2);
    assert_eq!(bytes, w2.into_vec());
    assert_eq!(restored.spc.apu_ram()[0x1234], 0x99);
}

#[test]
fn clone_is_independent() {
    let mut apu = SgbApu::new(48_000);
    apu.spc.apu_ram_mut()[0x2000] = 0x42;
    let mut cloned = apu.clone();
    cloned.spc.apu_ram_mut()[0x2000] = 0x00;
    // Mutating the clone must not affect the original's shared DSP/RAM.
    assert_eq!(apu.spc.apu_ram()[0x2000], 0x42);
}
