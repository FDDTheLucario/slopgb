//! Unit tests for the PPU renderer (OAM scan, fetcher, sprites, window,
//! mode-0 grid). Split out of `render.rs` for file size; compiled as
//! `super::tests` via the `#[path]` attribute there.

use super::super::Ppu;
use super::oam_glitch_magic_enable;
use crate::Model;

const WHITE: u32 = 0xFF_FFFF;
const LIGHT: u32 = 0xAA_AAAA;
const DARK: u32 = 0x55_5555;
const BLACK: u32 = 0x00_0000;

fn run_to(p: &mut Ppu, line: u8, dot: u16) {
    let mut guard = 0u32;
    while !(p.line == line && p.dot == dot) {
        p.tick();
        guard += 1;
        assert!(guard < 200_000, "run_to({line},{dot}) never reached");
    }
}

/// Render the given line to completion; returns the dot at which mode 3
/// ended (V0).
fn render_line(p: &mut Ppu, line: u8) -> u16 {
    run_to(p, line, 84);
    finish_line(p)
}

fn px(p: &Ppu, line: usize, x: usize) -> u32 {
    p.back[line * crate::SCREEN_W + x]
}

fn dmg_on(lcdc: u8) -> Ppu {
    let mut p = Ppu::new(Model::Dmg);
    p.write(0xFF47, 0xE4); // identity BGP
    p.write(0xFF48, 0xE4);
    p.write(0xFF49, 0xE4);
    p.write(0xFF40, lcdc);
    p
}

fn set_tile_row(p: &mut Ppu, bank: usize, tile: usize, row: usize, lo: u8, hi: u8) {
    p.vram[bank * 0x2000 + tile * 16 + row * 2] = lo;
    p.vram[bank * 0x2000 + tile * 16 + row * 2 + 1] = hi;
}

fn set_map(p: &mut Ppu, base: usize, row: usize, col: usize, tile: u8) {
    p.vram[base + row * 32 + col] = tile;
}

fn sprite(p: &mut Ppu, i: u8, y: u8, x: u8, tile: u8, flags: u8) {
    p.oam_dma_write(i * 4, y);
    p.oam_dma_write(i * 4 + 1, x);
    p.oam_dma_write(i * 4 + 2, tile);
    p.oam_dma_write(i * 4 + 3, flags);
}

/// Mimic the interconnect's write path exactly: stage at the register's
/// production commit offset (`stage_write_dots`, the same call `Bus::write`
/// makes), tick one M-cycle (4 dots at normal speed), then commit
/// architecturally. These render tests are single-speed, so `double_speed` is
/// `false`.
fn mcycle_write(p: &mut Ppu, addr: u16, value: u8) {
    let dots = p.stage_write_dots(addr, false);
    p.stage_write(addr, value, dots);
    for _ in 0..4 {
        p.tick();
    }
    p.write(addr, value);
}

/// Finish the current line's mode 3; returns the dot it ended on (V0).
fn finish_line(p: &mut Ppu) -> u16 {
    let mut flip = None;
    let mut guard = 0u32;
    while !p.line_render_done || p.render.active {
        p.tick();
        if p.line_render_done && flip.is_none() {
            flip = Some(p.dot);
        }
        guard += 1;
        assert!(guard < 2_000, "mode 3 never finished");
    }
    flip.expect("flip dot recorded")
}

fn penalty(xs: &[u8]) -> i32 {
    let mut p = dmg_on(0x93);
    for (i, &x) in xs.iter().enumerate() {
        sprite(&mut p, i as u8, 19, x, 0, 0); // row 0 on line 3
    }
    i32::from(render_line(&mut p, 3)) - 256
}

fn mgb_on(lcdc: u8) -> Ppu {
    let mut p = Ppu::new(Model::Mgb);
    p.write(0xFF47, 0xE4);
    p.write(0xFF40, lcdc);
    p
}

fn cgb_on(lcdc: u8) -> Ppu {
    let mut p = Ppu::new(Model::Cgb);
    // BG palette 0 color 0 = white, identity-ish grayscale for colors.
    for pal in 0..2usize {
        for (c, raw) in [(0usize, 0x7FFFu16), (1, 0x294A), (2, 0x14A5), (3, 0x0000)] {
            p.bg_pal_ram[pal * 8 + c * 2] = raw as u8;
            p.bg_pal_ram[pal * 8 + c * 2 + 1] = (raw >> 8) as u8;
            p.obj_pal_ram[pal * 8 + c * 2] = raw as u8;
            p.obj_pal_ram[pal * 8 + c * 2 + 1] = (raw >> 8) as u8;
        }
    }
    // Make palette 1 color 1 pure red, obj palette 1 color 1 pure blue.
    p.bg_pal_ram[8 + 2] = 0x1F;
    p.bg_pal_ram[8 + 3] = 0x00;
    p.obj_pal_ram[8 + 2] = 0x00;
    p.obj_pal_ram[8 + 3] = 0x7C;
    p.write(0xFF40, lcdc);
    p
}

const CGB_WHITE: u32 = 0xFF_FFFF;
const RED: u32 = 0xFF_0000;
const BLUE: u32 = 0x00_00FF;

#[path = "render_tests/bg.rs"]
mod bg;

#[path = "render_tests/cgb.rs"]
mod cgb;

#[path = "render_tests/sprite.rs"]
mod sprite;

#[path = "render_tests/window.rs"]
mod window;
