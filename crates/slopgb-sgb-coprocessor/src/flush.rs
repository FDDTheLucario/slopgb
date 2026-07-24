//! The clocking pump: the Game Boy cycle stream batched into [`FLUSH_CHUNK`]
//! passes, and everything one pass does — apply the guest's captured MMIO
//! writes, rasterize the scanlines the SNES beam has passed, maintain the
//! ICD2 window and the SNES frame / NMI / autopoll shadows, mediate the two
//! chips' comm ports, and advance the MSU-1 chip.

use super::*;

impl SgbCoprocessor {
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
        // granularity. See `render.rs` for the fast-forward render-skip.
        self.pump_ppu_scanlines();
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
        // 5. Advance the MSU-1 chip + refresh its $2000 read shadow, then emit
        // output-rate samples (S-DSP + MSU-1 → output rate, zero-order-hold).
        self.pump_msu(span);
        self.emit_output(span);
        // From-start capture: once the resident engine has run a frame past the
        // play command (`nspc_flush` armed `capture_at`), the song-init has
        // finished — grab a self-sustaining snapshot for a from-the-top export.
        if let Some(at) = self.capture_at {
            if self.spc_pos >= at {
                self.capture_at = None;
                if let Ok(spc) = self.spc.borrow_mut().dump_spc() {
                    if !spc.is_empty() {
                        self.song_start_spc = Some(spc);
                    }
                }
            }
        }
        tp.lap(6);
        tp.finish();
    }

    /// Advance the MSU-1 chip a `span` of GB T-cycles' worth of 44.1 kHz samples,
    /// drain its PCM into `msu_src` for the mix, and refresh the CPU's read shadow
    /// for `$2000-$2007` (MSU_STATUS + the `S-MSU1` id) so the game's resident
    /// SNES-side handler can detect the chip and poll its status. No-op — and no
    /// presence advertised — without a loaded plugin + a track pack.
    fn pump_msu(&mut self, span: u64) {
        if self.msu.is_none() {
            return;
        }
        // GB T-cycles → 44.1 kHz chip cycles (one cycle == one output sample),
        // fractional carry in `msu_acc` — the same ratio law as the SPC/CPU.
        self.msu_acc += span as i64 * MSU_RATE as i64;
        let adv = self.msu_acc.div_euclid(i64::from(GB_CLOCK_HZ)).max(0) as u64;
        self.msu_acc -= adv as i64 * i64::from(GB_CLOCK_HZ);
        self.msu_cycle += adv;
        let present = self.msu_present;
        let (batch, status) = {
            let mut cop = self.msu.as_ref().unwrap().borrow_mut();
            let _ = cop.run_until(self.msu_cycle);
            let batch = cop.drain_pcm().unwrap_or_default();
            // port_read(0) = MSU_STATUS (a pure read; the data-read port 1 is the
            // only one with a side effect, and it is never mirrored).
            let status = if present {
                cop.port_read(0).unwrap_or(0)
            } else {
                0
            };
            (batch, status)
        };
        self.msu_src.extend(batch);
        self.msu_playing = present && status & MSU_ST_PLAYING != 0;
        let mut shadow = [0u8; 8];
        if present {
            shadow[0] = status;
            shadow[2..8].copy_from_slice(&MSU_ID);
        }
        let _ = self.cpu.borrow_mut().write_ram(HW_MSU, &shadow);
    }
}
