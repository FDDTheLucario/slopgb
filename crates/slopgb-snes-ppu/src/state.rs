//! Snapshot serialization for [`SnesPpu`] — a fixed-length byte image of the
//! three memories + every port/latch/register, so the wasm plugin wrapper
//! (and any other host) can save/restore the chip without reaching into
//! private fields.

use super::*;

/// Serialized [`SnesPpu::save_state`] length: VRAM (32 K words LE), CGRAM
/// (256 words LE), OAM (544 bytes), then the register/latch block.
pub const PPU_STATE_LEN: usize = VRAM_WORDS * 2 + 256 * 2 + OAM_LEN + 41;

impl SnesPpu {
    /// Serialize the whole chip into exactly [`PPU_STATE_LEN`] bytes.
    pub fn save_state(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(PPU_STATE_LEN);
        for w in self.vram.iter() {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        for w in &self.cgram {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        buf.extend_from_slice(&self.oam);
        buf.push(self.vmain);
        buf.extend_from_slice(&self.vmadd.to_le_bytes());
        buf.extend_from_slice(&self.prefetch.to_le_bytes());
        buf.push(self.cgadd);
        buf.push(u8::from(self.cg_second));
        buf.push(self.cg_lsb);
        buf.extend_from_slice(&self.oam_reload.to_le_bytes());
        buf.push(u8::from(self.oam_priority));
        buf.extend_from_slice(&self.oam_addr.to_le_bytes());
        buf.push(self.oam_lsb);
        buf.push(self.bgmode);
        buf.extend_from_slice(&self.bgsc);
        buf.extend_from_slice(&self.nba);
        for v in self.hofs.iter().chain(self.vofs.iter()) {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf.push(self.bg_old);
        buf.push(self.tm);
        buf.push(self.obsel);
        buf.push(self.inidisp);
        debug_assert_eq!(buf.len(), PPU_STATE_LEN);
        buf
    }

    /// Restore a [`Self::save_state`] image; a wrong-length buffer is
    /// ignored (the chip keeps its state) rather than a panic.
    pub fn load_state(&mut self, b: &[u8]) {
        if b.len() != PPU_STATE_LEN {
            return;
        }
        let mut pos = 0usize;
        let u8_ = |pos: &mut usize| {
            let v = b[*pos];
            *pos += 1;
            v
        };
        for w in self.vram.iter_mut() {
            *w = u16::from_le_bytes([b[pos], b[pos + 1]]);
            pos += 2;
        }
        for w in &mut self.cgram {
            *w = u16::from_le_bytes([b[pos], b[pos + 1]]);
            pos += 2;
        }
        self.oam.copy_from_slice(&b[pos..pos + OAM_LEN]);
        pos += OAM_LEN;
        let u16_ = |pos: &mut usize| {
            let v = u16::from_le_bytes([b[*pos], b[*pos + 1]]);
            *pos += 2;
            v
        };
        self.vmain = u8_(&mut pos);
        self.vmadd = u16_(&mut pos);
        self.prefetch = u16_(&mut pos);
        self.cgadd = u8_(&mut pos);
        self.cg_second = u8_(&mut pos) != 0;
        self.cg_lsb = u8_(&mut pos);
        self.oam_reload = u16_(&mut pos) & 0x1FF;
        self.oam_priority = u8_(&mut pos) != 0;
        self.oam_addr = u16_(&mut pos) & 0x3FF;
        self.oam_lsb = u8_(&mut pos);
        self.bgmode = u8_(&mut pos);
        for s in &mut self.bgsc {
            *s = u8_(&mut pos);
        }
        for n in &mut self.nba {
            *n = u8_(&mut pos);
        }
        for v in self.hofs.iter_mut().chain(self.vofs.iter_mut()) {
            *v = u16_(&mut pos);
        }
        self.bg_old = u8_(&mut pos);
        self.tm = u8_(&mut pos);
        self.obsel = u8_(&mut pos);
        self.inidisp = u8_(&mut pos);
    }
}
