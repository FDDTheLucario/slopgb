//! Headless throughput bench for [`SgbCoprocessor`]: how fast the two SGB
//! SNES-side workloads run with no GUI/audio-device attached.
//!
//! There is no commercial SGB ROM in this repo (only mooneye's
//! `boot_regs-sgb.gb`, which halts at `LD B,B` — useless for a sustained
//! run). So, like `lib_tests.rs`/`lib_tests_ppu.rs`, this drives
//! `SgbCoprocessor` directly with a synthetic 65C816 program instead of
//! loading a ROM into a `GameBoy`.
//!
//! ```text
//! cargo run --release -p slopgb-sgb-coprocessor --example throughput -- <plugins-dir> [frames]
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use slopgb_core::sgb::{AudioCoprocessor, SgbCommandSource};
use slopgb_core::{DEFAULT_SAMPLE_RATE, SgbFlags, SgbSound};
use slopgb_sgb_coprocessor::{CPU_WASM, PPU_WASM, SPC_WASM, SgbCoprocessor};

/// GB frame length in T-cycles (`GameBoy::run_frame`'s span) — mirrors
/// `slopgb_core::CYCLES_PER_FRAME`.
const GB_FRAME_CYCLES: u64 = 70_224;
/// The Game Boy's real frame rate (4194304 Hz / 70224 cycles/frame).
const REAL_FPS: f64 = 59.7275;
/// Runs per workload; the reported figure is their median.
const RUNS: u32 = 3;

/// A one-shot [`SgbCommandSource`]: yields a single queued SOUND event, then
/// nothing — enough to trigger the resident square driver, which then loops
/// the S-DSP forever on its own (see `spc_firmware`'s `BRA *`).
#[derive(Default)]
struct SoundOnce {
    sound: Option<SgbSound>,
}

impl SgbCommandSource for SoundOnce {
    fn take_sound_event(&mut self) -> Option<SgbSound> {
        self.sound.take()
    }
    fn take_data_snd(&mut self) -> Option<Vec<u8>> {
        None
    }
    fn sou_trn_data(&self) -> Option<&[u8]> {
        None
    }
    fn data_trn_data(&self) -> Option<&[u8]> {
        None
    }
    fn flags(&self) -> Option<SgbFlags> {
        None
    }
}

fn read_plugin(dir: &Path, name: &str) -> Vec<u8> {
    match fs::read(dir.join(name)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "slopgb: cannot read '{}' ({e}); stage plugins first (cargo xtask stage-plugins <dir>)",
                dir.join(name).display(),
            );
            std::process::exit(1);
        }
    }
}

/// Plain-SGB workload: audio-only coprocessor (spc700 + w65c816, no
/// snes-ppu), primed with a bare SOUND command — mirrors
/// `lib_tests_apu.rs`'s `sound_command_drives_the_firmware_chain_to_audio`.
fn build_plain(spc: &[u8], cpu: &[u8]) -> SgbCoprocessor {
    let mut cop = SgbCoprocessor::from_wasm(spc, cpu, DEFAULT_SAMPLE_RATE).unwrap();
    let mut cmds = SoundOnce {
        sound: Some(SgbSound {
            effect_a: 0x40,
            effect_b: 0x00,
            attenuation: 0x00,
            effect_bank: 0x00,
        }),
    };
    AudioCoprocessor::poll(&mut cop, &mut cmds);
    cop
}

/// Arcade workload: the full coprocessor (spc700 + w65c816 + snes-ppu),
/// primed with a takeover-style program — mirrors
/// `lib_tests_ppu.rs`'s `captured_21xx_routes_and_frames_render` (the same
/// `$21xx` backdrop + INIDISP writes) plus a `nmi_counting_cop`-style
/// `WAI`-loop main so every vblank NMI re-touches `$2100`, keeping the
/// register-capture -> PPU-apply path (not just the scanline pump) live
/// every frame instead of running once and halting.
fn build_arcade(spc: &[u8], cpu: &[u8], ppu: &[u8]) -> SgbCoprocessor {
    let cop = SgbCoprocessor::from_wasm_full(spc, cpu, Some(ppu), DEFAULT_SAMPLE_RATE).unwrap();
    // main @ $9000: CGADD=0 / backdrop = $2A55 / NMITIMEN = $81 (NMI + autopoll)
    // / loop: WAI / BRA loop.
    let main = [
        0xA9, 0x00, 0x8D, 0x21, 0x21, // LDA #$00 / STA $2121 (CGADD)
        0xA9, 0x55, 0x8D, 0x22, 0x21, // LDA #$55 / STA $2122
        0xA9, 0x2A, 0x8D, 0x22, 0x21, // LDA #$2A / STA $2122 (backdrop $2A55)
        0xA9, 0x81, 0x8D, 0x00, 0x42, // LDA #$81 / STA $4200 (NMITIMEN)
        0xCB, 0x80, 0xFD, // loop: WAI / BRA loop
    ];
    // NMI body @ $9200: re-assert INIDISP (full brightness) every vblank, RTI.
    let nmi = [0xA9, 0x0F, 0x8D, 0x00, 0x21, 0x40]; // LDA #$0F / STA $2100 / RTI
    cop.debug_cpu_write(0x9000, &main);
    cop.debug_cpu_write(0x9200, &nmi);
    cop.debug_cpu_write(0xFFFA, &[0x00, 0x92]); // emulation NMI vector -> $9200
    cop.debug_cpu_set_pc(0x9000);
    cop
}

/// Run `frames` flat out, pulling output the way a real host would each
/// frame (audio drain for the plain workload, frame pull for arcade).
/// Returns the wall-clock duration.
fn run_flat_out(cop: &mut SgbCoprocessor, frames: u64, pull_frame: bool) -> std::time::Duration {
    let start = Instant::now();
    for _ in 0..frames {
        AudioCoprocessor::clock(cop, GB_FRAME_CYCLES);
        let _ = cop.drain_pcm();
        if pull_frame {
            let _ = cop.take_snes_frame();
        }
    }
    start.elapsed()
}

/// Time `RUNS` fresh instances of a workload for `frames` frames each and
/// return the median fps. `render_on` mirrors what a frontend would set via
/// `GameBoy::set_coprocessor_render` — `false` reproduces the fast-forward
/// path (`app_pacing::run_turbo`).
fn bench(
    frames: u64,
    pull_frame: bool,
    render_on: bool,
    mut build: impl FnMut() -> SgbCoprocessor,
) -> f64 {
    let mut fps: Vec<f64> = (0..RUNS)
        .map(|_| {
            let mut cop = build();
            AudioCoprocessor::set_render_enabled(&mut cop, render_on);
            let elapsed = run_flat_out(&mut cop, frames, pull_frame);
            frames as f64 / elapsed.as_secs_f64()
        })
        .collect();
    fps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    fps[fps.len() / 2]
}

fn main() {
    let mut args = env::args().skip(1);
    let plugins_dir = args
        .next()
        .filter(|s| !s.is_empty())
        .or_else(|| env::var("SLOPGB_PLUGINS_DIR").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"));
    let frames: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(600);

    let spc = read_plugin(&plugins_dir, SPC_WASM);
    let cpu = read_plugin(&plugins_dir, CPU_WASM);
    let ppu = read_plugin(&plugins_dir, PPU_WASM);

    println!(
        "{frames} frames/run, median of {RUNS} runs, plugins from {}",
        plugins_dir.display()
    );
    println!(
        "{:<40} {:>8} {:>10} {:>10}",
        "workload", "render", "fps", "x real-time"
    );
    let row = |name: &str,
               pull_frame: bool,
               render_on: bool,
               build: &mut dyn FnMut() -> SgbCoprocessor| {
        let fps = bench(frames, pull_frame, render_on, build);
        println!(
            "{:<40} {:>8} {:>10.1} {:>9.2}x",
            name,
            if render_on { "on" } else { "off" },
            fps,
            fps / REAL_FPS
        );
        fps
    };

    // Render ON is the pass condition's baseline (matches normal play —
    // `run_audio_paced`/`run_timer_paced` always restore it). Render OFF
    // reproduces the fast-forward path (`app_pacing::run_turbo`). Printing
    // both back to back, in the same process, keeps the on/off comparison
    // from a noisy shared machine's run-to-run jitter (each pair is measured
    // moments apart rather than across separate invocations).
    let plain_on = row("plain SGB (spc700+w65c816)", false, true, &mut || {
        build_plain(&spc, &cpu)
    });
    row("plain SGB (spc700+w65c816)", false, false, &mut || {
        build_plain(&spc, &cpu)
    });
    let arcade_on = row("arcade (spc700+w65c816+snes-ppu)", true, true, &mut || {
        build_arcade(&spc, &cpu, &ppu)
    });
    row("arcade (spc700+w65c816+snes-ppu)", true, false, &mut || {
        build_arcade(&spc, &cpu, &ppu)
    });

    // One loose, opt-in regression floor on the render-ON (baseline) medians
    // — unset by default, so a normal run never asserts:
    // `SLOPGB_MIN_FPS_RATIO=0.5` fails only if either drops under half its
    // recorded baseline ratio to real-time.
    if let Some(ratio) = env::var("SLOPGB_MIN_FPS_RATIO")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
    {
        let floor = ratio * REAL_FPS;
        assert!(
            plain_on >= floor && arcade_on >= floor,
            "throughput floor: plain {plain_on:.1} / arcade {arcade_on:.1} fps, floor {floor:.1}",
        );
    }
}
