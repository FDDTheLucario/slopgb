//! The SNES-side scanline rasterization pump `flush` calls each pass and the
//! per-frame handoff of the image it builds, plus the raw guest-RAM debug
//! helpers — the read-only plugin-memory peeks the debugger/MCP reads the
//! SNES side through, and the write/set-PC pair `examples/throughput.rs` uses
//! to deposit a synthetic 65C816 program.

use super::*;

impl SgbCoprocessor {
    /// Rasterize every framebuffer row the SNES beam has passed since the
    /// last flush, or — while `render_enabled` is off (see
    /// [`AudioCoprocessor::set_render_enabled`]; a frontend fast-forwarding
    /// past frames nobody presents) — skip the wasm rasterization call but
    /// still advance `ppu_row` by the same amount, so the vblank-edge
    /// `frame_ready`/`frames_done` bookkeeping (which only checks that the
    /// row cursor reached the frame) stays consistent whether or not a
    /// frame gets drawn.
    pub(crate) fn pump_ppu_scanlines(&mut self) {
        let Some(ppu) = &self.ppu else { return };
        let v = self.gb_pos * SNES_LINES / GB_FRAME_CYCLES;
        let target = v.min(SNES_FB_H as u64) as u16;
        if self.ppu_row < target {
            let count = (target - self.ppu_row).min(255) as u8;
            if self.render_enabled {
                let _ = ppu.borrow_mut().write_ram(
                    PPU_HW_LINE,
                    &[self.ppu_row as u8, (self.ppu_row >> 8) as u8, count],
                );
            }
            self.ppu_row += u16::from(count);
        }
    }

    /// Write `data` into the 65C816 plugin's memory at the 24-bit `addr` — a
    /// test/bench seam (the `cpu`/`spc` plugin handles are otherwise private
    /// to this crate) for depositing a synthetic guest program, e.g. from
    /// `examples/throughput.rs`. Never called by the emulation loop itself.
    pub fn debug_cpu_write(&self, addr: u32, data: &[u8]) {
        let _ = self.cpu.borrow_mut().write_ram(addr, data);
    }

    /// Point the 65C816 plugin's program counter at `pc` — pairs with
    /// [`Self::debug_cpu_write`] to start a synthetic guest program from
    /// outside the crate.
    pub fn debug_cpu_set_pc(&self, pc: u32) {
        let _ = self.cpu.borrow_mut().set_pc(pc);
    }

    /// Fetch the last completed SNES frame (256x224 RGB555 words,
    /// row-major), at most once per vblank. `None` without a PPU plugin,
    /// until the next frame completes, or until the SNES display has ever
    /// shown a picture (`snes_live`) — before a takeover programs the PPU
    /// the framebuffer is permanently black, and surfacing it would black
    /// out the frontend over the live HLE presentation. Sticky once live:
    /// the takeover's own blank stretches present as a real TV would.
    pub fn take_snes_frame(&mut self) -> Option<Vec<u16>> {
        if !self.frame_ready || !self.snes_live {
            return None;
        }
        self.frame_ready = false;
        let ppu = self.ppu.as_ref()?;
        let bytes = ppu
            .borrow_mut()
            .read_ram(PPU_HW_FB, SNES_FB_W * SNES_FB_H * 2)
            .ok()?;
        Some(
            bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect(),
        )
    }

    /// Read `len` bytes of the 65C816 plugin's memory at the 24-bit `addr` —
    /// read-only introspection for the debugger/MCP (a `peek` into the SNES
    /// side; never advances a cycle).
    pub fn debug_cpu_ram(&self, addr: u32, len: usize) -> Vec<u8> {
        self.cpu
            .borrow_mut()
            .read_ram(addr, len)
            .unwrap_or_default()
    }

    /// The PPU plugin's raw state snapshot (the `slopgb-snes-ppu` image:
    /// VRAM, CGRAM, OAM, registers, framebuffer) — read-only introspection
    /// for the debugger/MCP; empty without a PPU plugin.
    pub fn debug_ppu_state(&self) -> Vec<u8> {
        self.ppu
            .as_ref()
            .and_then(|p| p.borrow_mut().save_state().ok())
            .unwrap_or_default()
    }
}
