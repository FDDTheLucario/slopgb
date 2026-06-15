//! OAM DMA engine: 160-cycle transfer, FF46 setup delay/restart, per-source-class bus conflicts (gambatte memory.cpp `oamDma*`). Oracle: gbtr oamdma/*, blargg oam_bug/*, mooneye acceptance/oam_dma/.

use super::*;

impl Interconnect {
    /// Commit the previous M-cycle's OAM DMA byte to OAM. gambatte
    /// timestamps each copy at the *end* of its M-cycle (memory.cpp
    /// `updateOamDma`: `lastOamDmaUpdate_ += 4` before the
    /// `ioamhram_[oamDmaPos_]` store), so the PPU dots of the copying
    /// cycle still see the old byte — the mode-2 scan latch racing the
    /// transfer's first byte depends on it (late_sp01x/02x `_1`: the
    /// slot's Y is rewritten by byte 0 in the very cycle the scan latches
    /// it, and hardware still selects the old sprite). Runs at the head
    /// of every controller advance, before this cycle's copy is staged.
    pub(super) fn oam_dma_commit_pending(&mut self) {
        if let Some((idx, byte)) = self.dma_pending_oam.take() {
            self.ppu.oam_dma_write(idx, byte);
        }
    }

    pub(super) fn oam_dma_tick(&mut self) {
        self.dma_conflict = None;
        self.oam_dma_commit_pending();
        // The PPU's OAM view for this M-cycle's dots: disconnected while
        // the controller owns OAM, so the mode-2 scan latches $FF
        // (gambatte memory.cpp startOamDma/endOamDma switch the
        // OamReader's source to rdisabledRam). Both edges trail the
        // controller state by one M-cycle: gambatte timestamps
        // startOamDma at the byte-0 copy step — the end of our promoting
        // cycle, whose dots therefore still latch real OAM — and
        // endOamDma one position step *past* byte 159, so the dots of
        // the cycle after the last copy are still disconnected (the
        // `prev ||` term). The level deliberately keeps refreshing
        // through the halted/bus-stolen early returns below: a frozen
        // transfer owns OAM for the whole freeze
        // (oamdma_late_halt_stat/late_speedchange_stat).
        let owned = self.dma_run.is_some();
        self.ppu
            .set_oam_dma_active(owned || self.dma_oam_owned_prev);
        self.dma_oam_owned_prev = owned;
        // HALT/STOP gate the CPU core clock that drives this controller:
        // neither the setup delay nor the transfer advances while the CPU
        // is halted (see set_cpu_halted; madness/mgb_oam_dma_halt_sprites).
        // While a VRAM DMA owns the bus the controller cannot read its
        // source either: it advances via [`Self::oam_dma_bus_capture`]
        // from the steal loop instead of here.
        if self.cpu_halted || self.vram_dma_owns_bus {
            return;
        }
        self.oam_dma_promote_start();
        if let Some(run) = self.dma_run {
            let byte = self.oam_dma_source_read(run.src, run.idx);
            self.dma_pending_oam = Some((run.idx, byte));
            self.dma_conflict = Some(DmaConflict {
                kind: self.dma_src_kind(run.src),
                src_hi: (run.src >> 8) as u8,
                idx: run.idx,
                byte,
            });
            self.dma_run = (run.idx < 159).then_some(OamDmaRun {
                idx: run.idx + 1,
                ..run
            });
        }
    }

    /// Promote a pending start whose setup delay has elapsed. The old
    /// transfer (if any) keeps copying during the delay cycle
    /// (acceptance/oam_dma_restart) and is replaced exactly when the new
    /// one copies its first byte. Shared by the controller's own clock
    /// ([`Self::oam_dma_tick`]) and the VRAM-DMA steal advances
    /// ([`Self::oam_dma_bus_capture`]): gambatte's `oamDmaPos_` counts
    /// toward `oamDmaStartPos_` identically on both paths.
    fn oam_dma_promote_start(&mut self) {
        match &mut self.dma_start {
            Some(s) if s.delay == 0 => {
                let src = s.src;
                self.dma_start = None;
                self.dma_run = Some(OamDmaRun { src, idx: 0 });
            }
            Some(s) => s.delay -= 1,
            None => {}
        }
    }

    /// One OAM DMA controller advance during a VRAM-DMA bus steal: the
    /// controller cannot perform its own source read — the VRAM DMA owns
    /// the bus — so it latches the stolen transfer's bus traffic instead,
    /// writing `data` to the OAM cell addressed by the *bus address* low
    /// byte (`src & 0xFF`, NOT the controller's own position), or to the
    /// CGB-C extra OAM RAM with the usual bits-3/4 alias for low bytes ≥
    /// $A0. The position still advances, so the skipped source bytes are
    /// never copied (gambatte-core memory.cpp `dma()`:
    /// `ioamhram_[src & 0xFF] = data` / `ioamhram_[p & 0xE7] = data`
    /// per `oamDmaPos_` advance, gated on `!halted()`; hardware-pinned by
    /// gambatte dma/hdma_transition_oamdma_1/_2 and
    /// oamdma/oamdmasrcC000_hdmasrc0000).
    ///
    /// Called once per stolen M-cycle with that cycle's *last* byte: at
    /// normal speed gambatte's 4-cc advance grid (`cc - 3 >
    /// lOamDmaUpdate`, both M-cycle-aligned) lands every advance on the
    /// second of the two bytes copied per M-cycle; in double speed each
    /// M-cycle copies one byte and every byte advances.
    pub(super) fn oam_dma_bus_capture(&mut self, src: u16, data: u8) {
        // The speed-switch pause can service a block while the core
        // clock is gated: the OAM DMA controller is frozen with the CPU
        // and does not advance (gambatte dma(): `&& !halted()`).
        if self.cpu_halted {
            return;
        }
        self.oam_dma_promote_start();
        if let Some(run) = self.dma_run {
            let p = src & 0xFF;
            if p < 0xA0 {
                self.ppu.oam_dma_write(p as u8, data);
            } else if self.model == Model::Cgb {
                // AGB skips the extra-RAM write (gambatte `!agbFlag_`).
                self.extra_oam[Self::extra_oam_index(src)] = data;
            }
            self.dma_run = (run.idx < 159).then_some(OamDmaRun {
                idx: run.idx + 1,
                ..run
            });
        }
    }

    /// Source class of a transfer from `src` (gambatte-core memory.cpp
    /// `oamDmaInitSetup`; see [`DmaSrcKind`]).
    fn dma_src_kind(&self, src: u16) -> DmaSrcKind {
        match src >> 8 {
            0x00..=0x7F => DmaSrcKind::Rom,
            0x80..=0x9F => DmaSrcKind::Vram,
            0xA0..=0xBF => DmaSrcKind::Sram,
            0xC0..=0xDF => DmaSrcKind::Wram,
            _ if self.model.is_cgb() => DmaSrcKind::Invalid,
            _ => DmaSrcKind::Wram,
        }
    }

    /// What the OAM DMA engine reads for byte `idx` of a transfer from
    /// `src`. Mode-based PPU blocking does not apply; ROM/SRAM/VRAM
    /// banking is live (gambatte memory.cpp `oamDmaSrcPtr`).
    pub(super) fn oam_dma_source_read(&self, src: u16, idx: u8) -> u8 {
        let addr = src + u16::from(idx);
        match self.dma_src_kind(src) {
            DmaSrcKind::Rom => self.cart.read_rom(addr),
            DmaSrcKind::Vram => self.ppu.vram_read_raw(addr),
            DmaSrcKind::Sram => self.cart.read_ram(addr),
            // DMG $E0-$FF: incomplete address decoding re-reads WRAM
            // (acceptance/oam_dma/sources-GS: $FE/$FF read $DE00/$DF00).
            DmaSrcKind::Wram => {
                let addr = if addr >= 0xE000 { addr - 0x2000 } else { addr };
                self.wram[self.wram_index(addr)]
            }
            DmaSrcKind::Invalid => 0xFF,
        }
    }

    /// Whether a CPU access to `addr` collides with the running transfer's
    /// bus. One 16-bit mask per source class, bit n = 4 KiB page n
    /// conflicts (transcribed from gambatte-core memptrs.cpp
    /// `OamDmaConflictMap`; the FE/FF page never conflicts):
    ///
    /// * ROM/SRAM sources drive the external bus; on CGB the WRAM pages
    ///   C-F conflict *too*, with accesses redirected to WRAM (see
    ///   [`Self::dma_redirect_wram_index`]).
    /// * VRAM sources collide only with the VRAM pages 8-9.
    /// * WRAM sources collide with everything but VRAM on DMG (WRAM sits
    ///   on the external bus there) but only with pages C-F on CGB (its
    ///   own bus).
    pub(super) fn in_dma_conflict_area(&self, kind: DmaSrcKind, addr: u16) -> bool {
        let pages: u16 = match kind {
            DmaSrcKind::Rom | DmaSrcKind::Sram | DmaSrcKind::Invalid => 0xFCFF,
            DmaSrcKind::Vram => 0x0300,
            DmaSrcKind::Wram if self.model.is_cgb() => 0xF000,
            DmaSrcKind::Wram => 0xFCFF,
        };
        addr < 0xFE00 && pages >> (addr >> 12) & 1 != 0
    }

    /// CGB conflict redirect for WRAM-region accesses during a non-WRAM
    /// transfer: the cell actually accessed is WRAM page 0 or the banked
    /// page — chosen by FF46 bit 4, not by the address — at offset
    /// `addr & 0xFFF` (gambatte memory.cpp:
    /// `cart_.wramdata(ioamhram_[0x146] >> 4 & 1)[p & 0xFFF]`; pinned by
    /// oamdma_srcXXXX_busypopDFFF/busypushC001+ cgb04c rows).
    pub(super) fn dma_redirect_wram_index(&self, c: &DmaConflict, addr: u16) -> usize {
        if c.src_hi & 0x10 != 0 {
            self.wram_index(0xD000 | (addr & 0x0FFF))
        } else {
            usize::from(addr & 0x0FFF)
        }
    }
}
