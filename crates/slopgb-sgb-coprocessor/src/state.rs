//! Save-state serialization for [`SgbCoprocessor`] (the two plugins' opaque
//! state blocks + the host-side runtime), the deep clone used by
//! `AudioCoprocessor::clone_box`, and the [`InertCoprocessor`] fallback that
//! keeps the on-disk layout parseable when no plugins are live.

use super::*;

impl SgbCoprocessor {
    pub(crate) fn write_state(&self, w: &mut Writer) {
        let spc = self.spc.borrow_mut().save_state().unwrap_or_else(|e| {
            eprintln!("slopgb: SGB coprocessor SPC700 save_state failed: {e}");
            Vec::new()
        });
        let cpu = self.cpu.borrow_mut().save_state().unwrap_or_else(|e| {
            eprintln!("slopgb: SGB coprocessor 65C816 save_state failed: {e}");
            Vec::new()
        });
        w.u32(spc.len() as u32);
        w.bytes(&spc);
        w.u32(cpu.len() as u32);
        w.bytes(&cpu);
        w.u64(self.spc_target);
        w.u64(self.cpu_target);
        w.u64(self.spc_acc as u64);
        w.u64(self.cpu_acc as u64);
        w.u64(self.pending_gb);
        for &b in &self.to_spc {
            w.u8(b);
        }
        w.u64(self.src_acc.to_bits());
        w.u16(self.cur.0 as u16);
        w.u16(self.cur.1 as u16);
        w.u64(self.samp_acc.to_bits());
        w.u32(self.poll_ctr);
        w.u64(self.sou_trn_sig);
        w.u64(self.data_trn_sig);
        w.bool(self.jump.is_some());
        w.u32(self.jump.unwrap_or(0));
        w.u8(self.pending_packets.len() as u8);
        for p in &self.pending_packets {
            w.bytes(p);
        }
        w.u64(self.gb_pos);
        w.u8(self.nmitimen);
        w.bool(self.in_vblank);
        w.u8(self.pending_trn.len() as u8);
        for p in &self.pending_trn {
            w.bytes(p);
        }
        for ch in &self.dma_regs {
            w.bytes(ch);
        }
        w.u32(self.wmadd);
        w.bool(self.joy_busy);
        w.bool(self.trn_flip);
        // The optional PPU plugin: a length-prefixed opaque block (empty =
        // no PPU loaded) + the scanline-pump cursor and frame flag.
        let ppu = match &self.ppu {
            Some(p) => p.borrow_mut().save_state().unwrap_or_else(|e| {
                eprintln!("slopgb: SGB coprocessor PPU save_state failed: {e}");
                Vec::new()
            }),
            None => Vec::new(),
        };
        w.u32(ppu.len() as u32);
        w.bytes(&ppu);
        w.u16(self.ppu_row);
        w.bool(self.frame_ready);
    }

    pub(crate) fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        let n = r.u32()? as usize;
        let spc = r.bytes_vec(n)?;
        let n = r.u32()? as usize;
        let cpu = r.bytes_vec(n)?;
        if let Err(e) = self.spc.get_mut().load_state(&spc) {
            eprintln!("slopgb: SGB coprocessor SPC700 load_state failed: {e}");
        }
        if let Err(e) = self.cpu.get_mut().load_state(&cpu) {
            eprintln!("slopgb: SGB coprocessor 65C816 load_state failed: {e}");
        }
        self.spc_target = r.u64()?;
        self.cpu_target = r.u64()?;
        // Pacing state, not serialized: after a load the chips sit exactly at
        // their targets.
        self.spc_pos = self.spc_target;
        self.cpu_pos = self.cpu_target;
        self.spc_acc = r.u64()? as i64;
        self.cpu_acc = r.u64()? as i64;
        self.pending_gb = r.u64()?;
        for slot in &mut self.to_spc {
            *slot = r.u8()?;
        }
        self.src_acc = f64::from_bits(r.u64()?);
        self.cur.0 = r.u16()? as i16;
        self.cur.1 = r.u16()? as i16;
        self.samp_acc = f64::from_bits(r.u64()?);
        self.poll_ctr = r.u32()?;
        self.sou_trn_sig = r.u64()?;
        self.data_trn_sig = r.u64()?;
        let has_jump = r.bool()?;
        let j = r.u32()?;
        self.jump = has_jump.then_some(j);
        let n = usize::from(r.u8()?);
        if n > PACKET_QUEUE_CAP {
            return Err(StateError::Truncated);
        }
        self.pending_packets.clear();
        for _ in 0..n {
            let mut p = [0u8; 16];
            r.bytes_into(&mut p)?;
            self.pending_packets.push_back(p);
        }
        self.gb_pos = r.u64()? % GB_FRAME_CYCLES;
        self.nmitimen = r.u8()?;
        self.in_vblank = r.bool()?;
        let n = usize::from(r.u8()?);
        if n > PACKET_QUEUE_CAP {
            return Err(StateError::Truncated);
        }
        self.pending_trn.clear();
        for _ in 0..n {
            let mut p = [0u8; 16];
            r.bytes_into(&mut p)?;
            self.pending_trn.push_back(p);
        }
        for ch in &mut self.dma_regs {
            r.bytes_into(ch)?;
        }
        self.wmadd = r.u32()? & 0x1_FFFF;
        self.joy_busy = r.bool()?;
        self.trn_flip = r.bool()?;
        let n = r.u32()? as usize;
        let ppu_state = r.bytes_vec(n)?;
        if let Some(p) = &self.ppu {
            // A state saved without a PPU loads into a PPU-bearing machine
            // with the chip left at reset (nothing to restore).
            if !ppu_state.is_empty() {
                if let Err(e) = p.borrow_mut().load_state(&ppu_state) {
                    eprintln!("slopgb: SGB coprocessor PPU load_state failed: {e}");
                }
            }
        }
        self.ppu_row = r.u16()?.min(SNES_FB_H as u16);
        self.frame_ready = r.bool()?;
        // The undrained source/output PCM is transient, not part of the
        // snapshot — and so are the pad feed and the pushed input matrix
        // (the core re-supplies both on the next step/flush).
        self.src.clear();
        self.out.clear();
        self.feed = None;
        self.input = (0x0F, 0x0F);
        Ok(())
    }

    /// Deep-clone: re-instantiate the two plugins from the kept wasm bytes, load
    /// this coprocessor's current chip state into them, and copy the host-side
    /// runtime. Used by [`AudioCoprocessor::clone_box`] for the save-state restore
    /// that clones the whole `GameBoy`.
    pub(crate) fn deep_clone(&self) -> Result<Self, LoadError> {
        let spc_state = self.spc.borrow_mut().save_state().unwrap_or_else(|e| {
            eprintln!("slopgb: SGB coprocessor SPC700 save_state failed on clone: {e}");
            Vec::new()
        });
        let cpu_state = self.cpu.borrow_mut().save_state().unwrap_or_else(|e| {
            eprintln!("slopgb: SGB coprocessor 65C816 save_state failed on clone: {e}");
            Vec::new()
        });
        let ppu_state = self.ppu.as_ref().map(|p| {
            p.borrow_mut().save_state().unwrap_or_else(|e| {
                eprintln!("slopgb: SGB coprocessor PPU save_state failed on clone: {e}");
                Vec::new()
            })
        });
        let mut fresh = Self::from_wasm_full(
            &self.spc_wasm,
            &self.cpu_wasm,
            self.ppu_wasm.as_deref(),
            self.out_rate,
        )?;
        if let (Some(p), Some(state)) = (&fresh.ppu, &ppu_state) {
            if !state.is_empty() {
                if let Err(e) = p.borrow_mut().load_state(state) {
                    eprintln!("slopgb: SGB coprocessor PPU load_state failed on clone: {e}");
                }
            }
        }
        if let Err(e) = fresh.spc.get_mut().load_state(&spc_state) {
            eprintln!("slopgb: SGB coprocessor SPC700 load_state failed on clone: {e}");
        }
        if let Err(e) = fresh.cpu.get_mut().load_state(&cpu_state) {
            eprintln!("slopgb: SGB coprocessor 65C816 load_state failed on clone: {e}");
        }
        fresh.spc_target = self.spc_target;
        fresh.cpu_target = self.cpu_target;
        fresh.spc_pos = self.spc_pos;
        fresh.cpu_pos = self.cpu_pos;
        fresh.spc_acc = self.spc_acc;
        fresh.cpu_acc = self.cpu_acc;
        fresh.pending_gb = self.pending_gb;
        fresh.to_spc = self.to_spc;
        fresh.src = self.src.clone();
        fresh.src_acc = self.src_acc;
        fresh.cur = self.cur;
        fresh.samp_acc = self.samp_acc;
        fresh.out = self.out.clone();
        fresh.poll_ctr = self.poll_ctr;
        fresh.sou_trn_sig = self.sou_trn_sig;
        fresh.data_trn_sig = self.data_trn_sig;
        fresh.jump = self.jump;
        fresh.pending_packets = self.pending_packets.clone();
        fresh.feed = self.feed;
        fresh.gb_pos = self.gb_pos;
        fresh.nmitimen = self.nmitimen;
        fresh.in_vblank = self.in_vblank;
        fresh.pending_trn = self.pending_trn.clone();
        fresh.dma_regs = self.dma_regs;
        fresh.wmadd = self.wmadd;
        fresh.input = self.input;
        fresh.joy_busy = self.joy_busy;
        fresh.trn_flip = self.trn_flip;
        fresh.ppu_row = self.ppu_row;
        fresh.frame_ready = self.frame_ready;
        Ok(fresh)
    }
}

/// The on-disk state layout [`SgbCoprocessor::write_state`] emits, all zeroed
/// (empty chip-state blocks + zeroed runtime). [`InertCoprocessor`] writes this
/// so a machine saved with an inert coprocessor still reloads through
/// [`SgbCoprocessor::read_state`]. **Keep in sync with `write_state`.**
fn write_empty_state(w: &mut Writer) {
    w.u32(0); // spc state len
    w.u32(0); // cpu state len
    for _ in 0..5 {
        w.u64(0); // spc/cpu target, spc/cpu acc, pending_gb
    }
    for _ in 0..N_PORTS {
        w.u8(0); // to_spc
    }
    w.u64(0); // src_acc
    w.u16(0); // cur.0
    w.u16(0); // cur.1
    w.u64(0); // samp_acc
    w.u32(0); // poll_ctr
    w.u64(0); // sou_trn_sig
    w.u64(0); // data_trn_sig
    w.bool(false); // jump present
    w.u32(0); // jump target
    w.u8(0); // pending ICD2 packets
    w.u64(0); // gb_pos
    w.u8(0); // nmitimen
    w.bool(false); // in_vblank
    w.u8(0); // pending DATA_TRN packets
    w.bytes(&[0u8; 7 * 8]); // dma channel registers
    w.u32(0); // wmadd
    w.bool(false); // joy_busy
    w.bool(false); // trn_flip
    w.u32(0); // ppu state len (no PPU)
    w.u16(0); // ppu_row
    w.bool(false); // frame_ready
}

/// A no-op [`AudioCoprocessor`] producing silence. Only ever the result of
/// [`SgbCoprocessor::clone_box`] failing to re-instantiate the plugin wasm
/// (near-impossible — the bytes loaded once already), so a save-state can never
/// panic the emulator.
pub(crate) struct InertCoprocessor;

impl AudioCoprocessor for InertCoprocessor {
    fn clock(&mut self, _gb_cycles: u64) {}
    fn poll(&mut self, _cmds: &mut dyn SgbCommandSource) {}
    fn mix_into(&mut self, _out: &mut [(f32, f32)]) {}
    fn set_output_rate(&mut self, _hz: u32) {}
    fn load_bios(&mut self, _bios: &[u8]) {}
    fn write_state(&self, w: &mut Writer) {
        write_empty_state(w);
    }
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        // Consume (and discard) the same layout, so an inert-then-restored path
        // stays byte-aligned.
        let n = r.u32()? as usize;
        r.bytes_vec(n)?;
        let n = r.u32()? as usize;
        r.bytes_vec(n)?;
        for _ in 0..5 {
            r.u64()?;
        }
        for _ in 0..N_PORTS {
            r.u8()?;
        }
        r.u64()?;
        r.u16()?;
        r.u16()?;
        r.u64()?;
        r.u32()?;
        r.u64()?;
        r.u64()?;
        r.bool()?;
        r.u32()?;
        let n = usize::from(r.u8()?);
        for _ in 0..n {
            let mut p = [0u8; 16];
            r.bytes_into(&mut p)?;
        }
        r.u64()?;
        r.u8()?;
        r.bool()?;
        let n = usize::from(r.u8()?);
        for _ in 0..n {
            let mut p = [0u8; 16];
            r.bytes_into(&mut p)?;
        }
        let mut dma = [0u8; 7 * 8];
        r.bytes_into(&mut dma)?;
        r.u32()?;
        r.bool()?;
        let n = r.u32()? as usize;
        r.bytes_vec(n)?;
        r.u16()?;
        r.bool()?;
        Ok(())
    }
    fn clone_box(&self) -> Box<dyn AudioCoprocessor> {
        Box::new(InertCoprocessor)
    }
}
