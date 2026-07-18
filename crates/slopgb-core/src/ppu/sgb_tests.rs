//! Unit tests for the SGB presentation parser ([`SgbView::sgb_command`]).
//! Packet layouts cite Pan Docs "SGB Command $xx".

use super::*;

/// Build a 16-byte command packet: `header` (command × 8 + length) then the
/// data bytes, zero-padded.
fn packet(header: u8, body: &[u8]) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0] = header;
    p[1..1 + body.len()].copy_from_slice(body);
    p
}

#[test]
fn bgr555_expands_like_cgb() {
    // 5-bit 31 → 0xFF, 5-bit 1 → 0x08 (straight (c<<3)|(c>>2) fill).
    assert_eq!(bgr555(0x1F, 0x00), 0xFF_0000); // R=31
    assert_eq!(bgr555(0xE0, 0x03), 0x00_FF00); // G=31
    assert_eq!(bgr555(0x00, 0x7C), 0x00_00FF); // B=31
    assert_eq!(bgr555(0xFF, 0x7F), 0xFF_FFFF); // all 31
    assert_eq!(bgr555(0x01, 0x00), 0x08_0000); // R=1
}

/// PAL01 ($00): color 0 is the shared entry-0 of all four palettes; colors
/// 1-3 fill palette 0, colors 4-6 fill palette 1; palettes 2/3 keep the DMG
/// default. (Pan Docs "SGB Command $00 — PAL01".)
#[test]
fn pal01_sets_two_palettes_and_shared_bg() {
    let mut s = SgbView::new();
    // 7 BGR555 colors: bg=red, then pal0 = {green, blue, white}, pal1 = R/G/B=1.
    s.sgb_command(&packet(
        0x01,
        &[
            0x1F, 0x00, // color 0 (shared bg) = red
            0xE0, 0x03, // color 1 (pal0 e1) = green
            0x00, 0x7C, // color 2 (pal0 e2) = blue
            0xFF, 0x7F, // color 3 (pal0 e3) = white
            0x01, 0x00, // color 4 (pal1 e1)
            0x20, 0x00, // color 5 (pal1 e2)
            0x00, 0x04, // color 6 (pal1 e3)
        ],
    ));
    assert_eq!(s.pal[0], [0xFF_0000, 0x00_FF00, 0x00_00FF, 0xFF_FFFF]);
    assert_eq!(s.pal[1], [0xFF_0000, 0x08_0000, 0x00_0800, 0x00_0008]);
    // Unnamed palettes: shared bg in entry 0, DMG default in 1-3.
    assert_eq!(s.pal[2], [0xFF_0000, 0xAA_AAAA, 0x55_5555, 0x00_0000]);
    assert_eq!(s.pal[3], [0xFF_0000, 0xAA_AAAA, 0x55_5555, 0x00_0000]);
}

/// PAL12 ($03) writes palettes 1 and 2 (not 0/1): guards the a/b table.
#[test]
fn pal12_targets_palettes_1_and_2() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(
        0x03 * 8 + 1,
        &[
            0x1F, 0x00, 0xE0, 0x03, 0xE0, 0x03, 0xE0, 0x03, 0x00, 0x7C, 0x00, 0x7C, 0x00, 0x7C,
        ],
    ));
    assert_eq!(s.pal[1][1], 0x00_FF00, "pal1 gets colors 1-3 (green)");
    assert_eq!(s.pal[2][1], 0x00_00FF, "pal2 gets colors 4-6 (blue)");
    // Palette 0 untouched except the shared entry 0.
    assert_eq!(s.pal[0], [0xFF_0000, 0xAA_AAAA, 0x55_5555, 0x00_0000]);
}

/// ATTR_BLK ($04): a 5,5..10,10 rect recolors inside/border/outside cells by
/// their control bits. (Pan Docs "SGB Command $04 — ATTR_BLK".)
#[test]
fn attr_blk_fills_inside_border_outside() {
    let mut s = SgbView::new();
    // control = 0b111 (all three), palettes: inside 1, border 2, outside 3.
    let pals = (3 << 4) | (2 << 2) | 1;
    s.sgb_command(&packet(0x04 * 8 + 1, &[1, 0b111, pals, 5, 5, 10, 10]));
    let at = |cx: usize, cy: usize| s.attr[cy * 20 + cx];
    assert_eq!(at(7, 7), 1, "strictly inside");
    assert_eq!(at(5, 7), 2, "on the left border (cx == x1)");
    assert_eq!(at(5, 5), 2, "corner is on-border");
    assert_eq!(at(0, 0), 3, "outside");
    assert_eq!(at(10, 10), 2, "bottom-right corner on-border");
}

/// ATTR_BLK honours only the region control bits that are set. control = 0b101
/// (inside + outside, no border): the border stays palette 0 — and because both
/// inside and outside are set, neither implicit-border promotion fires.
#[test]
fn attr_blk_skips_regions_without_control_bit() {
    let mut s = SgbView::new();
    let pals = (3 << 4) | (2 << 2) | 1;
    s.sgb_command(&packet(0x04 * 8 + 1, &[1, 0b101, pals, 5, 5, 10, 10]));
    assert_eq!(s.attr[7 * 20 + 7], 1, "inside recolored");
    assert_eq!(s.attr[0], 3, "outside recolored");
    assert_eq!(
        s.attr[7 * 20 + 5],
        0,
        "border untouched (bit1 clear, no promotion)"
    );
}

/// SameBoy quirk: an inside-only ATTR_BLK also recolors the block's border with
/// the inside palette; outside-only likewise uses the outside palette (SameBoy
/// `command_ready` ATTR_BLK; SGB dev manual "surrounding line treated as
/// inside").
#[test]
fn attr_blk_inside_only_promotes_border() {
    let mut s = SgbView::new();
    let pals = (3 << 4) | (2 << 2) | 1;
    s.sgb_command(&packet(0x04 * 8 + 1, &[1, 0b001, pals, 5, 5, 10, 10]));
    assert_eq!(s.attr[7 * 20 + 7], 1, "inside recolored");
    assert_eq!(
        s.attr[7 * 20 + 5],
        1,
        "border promoted to the inside palette"
    );
    assert_eq!(s.attr[0], 0, "outside still untouched");
}

/// MASK_EN ($17): byte 1 low 2 bits select freeze/black/color-0/cancel.
/// (Pan Docs "SGB Command $17 — MASK_EN".)
#[test]
fn mask_en_modes() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(0x17 * 8 + 1, &[1]));
    assert!(s.holds_frame(), "freeze holds the last frame");
    assert_eq!(s.mask_fill(), None);

    s.sgb_command(&packet(0x17 * 8 + 1, &[2]));
    assert!(!s.holds_frame());
    assert_eq!(s.mask_fill(), Some(0x00_0000), "black fill");

    s.sgb_command(&packet(0x17 * 8 + 1, &[3]));
    assert_eq!(s.mask_fill(), Some(s.pal[0][0]), "palette-0 color-0 fill");

    s.sgb_command(&packet(0x17 * 8 + 1, &[0]));
    assert!(!s.holds_frame());
    assert_eq!(s.mask_fill(), None, "cancel");
}

/// A malformed (short) command is ignored, not a panic.
#[test]
fn short_command_ignored() {
    let mut s = SgbView::new();
    s.sgb_command(&[0x01, 0x1F, 0x00]);
    assert_eq!(
        s.pal[0],
        [DMG_SHADES[0], DMG_SHADES[1], DMG_SHADES[2], DMG_SHADES[3]]
    );
}

/// Palettes, attribute map and mask survive a save-state round-trip.
#[test]
fn state_round_trips() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(
        0x01,
        &[
            0x1F, 0x00, 0xE0, 0x03, 0x00, 0x7C, 0xFF, 0x7F, 0x01, 0x00, 0x20, 0x00, 0x00, 0x04,
        ],
    ));
    let pals = (3 << 4) | (2 << 2) | 1;
    s.sgb_command(&packet(0x04 * 8 + 1, &[1, 0b111, pals, 5, 5, 10, 10]));
    s.sgb_command(&packet(0x17 * 8 + 1, &[1]));
    // Exercise the transfer / border / sound / flag state too.
    s.shade_buf[0] = 3;
    s.sgb_command(&packet(0x0B * 8 + 1, &[])); // PAL_TRN
    s.run_pending_transfer();
    s.has_chr = true;
    s.has_pct = true;
    s.sgb_command(&packet(0x08 * 8 + 1, &[1, 2, 3, 4])); // SOUND
    s.sgb_command(&packet(0x0C * 8 + 1, &[1])); // ATRC_EN
    s.sgb_command(&packet(0x12 * 8 + 1, &[0x34, 0x12, 0x01])); // JUMP

    let mut w = crate::state::Writer::new();
    s.write_state(&mut w);
    let bytes = w.into_vec();
    let mut t = SgbView::new();
    let mut r = crate::state::Reader::new(&bytes);
    t.read_state(&mut r).unwrap();

    assert_eq!(t.pal, s.pal);
    assert_eq!(t.attr, s.attr);
    assert_eq!(t.mask, s.mask);
    assert_eq!(t.ram_palettes, s.ram_palettes);
    assert_eq!(t.has_chr, s.has_chr);
    assert_eq!(t.atrc_en, s.atrc_en);
    assert_eq!(t.jump, s.jump);
    assert_eq!(t.sound_events, s.sound_events);
    assert_eq!(t.shade_buf, s.shade_buf);
}

// ---- Attribute fills ($05-$07, $16) ----

/// ATTR_LIN ($05): a horizontal entry recolors a whole row, a vertical one a
/// whole column. (SameBoy `ATTR_LIN`.)
#[test]
fn attr_lin_rows_and_columns() {
    let mut s = SgbView::new();
    // horizontal row 3 → palette 2 (0x80|2<<5|3 = 0xC3); vertical col 5 →
    // palette 1 (1<<5|5 = 0x25).
    s.sgb_command(&packet(0x05 * 8 + 1, &[2, 0xC3, 0x25]));
    assert_eq!(s.attr[3 * 20], 2, "row 3 recolored");
    assert_eq!(s.attr[3 * 20 + 19], 2);
    assert_eq!(s.attr[5], 1, "col 5 recolored");
    assert_eq!(s.attr[17 * 20 + 5], 1);
    assert_eq!(s.attr[0], 0, "untouched cell stays 0");
}

/// ATTR_DIV ($06): a horizontal split at row 4 → low palette above, middle on
/// the line, high below. (SameBoy `ATTR_DIV`.)
#[test]
fn attr_div_three_regions() {
    let mut s = SgbView::new();
    // high=1, low=2, middle=3, horizontal (bit6), line=4.
    let b1 = 1 | (2 << 2) | (3 << 4) | 0x40;
    s.sgb_command(&packet(0x06 * 8 + 1, &[b1, 4]));
    assert_eq!(s.attr[0], 2, "above the line = low");
    assert_eq!(s.attr[4 * 20], 3, "on the line = middle");
    assert_eq!(s.attr[5 * 20], 1, "below the line = high");
}

/// ATTR_CHR ($07): per-cell writes from (0,0), left→right then down.
#[test]
fn attr_chr_horizontal_order() {
    let mut s = SgbView::new();
    // start (0,0), count 4, direction 0, one data byte 0b00_01_10_11 → cells
    // get high pair first: 0,1,2,3.
    s.sgb_command(&packet(0x07 * 8 + 1, &[0, 0, 4, 0, 0, 0b00_01_10_11]));
    assert_eq!([s.attr[0], s.attr[1], s.attr[2], s.attr[3]], [0, 1, 2, 3]);
    assert_eq!(s.attr[4], 0, "past count untouched");
}

/// ATTR_CHR vertical direction wraps a column then advances x.
#[test]
fn attr_chr_vertical_wraps() {
    let mut s = SgbView::new();
    // start (0,0), count 20 (18 down col 0, then 2 into col 1), direction 1.
    let mut body = vec![0u8, 0, 20, 0, 1];
    body.extend(std::iter::repeat_n(0xFF, 5)); // all palette 3
    s.sgb_command(&packet(0x07 * 8 + 1, &body));
    assert_eq!(s.attr[17 * 20], 3, "bottom of col 0");
    assert_eq!(s.attr[1], 3, "wrapped to col 1 top");
}

/// ATTR_SET ($16): loads an ATTR_TRN file and cancels the mask when bit6 set.
#[test]
fn attr_set_loads_file_and_cancels_mask() {
    let mut s = SgbView::new();
    // File 0: first byte 0b11_10_01_00 → cells 0..4 = 3,2,1,0 (high pair first).
    s.attr_files[0] = 0b11_10_01_00;
    s.mask = 1; // frozen
    s.sgb_command(&packet(0x16 * 8 + 1, &[0x40])); // file 0, bit6 = cancel mask
    assert_eq!([s.attr[0], s.attr[1], s.attr[2], s.attr[3]], [3, 2, 1, 0]);
    assert_eq!(s.mask, 0, "bit6 cancels MASK_EN");
}

// ---- Palette RAM select / transfer ($0A/$0B) ----

/// PAL_SET ($0A) selects palettes from PAL_TRN RAM; entry 0 is the shared
/// background (palette-0 color 0). (SameBoy `PAL_SET`.)
#[test]
fn pal_set_selects_from_ram() {
    let mut s = SgbView::new();
    // ram palette 0 = {red, green, blue, white}; palette 1 = 4 whites.
    let put = |ram: &mut [u8], i: usize, v: u16| {
        ram[i * 2] = v as u8;
        ram[i * 2 + 1] = (v >> 8) as u8;
    };
    put(&mut s.ram_palettes[..], 0, 0x001F); // red
    put(&mut s.ram_palettes[..], 1, 0x03E0); // green
    put(&mut s.ram_palettes[..], 2, 0x7C00); // blue
    put(&mut s.ram_palettes[..], 3, 0x7FFF); // white
    for c in 4..8 {
        put(&mut s.ram_palettes[..], c, 0x7FFF);
    }
    // Select ram palette 0 into SGB pal 0, ram palette 1 into pal 1-3.
    s.sgb_command(&packet(0x0A * 8 + 1, &[0, 0, 1, 0, 1, 0, 1, 0, 0]));
    assert_eq!(s.pal[0][1], 0x00_FF00, "pal0 e1 = green");
    assert_eq!(s.pal[0][3], 0xFF_FFFF, "pal0 e3 = white");
    assert_eq!(s.pal[0][0], 0xFF_0000, "shared bg = ram pal0 color0 (red)");
    assert_eq!(
        s.pal[1][0], 0xFF_0000,
        "shared bg replicated to all palettes"
    );
}

/// PAL_TRN ($0B) decodes the rendered screen shades into palette RAM: a screen
/// row's 8 pixels encode one BGR555 color (SameBoy `GB_sgb_render`).
#[test]
fn pal_trn_decodes_screen_shades() {
    let mut s = SgbView::new();
    // Encode BGR555 0x8055 in tile 0 row 0: lo = 0x55, hi = 0x80. Pixel x's
    // bit0 = lo bit (7-x), bit1 = hi bit (7-x).
    let (lo, hi) = (0x55u8, 0x80u8);
    for x in 0..8 {
        let b0 = (lo >> (7 - x)) & 1;
        let b1 = (hi >> (7 - x)) & 1;
        s.shade_buf[x] = b0 | (b1 << 1);
    }
    s.sgb_command(&packet(0x0B * 8 + 1, &[])); // PAL_TRN opens a capture window
    assert_eq!(s.pending_transfer, Some(TR_PAL));
    s.run_pending_transfer();
    assert_eq!(s.ram_palettes[0], lo, "color low byte from bit0 plane");
    assert_eq!(s.ram_palettes[1], hi, "color high byte from bit1 plane");
    assert_eq!(s.pending_transfer, None, "transfer consumed");
}

// ---- Sound / data / flags / jump ----

/// SOUND ($08) queues an effect event; the host drains it.
#[test]
fn sound_event_queues_and_drains() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(0x08 * 8 + 1, &[0x11, 0x22, 0x33, 0x44]));
    assert_eq!(
        s.take_sound_event(),
        Some(crate::SgbSound {
            effect_a: 0x11,
            effect_b: 0x22,
            attenuation: 0x33,
            effect_bank: 0x44,
        })
    );
    assert_eq!(s.take_sound_event(), None, "queue drained");
}

/// DATA_SND ($0F) stores an inline SNES-RAM write for the host to consume.
#[test]
fn data_snd_stores_packet() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(0x0F * 8 + 1, &[0xAB, 0xCD]));
    let got = s.take_data_snd().expect("packet stored");
    assert_eq!(got[0], 0xAB);
    assert_eq!(got[1], 0xCD);
    assert_eq!(s.take_data_snd(), None);
}

/// The flag commands ($0C-$0E/$19) and JUMP ($12) are stored and exposed.
#[test]
fn flags_and_jump_stored() {
    let mut s = SgbView::new();
    s.sgb_command(&packet(0x0C * 8 + 1, &[1])); // ATRC_EN
    s.sgb_command(&packet(0x0E * 8 + 1, &[1])); // ICON_EN
    s.sgb_command(&packet(0x19 * 8 + 1, &[1])); // PAL_PRI
    s.sgb_command(&packet(0x12 * 8 + 1, &[0x00, 0x80, 0x01])); // JUMP → 0x018000
    let f = s.flags();
    assert!(f.atrc_en && f.icon_en && f.pal_pri);
    assert!(!f.test_en);
    assert_eq!(f.jump, Some(0x01_8000));
}

/// A queue never grows without bound: `SOUND_QUEUE_CAP` events are retained.
#[test]
fn sound_queue_is_capped() {
    let mut s = SgbView::new();
    for i in 0..(SOUND_QUEUE_CAP + 10) {
        s.sgb_command(&packet(0x08 * 8 + 1, &[i as u8, 0, 0, 0]));
    }
    assert_eq!(s.sound_events.len(), SOUND_QUEUE_CAP);
    // The oldest were dropped: the first retained is event #10.
    assert_eq!(s.take_sound_event().unwrap().effect_a, 10);
}

// ---- Border composite ($13/$14 → sgb_border) ----

/// The border surface stays `None` until both a CHR_TRN and a PCT_TRN land; then
/// tiles composite around the GB inset, and a color-0 tile over the GB area is
/// transparent (the inset shows through). (SameBoy `GB_sgb_render` border loop.)
#[test]
fn border_composites_after_chr_and_pct() {
    let mut ppu = Ppu::new(Model::Sgb);
    // An SGB always has a border now — the built-in default until the ROM
    // sends its own CHR_TRN+PCT_TRN (below).
    assert!(
        ppu.sgb_border().is_some(),
        "default border present at power-on"
    );
    assert!(
        !ppu.sgb.as_ref().unwrap().border_ready(),
        "but not a ROM border yet"
    );

    let inset0 = 0x12_3456;
    ppu.front[0] = inset0; // GB screen top-left
    let s = ppu.sgb.as_mut().unwrap();

    // Border tile 0 = solid color 1 (plane0 set on every row, planes 1-3 clear).
    for row in 0..8 {
        s.border_tiles[row * 2] = 0xFF;
    }
    // Border palette 4 (index base 0) color 1 = red (BGR555 0x001F) at raw 0x800.
    s.border_raw[2048 + 2] = 0x1F;
    s.border_raw[2048 + 3] = 0x00;
    // Tilemap: non-gb-area tile (0,0) → tile 0 pal 0; gb-area corner (6,5) →
    // tile 1 (all zeros → color 0, transparent).
    // entry (0,0) at raw[0..2] = 0x0000 (tile 0). entry (6,5) at (6+5*32)*2.
    let e = (6 + 5 * 32) * 2;
    s.border_raw[e] = 1; // tile index 1
    s.has_chr = true;
    s.has_pct = true;

    ppu.sgb_composite_border();
    let b = ppu.sgb_border().expect("border ready");
    assert_eq!(b[0], 0xFF_0000, "outside tile drawn red (color 1, pal 4)");
    // GB inset top-left sits at (INSET_X, INSET_Y); the color-0 gb-area tile is
    // transparent so the inset shows.
    let inset_idx = crate::SGB_BORDER_W * 40 + 48;
    assert_eq!(b[inset_idx], inset0, "gb inset shows through color-0 tile");
}

// ---- Default border + boot intro / cross-fade ($01/§Phase-1b) ----

/// The original default border is drawn from power-on: the live GB screen is
/// blitted into the inset and the surrounding area is the (non-inset) frame.
#[test]
fn default_border_draws_frame_and_inset() {
    let mut ppu = Ppu::new(Model::Sgb);
    let inset = 0x11_2233;
    ppu.front.fill(inset);
    ppu.sgb_composite_border();
    let b = ppu
        .sgb_border()
        .expect("default border always present on SGB");
    let at = |x: usize, y: usize| b[y * crate::SGB_BORDER_W + x];
    assert_eq!(at(48, 40), inset, "inset top-left shows the GB screen");
    assert_eq!(
        at(207, 183),
        inset,
        "inset bottom-right shows the GB screen"
    );
    assert_ne!(
        at(0, 0),
        inset,
        "the border corner is the default frame, not the inset"
    );
}

/// Boot intro: the default border fades up from black. The first presented
/// frame is dimmer than the settled frame; after `FADE_LEN` boundaries it is
/// fully settled (the inset shows its true colour).
#[test]
fn boot_intro_fades_in_from_black() {
    let mut ppu = Ppu::new(Model::Sgb);
    ppu.front.fill(0xFF_FFFF); // white inset → easy brightness check
    let inset_idx = 40 * crate::SGB_BORDER_W + 48;

    ppu.sgb_frame_boundary(); // frame 1 of the fade
    let after_first = ppu.sgb_border().unwrap()[inset_idx];
    assert_ne!(
        after_first, 0xFF_FFFF,
        "first frame is mid-fade (not full brightness)"
    );
    assert!(after_first < 0xFF_FFFF, "fading up from black");

    for _ in 1..FADE_LEN {
        ppu.sgb_frame_boundary();
    }
    assert_eq!(
        ppu.sgb_border().unwrap()[inset_idx],
        0xFF_FFFF,
        "settled after FADE_LEN frames"
    );
    assert_eq!(ppu.sgb.as_ref().unwrap().fade, 0, "fade counter exhausted");
}

/// A ROM border transfer (CHR_TRN+PCT_TRN) restarts the fade — the cross-fade
/// from the previous (default) border to the new one.
#[test]
fn border_transfer_restarts_crossfade() {
    let mut ppu = Ppu::new(Model::Sgb);
    ppu.front.fill(0x40_4040);
    // Settle the boot intro first.
    for _ in 0..FADE_LEN {
        ppu.sgb_frame_boundary();
    }
    assert_eq!(ppu.sgb.as_ref().unwrap().fade, 0, "boot fade settled");

    // A CHR_TRN and a PCT_TRN land, then the frame boundary consumes them.
    {
        let s = ppu.sgb.as_mut().unwrap();
        s.sgb_command(&packet(0x13 * 8 + 1, &[0])); // CHR_TRN bank 0
        s.run_pending_transfer();
        s.sgb_command(&packet(0x14 * 8 + 1, &[])); // PCT_TRN
        s.run_pending_transfer();
        assert!(s.fade_pending, "border transfer flags a cross-fade");
    }
    ppu.sgb_frame_boundary();
    assert_eq!(
        ppu.sgb.as_ref().unwrap().fade,
        FADE_LEN - 1,
        "cross-fade started and stepped once"
    );
}

/// A `*_TRN` command captures the screen one frame after the command, on the
/// free-running SNES-side capture clock — not at the GB's next line-144 (an
/// LCD-off window can skip that entirely, losing the screen) and not at
/// command time (a game may still be streaming the payload when the command
/// completes: Space Invaders sends DATA_TRN mid-redraw and relies on the
/// following-frame capture).
#[test]
fn trn_captures_one_frame_after_the_command() {
    let mut s = SgbView::new();
    s.shade_buf.fill(2);
    s.sgb_command(&packet(0x10 * 8 + 1, &[0, 0x01, 0x7F])); // DATA_TRN #1
    assert!(s.data_trn_data().is_none(), "no capture at command time");
    // The GB finishes streaming the real payload inside the window.
    s.shade_buf.fill(1);
    s.tick_trn(70_223);
    assert!(s.data_trn_data().is_none(), "window still open");
    s.tick_trn(1);
    let first = s.data_trn_data().expect("captured at window end").to_vec();
    assert_eq!(first[0], 0xFF, "shade 1 = low bitplane set");
    assert_eq!(first[1], 0x00, "shade 1 = high bitplane clear");
    // The next command opens its own window; the GB never reaches line 144
    // in between and the capture still happens.
    s.sgb_command(&packet(0x10 * 8 + 1, &[0, 0x11, 0x7F])); // DATA_TRN #2
    s.shade_buf.fill(2);
    s.tick_trn(70_224);
    let second = s.data_trn_data().expect("second screen captured");
    assert_eq!(second[0], 0x00, "shade 2 = low bitplane clear");
    assert_eq!(second[1], 0xFF, "shade 2 = high bitplane set");
}
