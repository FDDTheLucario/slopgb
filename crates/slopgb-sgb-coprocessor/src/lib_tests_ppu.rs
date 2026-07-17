//! The optional SNES-PPU plugin seam: absent = audio-only (unchanged),
//! present = captured `$21xx` writes + DMA B-bus bytes route to the PPU and
//! the clocking loop renders scanlines into a per-frame fetchable image.

use super::*;
use std::sync::OnceLock;

/// The snes-ppu plugin wasm (built once). `None` when unavailable → skip.
fn ppu_plugin() -> Option<Vec<u8>> {
    static CACHE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    CACHE
        .get_or_init(|| build("slopgb-snes-ppu-plugin", "slopgb_snes_ppu_plugin"))
        .clone()
}

/// A coprocessor with all three plugins loaded, or `None` to skip.
fn build_cop_ppu(rate: u32) -> Option<SgbCoprocessor> {
    let (spc, cpu) = plugins()?;
    let ppu = ppu_plugin()?;
    Some(SgbCoprocessor::from_wasm_full(&spc, &cpu, Some(&ppu), rate).unwrap())
}

/// Without a PPU plugin nothing changes: no frames ever, audio-only.
#[test]
fn absent_ppu_stays_audio_only() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    assert!(cop.take_snes_frame().is_none());
    cop.clock(70_224 * 2);
    assert!(cop.take_snes_frame().is_none(), "no PPU: never a frame");
}

/// Captured guest `$21xx` writes reach the PPU, the clocking loop renders
/// the frame, and `take_snes_frame` hands it out exactly once per vblank.
#[test]
fn captured_21xx_routes_and_frames_render() {
    let Some(mut cop) = build_cop_ppu(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        let prog = stores(
            &[
                (0x2121, 0x00), // CGADD = color 0 (the backdrop)
                (0x2122, 0x55), // backdrop = $2A55
                (0x2122, 0x2A),
                (0x2100, 0x0F), // INIDISP: full brightness
            ],
            &[0xDB],
        );
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224 * 2);
    let frame = cop.take_snes_frame().expect("a frame after vblank");
    assert_eq!(frame.len(), 256 * 224);
    assert_eq!(frame[0], 0x2A55, "the guest's backdrop rendered");
    assert_eq!(frame[255 + 223 * 256], 0x2A55, "last pixel too");
    assert!(cop.take_snes_frame().is_none(), "one fetch per frame");
    cop.clock(70_224);
    assert!(cop.take_snes_frame().is_some(), "the next vblank re-arms");
}

/// A GP-DMA with a `$21xx` B-bus target feeds the PPU through the same
/// consumer as captured CPU writes (here: two bytes into the CGRAM
/// write-twice port).
#[test]
fn dma_bbus_feeds_the_ppu() {
    let Some(mut cop) = build_cop_ppu(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9840, &[0x33, 0x11]).unwrap(); // backdrop $1133
        let prog = stores(
            &[
                (0x2121, 0x00), // CGADD = color 0
                (0x2100, 0x0F),
                (0x4300, 0x00), // A->B, increment, mode 0
                (0x4301, 0x22), // BBAD: $2122 (CGDATA)
                (0x4302, 0x40), // A1T = $9840
                (0x4303, 0x98),
                (0x4304, 0x00),
                (0x4305, 0x02), // 2 bytes
                (0x4306, 0x00),
                (0x420B, 0x01),
            ],
            &[0xDB],
        );
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224 * 2);
    let frame = cop.take_snes_frame().expect("frame");
    assert_eq!(frame[0], 0x1133, "the DMA'd palette word rendered");
}

/// `debug_status` (the MCP coprocessor tool's line) reports the PPU state:
/// absent = audio-only note; present = frame count + the guest's INIDISP.
#[test]
fn debug_status_reports_the_ppu_line() {
    let Some(cop) = build_cop(48_000) else {
        return;
    };
    assert!(cop.debug_status().contains("no SNES PPU plugin"));

    let Some(mut cop) = build_cop_ppu(48_000) else {
        return;
    };
    assert!(cop.debug_status().contains("0 frames rendered"));
    {
        let mut cpu = cop.cpu.borrow_mut();
        let prog = stores(&[(0x2100, 0x8F)], &[0xDB]); // forced blank
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224 * 2);
    let status = cop.debug_status();
    assert!(status.contains("frames rendered"), "{status}");
    assert!(!status.contains("0 frames rendered"), "{status}");
    assert!(status.contains("INIDISP $8F"), "{status}");
}

/// The PPU block rides the coprocessor save state: a frame's registers
/// survive save/load and the restored machine keeps rendering them.
#[test]
fn ppu_state_rides_the_save_state() {
    let Some(mut cop) = build_cop_ppu(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        let prog = stores(
            &[
                (0x2121, 0x00),
                (0x2122, 0x77),
                (0x2122, 0x08),
                (0x2100, 0x0F),
            ],
            &[0xDB],
        );
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224 * 2);

    let mut w = Writer::new();
    cop.write_state(&mut w);
    let bytes = w.into_vec();

    let Some(mut restored) = build_cop_ppu(48_000) else {
        return;
    };
    let mut r = Reader::new(&bytes);
    restored.read_state(&mut r).unwrap();
    let mut w2 = Writer::new();
    restored.write_state(&mut w2);
    assert_eq!(bytes, w2.into_vec(), "byte-identical re-serialization");

    restored.clock(70_224);
    let frame = restored.take_snes_frame().expect("frame after restore");
    assert_eq!(frame[0], 0x0877, "restored CGRAM still renders");
}
