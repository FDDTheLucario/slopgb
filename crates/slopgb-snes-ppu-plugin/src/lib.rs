//! Coprocessor plugin wrapping the clean-room SNES PPU (`slopgb-snes-ppu`)
//! as a host-driven wasm subsystem.
//!
//! The chip is passive: it has no clock of its own, so `run_until` just
//! absorbs the span. The host feeds it through the generic coprocessor ABI:
//!
//! - `port_write(p, v)` / `port_read(p)`: a B-bus access to `$2100 + p` —
//!   the exact bytes a captured guest `$21xx` write (or DMA B-bus byte)
//!   carries.
//! - `write_ram(HW_LINE, [y_lo, y_hi])`: render main-screen line `y`
//!   (0-223) into the internal 256x224 framebuffer.
//! - `read_ram(HW_FB + byte_off, len)`: fetch the framebuffer as RGB555
//!   little-endian bytes, row-major.
//!
//! Addresses below [`HOST_WIN`] have no meaning (the PPU has no
//! CPU-addressable RAM): writes are dropped, reads return zeros.

#![deny(unsafe_code)]

use slopgb_plugin_api::{Coprocessor, slopgb_coprocessor_plugin};
use slopgb_snes_ppu::{PPU_STATE_LEN, SnesPpu};

/// Frame geometry: 256x224 (fullsnes "SNES PPU Resolution", NTSC).
pub const FB_WIDTH: usize = 256;
pub const FB_HEIGHT: usize = 224;
/// Framebuffer length in bytes (RGB555 LE words, row-major).
pub const FB_BYTES: usize = FB_WIDTH * FB_HEIGHT * 2;

/// Host-window base (mirrors the w65c816 plugin convention: nothing below
/// `0x0100_0000` can collide with chip addressing).
pub const HOST_WIN: u32 = 0x0100_0000;
/// `W len 2`: `[y_lo, y_hi]` — render line `y` into the framebuffer.
pub const HW_LINE: u32 = HOST_WIN;
/// `R len n` at `HW_FB + byte offset`: the framebuffer bytes.
pub const HW_FB: u32 = HOST_WIN + 0x1000;

/// Serialized state: the PPU snapshot + the framebuffer + the cycle counter.
const STATE_LEN: usize = PPU_STATE_LEN + FB_BYTES + 8;

struct SnesPpuCop {
    ppu: SnesPpu,
    fb: Box<[u16]>,
    /// Host-clock bookkeeping only (the chip is passive).
    cycles: u64,
}

impl Coprocessor for SnesPpuCop {
    fn new() -> Self {
        SnesPpuCop {
            ppu: SnesPpu::new(),
            fb: vec![0u16; FB_WIDTH * FB_HEIGHT].into_boxed_slice(),
            cycles: 0,
        }
    }

    fn reset(&mut self) {
        self.ppu = SnesPpu::new();
        self.fb.fill(0);
        self.cycles = 0;
    }

    fn run_until(&mut self, target_cycle: u64) -> u64 {
        // Passive chip: rendering happens on host command (HW_LINE), so the
        // span is absorbed like a halted CPU.
        self.cycles = self.cycles.max(target_cycle);
        self.cycles
    }

    fn port_write(&mut self, port: u8, val: u8) {
        self.ppu.write(port, val);
    }

    fn port_read(&mut self, port: u8) -> u8 {
        self.ppu.read(port)
    }

    fn write_ram(&mut self, addr: u32, bytes: &[u8]) {
        if addr == HW_LINE {
            if let [lo, hi] = *bytes {
                let y = usize::from(lo) | usize::from(hi) << 8;
                if y < FB_HEIGHT {
                    let mut line = [0u16; FB_WIDTH];
                    self.ppu.render_line(y as u16, &mut line);
                    self.fb[y * FB_WIDTH..(y + 1) * FB_WIDTH].copy_from_slice(&line);
                }
            }
        }
    }

    fn read_ram(&mut self, addr: u32, len: usize) -> Vec<u8> {
        let mut out = vec![0u8; len];
        if (HW_FB..HW_FB + FB_BYTES as u32).contains(&addr) {
            let start = (addr - HW_FB) as usize;
            for (i, slot) in out.iter_mut().enumerate() {
                let off = start + i;
                if off >= FB_BYTES {
                    break;
                }
                let w = self.fb[off / 2];
                *slot = if off % 2 == 0 {
                    w as u8
                } else {
                    (w >> 8) as u8
                };
            }
        }
        out
    }

    fn save_state(&self) -> Vec<u8> {
        let mut buf = self.ppu.save_state();
        for w in self.fb.iter() {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        buf.extend_from_slice(&self.cycles.to_le_bytes());
        debug_assert_eq!(buf.len(), STATE_LEN);
        buf
    }

    fn load_state(&mut self, bytes: &[u8]) {
        if bytes.len() != STATE_LEN {
            return;
        }
        self.ppu.load_state(&bytes[..PPU_STATE_LEN]);
        for (i, w) in self.fb.iter_mut().enumerate() {
            let off = PPU_STATE_LEN + i * 2;
            *w = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
        }
        self.cycles = u64::from_le_bytes(bytes[STATE_LEN - 8..].try_into().unwrap_or_default());
    }
}

slopgb_coprocessor_plugin!(SnesPpuCop);

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
