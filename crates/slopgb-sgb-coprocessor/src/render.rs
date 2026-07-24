//! The SNES-side scanline rasterization pump `flush` calls each pass, plus
//! the raw guest-RAM debug helpers `examples/throughput.rs` uses to deposit
//! a synthetic 65C816 program (split out of `lib.rs` to stay under the
//! module size cap).

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
}
