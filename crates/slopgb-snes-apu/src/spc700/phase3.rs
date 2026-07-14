//! Phase-3 seam completion — state access the S-DSP and the SGB APU wiring
//! need that the verified CPU surface does not expose:
//!
//! - a live **APU-RAM view** (the S-DSP shares the SPC700's 64 KB bus for BRR
//!   sample fetch, the sample directory, and the echo buffer; SGB bulk uploads
//!   — SOU_TRN / DATA_TRN / DATA_SND — land here too),
//! - [`Clone`] (the emulator clones the whole `GameBoy` for the atomic
//!   save-state restore), and
//! - byte (de)serialization for save states.
//!
//! **Additive only** — no opcode, addressing, timing, or I/O-decode behaviour
//! changes, so the `SingleStepTests/spc700` validation (256/256) is untouched.
//! Kept in this one file so the verified opcode / RAM / ports modules stay
//! pristine. The attached [`Dsp`] is intentionally NOT part of `Clone` /
//! `write_state`: the owner (`slopgb-core`'s `SgbApu`) re-attaches it, since
//! `Box<dyn Dsp>` is neither `Clone` nor serializable.

use super::*;
use crate::StateError;
use crate::state::{Reader, Writer};

impl Clone for Spc700 {
    fn clone(&self) -> Self {
        Spc700 {
            a: self.a,
            x: self.x,
            y: self.y,
            sp: self.sp,
            pc: self.pc,
            psw: self.psw,
            ram: self.ram.clone(),
            port_in: self.port_in,
            port_out: self.port_out,
            test: self.test,
            control: self.control,
            dsp_addr: self.dsp_addr,
            aux: self.aux,
            dsp_shadow: self.dsp_shadow,
            timer: self.timer,
            presc_8k: self.presc_8k,
            presc_64k: self.presc_64k,
            // The DSP is owned via a trait object (not `Clone`); the SGB APU
            // re-attaches a freshly-cloned S-DSP after cloning the CPU.
            dsp: None,
            flat_mem: self.flat_mem,
            stopped: self.stopped,
            cycles: self.cycles,
        }
    }
}

impl Spc700 {
    /// Read-only APU-RAM view (BRR sample fetch, sample directory, echo buffer)
    /// — observability for the owner + tests; the synthesis path uses the
    /// mutable [`Self::apu_ram_mut`].
    pub fn apu_ram(&self) -> &[u8; 0x1_0000] {
        &self.ram
    }

    /// Mutable APU-RAM view — the DSP writes the echo buffer here, and the SGB
    /// command stream DMAs uploads (SOU_TRN / DATA_TRN / DATA_SND) into it. On
    /// real hardware the DSP and the SMP share this same 64 KB bus.
    pub fn apu_ram_mut(&mut self) -> &mut [u8; 0x1_0000] {
        &mut self.ram
    }

    /// Redirect execution to `pc` and clear the SLEEP/STOP idle flag — used when
    /// the SGB uploads a fresh SPC700 sound driver (SOU_TRN) and starts it.
    pub fn set_pc(&mut self, pc: u16) {
        self.pc = pc;
        self.stopped = false;
    }

    /// APU-side input latch of comm port `n` (0-3) — what the SNES wrote via
    /// [`Self::snes_write_port`], i.e. what the APU reads at `$F4+n`.
    /// Observability for the owner + tests.
    pub fn apu_port_in(&self, n: usize) -> u8 {
        self.port_in[n & 3]
    }

    /// Serialize volatile state (RAM + registers + I/O ports + timers) to a save
    /// state. The attached DSP is written by the owner, not here.
    pub fn write_state(&self, w: &mut Writer) {
        w.bytes(&self.ram[..]);
        w.u8(self.a);
        w.u8(self.x);
        w.u8(self.y);
        w.u8(self.sp);
        w.u16(self.pc);
        w.u8(self.psw.to_byte());
        w.bytes(&self.port_in);
        w.bytes(&self.port_out);
        w.u8(self.test);
        w.u8(self.control);
        w.u8(self.dsp_addr);
        w.bytes(&self.aux);
        w.bytes(&self.dsp_shadow);
        for t in &self.timer {
            t.write_state(w);
        }
        w.u32(self.presc_8k);
        w.u32(self.presc_64k);
        w.bool(self.flat_mem);
        w.bool(self.stopped);
        w.u64(self.cycles);
    }

    /// Restore volatile state from a save state (symmetric with
    /// [`Self::write_state`]).
    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        r.bytes_into(&mut self.ram[..])?;
        self.a = r.u8()?;
        self.x = r.u8()?;
        self.y = r.u8()?;
        self.sp = r.u8()?;
        self.pc = r.u16()?;
        self.psw = Psw::from_byte(r.u8()?);
        r.bytes_into(&mut self.port_in)?;
        r.bytes_into(&mut self.port_out)?;
        self.test = r.u8()?;
        self.control = r.u8()?;
        self.dsp_addr = r.u8()?;
        r.bytes_into(&mut self.aux)?;
        r.bytes_into(&mut self.dsp_shadow)?;
        for t in &mut self.timer {
            t.read_state(r)?;
        }
        self.presc_8k = r.u32()?;
        self.presc_64k = r.u32()?;
        self.flat_mem = r.bool()?;
        self.stopped = r.bool()?;
        self.cycles = r.u64()?;
        Ok(())
    }
}
