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

/// The 3-byte HW_LINE form renders a whole span in one host call, exactly
/// like the equivalent sequence of single-line renders; out-of-range rows
/// clip at the frame bottom.
#[test]
fn hw_line_span_renders_like_single_lines() {
    let mut a = SnesPpuCop::new();
    let mut b = SnesPpuCop::new();
    for cop in [&mut a, &mut b] {
        cop.port_write(0x00, 0x0F);
        cop.port_write(0x05, 0x01);
        cop.port_write(0x2C, 0x01);
        cop.port_write(0x07, 0x04);
        cop.port_write(0x0B, 0x01);
        cop.port_write(0x15, 0x80);
        vram_word(cop, 0x400, 0x0002);
        vram_word(cop, 0x1000 + 2 * 16, 0x0080); // row 0 pixel 0
        vram_word(cop, 0x1000 + 2 * 16 + 1, 0x0040); // row 1 pixel 1
        cop.port_write(0x21, 0x01);
        cop.port_write(0x22, 0x1F);
        cop.port_write(0x22, 0x00);
    }
    for y in 0..4u16 {
        a.write_ram(HW_LINE, &y.to_le_bytes());
    }
    b.write_ram(HW_LINE, &[0, 0, 4]);
    assert_eq!(
        a.read_ram(HW_FB, 512 * 4),
        b.read_ram(HW_FB, 512 * 4),
        "span == singles"
    );
    // A span reaching past the last row clips instead of wrapping.
    b.write_ram(HW_LINE, &[220, 0, 40]);
    let tail = b.read_ram(HW_FB + 223 * 512, 4);
    assert_eq!(tail.len(), 4, "bottom row rendered, nothing wrapped");
}

/// The zero-copy framebuffer handoff must be byte-identical to the generic
/// [`Coprocessor::read_ram`] path it bypasses: wherever [`fb_words`] claims a
/// request, the little-endian image of those words is exactly the byte stream
/// `read_ram` would have built. Anything it declines still has to reach
/// `read_ram`, so this also pins which requests it declines.
#[test]
fn fb_word_range_is_byte_identical_to_read_ram() {
    let mut cop = SnesPpuCop::new();
    // A frame with distinct content per row + a non-symmetric backdrop, so a
    // wrong offset, length, or byte order cannot compare equal by accident.
    cop.port_write(0x00, 0x0F);
    for y in 0..FB_HEIGHT as u16 {
        cop.port_write(0x21, 0x00);
        cop.port_write(0x22, y as u8);
        cop.port_write(0x22, (0x2A ^ y >> 3) as u8);
        cop.write_ram(HW_LINE, &y.to_le_bytes());
    }

    let cases: &[(u32, usize)] = &[
        (HW_FB, FB_BYTES),            // the whole frame — the per-frame handoff
        (HW_FB, 0),                   // empty
        (HW_FB, 2),                   // one pixel
        (HW_FB + 5 * 512, 4),         // mid-frame, row-aligned
        (HW_FB + 512 * 223, 512),     // the last row
        (HW_FB + FB_BYTES as u32, 0), // exactly at the end
        (HW_FB + 1, 2),               // odd start: generic path
        (HW_FB, 3),                   // odd length: generic path
        (HW_FB, FB_BYTES + 2),        // past the end: generic path (zero-padded)
        (HW_FB + FB_BYTES as u32, 2), // wholly past the end
        (HW_FB - 2, 4),               // straddles the window base
        (0, 4),                       // outside the host window entirely
    ];
    assert!(
        fb_words(HW_FB, FB_BYTES).is_some(),
        "the once-per-vblank whole-frame pull must take the fast path"
    );
    for &(addr, len) in cases {
        let want = cop.read_ram(addr, len);
        if let Some(r) = fb_words(addr, len) {
            let got: Vec<u8> = cop.fb[r].iter().flat_map(|w| w.to_le_bytes()).collect();
            assert_eq!(got, want, "fast path differs at {addr:#X} len {len}");
        }
    }
}

/// The HW_PORTS window applies `(port, val)` pairs in order — one host
/// call standing in for a run of B-bus writes (a DMA's worth).
#[test]
fn hw_ports_batch_applies_pairs_in_order() {
    let mut a = SnesPpuCop::new();
    let mut b = SnesPpuCop::new();
    // Same VRAM upload: a via singles, b via one batch (VMAIN step-on-high,
    // address set, then data pairs — order matters).
    let singles: &[(u8, u8)] = &[
        (0x15, 0x80),
        (0x16, 0x00),
        (0x17, 0x04),
        (0x18, 0x34),
        (0x19, 0x12),
        (0x18, 0x78),
        (0x19, 0x56),
    ];
    for &(p, v) in singles {
        a.port_write(p, v);
    }
    let mut batch = Vec::new();
    for &(p, v) in singles {
        batch.extend_from_slice(&[p, v]);
    }
    b.write_ram(HW_PORTS, &batch);
    assert_eq!(a.save_state(), b.save_state(), "batch == singles");
}
