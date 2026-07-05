//! Throwaway: boot a cart through a boot ROM vs. the direct post-boot install,
//! report hand-off + dump both framebuffers (PPM) to compare.
//! `cargo run -p slopgb-core --example boot_check -- <boot.bin> <rom> <frames>`
use slopgb_core::{GameBoy, Model, SCREEN_H, SCREEN_W};

fn dump(gb: &GameBoy, path: &str) {
    let f = gb.frame();
    let mut out = format!("P6\n{SCREEN_W} {SCREEN_H}\n255\n").into_bytes();
    for &px in f.iter() {
        out.push((px >> 16) as u8);
        out.push((px >> 8) as u8);
        out.push(px as u8);
    }
    std::fs::write(path, out).unwrap();
}

fn main() {
    let mut a = std::env::args().skip(1);
    let boot = std::fs::read(a.next().expect("boot")).expect("read boot");
    let rom = std::fs::read(a.next().expect("rom")).expect("read rom");
    let frames: u32 = a.next().map_or(30, |s| s.parse().unwrap());

    let mut gb = GameBoy::new_with_boot(Model::Cgb, rom.clone(), boot).expect("build");
    gb.set_sample_rate(44100);
    let mut handed = None;
    let mut audio: Vec<(f32, f32)> = Vec::new();
    let mut peak = 0.0f32;
    for fr in 0..frames {
        gb.run_frame();
        audio.clear();
        gb.drain_audio(&mut audio);
        for (l, r) in &audio {
            peak = peak.max(l.abs()).max(r.abs());
        }
        if !gb.boot_active() && handed.is_none() {
            handed = Some((fr, gb.cpu_regs()));
        }
    }
    println!("audio peak amplitude during boot: {peak:.4} ({})",
        if peak > 0.01 { "chime audible" } else { "SILENT" });
    if let Some((fr, r)) = handed {
        println!(
            "handed off at frame {fr}: pc={:04X} a={:02X} bc={:04X} de={:04X} hl={:04X} sp={:04X}",
            r.pc, r.a, r.bc(), r.de(), r.hl(), r.sp
        );
    } else {
        println!("RESULT: FAIL — boot ROM never handed off (locked up?)");
    }
    dump(&gb, "/tmp/boot.ppm");

    // Baseline: the same cart from the direct post-boot install (no boot ROM).
    let mut base = GameBoy::new(Model::Cgb, rom).expect("base");
    for _ in 0..frames {
        base.run_frame();
    }
    dump(&base, "/tmp/base.ppm");

    let (a, b) = (gb.frame(), base.frame());
    let diff = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    println!(
        "framebuffer diff vs direct-boot: {diff}/{} pixels  (boot pc={:04X})",
        a.len(),
        gb.cpu_regs().pc
    );
    println!(
        "RESULT: {}",
        if gb.boot_active() {
            "FAIL (no handoff)"
        } else if diff == 0 {
            "OK (handed off; identical to direct boot)"
        } else {
            "handed off; screen differs from direct boot (see /tmp/boot.ppm vs base.ppm)"
        }
    );
}
