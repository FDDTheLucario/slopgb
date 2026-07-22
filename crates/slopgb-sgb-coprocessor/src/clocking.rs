//! The runtime pump: GB-cycle clocking, the per-flush co-simulation of the
//! two chips (ICD2 mailbox, comm-port mediation, MMIO routing, PCM output),
//! and the `AudioCoprocessor` trait wiring.
//!
//! A second `impl SgbCoprocessor` block, split out of `lib.rs` to keep it
//! under the 1000-line cap.

use super::*;

impl SgbCoprocessor {
    // -- Clocking -----------------------------------------------------------

    pub(crate) fn clock(&mut self, gb_cycles: u64) {
        self.pending_gb += gb_cycles;
        while self.pending_gb >= FLUSH_CHUNK {
            self.pending_gb -= FLUSH_CHUNK;
            self.flush(FLUSH_CHUNK);
        }
    }

    /// Pump both plugins once for a `span` of GB T-cycles: pump the ICD2
    /// window (LCD-row shadow + packet deposit), mediate the comm ports
    /// (65C816 → SPC700, then SPC700 → 65C816), advance each chip to its cycle
    /// target, pull the ICD2 pad latches back, drain the S-DSP PCM, and emit
    /// `span`'s worth of output samples.
    fn flush(&mut self, span: u64) {
        let mut tp = perf::PerfTimer::start();
        // ICD2, GB→SNES half: refresh the $6000 LCD-row shadow (fullsnes "SGB
        // Port 6000h": character row 0-$11, $11 = last row or vblank) and
        // deposit the next teed packet once the guest consumed the last one
        // ($6002 clear — never overwrite an unread mailbox).
        self.gb_pos = (self.gb_pos + span) % GB_FRAME_CYCLES;
        let line = self.gb_pos / GB_LINE_CYCLES;
        let row = ((line / 8) as u8).min(0x11);
        // Drain the guest's captured MMIO writes from the last slice and
        // apply the ones the clocking loop consumes (NMITIMEN for now; the
        // PPU/DMA routing grows here).
        let captured: Vec<(u16, u8)> = {
            let mut cpu = self.cpu.borrow_mut();
            // Two-phase drain: a 3-byte header probe reports the pending
            // count without draining; a sized read drains exactly `pending`
            // entries (clamped to full-window capacity).
            let pending = match cpu.read_ram(HW_MMIO_RING, 3) {
                Ok(h) if h.len() >= 2 => usize::from(h[0]) | usize::from(h[1]) << 8,
                _ => 0,
            };
            let bulk = if pending > 0 {
                let size = (3 + 3 * pending).min(3 + 3 * MMIO_RING_CAP);
                cpu.read_ram(HW_MMIO_RING, size)
            } else {
                Ok(Vec::new())
            };
            match bulk {
                Ok(buf) if buf.len() >= 3 => {
                    let n = usize::from(buf[0]) | usize::from(buf[1]) << 8;
                    if buf[2] != 0 {
                        eprintln!("slopgb: SNES MMIO capture ring overflowed; writes dropped");
                    }
                    buf[3..]
                        .chunks_exact(3)
                        .take(n)
                        .map(|e| (u16::from(e[0]) | u16::from(e[1]) << 8, e[2]))
                        .collect()
                }
                _ => Vec::new(),
            }
        };
        // Consecutive pure-PPU writes ($2100-$213F; the WMDATA quartet and
        // the CPU I/O block are host-consumed) apply as one batched wasm
        // call; any host-consumed register is an order barrier that flushes
        // the run first, so the guest-observable sequence is unchanged.
        let mut ppu_run: Vec<u8> = Vec::new();
        for (addr, val) in captured {
            if (0x2100..=0x213F).contains(&addr) {
                if addr == 0x2100 {
                    self.last_inidisp = val; // diagnostics (debug_status)
                    // The display shows a picture: not force-blanked and
                    // brightness above zero (fullsnes 2100h). Arms the
                    // frame handoff — see `take_snes_frame`.
                    if val & 0x80 == 0 && val & 0x0F != 0 {
                        self.snes_live = true;
                    }
                }
                if (0x2102..=0x2103).contains(&addr) && std::env::var_os("SLOPGB_OAMDBG").is_some()
                {
                    eprintln!("OAMREG {addr:04X}={val:02X}");
                }
                ppu_run.push((addr - 0x2100) as u8);
                ppu_run.push(val);
            } else {
                self.flush_ppu_run(&mut ppu_run);
                self.apply_mmio(addr, val);
            }
        }
        self.flush_ppu_run(&mut ppu_run);
        // The ring is applied — any $420B in it just ran its transfer — so
        // release a DMA-stalled CPU before this flush's run_until.
        let _ = self.cpu.get_mut().write_ram(HW_DMA_STALL, &[0]);
        tp.lap(0);
        // The scanline pump: render every framebuffer row the SNES beam has
        // passed since the last flush (display lines are 1-based, so row r
        // is complete once V > r) — one span render per flush. Runs after
        // the ring apply, so this flush's register writes land at ~10-line
        // granularity.
        if let Some(ppu) = &self.ppu {
            let v = self.gb_pos * SNES_LINES / GB_FRAME_CYCLES;
            let target = v.min(SNES_FB_H as u64) as u16;
            if self.ppu_row < target {
                let count = (target - self.ppu_row).min(255) as u8;
                let _ = ppu.borrow_mut().write_ram(
                    PPU_HW_LINE,
                    &[self.ppu_row as u8, (self.ppu_row >> 8) as u8, count],
                );
                self.ppu_row += u16::from(count);
            }
        }
        tp.lap(1);
        {
            let mut cpu = self.cpu.borrow_mut();
            // One character row per flush: land it in its rotating buffer
            // and advance the $6000 write-row shadow so the guest observes
            // every band value in sequence (18 bands/frame vs ~17 flushes
            // — near-lockstep with the real stream).
            if let Some((band, data)) = self.char_queue.pop_front() {
                let _ = cpu.write_ram(HW_CHAR_ROWS + u32::from(band % 4) * 320, &data[..]);
                self.char_write_row = band % 4;
            }
            let _ = cpu.write_ram(HW_LCD_ROW, &[row, self.char_write_row]);
            // The SNES frame clock: scale the GB frame position onto the
            // 262-line NTSC frame; on the vblank edges maintain the RDNMI
            // flag (set at begin, auto-clear at end — fullsnes 4210h; the
            // read-acknowledge runs guest-side) and deliver the NMI when
            // NMITIMEN bit 7 enables it (fullsnes 4200h).
            let v = self.gb_pos * SNES_LINES / GB_FRAME_CYCLES;
            let vblank = v >= SNES_VBLANK_LINE;
            if vblank != self.in_vblank {
                self.in_vblank = vblank;
                if vblank {
                    let _ = cpu.write_ram(HW_SHADOW + SH_RDNMI, &[0x82]);
                    if self.nmitimen & 0x80 != 0 {
                        let _ = cpu.write_ram(HW_NMI, &[1]);
                    }
                    // Joypad autopoll begins on the first vblank line when
                    // NMITIMEN bit 0 asks for it (fullsnes 4200h).
                    self.joy_busy = self.nmitimen & 1 != 0;
                    // The scanline pump completed the frame before this
                    // edge (V >= 225 implies all 224 rows rendered).
                    self.frame_ready = self.ppu.is_some();
                    self.frames_done += u64::from(self.frame_ready);
                } else {
                    let _ = cpu.write_ram(HW_SHADOW + SH_RDNMI, &[0x02]);
                    // A new frame begins as vblank ends.
                    self.ppu_row = 0;
                }
                // HVBJOY bit 7 tracks vblank, bit 0 the autopoll window
                // (bit 6 hblank is below this pump's resolution).
                let hvbjoy = (vblank as u8) << 7 | u8::from(self.joy_busy);
                let _ = cpu.write_ram(HW_SHADOW + SH_HVBJOY, &[hvbjoy]);
            } else if self.joy_busy {
                // The poll window ends (~4224 master cycles ≈ under one
                // flush): the JOY1 shadows become valid exactly when the
                // busy bit drops — mid-window reads are unreliable on
                // hardware, so nothing is published earlier.
                self.joy_busy = false;
                let (dpad, buttons) = self.input;
                let _ = cpu.write_ram(HW_SHADOW + SH_JOY1, &joy1_bytes(dpad, buttons));
                let _ = cpu.write_ram(HW_SHADOW + SH_HVBJOY, &[(vblank as u8) << 7]);
            }
            if !self.pending_packets.is_empty() {
                let clear = matches!(cpu.read_ram(HW_PACKET, 1).as_deref(), Ok([0]));
                if clear {
                    if let Some(p) = self.pending_packets.pop_front() {
                        let _ = cpu.write_ram(HW_PACKET, &p);
                    }
                }
            }
        }
        tp.lap(2);
        // Advance the chips' absolute cycle targets by the GB→chip ratios.
        self.spc_acc += span as i64 * SPC_NUM;
        let spc_adv = self.spc_acc.div_euclid(SPC_DEN).max(0) as u64;
        self.spc_acc -= spc_adv as i64 * SPC_DEN;
        self.spc_target += spc_adv;
        self.cpu_acc += span as i64 * CPU_NUM;
        let cpu_adv = self.cpu_acc.div_euclid(CPU_DEN).max(0) as u64;
        self.cpu_acc -= cpu_adv as i64 * CPU_DEN;
        self.cpu_target += cpu_adv;

        // 1+2. Interleaved co-simulation, in rounds: drain the 65C816's
        // ordered APU-port write ring, replay each write into the SPC700 with
        // a consume slice, hand the echoes back and give the CPU a produce
        // slice — then drain again, because those CPU slices produce the
        // *next* writes (an echo-paced upload advances exactly one byte per
        // round). Without the rounds a whole flush moved one byte; the bound
        // keeps a flush finite. Ordered per-event replay matters: the IPL
        // upload's index/ack pump repeats mod-256 values, so delivering only
        // final latch states aliases the handshake and desyncs the chips.
        //
        // A comm port is a register, not a queue: each write needs the SPC700
        // to run enough cycles to consume it before the next lands (the IPL's
        // per-byte pump is ~25 cycles — fullsnes "Boot ROM"), and the CPU
        // needs enough to check the echo and produce the next byte. The
        // per-event floors let both chips run ahead of their clock targets
        // during a burst; the positions are anchored across flushes
        // (`spc_pos`/`cpu_pos`), and later flush targets simply no-op until
        // real time catches back up.
        {
            let spc_start = self.spc_target - spc_adv;
            let cpu_start = self.cpu_target - cpu_adv;
            let dbg = std::env::var_os("SLOPGB_APUDBG").is_some();
            const SPC_MIN_EVENT_CYCLES: u64 = 64;
            const CPU_MIN_EVENT_CYCLES: u64 = 64;
            const MAX_MEDIATION_ROUNDS: usize = 512;
            let mut spc_pos = self.spc_pos.max(spc_start);
            let mut cpu_pos = self.cpu_pos.max(cpu_start);
            for _ in 0..MAX_MEDIATION_ROUNDS {
                let events: Vec<(u8, u8)> = {
                    let mut cpu = self.cpu.borrow_mut();
                    // Two-phase drain (see the MMIO ring): header probe
                    // first, a sized read drains exactly `pending` entries.
                    let pending = match cpu.read_ram(HW_PORT_RING, 3) {
                        Ok(h) if h.len() >= 2 => usize::from(h[0]) | usize::from(h[1]) << 8,
                        _ => 0,
                    };
                    let bulk = if pending > 0 {
                        let size = (3 + 2 * pending).min(3 + 2 * PORT_RING_CAP);
                        cpu.read_ram(HW_PORT_RING, size)
                    } else {
                        Ok(Vec::new())
                    };
                    match bulk {
                        Ok(buf) if buf.len() >= 3 => {
                            let n = usize::from(buf[0]) | usize::from(buf[1]) << 8;
                            if buf[2] != 0 {
                                eprintln!("slopgb: SNES APU port ring overflowed; writes dropped");
                            }
                            buf[3..]
                                .chunks_exact(2)
                                .take(n)
                                .map(|e| (e[0], e[1]))
                                .collect()
                        }
                        _ => Vec::new(),
                    }
                };
                if events.is_empty() {
                    break;
                }
                for &(p, v) in &events {
                    let mut spc = self.spc.borrow_mut();
                    let _ = spc.port_write(p, v);
                    if usize::from(p) < N_PORTS {
                        self.to_spc[usize::from(p)] = v;
                    }
                    spc_pos += SPC_MIN_EVENT_CYCLES;
                    let _ = spc.run_until(spc_pos);
                    let mut echoes = [0u8; N_PORTS];
                    for (q, slot) in echoes.iter_mut().enumerate() {
                        *slot = spc.port_read(q as u8).unwrap_or(0);
                    }
                    if dbg {
                        eprintln!(
                            "apu<- p{p}={v:02X} | out {:02X} {:02X}",
                            echoes[0], echoes[1]
                        );
                    }
                    drop(spc);
                    let mut cpu = self.cpu.borrow_mut();
                    for (q, &e) in echoes.iter().enumerate() {
                        let _ = cpu.port_write(q as u8, e);
                    }
                    cpu_pos += CPU_MIN_EVENT_CYCLES;
                    let _ = cpu.run_until(cpu_pos);
                }
            }
            tp.lap(3);
            // Run the SPC700 + S-DSP to the target (or wherever the event
            // floor left it, if that is already past) and pull the PCM.
            self.spc_pos = self.spc_target.max(spc_pos);
            self.cpu_pos = self.cpu_target.max(cpu_pos);
            let mut spc = self.spc.borrow_mut();
            let _ = spc.run_until(self.spc_pos);
            if let Ok(batch) = spc.drain_pcm() {
                self.src.extend(batch);
            }
        }
        tp.lap(4);
        // 3. Read the SPC700's final comm-port replies back for the 65C816.
        let mut spc_out = [0u8; N_PORTS];
        {
            let mut spc = self.spc.borrow_mut();
            for (p, slot) in spc_out.iter_mut().enumerate() {
                *slot = spc.port_read(p as u8).unwrap_or(0);
            }
        }
        {
            let mut cpu = self.cpu.borrow_mut();
            for (p, &v) in spc_out.iter().enumerate() {
                let _ = cpu.port_write(p as u8, v);
            }
            // 4. Run the 65C816 to its target (or the event floor's position).
            let _ = cpu.run_until(self.cpu_target.max(self.cpu_pos));
            // ICD2, SNES→GB half: drain the ordered pad-latch write ring.
            // Every write becomes one queued feed snapshot, so a sub-flush
            // protocol sequence (the takeover init's one-shot Select+Start
            // trigger chased by the hook's ACK sandwich) reaches the GB one
            // value per step instead of collapsing to the final latch state.
            // The sticky written flag still gates the takeover as a whole —
            // before the SNES side writes a latch, the GB's local matrix
            // must stay live.
            if let Ok(v) = cpu.read_ram(HW_PADS, 5) {
                if let [p1, p2, p3, p4, written] = v[..] {
                    if written != 0 {
                        self.pads_taken = true;
                        self.pads_shadow = [p1, p2, p3, p4];
                    }
                }
            }
            // Two-phase drain (see the MMIO ring): header probe first, sized read.
            let pad_pending = match cpu.read_ram(HW_PAD_RING, 2) {
                Ok(h) if !h.is_empty() => usize::from(h[0]),
                _ => 0,
            };
            let pad_bulk = if pad_pending > 0 {
                let size = (2 + 2 * pad_pending).min(2 + 2 * PAD_RING_CAP);
                cpu.read_ram(HW_PAD_RING, size)
            } else {
                Ok(Vec::new())
            };
            if let Ok(ring) = pad_bulk {
                if ring.len() >= 2 {
                    let n = usize::from(ring[0]);
                    let mut pads = self.feed_queue.back().copied().unwrap_or(self.pads_shadow);
                    for i in 0..n {
                        let (r, v) = (ring[2 + i * 2], ring[3 + i * 2]);
                        if usize::from(r) < 4 {
                            pads[usize::from(r)] = v;
                            self.feed_queue.push_back(pads);
                        }
                    }
                    while self.feed_queue.len() > FEED_QUEUE_CAP {
                        self.feed_queue.pop_front();
                    }
                }
            }
        }
        tp.lap(5);
        // 5. Emit output-rate samples (32 kHz S-DSP → output rate, zero-order-hold).
        self.emit_output(span);
        tp.lap(6);
        tp.finish();
    }

    /// Emit the output-rate samples owed for a `span` of GB T-cycles, resampling
    /// the 32 kHz S-DSP source by holding the current sample (32 kHz < output).
    fn emit_output(&mut self, span: u64) {
        self.samp_acc += span as f64;
        while self.samp_acc >= self.cycles_per_sample {
            self.samp_acc -= self.cycles_per_sample;
            self.src_acc += DSP_RATE;
            while self.src_acc >= f64::from(self.out_rate) {
                self.src_acc -= f64::from(self.out_rate);
                if let Some(s) = self.src.pop_front() {
                    self.cur = s;
                }
            }
            if self.out.len() < self.max_out {
                self.out.push((
                    f32::from(self.cur.0) * MIX_SCALE,
                    f32::from(self.cur.1) * MIX_SCALE,
                ));
            }
        }
    }

    fn mix_into(&mut self, gb: &mut [(f32, f32)]) {
        let n = gb.len().min(self.out.len());
        for (dst, src) in gb.iter_mut().zip(self.out.iter()).take(n) {
            dst.0 += src.0;
            dst.1 += src.1;
        }
        self.out.drain(..n);
    }

    fn set_output_rate(&mut self, hz: u32) {
        let hz = hz.max(1);
        self.out_rate = hz;
        self.cycles_per_sample = f64::from(GB_CLOCK_HZ) / f64::from(hz);
        self.max_out = hz as usize;
        self.samp_acc = 0.0;
        self.src_acc = 0.0;
        self.src.clear();
        self.out.clear();
    }

    /// Drain the stereo output-rate PCM synthesized since the last drain, oldest
    /// first — the equivalent of the tier-3 plugin ABI's `drain_pcm`, for a host
    /// that would rather pull the samples than have them mixed in.
    pub fn drain_pcm(&mut self) -> Vec<(f32, f32)> {
        std::mem::take(&mut self.out)
    }

    /// Apply a buffered run of pure-PPU `(port, val)` pairs as one batched
    /// plugin call, in order, and clear the buffer. No-op when empty or
    /// without a PPU plugin (matching the unbatched path's routing).
    pub(crate) fn flush_ppu_run(&mut self, run: &mut Vec<u8>) {
        if run.is_empty() {
            return;
        }
        if let Some(ppu) = &self.ppu {
            let _ = ppu.borrow_mut().write_ram(PPU_HW_PORTS, run);
        }
        run.clear();
    }

    /// Apply one captured MMIO write from the guest (also the target of DMA
    /// B-bus writes — `dma::bbus_write` routes through here). The clocking
    /// loop consumes NMITIMEN; the DMA engine consumes the channel registers,
    /// MDMAEN, and the WRAM access ports; every other B-bus port routes to the
    /// PPU when one is loaded.
    pub(crate) fn apply_mmio(&mut self, addr: u16, val: u8) {
        match addr {
            0x2180 => self.wmdata_write(val),
            0x2181 => self.wmadd = self.wmadd & 0x1_FF00 | u32::from(val),
            0x2182 => self.wmadd = self.wmadd & 0x1_00FF | u32::from(val) << 8,
            // WMADDH: one bit — WMADD addresses 128 KB (fullsnes 2183h).
            0x2183 => self.wmadd = self.wmadd & 0xFFFF | u32::from(val & 1) << 16,
            0x4200 => self.nmitimen = val,
            0x420B => self.run_gp_dma(val),
            0x4300..=0x437F if usize::from(addr & 0xF) < 7 => {
                self.dma_regs[usize::from(addr >> 4 & 7)][usize::from(addr & 0xF)] = val;
            }
            // Every other B-bus port belongs to the PPU when one is loaded
            // (unknown ports are inert inside the chip). $2140-$2143 only
            // arrive via DMA (the CPU-side APU ports route earlier) — a
            // DMA-to-APU transfer is unimplemented and lands inert too.
            0x2100..=0x21FF => {
                if (0x2102..=0x2103).contains(&addr) && std::env::var_os("SLOPGB_OAMDBG").is_some()
                {
                    eprintln!("OAMREG {addr:04X}={val:02X}");
                }
                if addr == 0x2100 {
                    self.last_inidisp = val; // diagnostics (debug_status)
                    // The display shows a picture: not force-blanked and
                    // brightness above zero (fullsnes 2100h). Arms the
                    // frame handoff — see `take_snes_frame`.
                    if val & 0x80 == 0 && val & 0x0F != 0 {
                        self.snes_live = true;
                    }
                }
                if let Some(ppu) = &self.ppu {
                    let _ = ppu.borrow_mut().port_write((addr - 0x2100) as u8, val);
                }
            }
            _ => {}
        }
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
}

impl AudioCoprocessor for SgbCoprocessor {
    fn clock(&mut self, gb_cycles: u64) {
        SgbCoprocessor::clock(self, gb_cycles);
    }
    fn poll(&mut self, cmds: &mut dyn SgbCommandSource) {
        SgbCoprocessor::poll(self, cmds);
    }
    fn joypad_feed(&mut self) -> Option<[u8; 4]> {
        // Queued latch writes first, each dwelling long enough for the GB's
        // polls to see it (ordered protocol sequences — an ACK handshake, a
        // one-shot phase trigger). With the queue idle, forward the local
        // matrix — the resident BIOS's continuous pad pass-through, the
        // only way the player's buttons reach a taken-over GB (ICD2 latch
        // encoding: buttons high nibble, d-pad low, active low).
        if self.feed_hold > 0 {
            self.feed_hold -= 1;
            return Some(self.pads_shadow);
        }
        if let Some(pads) = self.feed_queue.pop_front() {
            self.pads_shadow = pads;
            self.feed_hold = FEED_DWELL_STEPS;
            return Some(pads);
        }
        if self.pads_taken {
            let (dpad, buttons) = self.input;
            Some([buttons << 4 | dpad & 0x0F, 0xFF, 0xFF, 0xFF])
        } else {
            None
        }
    }
    fn set_input(&mut self, dpad: u8, buttons: u8) {
        self.input = (dpad, buttons);
    }
    fn take_frame(&mut self) -> Option<Vec<u16>> {
        self.take_snes_frame()
    }
    fn mix_into(&mut self, out: &mut [(f32, f32)]) {
        SgbCoprocessor::mix_into(self, out);
    }
    fn set_output_rate(&mut self, hz: u32) {
        SgbCoprocessor::set_output_rate(self, hz);
    }
    fn load_bios(&mut self, _bios: &[u8]) {
        // The resident clean-room firmware is fixed; there is no user BIOS image
        // to install (and slopgb never reads the copyrighted SGB system ROM).
    }
    fn write_state(&self, w: &mut Writer) {
        SgbCoprocessor::write_state(self, w);
    }
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        SgbCoprocessor::read_state(self, r)
    }
    fn clone_box(&self) -> Box<dyn AudioCoprocessor> {
        // Re-instantiating the already-validated plugin wasm can only fail on an
        // allocation error (which aborts anyway), so this is near-unreachable —
        // but a save-state clone must never panic the emulator. Degrade to a
        // silent inert coprocessor and log, rather than `.expect`.
        match self.deep_clone() {
            Ok(fresh) => Box::new(fresh),
            Err(e) => {
                eprintln!(
                    "slopgb: SGB coprocessor clone failed ({e}); audio inert for this snapshot"
                );
                Box::new(InertCoprocessor)
            }
        }
    }

    fn debug_status(&self) -> String {
        // The run-cycle targets grow only while the host clocks the chips, so a
        // zero here means the coprocessor loaded but was never driven (the
        // machine isn't in SGB mode, or the GB is sending nothing) — the exact
        // "SNES side isn't running" case. Non-zero = the chips are executing.
        let running = self.cpu_target > 0 || self.spc_target > 0;
        let ppu = match &self.ppu {
            Some(_) => format!(
                "SNES PPU plugin loaded: {} frames rendered, last INIDISP ${:02X}",
                self.frames_done, self.last_inidisp
            ),
            None => "no SNES PPU plugin (audio-only)".into(),
        };
        format!(
            "wasm SGB coprocessor: SPC700 + 65C816 plugins loaded; {} \
             (65C816 ran to cyc {}, SPC700 to cyc {}); last GB->SPC ports {:02X?}; {}",
            if running {
                "RUNNING"
            } else {
                "NOT yet clocked"
            },
            self.cpu_target,
            self.spc_target,
            self.to_spc,
            ppu,
        )
    }
}
