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

/// An **original**, clean-room SPC700 sound driver (authored from the SPC700 /
/// S-DSP register docs — nocash fullsnes, Blargg `SPC_DSP` — not derived from any
/// ROM), packaged as an SGB `SOU_TRN` block. The block carries three descriptors:
/// the SPC700 program (the `SOU_TRN` entry point), a one-entry sample directory,
/// and a 16-sample square-wave BRR sample. Run, the program writes the S-DSP
/// registers over `$F2`/`$F3` to play a ~2 kHz square-wave "menu tone" on voice 0
/// (my own synthesis: a hand-built square BRR looped at the sample rate).
fn original_sgb_tone_driver() -> Vec<u8> {
    // SPC700: `MOV dp,#imm` = `8F imm dp` (fullsnes opcode table).
    let mov = |dp: u8, imm: u8| [0x8F, imm, dp];
    // The DSP setup the program performs, voice 0. KON is written last so the
    // voice keys on only once everything is configured (nocash "SNES APU DSP").
    let dsp_writes: [(u8, u8); 12] = [
        (0x6C, 0x00), // FLG: unmute, no soft-reset, noise off
        (0x5D, 0x02), // DIR = page $02 (sample directory at $0200)
        (0x0C, 0x7F), // MVOLL (master volume L)
        (0x1C, 0x7F), // MVOLR
        (0x00, 0x7F), // V0 VOLL
        (0x01, 0x7F), // V0 VOLR
        (0x02, 0x00), // V0 pitch lo
        (0x03, 0x10), // V0 pitch hi -> $1000 (1 BRR sample / output sample)
        (0x04, 0x00), // V0 SRCN = directory entry 0
        (0x05, 0x00), // V0 ADSR1 = 0 -> use GAIN
        (0x07, 0x7F), // V0 GAIN = direct max (audible, steady)
        (0x4C, 0x01), // KON voice 0 (last)
    ];
    let mut prog = Vec::new();
    for (dp, imm) in dsp_writes {
        prog.extend_from_slice(&mov(0xF2, dp)); // select DSP register
        prog.extend_from_slice(&mov(0xF3, imm)); // write it
    }
    prog.extend_from_slice(&[0x2F, 0xFE]); // BRA * (spin so the DSP keeps playing)

    // One-entry sample directory: start = loop = $0210.
    let dir = [0x10u8, 0x02, 0x10, 0x02];
    // A 16-sample square BRR block: header shift 9 / filter 0 / loop + end, then
    // eight nibbles +7 and eight nibbles -8 -> a square wave, looped at $1000
    // pitch = 32 kHz / 16 = 2 kHz.
    let mut brr = vec![0x93u8];
    brr.extend_from_slice(&[0x77, 0x77, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88]);

    // Assemble the SOU_TRN descriptor stream: (dest_le, len_le, data...). The
    // FIRST descriptor's address is the entry point, so the program leads.
    let mut block = Vec::new();
    let mut push = |dest: u16, data: &[u8]| {
        block.extend_from_slice(&dest.to_le_bytes());
        block.extend_from_slice(&(data.len() as u16).to_le_bytes());
        block.extend_from_slice(data);
    };
    push(0x0400, &prog); // program (entry)
    push(0x0200, &dir); // directory
    push(0x0210, &brr); // sample
    block
}

/// The clean-room SPC700 driver, uploaded via `SOU_TRN` and executed on the
/// emulated SPC700, sets up the S-DSP and synthesizes audible tone output —
/// proving "a `SOU_TRN`-uploaded driver runs exactly". No DSP register is poked
/// from Rust here: the audio can only appear if the SPC700 ran the program, so a
/// non-zero peak is proof the uploaded driver executed end to end.
#[test]
fn original_sou_trn_driver_synthesizes_a_tone() {
    let mut apu = SgbApu::new(48_000);
    apu.upload_transfer(&original_sgb_tone_driver(), true);
    assert_eq!(
        apu.spc.pc, 0x0400,
        "SOU_TRN entry = the program's load address"
    );

    // Run a few frames so the SPC700 executes the setup and the DSP synthesizes.
    apu.clock(70_224 * 4);

    // The driver wrote MVOLL over $F2/$F3 — readable back proves it executed.
    assert_eq!(
        apu.dsp.borrow().read(0x0C),
        0x7F,
        "the uploaded program set MVOLL via the SPC700 DSP ports",
    );
    let peak = apu
        .out
        .iter()
        .fold(0.0f32, |m, &(l, r)| m.max(l.abs()).max(r.abs()));
    assert!(
        peak > 0.0,
        "the uploaded driver synthesized audible tone output"
    );
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
    // Seed a known DSP register on the ORIGINAL before cloning. The DSP is the
    // one genuinely shared object (an `Rc<RefCell<SDsp>>` aliased by the SPC700's
    // `DspLink`); APU RAM lives in the SPC700's own `Box` and is never shared.
    apu.dsp.borrow_mut().write(0x0C, 0x11); // MVOLL

    let mut cloned = apu.clone();

    // Mutate the clone's APU RAM *and* its DSP cell.
    cloned.spc.apu_ram_mut()[0x2000] = 0x00;
    cloned.dsp.borrow_mut().write(0x0C, 0x77);

    // The original must be untouched on both. The DSP assert fails if `clone`
    // regresses to a shallow `Rc::clone(&self.dsp)` (aliasing the same cell)
    // instead of deep-copying the S-DSP into a fresh `Rc`.
    assert_eq!(apu.spc.apu_ram()[0x2000], 0x42);
    assert_eq!(
        apu.dsp.borrow().read(0x0C),
        0x11,
        "clone shares the DSP cell"
    );
    assert_eq!(cloned.dsp.borrow().read(0x0C), 0x77);
}
