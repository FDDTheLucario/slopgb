//! The S-DSP echo unit: an 8-tap FIR filter over a delay-line ring buffer that
//! lives in APU RAM.
//!
//! Each output sample the DSP: reads one stereo echo sample (4 bytes: L then R,
//! 16-bit little-endian) from the ring at `ESA*0x100 + offset`; runs the 8
//! newest reads through the `FIR0..FIR7` coefficients (÷128); adds that,
//! scaled by `EVOL(L/R)`, to the master output; and — unless echo writes are
//! disabled (`FLG` bit 5, ECEN) — writes `echo_bus + FIR*EFB/128` back into the
//! ring. The ring length is `(EDL & 0x0F) * 2 KiB` bytes (a zero `EDL` uses a
//! single 4-byte slot).
//!
//! Sources: nocash **fullsnes** ("SNES APU DSP - Echo") for the buffer layout,
//! the `EDL`/`ESA` addressing and the ECEN write-disable; **bsnes** `dsp` for
//! the FIR accumulation and the feedback path.

/// Echo delay-line + FIR history state.
#[derive(Clone, Default)]
pub(super) struct Echo {
    /// Ring of the 8 most recent stereo reads from the echo buffer (`[L, R]`),
    /// newest last. Drives the 8-tap FIR.
    fir_hist: [[i32; 2]; 8],
    /// Byte offset of the current echo sample within the ring buffer.
    offset: u32,
    /// Ring length in bytes, latched when the buffer wraps (as on hardware).
    len: u32,
}

impl Echo {
    pub(super) fn write_state(&self, w: &mut crate::state::Writer) {
        for tap in &self.fir_hist {
            w.u32(tap[0] as u32);
            w.u32(tap[1] as u32);
        }
        w.u32(self.offset);
        w.u32(self.len);
    }

    pub(super) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::StateError> {
        for tap in &mut self.fir_hist {
            tap[0] = r.u32()? as i32;
            tap[1] = r.u32()? as i32;
        }
        self.offset = r.u32()?;
        self.len = r.u32()?;
        Ok(())
    }
}

#[inline]
fn read16(ram: &[u8; 0x1_0000], addr: u32) -> i32 {
    let a = (addr & 0xFFFF) as usize;
    let b = (addr.wrapping_add(1) & 0xFFFF) as usize;
    i32::from(i16::from_le_bytes([ram[a], ram[b]]))
}

#[inline]
fn write16(ram: &mut [u8; 0x1_0000], addr: u32, v: i32) {
    let bytes = (v as i16).to_le_bytes();
    ram[(addr & 0xFFFF) as usize] = bytes[0];
    ram[(addr.wrapping_add(1) & 0xFFFF) as usize] = bytes[1];
}

#[inline]
fn clamp16(v: i32) -> i32 {
    v.clamp(-32768, 32767)
}

/// Registers the echo unit needs each sample.
pub(super) struct EchoRegs {
    pub esa: u8,
    pub edl: u8,
    pub efb: i8,
    pub evol_l: i8,
    pub evol_r: i8,
    pub fir: [i8; 8],
    /// FLG bit 5 — when set, echo-buffer writes are disabled.
    pub write_disabled: bool,
}

impl Echo {
    /// Process one sample. `echo_in` is the summed output of the EON voices
    /// (the echo bus). Returns the stereo echo contribution to add to the
    /// master mix (already scaled by `EVOL`). May write the ring buffer in
    /// `ram`.
    pub fn process(
        &mut self,
        ram: &mut [u8; 0x1_0000],
        regs: &EchoRegs,
        echo_in: (i32, i32),
    ) -> (i32, i32) {
        // Latch the ring length at the start of each pass (offset 0), as the
        // hardware samples EDL only when the buffer restarts.
        if self.offset == 0 {
            self.len = (u32::from(regs.edl) & 0x0F) * 0x800;
            if self.len == 0 {
                self.len = 4;
            }
        }
        let base = u32::from(regs.esa) << 8;
        let addr = base.wrapping_add(self.offset);

        // Read this slot and push it through the FIR history (newest last).
        let l = read16(ram, addr);
        let r = read16(ram, addr.wrapping_add(2));
        for i in 0..7 {
            self.fir_hist[i] = self.fir_hist[i + 1];
        }
        self.fir_hist[7] = [l, r];

        // 8-tap FIR (÷128). Coefficients FIR0 is the oldest tap.
        let mut fir_l = 0i32;
        let mut fir_r = 0i32;
        for i in 0..8 {
            fir_l += self.fir_hist[i][0] * i32::from(regs.fir[i]);
            fir_r += self.fir_hist[i][1] * i32::from(regs.fir[i]);
        }
        fir_l = clamp16(fir_l >> 7);
        fir_r = clamp16(fir_r >> 7);

        // Feedback write: echo bus + filtered echo * EFB/128, clamped.
        if !regs.write_disabled {
            let wl = clamp16(echo_in.0 + ((fir_l * i32::from(regs.efb)) >> 7));
            let wr = clamp16(echo_in.1 + ((fir_r * i32::from(regs.efb)) >> 7));
            write16(ram, addr, wl);
            write16(ram, addr.wrapping_add(2), wr);
        }

        // Advance the ring; wrap at the latched length.
        self.offset += 4;
        if self.offset >= self.len {
            self.offset = 0;
        }

        // Echo output added to the master mix (scaled by EVOL/128).
        let out_l = (fir_l * i32::from(regs.evol_l)) >> 7;
        let out_r = (fir_r * i32::from(regs.evol_r)) >> 7;
        (out_l, out_r)
    }
}

#[cfg(test)]
#[path = "echo_tests.rs"]
mod tests;
