//! Super Game Boy colorization.

use super::*;

/// End-to-end SGB wiring: a PAL01 packet driven through the real `Joypad`
/// reaches the PPU and recolors the rendered DMG output — proving the
/// joypad → interconnect → ppu → render path (Pan Docs "SGB Command $00").
#[test]
fn sgb_pal01_colorizes_rendered_frame() {
    let mut rom = vec![0u8; 0x8000]; // ROM-only, NOP sled; CPU never touches IO
    rom[0x146] = 0x03; // SGB flag — both bytes required for `supports_sgb`
    rom[0x14B] = 0x33; // old licensee code
    let mut gb = GameBoy::new(Model::Sgb, rom).unwrap();

    // PAL01 with shared background color 0 = red (BGR555 0x001F).
    let mut packet = [0u8; 16];
    packet[0] = 0x01; // command 0 (PAL01), length 1
    packet[1] = 0x1F; // color 0 lo (R = 31)
    send_sgb_packet(&mut gb, &packet);

    // BG-only with empty VRAM → every pixel is BG color 0 → BGP shade 0 →
    // palette-0 entry 0 = the shared background just installed.
    gb.debug_write(0xFF47, 0xE4); // BGP: color 0 → shade 0
    gb.debug_write(0xFF40, 0x91); // LCDC: LCD on, BG on, 8000 tile data
    for _ in 0..3 {
        gb.run_frame(); // first frame after LCD enable is skipped (LCDC.7)
    }
    assert_eq!(
        gb.frame()[0],
        0xFF_0000,
        "top-left pixel takes the SGB-provided background color"
    );
}
