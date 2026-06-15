//! The bgb I/O map window (Layer C): every I/O register's live value in bgb's
//! groups, plus the LCDC/STAT bit breakdowns. Pure content over
//! `GameBoy::debug_read`; the winit surface comes with B12b.

use crate::ui::Theme;
use crate::ui::canvas::Canvas;
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::checkbox;

/// One I/O register: address + bgb's short name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoReg {
    pub addr: u16,
    pub name: &'static str,
}

const fn r(addr: u16, name: &'static str) -> IoReg {
    IoReg { addr, name }
}

/// bgb's "LCD" group (FF40–FF4B).
pub const LCD: &[IoReg] = &[
    r(0xFF40, "LCDC"),
    r(0xFF41, "STAT"),
    r(0xFF42, "SCY"),
    r(0xFF43, "SCX"),
    r(0xFF44, "LY"),
    r(0xFF45, "LYC"),
    r(0xFF46, "DMA"),
    r(0xFF47, "BGP"),
    r(0xFF48, "OBP0"),
    r(0xFF49, "OBP1"),
    r(0xFF4A, "WY"),
    r(0xFF4B, "WX"),
];

/// bgb's "various" group (timer/interrupt/joypad/serial/banking).
pub const VARIOUS: &[IoReg] = &[
    r(0xFF70, "SVBK"),
    r(0xFF4F, "VBK"),
    r(0xFF4D, "KEY1"),
    r(0xFF00, "JOYP"),
    r(0xFF01, "SB"),
    r(0xFF02, "SC"),
    r(0xFF04, "DIV"),
    r(0xFF05, "TIMA"),
    r(0xFF06, "TMA"),
    r(0xFF07, "TAC"),
    r(0xFF0F, "IF"),
    r(0xFFFF, "IE"),
];

/// The four sound channels (FF10–FF23) + master control (FF24–FF26).
pub const SOUND: &[IoReg] = &[
    r(0xFF10, "NR10"),
    r(0xFF11, "NR11"),
    r(0xFF12, "NR12"),
    r(0xFF13, "NR13"),
    r(0xFF14, "NR14"),
    r(0xFF16, "NR21"),
    r(0xFF17, "NR22"),
    r(0xFF18, "NR23"),
    r(0xFF19, "NR24"),
    r(0xFF1A, "NR30"),
    r(0xFF1B, "NR31"),
    r(0xFF1C, "NR32"),
    r(0xFF1D, "NR33"),
    r(0xFF1E, "NR34"),
    r(0xFF20, "NR41"),
    r(0xFF21, "NR42"),
    r(0xFF22, "NR43"),
    r(0xFF23, "NR44"),
    r(0xFF24, "NR50"),
    r(0xFF25, "NR51"),
    r(0xFF26, "NR52"),
];

/// bgb's "GBC DMA" group (HDMA source/dest/control, FF51–FF55).
pub const GBC_DMA: &[IoReg] = &[
    r(0xFF51, "HDMA1"),
    r(0xFF52, "HDMA2"),
    r(0xFF53, "HDMA3"),
    r(0xFF54, "HDMA4"),
    r(0xFF55, "HDMA5"),
];

/// bgb's "GBC pal" group (CGB palette index/data ports, FF68–FF6B).
pub const GBC_PAL: &[IoReg] = &[
    r(0xFF68, "BCPS"),
    r(0xFF69, "BCPD"),
    r(0xFF6A, "OCPS"),
    r(0xFF6B, "OCPD"),
];

/// The five interrupt vectors (vector address, bgb name), IE/IF **bit 0 → 4**.
pub const INT_VECTORS: [(u16, &str); 5] = [
    (0x40, "VBlank"),
    (0x48, "LCD"),
    (0x50, "Timer"),
    (0x58, "Serial"),
    (0x60, "Joypad"),
];

/// Wave RAM (FF30–FF3F) as a 32-hex-digit string for the wave-pattern panel.
#[must_use]
pub fn wave_row(read: impl Fn(u16) -> u8) -> String {
    (0..16)
        .map(|i| format!("{:02X}", read(0xFF30 + i)))
        .collect()
}

/// One IF/IE vector row: its label (`40 VBlank`, plus ` *` when the `IF` bit is
/// pending) and whether the `IE` enable bit is set. `i` indexes [`INT_VECTORS`].
#[must_use]
pub fn vector_line(i: usize, iflag: u8, ie: u8) -> (String, bool) {
    let (vec, name) = INT_VECTORS[i];
    let bit = 1u8 << i;
    let label = format!(
        "{vec:02X} {name}{}",
        if iflag & bit != 0 { " *" } else { "" }
    );
    (label, ie & bit != 0)
}

/// Draw the IF/IE interrupt vectors as enable check-boxes (checked = `IE` set,
/// ` *` suffix = `IF` pending) down from `(x, y)`.
pub fn render_vectors(c: &mut Canvas, x: i32, y: i32, iflag: u8, ie: u8, theme: &Theme) {
    let lh = line_height();
    for i in 0..INT_VECTORS.len() {
        let (label, enabled) = vector_line(i, iflag, ie);
        checkbox(c, x, y + i as i32 * lh, enabled, &label, theme);
    }
}

/// `FFNN NAME XX` — one register line from `read` (use `GameBoy::debug_read`).
#[must_use]
pub fn reg_line(read: impl Fn(u16) -> u8, reg: IoReg) -> String {
    format!("{:04X} {:<5}{:02X}", reg.addr, reg.name, read(reg.addr))
}

/// Draw a register group as a vertical list at `(x, y)`; returns the y below it.
pub fn render_group(
    c: &mut Canvas,
    x: i32,
    y: i32,
    read: &impl Fn(u16) -> u8,
    regs: &[IoReg],
    theme: &Theme,
) -> i32 {
    let lh = line_height();
    for (i, &reg) in regs.iter().enumerate() {
        draw_text(c, x, y + i as i32 * lh, &reg_line(read, reg), theme.text);
    }
    y + regs.len() as i32 * lh
}

/// LCDC (FF40) bit labels, bit 7 → bit 0, in bgb's reading order.
pub const LCDC_BITS: [&str; 8] = [
    "LCD on", "WIN map", "WIN on", "BG tiles", "BG map", "OBJ 8x16", "OBJ on", "BG on",
];

/// STAT (FF41) interrupt-enable + status labels (bits 6 → 2).
pub const STAT_BITS: [&str; 5] = ["LYC int", "OAM int", "VBL int", "HBL int", "LY=LYC"];

/// Decode a register's bits into `(label, set)` pairs (MSB first) for the
/// checkbox breakdown.
#[must_use]
pub fn bit_states<'a>(value: u8, labels: &[&'a str], top_bit: u8) -> Vec<(&'a str, bool)> {
    labels
        .iter()
        .enumerate()
        .map(|(i, &lbl)| (lbl, value & (1 << (top_bit - i as u8)) != 0))
        .collect()
}

/// Draw a bit breakdown (one checkbox per labelled bit) down from `(x, y)`.
pub fn render_bits(
    c: &mut Canvas,
    x: i32,
    y: i32,
    value: u8,
    labels: &[&str],
    top_bit: u8,
    theme: &Theme,
) {
    let lh = line_height();
    for (i, (lbl, set)) in bit_states(value, labels, top_bit).into_iter().enumerate() {
        checkbox(c, x, y + i as i32 * lh, set, lbl, theme);
    }
}

#[cfg(test)]
#[path = "iomap_tests.rs"]
mod tests;
