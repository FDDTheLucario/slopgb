//! Super Game Boy colorization.

use super::*;

/// A stub coprocessor that records every raw packet its `poll` drains.
#[derive(Clone)]
struct TeeCop(std::rc::Rc<std::cell::RefCell<Vec<[u8; 16]>>>);

impl sgb::AudioCoprocessor for TeeCop {
    fn clock(&mut self, _gb_cycles: u64) {}
    fn poll(&mut self, cmds: &mut dyn sgb::SgbCommandSource) {
        while let Some(p) = cmds.take_packet() {
            self.0.borrow_mut().push(p);
        }
    }
    fn mix_into(&mut self, _out: &mut [(f32, f32)]) {}
    fn set_output_rate(&mut self, _hz: u32) {}
    fn load_bios(&mut self, _bios: &[u8]) {}
    fn write_state(&self, _w: &mut crate::state::Writer) {}
    fn read_state(&mut self, _r: &mut crate::state::Reader<'_>) -> Result<(), StateError> {
        Ok(())
    }
    fn clone_box(&self) -> Box<dyn sgb::AudioCoprocessor> {
        Box::new(self.clone())
    }
}

/// A stub coprocessor recording what the GB→SNES input push delivers.
#[derive(Clone)]
struct InputCop(std::rc::Rc<std::cell::RefCell<Vec<(u8, u8)>>>);

impl sgb::AudioCoprocessor for InputCop {
    fn clock(&mut self, _gb_cycles: u64) {}
    fn poll(&mut self, _cmds: &mut dyn sgb::SgbCommandSource) {}
    fn set_input(&mut self, dpad: u8, buttons: u8) {
        self.0.borrow_mut().push((dpad, buttons));
    }
    fn mix_into(&mut self, _out: &mut [(f32, f32)]) {}
    fn set_output_rate(&mut self, _hz: u32) {}
    fn load_bios(&mut self, _bios: &[u8]) {}
    fn write_state(&self, _w: &mut crate::state::Writer) {}
    fn read_state(&mut self, _r: &mut crate::state::Reader<'_>) -> Result<(), StateError> {
        Ok(())
    }
    fn clone_box(&self) -> Box<dyn sgb::AudioCoprocessor> {
        Box::new(self.clone())
    }
}

/// The GB→SNES input path: every step pushes the local (physical) active-low
/// matrix nibbles into the coprocessor — the raw frontend state, not the
/// SGB-feed-overridden view — so its joypad autopoll can serve them back.
#[test]
fn sgb_local_matrix_is_pushed_to_the_coprocessor() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33;
    let mut gb = GameBoy::new(Model::Sgb, rom).unwrap();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    gb.set_audio_coprocessor(Box::new(InputCop(seen.clone())));

    gb.step();
    assert_eq!(
        seen.borrow().last(),
        Some(&(0x0F, 0x0F)),
        "idle matrix pushed"
    );

    gb.bus.joypad_mut().press(Button::A);
    gb.bus.joypad_mut().press(Button::Right);
    gb.step();
    assert_eq!(
        seen.borrow().last(),
        Some(&(0x0E, 0x0E)),
        "Right + A pressed (active low) pushed"
    );
}

/// A stub coprocessor that feeds fixed ICD2 pad bytes (the SNES→GB return
/// path). `joypad_feed` returning `Some` engages the override on `step`.
struct FeedCop([u8; 4]);

impl sgb::AudioCoprocessor for FeedCop {
    fn clock(&mut self, _gb_cycles: u64) {}
    fn poll(&mut self, _cmds: &mut dyn sgb::SgbCommandSource) {}
    fn joypad_feed(&mut self) -> Option<[u8; 4]> {
        Some(self.0)
    }
    fn mix_into(&mut self, _out: &mut [(f32, f32)]) {}
    fn set_output_rate(&mut self, _hz: u32) {}
    fn load_bios(&mut self, _bios: &[u8]) {}
    fn write_state(&self, _w: &mut crate::state::Writer) {}
    fn read_state(&mut self, _r: &mut crate::state::Reader<'_>) -> Result<(), StateError> {
        Ok(())
    }
    fn clone_box(&self) -> Box<dyn sgb::AudioCoprocessor> {
        Box::new(FeedCop(self.0))
    }
}

/// The SNES→GB return path end to end: a coprocessor feeding ICD2 pad bytes
/// overrides what the game reads at P1 — and without a feeding coprocessor
/// the read is byte-identical to the plain matrix (default-inert seam).
#[test]
fn sgb_joypad_feed_overrides_p1_reads_and_defaults_inert() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33;

    // Default machine (built-in HLE SgbApu): feed seam inert.
    let mut gb = GameBoy::new(Model::Sgb, rom.clone()).unwrap();
    gb.debug_write(0xFF00, 0x10); // select the button column
    gb.step();
    assert_eq!(
        gb.debug_read(0xFF00),
        0xDF,
        "no coprocessor: plain idle matrix"
    );

    // A feeding coprocessor: the fed "A pressed" replaces the matrix.
    let mut gb = GameBoy::new(Model::Sgb, rom).unwrap();
    gb.set_audio_coprocessor(Box::new(FeedCop([0xEF, 0xFF, 0xFF, 0xFF])));
    gb.debug_write(0xFF00, 0x10);
    gb.step();
    assert_eq!(gb.debug_read(0xFF00), 0xDE, "fed A visible at P1");
}

/// The raw-packet tee end to end: a packet pulsed through P1 reaches an
/// installed coprocessor via `SgbCommandSource::take_packet` on the next
/// step, while the HLE presentation still consumes the same command (a tee,
/// not a takeover).
#[test]
fn sgb_packet_tee_reaches_coprocessor_while_hle_still_applies() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33;
    let mut gb = GameBoy::new(Model::Sgb, rom).unwrap();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    gb.set_audio_coprocessor(Box::new(TeeCop(seen.clone())));

    let mut packet = [0u8; 16];
    packet[0] = 0x01; // PAL01, one packet
    packet[1] = 0x1F; // shared background color 0 = red
    send_sgb_packet(&mut gb, &packet);
    gb.debug_write(0xFF47, 0xE4);
    gb.debug_write(0xFF40, 0x91);
    for _ in 0..3 {
        gb.run_frame();
    }
    assert_eq!(
        seen.borrow().as_slice(),
        &[packet],
        "coprocessor got the raw packet"
    );
    assert_eq!(gb.frame()[0], 0xFF_0000, "HLE colorization still applied");
}

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

/// Graphics → "disable SGB colors": with `set_sgb_mono`, the SGB per-cell
/// colors are dropped and the game screen renders through the plain DMG palette
/// (default off, so the colorized path stays byte-identical).
#[test]
fn disable_sgb_colors_renders_through_the_dmg_palette() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03;
    rom[0x14B] = 0x33;
    let mut gb = GameBoy::new(Model::Sgb, rom).unwrap();
    let mut packet = [0u8; 16];
    packet[0] = 0x01; // PAL01
    packet[1] = 0x1F; // shared background color 0 = red
    send_sgb_packet(&mut gb, &packet);
    gb.debug_write(0xFF47, 0xE4); // BGP
    gb.debug_write(0xFF40, 0x91); // LCD on, BG on
    gb.set_sgb_mono(true);
    for _ in 0..3 {
        gb.run_frame();
    }
    assert_eq!(
        gb.frame()[0],
        0xFF_FFFF,
        "the SGB red is gone: shade 0 uses the default DMG palette"
    );
    // Re-enabling colors restores the SGB-provided red.
    gb.set_sgb_mono(false);
    for _ in 0..2 {
        gb.run_frame();
    }
    assert_eq!(gb.frame()[0], 0xFF_0000, "SGB color restored");
}

/// The single BIOS entry point feeds the audio path but, being high-level (no
/// SNES 65816), never fabricates a border or palette from an arbitrary image:
/// an unverifiable BIOS leaves the original default border untouched, and off
/// SGB it is an inert no-op. Guards the honest refusal to guess firmware
/// offsets (see `docs/hardware-state/sgb.md`).
#[test]
fn load_sgb_bios_keeps_default_border_off_sgb_noop() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03; // SGB flag
    rom[0x14B] = 0x33;
    let mut gb = GameBoy::new(Model::Sgb, rom).unwrap();
    let before = gb
        .sgb_border()
        .expect("an SGB shows the default border from power-on")
        .to_vec();

    // A bare image can't be trusted for the border/palette → default kept.
    gb.load_sgb_bios(&[0xABu8; 4096]);
    assert_eq!(
        gb.sgb_border().unwrap().as_slice(),
        before.as_slice(),
        "an unverifiable BIOS leaves the default border unchanged"
    );

    // Off SGB: no border, no panic.
    let mut dmg = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    dmg.load_sgb_bios(&[0xABu8; 4096]);
    assert!(dmg.sgb_border().is_none());
}

// ---- End-to-end border acquisition (CHR_TRN + PCT_TRN, no injected state) ----
//
// These exercise the *whole* chain a real SGB-enhanced game takes to install its
// own border, with nothing downstream of `Ppu::sgb_command` faked: real P1 pulse
// packets → the joypad receiver → the interconnect FF00 drain → `Ppu::sgb_command`
// → a `*_TRN` latch → the *actual* PPU render filling `shade_buf` → the
// frame-boundary capture → `decode_tiles` → the border buffers → `sgb_border()`.
// The screen is programmed through the real bus so the payload is captured the
// same way SameBoy's `pixel_to_bits` reads it (`Core/sgb.c` `GB_sgb_render`).

/// Number of frame boundaries after which the border cross-fade has fully
/// settled (`SgbView::FADE_LEN` = 24; a margin over it leaves `fade == 0`).
const FADE_SETTLE_FRAMES: usize = 30;

/// A minimal SGB-enhanced cart: ROM-only NOP sled, SGB header flags set so the
/// machine boots as an SGB. The CPU never touches IO/VRAM, so the screen the
/// test programs through the bus is exactly what renders.
fn sgb_machine() -> GameBoy {
    let mut rom = vec![0u8; 0x8000];
    rom[0x146] = 0x03; // SGB flag (both bytes required for `supports_sgb`)
    rom[0x14B] = 0x33; // old licensee code
    GameBoy::new(Model::Sgb, rom).unwrap()
}

/// A well-mixed, deterministic per-index byte payload: a layout/index/frame
/// bug that shifts, transposes, or drops any byte changes the compare.
fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| {
            let x = (i as u32)
                .wrapping_mul(2_654_435_761)
                .wrapping_add(u32::from(seed));
            (x ^ (x >> 15)) as u8
        })
        .collect()
}

/// CHR_TRN ($13) packet, `bank` = byte-1 bit0 (0 = tiles 0-127, 1 = 128-255).
fn chr_trn_packet(bank: u8) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0] = 0x13 * 8 + 1; // command $13, length 1
    p[1] = bank & 1;
    p
}

/// PCT_TRN ($14) packet (tilemap + border palettes; no body byte).
fn pct_trn_packet() -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0] = 0x14 * 8 + 1;
    p
}

fn run_frames(gb: &mut GameBoy, n: usize) {
    for _ in 0..n {
        gb.run_frame();
    }
}

/// Program VRAM + the BG tilemap so the rendered 160×144 screen encodes
/// `payload` (`payload.len()/16` standard 2bpp GB tiles) exactly as SameBoy's
/// `pixel_to_bits` reads it, then render frames so the real PPU fills
/// `shade_buf` naturally. Identity BGP makes each pixel's *recorded* 2-bit shade
/// equal to the tile's colour index; the capture grid and the BG grid are both
/// 20-wide 8×8, so `decode_tiles` tile `t` reads back exactly GB tile `t` — i.e.
/// the payload bytes. VRAM is written with the LCD off (unlocked), then on.
fn encode_payload_to_screen(gb: &mut GameBoy, payload: &[u8]) {
    let n_tiles = payload.len() / 16;
    assert!(
        payload.len() % 16 == 0 && n_tiles <= 256,
        "payload = whole tiles"
    );
    gb.debug_write(0xFF40, 0x00); // LCD off: VRAM unlocked, clean re-enable
    for (i, &b) in payload.iter().enumerate() {
        gb.debug_write(0x8000 + i as u16, b); // BG tile data (0x8000 method)
    }
    // BG map (0x9800, 32 wide): screen cell (t%20, t/20) → tile index t.
    for t in 0..n_tiles {
        let (col, row) = (t % 20, t / 20);
        gb.debug_write(0x9800 + (row * 32 + col) as u16, t as u8);
    }
    gb.debug_write(0xFF47, 0xE4); // BGP identity: recorded shade == colour index
    gb.debug_write(0xFF42, 0x00); // SCY = 0
    gb.debug_write(0xFF43, 0x00); // SCX = 0
    gb.debug_write(0xFF40, 0x91); // LCD on, BG on, 8000 tiles, 9800 map, no win/obj
    run_frames(gb, 3); // 1st frame after enable is skipped; shade_buf still fills
}

/// A blank screen (BG all colour 0) so the composited GB inset is a known,
/// uniform colour — SGB palette-0 entry 0 (default white), letting the inset /
/// transparency check use an exact value.
fn blank_screen(gb: &mut GameBoy) {
    gb.debug_write(0xFF40, 0x00);
    for i in 0..16 {
        gb.debug_write(0x8000 + i, 0x00); // tile 0 = colour 0
    }
    for i in 0..(32u16 * 32) {
        gb.debug_write(0x9800 + i, 0x00); // whole map → tile 0
    }
    gb.debug_write(0xFF47, 0xE4);
    gb.debug_write(0xFF40, 0x91);
    run_frames(gb, 3);
}

/// The screen-capture round-trip, byte-exact: a payload programmed into the
/// screen and captured by the *real* renderer decodes back to precisely the
/// bytes encoded — pinning the `record_sgb_shade` index layout and the frame
/// alignment (task items 1 & 2). Both CHR_TRN banks and the PCT_TRN buffer
/// (map + palettes at 0x800) are covered.
#[test]
fn trn_capture_round_trips_bytes_through_the_real_renderer() {
    let mut gb = sgb_machine();
    let chr0 = pattern(4096, 0x11); // CHR_TRN bank 0 → tiles 0-127
    let chr1 = pattern(4096, 0x83); // CHR_TRN bank 1 → tiles 128-255
    let pct = pattern(2176, 0xC5); // PCT_TRN → tilemap + palettes

    encode_payload_to_screen(&mut gb, &chr0);
    send_sgb_packet(&mut gb, &chr_trn_packet(0));
    run_frames(&mut gb, 2);

    encode_payload_to_screen(&mut gb, &chr1);
    send_sgb_packet(&mut gb, &chr_trn_packet(1));
    run_frames(&mut gb, 2);

    encode_payload_to_screen(&mut gb, &pct);
    send_sgb_packet(&mut gb, &pct_trn_packet());
    run_frames(&mut gb, 2);

    let (tiles, raw) = gb.bus.ppu().sgb_captured_border().expect("SGB present");
    assert_eq!(
        &tiles[0..4096],
        &chr0[..],
        "CHR_TRN bank 0 → border tiles 0-127"
    );
    assert_eq!(
        &tiles[4096..8192],
        &chr1[..],
        "CHR_TRN bank-1 bit routes to tiles 128-255 (byte offset 4096)"
    );
    assert_eq!(
        &raw[..],
        &pct[..],
        "PCT_TRN → border_raw, incl. the palette block at offset 0x800"
    );
}

/// The headline claim: a real SGB-enhanced game's own border loads and renders.
/// A hand-designed border (SNES 4bpp tiles + a 32×32 tilemap + BGR555 palettes)
/// is encoded into the screen and delivered as genuine CHR_TRN + PCT_TRN pulse
/// packets; `sgb_border()` then shows the designed tiles in the designed colours
/// at the designed positions, with a colour-0 tile over the GB area transparent.
#[test]
fn enhanced_game_border_loads_and_renders_end_to_end() {
    let mut gb = sgb_machine();

    // --- Design the border in slopgb's internal (SameBoy `border.*`) form ---
    // CHR bank 0: SNES 4bpp tiles. Tile 1 = solid colour 1 (plane 0 all set on
    // every row); tile 2 left all-zero = colour 0 (transparent).
    let mut chr = vec![0u8; 4096];
    let tile1_base = 32; // SNES 4bpp tile 1 starts at byte 1*32
    for y in 0..8 {
        chr[tile1_base + y * 2] = 0xFF; // tile 1, plane 0
    }
    // PCT: 32×32 tilemap (LE u16) @0, border palettes 4-7 (16 BGR555 each) @0x800.
    let mut pct = vec![0u8; 2176];
    let pal_c1 = 2048 + 2; // palette 0, colour 1 (2 bytes: BGR555 LE)
    pct[pal_c1] = 0x1F; // = red (BGR555 0x001F)
    pct[pal_c1 + 1] = 0x00;
    let mut put_map = |tx: usize, ty: usize, entry: u16| {
        let o = (tx + ty * 32) * 2;
        pct[o] = entry as u8;
        pct[o + 1] = (entry >> 8) as u8;
    };
    put_map(0, 0, 0x0001); // outside-gb corner cell → tile 1, palette 0 (red)
    put_map(6, 5, 0x0002); // gb-area top-left corner → tile 2 (colour 0)

    // --- Drive the real acquisition chain for each transfer ---
    encode_payload_to_screen(&mut gb, &chr);
    send_sgb_packet(&mut gb, &chr_trn_packet(0));
    run_frames(&mut gb, 2);
    encode_payload_to_screen(&mut gb, &pct);
    send_sgb_packet(&mut gb, &pct_trn_packet());
    run_frames(&mut gb, 2);

    // Present a blank (uniform white) GB screen, then settle the cross-fade so
    // the border surface is the pure composite.
    blank_screen(&mut gb);
    run_frames(&mut gb, FADE_SETTLE_FRAMES);

    let b = gb
        .sgb_border()
        .expect("a ROM border is present after CHR+PCT land");
    let at = |x: usize, y: usize| b[y * SGB_BORDER_W + x];
    // Cell (0,0) is outside the GB area: its tile-1 colour-1 pixels are red.
    assert_eq!(at(0, 0), 0xFF_0000, "the game's border tile drew red");
    assert_eq!(at(7, 7), 0xFF_0000, "…across the whole 8×8 cell");
    // Cell (6,5) is the GB inset corner (px 48,40); its colour-0 tile is
    // transparent, so the GB screen shows through.
    let inset = gb.frame()[0];
    assert_eq!(at(48, 40), inset, "colour-0 gb-area tile is transparent");
    assert_ne!(
        inset, 0xFF_0000,
        "the inset is distinct from the border colour"
    );
}

/// `rom_supports_sgb` needs BOTH header bytes (SGB flag 0x146 == 0x03 and old
/// licensee 0x14B == 0x33), and never panics on a truncated image — the
/// frontend "automatic, prefer SGB" policy calls it on raw ROM bytes.
#[test]
fn rom_supports_sgb_needs_both_header_bytes() {
    let mut rom = vec![0u8; 0x8000];
    assert!(!GameBoy::rom_supports_sgb(&rom), "bare ROM: no SGB");
    rom[0x146] = 0x03;
    assert!(
        !GameBoy::rom_supports_sgb(&rom),
        "the SGB flag alone is not enough"
    );
    rom[0x14B] = 0x33;
    assert!(
        GameBoy::rom_supports_sgb(&rom),
        "flag + old licensee unlocks SGB"
    );
    assert!(
        !GameBoy::rom_supports_sgb(&[0u8; 4]),
        "a truncated image is simply 'no SGB', never a panic"
    );
}

/// "GBC + initial SGB border" (`ModelChoice::CgbBorder`): `enable_sgb_border`
/// attaches the border surface to a CGB machine and arms the joypad SGB receiver
/// so an SGB-capable game uploads its own border while rendering GBC color
/// inside. Golden-safety is by construction: a plain `GameBoy::new(Cgb, ..)`
/// (what the golden path uses) never has a border surface; only an explicit
/// `enable_sgb_border` adds one.
#[test]
fn cgb_border_enables_a_border_surface_off_the_golden_path() {
    let rom = || {
        let mut r = vec![0u8; 0x8000];
        r[0x143] = 0xC0; // CGB only
        r
    };
    let mut plain = GameBoy::new(Model::Cgb, rom()).unwrap();
    let mut bordered = GameBoy::new(Model::Cgb, rom()).unwrap();
    bordered.enable_sgb_border();

    assert!(
        plain.sgb_border().is_none(),
        "the golden path (plain CGB) has no border surface — golden-safe"
    );
    assert!(
        bordered.sgb_border().is_some(),
        "enable_sgb_border exposes the built-in default border (until the game \
         uploads its own)"
    );

    // Both run without panicking; the bordered machine keeps a border surface.
    for _ in 0..10 {
        plain.run_frame();
        bordered.run_frame();
    }
    assert!(bordered.sgb_border().is_some());
    // Idempotent: re-enabling is a no-op, no panic.
    bordered.enable_sgb_border();
    assert!(bordered.sgb_border().is_some());
}

// ---- Audio-coprocessor injection (the public `set_audio_coprocessor` seam) ----

use std::cell::RefCell;
use std::rc::Rc;

use crate::sgb::{AudioCoprocessor, SgbCommandSource};

#[derive(Default)]
struct SpyLog {
    clocked: u64,
    sounds: Vec<crate::SgbSound>,
    mixed: usize,
    rate: u32,
}

/// A minimal alternative [`AudioCoprocessor`] written the way an *out-of-core*
/// implementer must: it names only the public `AudioCoprocessor` +
/// [`SgbCommandSource`] traits — never the core-private `Interconnect` — so it
/// compiling at all proves the decoupling. It records what `GameBoy` drives it
/// with, through a shared handle the test keeps after the box is moved in.
struct SpyCoprocessor(Rc<RefCell<SpyLog>>);

impl AudioCoprocessor for SpyCoprocessor {
    fn clock(&mut self, gb_cycles: u64) {
        self.0.borrow_mut().clocked += gb_cycles;
    }
    fn poll(&mut self, cmds: &mut dyn SgbCommandSource) {
        while let Some(s) = cmds.take_sound_event() {
            self.0.borrow_mut().sounds.push(s);
        }
        while cmds.take_data_snd().is_some() {}
    }
    fn mix_into(&mut self, out: &mut [(f32, f32)]) {
        self.0.borrow_mut().mixed += out.len();
    }
    fn set_output_rate(&mut self, hz: u32) {
        self.0.borrow_mut().rate = hz;
    }
    fn load_bios(&mut self, _bios: &[u8]) {}
    fn write_state(&self, w: &mut crate::state::Writer) {
        w.u32(self.0.borrow().clocked as u32);
    }
    fn read_state(&mut self, r: &mut crate::state::Reader<'_>) -> Result<(), crate::StateError> {
        self.0.borrow_mut().clocked = u64::from(r.u32()?);
        Ok(())
    }
    fn clone_box(&self) -> Box<dyn AudioCoprocessor> {
        Box::new(SpyCoprocessor(Rc::clone(&self.0)))
    }
}

/// The keystone: an injected coprocessor is driven by `GameBoy` through the
/// public seams — `clock` each step, `poll` fed a `SgbCommandSource` (a SOUND
/// packet crosses it), `mix_into` on drain, `set_output_rate` on a rate change —
/// with no reference to the core-private bus anywhere in `SpyCoprocessor`.
#[test]
fn injected_audio_coprocessor_is_driven_through_the_public_seam() {
    let mut gb = sgb_machine();
    let log = Rc::new(RefCell::new(SpyLog::default()));
    gb.set_audio_coprocessor(Box::new(SpyCoprocessor(Rc::clone(&log))));
    gb.set_sample_rate(44_100); // propagates to the freshly installed cop

    // A SOUND ($08) packet must reach the injected cop's `poll` via the command
    // source seam (byte-1..4 = effect A/B, attenuation, bank).
    let mut sound = [0u8; 16];
    sound[0] = 0x08 * 8 + 1; // command $08, length 1
    sound[1] = 0xA1;
    sound[2] = 0xB2;
    sound[3] = 0xC3;
    sound[4] = 0xD4;
    send_sgb_packet(&mut gb, &sound);

    gb.run_frame();
    let mut out = Vec::new();
    gb.drain_audio(&mut out);

    let l = log.borrow();
    assert!(l.clocked > 0, "clock() drove the injected coprocessor");
    assert!(l.mixed > 0, "mix_into() drove it on drain");
    assert_eq!(l.rate, 44_100, "set_sample_rate propagated to it");
    assert!(
        l.sounds.contains(&crate::SgbSound {
            effect_a: 0xA1,
            effect_b: 0xB2,
            attenuation: 0xC3,
            effect_bank: 0xD4,
        }),
        "the SOUND command crossed the SgbCommandSource seam to the injected cop",
    );
}

/// Off `Model::Sgb`/`Sgb2` there is no coprocessor slot, so injection drops the
/// box and never drives it — `Dmg`/`Cgb` stay byte-identical (golden-safe).
#[test]
fn set_audio_coprocessor_is_a_noop_off_sgb() {
    let mut dmg = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    let log = Rc::new(RefCell::new(SpyLog::default()));
    dmg.set_audio_coprocessor(Box::new(SpyCoprocessor(Rc::clone(&log))));

    dmg.run_frame();
    let mut out = Vec::new();
    dmg.drain_audio(&mut out);

    assert_eq!(
        log.borrow().clocked,
        0,
        "off SGB the injected coprocessor is never installed or driven",
    );
}

/// `capture_initial_sgb_border` returns `None` for a ROM that never uploads an
/// SGB border (here a NOP sled), so the frontend falls back to the default
/// border. (The successful-capture path needs a real SGB game and is verified
/// by hand; it can't be a committed test without shipping a copyrighted ROM.)
#[test]
fn capture_initial_sgb_border_is_none_when_the_game_uploads_nothing() {
    let mut rom = vec![0u8; 0x8000]; // ROM-only NOP sled
    rom[0x146] = 0x03; // SGB flag
    rom[0x14B] = 0x33; // old licensee — so the receiver is armed
    assert!(
        GameBoy::capture_initial_sgb_border(&rom, 30).is_none(),
        "a game that uploads no CHR_TRN/PCT_TRN yields no captured border"
    );
}
