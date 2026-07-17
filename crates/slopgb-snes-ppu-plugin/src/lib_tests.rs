//! Native tests for the S-PPU coprocessor wrapper: the Coprocessor logic is
//! target-independent, so these drive it directly (no wasm boundary). The
//! wasm-crossing proof is `slopgb-plugin-host`'s `snes_ppu_roundtrip`.

use super::*;

/// Write one VRAM word through the `$2118/$2119` ports.
fn vram_word(cop: &mut SnesPpuCop, addr: u16, word: u16) {
    cop.port_write(0x16, addr as u8);
    cop.port_write(0x17, (addr >> 8) as u8);
    cop.port_write(0x18, word as u8);
    cop.port_write(0x19, (word >> 8) as u8);
}

/// Build a one-tile mode-1 scene through the B-bus ports alone, render a
/// line through the host window, and read the pixel back out of the
/// framebuffer — the whole guest-visible surface end to end.
#[test]
fn ports_render_line_and_framebuffer_read() {
    let mut cop = SnesPpuCop::new();
    cop.port_write(0x00, 0x0F); // INIDISP: full brightness
    cop.port_write(0x05, 0x01); // mode 1
    cop.port_write(0x2C, 0x01); // TM: BG1
    cop.port_write(0x07, 0x04); // BG1 map at word $400
    cop.port_write(0x0B, 0x01); // BG1 tiles at word $1000
    cop.port_write(0x15, 0x80); // VMAIN: word writes, step 1
    vram_word(&mut cop, 0x400, 0x0002); // map (0,0) = char 2
    vram_word(&mut cop, 0x1000 + 2 * 16, 0x0080); // pixel 0 = color 1
    cop.port_write(0x21, 0x01); // CGADD = color 1
    cop.port_write(0x22, 0x1F); // red
    cop.port_write(0x22, 0x00);

    cop.write_ram(HW_LINE, &[0, 0]);
    let px = cop.read_ram(HW_FB, 4);
    assert_eq!(&px[..2], &[0x1F, 0x00], "pixel 0 is CGRAM color 1");
    assert_eq!(&px[2..], &[0x00, 0x00], "pixel 1 transparent -> backdrop 0");

    // An unrendered row reads zeros; row offsets are 512 bytes.
    let row1 = cop.read_ram(HW_FB + 512, 2);
    assert_eq!(row1, vec![0, 0]);

    // The passive clock absorbs run_until spans.
    assert_eq!(cop.run_until(1000), 1000);
}

/// Save/load round-trips the chip and the framebuffer; reset clears both.
#[test]
fn state_round_trip_and_reset() {
    let mut cop = SnesPpuCop::new();
    cop.port_write(0x00, 0x0F);
    cop.port_write(0x21, 0x00);
    cop.port_write(0x22, 0x55);
    cop.port_write(0x22, 0x2A); // backdrop = $2A55
    cop.write_ram(HW_LINE, &[10, 0]);
    let state = cop.save_state();
    assert_eq!(state.len(), STATE_LEN);

    let mut fresh = SnesPpuCop::new();
    fresh.load_state(&state);
    assert_eq!(
        fresh.read_ram(HW_FB + 10 * 512, 2),
        vec![0x55, 0x2A],
        "framebuffer row restored"
    );
    assert_eq!(fresh.save_state(), state, "byte-identical re-serialization");

    fresh.reset();
    assert_eq!(fresh.read_ram(HW_FB + 10 * 512, 2), vec![0, 0]);
    assert_eq!(fresh.save_state().len(), STATE_LEN);
}
