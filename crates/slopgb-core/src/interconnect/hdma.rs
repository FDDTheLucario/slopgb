//! CGB VRAM (HBlank/General) DMA request engine: bus-stealing service seam, 2-bytes/cycle (1 in double speed), teardown cycle, halt/stop deferral. gambatte memory.cpp Hdma. Oracle: gbtr hdma_*, same-suite hdma.

use super::*;

impl Interconnect {
    /// Service any flagged VRAM-DMA request at the *head* of a CPU bus
    /// operation, before that operation's machine tick: the bus steal sits
    /// between the M-cycle whose dots flagged the request and the CPU's
    /// next M-cycle. M-cycle (not instruction) granularity is what the
    /// gambatte hdma_late_destl/_length/_wrambank and hdma_start `_1`/`_2`
    /// adjacent-cycle pairs resolve, and they also split by access type:
    /// a *write* whose own cycle contains the trigger commits before the
    /// steal (hdma_late_destl_1: the racing FF54 write wins), while a
    /// *read* in the trigger cycle yields — the DMA owns the bus before
    /// the read samples (hdma_start_2: the fetch's data byte is the
    /// post-block value), hence the second `service_vram_dma` call after
    /// the tick in `Bus::read`/`read_inc`. A request flagged during the
    /// STOP opcode fetch is deliberately still pending when `Bus::stop`
    /// runs (no bus operation in between) — the
    /// hdma_transition_speedchange matrix needs exactly that request.
    pub(super) fn service_vram_dma(&mut self) {
        while self.vram_dma_req.is_some() && !self.vram_dma_stall {
            self.run_vram_dma();
        }
    }

    /// Service one flagged VRAM-DMA request, stalling the CPU while the
    /// rest of the machine keeps running. Pace: 2 bytes per stolen M-cycle
    /// at normal speed, 1 in double speed (gambatte memory.cpp `dma()`:
    /// `cc += 2 + 2 * doubleSpeed` per byte), plus one teardown M-cycle
    /// per service (`cc += 4`) — skipped when the block was deferred by a
    /// halt wake (`Memory::event` intevent_dma: `cc -= 4` for an
    /// unhalt-requested block).
    pub(super) fn run_vram_dma(&mut self) {
        let Some(req) = self.vram_dma_req.take() else {
            return;
        };
        probe!(if crate::probe::s5dbg_on() {
            let (l, d) = self.ppu.scan_pos();
            eprintln!(
                "SLOPGB hdmarun ly={l} dot={d} clk={} req={req:?} src={:04x} dst={:04x}",
                self.clock.now(),
                self.hdma_src,
                self.hdma_dst
            );
        });
        self.vram_dma_stall = true;
        let mut remaining = (usize::from(self.hdma5 & 0x7F) + 1) * 0x10;
        let mut length = if req == VramDmaReq::Gdma {
            remaining
        } else {
            0x10
        };
        // The full 16-bit destination counter terminates the transfer at
        // the 0x10000 crossing, latching FF55 bit 7 (gambatte dma():
        // `if (dmaDest + length >= 0x10000) { length = 0x10000 - dmaDest;
        // ioamhram_[0x155] |= 0x80; }`; hardware capture dma/dma_dst_wrap_2
        // — the transfer does NOT wrap back into VRAM).
        let to_wrap = 0x1_0000 - usize::from(self.hdma_dst);
        if length >= to_wrap {
            length = to_wrap;
            self.hdma5 |= 0x80;
        }
        remaining -= length;
        // A GDMA with the display off always retires its whole length
        // (gambatte dma(): `if (!(lcdc & en) && gdmaReqFlagged) dmaLength
        // = 0`), so a 0x10000-truncated copy still reads back $FF.
        if req == VramDmaReq::Gdma && !self.ppu.lcd_enabled() {
            remaining = 0;
        }
        let per_m = if self.double_speed { 1 } else { 2 };
        while length > 0 {
            // The byte-copy M-cycles own the bus: the OAM DMA controller's
            // own clocking is suppressed (the teardown cycle below is a
            // plain machine cycle again — gambatte restores
            // lastOamDmaUpdate_ before its `cc += 4`).
            self.vram_dma_owns_bus = true;
            self.tick_machine();
            self.vram_dma_owns_bus = false;
            for i in 0..per_m.min(length) {
                let src = self.hdma_src;
                let byte = self.vram_dma_source_read(src);
                self.ppu
                    .vram_write_raw(0x8000 | (self.hdma_dst & 0x1FFF), byte);
                self.hdma_src = src.wrapping_add(1);
                self.hdma_dst = self.hdma_dst.wrapping_add(1);
                length -= 1;
                if i + 1 == per_m {
                    // A concurrent OAM DMA advances once per stolen
                    // M-cycle, latching this cycle's last bus byte.
                    self.oam_dma_bus_capture(src, byte);
                }
            }
        }
        if req != VramDmaReq::HblankUnhalt {
            self.tick_machine(); // teardown (gambatte dma(): cc += 4)
        }
        if self.cpu_halted {
            // A block serviced while the core clock is gated — only the
            // speed-switch pause can do this (see `Interconnect::stop`) —
            // aborts the HBlank transfer: FF55 keeps its length bits and
            // gains bit 7 (gambatte dma(): `ioamhram_[0x155] = halted() ?
            // ioamhram_[0x155] | 0x80 : …`; pinned by
            // hdma_transition_speedchange_hdmalen*_hdma5).
            self.hdma5 |= 0x80;
        } else {
            self.hdma5 = ((remaining / 0x10) as u8).wrapping_sub(1) | (self.hdma5 & 0x80);
        }
        if self.hdma5 & 0x80 != 0 {
            // Completion/termination/abort disarms the HBlank engine
            // (gambatte dma(): `if ((FF55 & 0x80) && hdmaIsEnabled())
            // lcd_.disableHdma(cc)`).
            self.hdma_mode = HdmaMode::Disabled;
        }
        self.vram_dma_stall = false;
    }

    /// Wake-side HBlank-DMA re-evaluation, shared by halt/stop wake and
    /// the end of the speed-switch pause (gambatte Memory::event,
    /// `intevent_unhalt` and the halted `intevent_interrupts` path): a
    /// deferred request re-flags (without the teardown cycle), and an
    /// armed transfer whose halt began outside the hblank window fires if
    /// the wake lands inside one.
    pub(super) fn vram_dma_unhalt(&mut self) {
        match self.halt_hdma {
            HaltHdmaState::Requested => self.vram_dma_req = Some(VramDmaReq::HblankUnhalt),
            HaltHdmaState::Low
                if self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period_law() =>
            {
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
            _ => {}
        }
        self.halt_hdma = HaltHdmaState::Low;
    }

    /// VRAM DMA source read. VRAM itself and the 0xE000+ region are not
    /// valid sources (Pan Docs); they read as 0xFF here (SameBoy
    /// Core/memory.c GB_hdma_run drives the bus only for ROM/SRAM/WRAM
    /// sources and leaves the idle data-bus byte for everything else —
    /// `gdma_invalid_sources_fill_destination_with_ff`; gambatte reads its
    /// decaying cart-bus latch, unmodelled here).
    fn vram_dma_source_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xDFFF => self.wram[self.wram_index(addr)],
            _ => 0xFF,
        }
    }

    /// FF55 write (gambatte memory.cpp nontrivial_ff_write case 0x55; the
    /// `hdma_mode` branches mirror `lcd_.hdmaIsEnabled()`).
    pub(super) fn hdma5_write(&mut self, value: u8) {
        // The write always replaces the live length bits — cancelling
        // latches the *written* length with bit 7 set, not the old
        // remaining count (gambatte: `ioamhram_[0x155] = data & 0x7F`
        // before the cancel branch; SameBoy memory.c GB_IO_HDMA5 likewise
        // sets hdma_steps_left first).
        self.hdma5 = value & 0x7F;
        if self.hdma_mode != HdmaMode::Disabled {
            if value & 0x80 == 0 {
                self.hdma5 |= 0x80;
                self.hdma_mode = HdmaMode::Disabled;
            }
            // Bit 7 set while armed: only the length bits change.
        } else if value & 0x80 != 0 {
            // gambatte video.cpp enableHdma: with the LCD off one block
            // copies immediately (flagHdmaReq, SameBoy: `(STAT & 3) == 0
            // && display_state != 7 → hdma_on = true`); with the LCD on a
            // block fires at once only inside the hblank window.
            if self.ppu.lcd_enabled() {
                self.hdma_mode = HdmaMode::ArmedLcdOn;
                probe!(if crate::probe::s5dbg_on() {
                    let (l, d) = self.ppu.scan_pos();
                    eprintln!(
                        "SLOPGB wff55 arm ly={l} dot={d} period={}",
                        self.ppu.hdma_period_law()
                    );
                });
                if self.ppu.hdma_period_law() {
                    self.vram_dma_req = Some(VramDmaReq::Hblank);
                }
            } else {
                self.hdma_mode = HdmaMode::ArmedLcdOff;
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
        } else {
            // General-purpose DMA: requested now, serviced at the next
            // instruction boundary (gambatte flagGdmaReq). Note a stale
            // active-looking FF55 (HBlank arming killed by an LCD
            // disable) reaches this branch too, exactly like upstream.
            self.vram_dma_req = Some(VramDmaReq::Gdma);
        }
    }
}
