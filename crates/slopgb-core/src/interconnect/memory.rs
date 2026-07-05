//! Memory-map routing: WRAM/echo/HRAM indexing, prohibited area, OAM-bug trigger, the untimed read_no_tick/write_no_tick + IO register read/write dispatch. Oracle: full mooneye + gbtr matrix.

use super::*;

impl Interconnect {
    pub(super) fn wram_index(&self, addr: u16) -> usize {
        let offset = usize::from(addr & 0x1FFF);
        if offset < 0x1000 {
            offset
        } else {
            let bank = if self.model.is_cgb() {
                usize::from(self.svbk & 7).max(1)
            } else {
                1
            };
            bank * 0x1000 + (offset - 0x1000)
        }
    }

    /// DMG OAM corruption bug (Pan Docs "OAM Corruption Bug"): a CPU
    /// access cycle with a $FE00-$FEFF value on the address bus while the
    /// PPU's mode-2 scan is on a corruptible row mangles that row.
    /// `addr` is the bus value: the access address for reads/writes, the
    /// 16-bit inc/dec unit's register value for internal cycles
    /// ([`Bus::tick_addr`]). The whole page triggers, including the
    /// FEA0-FEFF prohibited area (SameBoy keys on `addr < 0xFF00`; blargg
    /// oam_bug/8-instr_effect pops from $FEF0). Suppressed
    ///
    /// * on CGB/AGB — the bug is DMG-family-only,
    /// * while the core clock is gated off — the halted CPU performs no
    ///   bus accesses on hardware, keeping the discarded halt prefetch
    ///   (see `cpu::execute::step`) phantom-free, and
    /// * while the OAM DMA engine owns OAM — the interplay is untested on
    ///   hardware (SameBoy carries the same Todo), so the conservative
    ///   gate wins; `dma_conflict` is `Some` exactly in the M-cycles a
    ///   DMA byte copies.
    pub(super) fn maybe_oam_bug(&mut self, addr: u16, kind: OamBugKind) {
        if !(0xFE00..=0xFEFF).contains(&addr)
            || self.model.is_cgb()
            || self.cpu_halted
            || self.dma_conflict.is_some()
        {
            return;
        }
        self.ppu.oam_bug(kind);
    }

    /// Cell of [`Self::extra_oam`] a FEA0-FEFF access decodes to: bits 3-4
    /// of the low address byte are ignored (gambatte-core memory.cpp masks
    /// the OAM-relative offset with $E7), leaving 24 cells — offsets
    /// $A0-$A7/$C0-$C7/$E0-$E7 — each mirrored 4 times.
    pub(super) fn extra_oam_index(addr: u16) -> usize {
        let masked = usize::from(addr & 0xE7);
        (masked >> 5) * 8 + (masked & 7) - 40
    }

    /// FEA0-FEFF "prohibited" reads (Pan Docs "FEA0-FEFF range").
    ///
    /// * DMG family: $00 while OAM is idle, $FF while the PPU has OAM
    ///   locked (the mode-2 corruption itself lives in
    ///   [`Self::maybe_oam_bug`]).
    /// * CPU CGB C ([`Model::Cgb`], ARCHITECTURE §CGB revision policy):
    ///   extra OAM RAM behind the same lockout as OAM proper
    ///   ([`Self::extra_oam`]; gambatte oamdma busypushFEA1/FF01 rows).
    /// * AGB (and CGB revision E): the high nibble of the low address byte
    ///   twice.
    fn prohibited_read(&self, addr: u16) -> u8 {
        match self.model {
            Model::Cgb => {
                // The CGB FEA0-FEFF extra OAM RAM mirrors OAM read
                // blocking, including the cc+2 MID-phase second-half
                // unblock view (sub-dot event-phase model).
                if self.ppu.oam_read_blocked()
                    || (!self.tier2_reclock && stamp_blocks(self.m0_access_edge, ACCESS_PHASE))
                {
                    0xFF
                } else {
                    self.extra_oam[Self::extra_oam_index(addr)]
                }
            }
            Model::Agb => {
                let lo = addr as u8;
                (lo & 0xF0) | (lo >> 4)
            }
            _ if self.ppu.mode_bits() >= 2 => 0xFF,
            _ => 0x00,
        }
    }

    /// Write counterpart of [`Self::prohibited_read`]: only the CGB-C
    /// extra OAM RAM is writable, under the same gating as OAM proper
    /// (dropped while the PPU scans or a DMA byte is in flight; the
    /// in-flight gate is gambatte memory.cpp's `oamDmaPos_ < oam_size`).
    fn prohibited_write(&mut self, addr: u16, value: u8) {
        if self.model == Model::Cgb && self.dma_conflict.is_none() && !self.ppu.oam_write_blocked()
        {
            self.extra_oam[Self::extra_oam_index(addr)] = value;
        }
    }

    pub(super) fn read_no_tick(&mut self, addr: u16) -> u8 {
        // Port Stage B C1.3 (S7) — one-shot post-mode-0-halt-wake LY phase
        // carry. The mode-0 halt-wake set `halt_ly_phase` to the sub-M-cycle
        // carry; the FIRST post-wake FF44 read (hblank's measurement read)
        // back-dates the line by it, then clears. The pre-halt `wait_ly` poll
        // never sees it (it ran before the wake), so — unlike a uniform LY
        // back-date — the poll is uncorrupted. See `Interconnect::halt_ly_phase`.
        if addr == 0xFF44 && self.tier2_reclock && self.halt_ly_phase > 0 {
            let off = u16::from(self.halt_ly_phase);
            self.halt_ly_phase = 0;
            let (line, dot) = self.ppu.line_dot();
            if (1..=143).contains(&line) && dot < off {
                return line - 1;
            }
        }
        if let Some(c) = self.dma_conflict {
            // OAM (and the prohibited area behind it) reads $FF while a DMA
            // byte is in flight (gbctr OAM DMA).
            if (0xFE00..=0xFEFF).contains(&addr) {
                return 0xFF;
            }
            // Reads on conflicting pages see the DMA engine's bus, except
            // the CGB WRAM-region redirect (gambatte-core memory.cpp
            // nontrivial_read's OAM-DMA conflict block).
            if self.in_dma_conflict_area(c.kind, addr) {
                if self.model.is_cgb() && c.kind != DmaSrcKind::Wram && addr >= 0xC000 {
                    return self.wram[self.dma_redirect_wram_index(&c, addr)];
                }
                if self.model.is_cgb() && c.kind == DmaSrcKind::Vram {
                    // CGB quirk: the conflicted read also clears the
                    // in-flight OAM byte (gambatte memory.cpp:
                    // `ioamhram_[oamDmaPos_] = 0` after a vram-source
                    // conflict read) — replacing the cycle's pending
                    // byte, like a derailed write.
                    self.dma_pending_oam = Some((c.idx, 0));
                }
                return c.byte;
            }
        }
        match addr {
            // Boot ROM overlays the low cart region while mapped (opt-in);
            // `boot_rom_byte` is `None` whenever no boot ROM is active, so this
            // is exactly `cart.read_rom(addr)` on every golden path.
            0x0000..=0x7FFF => self
                .boot_rom_byte(addr)
                .unwrap_or_else(|| self.cart.read_rom(addr)),
            // cc+2 MID-phase VRAM read: same mode-3→mode-0 unblock edge as
            // OAM below — a second-half unblock is not yet visible here
            // (sub-dot event-phase model, increment 2). Part C: tier2 BYPASSES
            // every M0Access straddle stamp (all five sites) — the deferred
            // (cc+0) access is resolved to its exact half-dot before sampling,
            // so the cc+4 straddle-M-cycle approximation double-blocks
            // accesses landing legitimately past the unblock (the DS
            // `postread_scx5_ds_2` SameBoy-passes); the deferred frame's
            // release laws live in `Ppu::{oam,vram}_read_blocked` /
            // `ds_lineend_open`. EXCEPT a readback within 8 dots of a
            // same-line VRAM write ATTEMPT (`vram_wr_recent`, the #11as
            // co-temporality): the write's M-cycle cost is what SameBoy
            // spreads across the readback, so those keep the straddle stamp
            // (`vramw_m3end_scx5_ds_{2,4}` SameBoy-passes, measured drop
            // without the guard). Suppressed while an
            // HDMA is armed: the HDMA service seam writes VRAM at the same
            // mode-0 entry and its read-back interaction (gambatte
            // dma/hdma_start_*) is the HDMA-seam increment's job.
            0x8000..=0x9FFF
                if (!self.tier2_reclock || self.ppu.vram_wr_recent())
                    && stamp_blocks(self.m0_access_edge, ACCESS_PHASE)
                    && self.hdma_mode == HdmaMode::Disabled =>
            {
                0xFF
            }
            0x8000..=0x9FFF => self.ppu.read(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
            0xFE00..=0xFE9F => {
                // cc+2 MID-phase OAM read: a mode-3→mode-0 unblock landing
                // in this M-cycle's second half is not yet visible here
                // (sub-dot event-phase model, increment 1).
                if !self.tier2_reclock && stamp_blocks(self.m0_access_edge, ACCESS_PHASE) {
                    0xFF
                } else {
                    self.ppu.read(addr)
                }
            }
            0xFEA0..=0xFEFF => self.prohibited_read(addr),
            0xFF00..=0xFF7F => self.io_read(addr),
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)],
            0xFFFF => self.ie,
        }
    }

    pub(super) fn write_no_tick(&mut self, addr: u16, value: u8) {
        // A CPU write on a conflicting page derails: the addressed memory
        // (including MBC registers) never sees it. On DMG the byte lands
        // in the in-flight OAM slot — wire-ANDed with the DMA byte for
        // WRAM sources, as-is otherwise; CGB additionally zeroes the data
        // for VRAM sources and redirects WRAM-region writes to WRAM
        // instead of OAM (gambatte-core memory.cpp nontrivial_write's
        // OAM-DMA conflict block; pinned by the gambatte
        // oamdma_srcXXXX_busypush/busypop matrix).
        if let Some(c) = self.dma_conflict {
            if self.in_dma_conflict_area(c.kind, addr) {
                // The derailed byte rides the transfer's own OAM write
                // slot, so it replaces the cycle's pending byte and lands
                // at the same end-of-cycle commit point
                // ([`Self::oam_dma_commit_pending`]).
                if self.model.is_cgb() {
                    if addr < 0xC000 {
                        let byte = if c.kind == DmaSrcKind::Vram { 0 } else { value };
                        self.dma_pending_oam = Some((c.idx, byte));
                    } else if c.kind != DmaSrcKind::Wram {
                        let i = self.dma_redirect_wram_index(&c, addr);
                        self.wram[i] = value;
                    }
                    // WRAM source + WRAM region: the write is swallowed.
                } else {
                    let byte = if c.kind == DmaSrcKind::Wram {
                        c.byte & value
                    } else {
                        value
                    };
                    self.dma_pending_oam = Some((c.idx, byte));
                }
                return;
            }
        }
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, value),
            // cc+2 MID-phase VRAM write: a mode-3→mode-0 unblock landing in
            // this M-cycle's second half is not yet visible here, so the
            // write is still locked out (dropped) — same edge/sub-dot
            // phase as the OAM/VRAM read (sub-dot event-phase model).
            // The VRAM WRITE straddle stamp stays on BOTH paths: the tier2
            // bypass here dropped the SameBoy-passing `vramw_m3end_scx5_ds_4`
            // (measured — the write side of the #11as co-temporality; only
            // the READ sites + the OAM write resolve at the deferred frame).
            0x8000..=0x9FFF if stamp_blocks(self.m0_access_edge, ACCESS_PHASE) => {}
            0x8000..=0x9FFF => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xA000..=0xBFFF => self.cart.write_ram(addr, value),
            0xC000..=0xFDFF => {
                let i = self.wram_index(addr);
                self.wram[i] = value;
            }
            0xFE00..=0xFE9F => {
                // CPU OAM writes are dropped while DMA owns OAM, and while
                // the cc+2 MID view still reads mode 3 (sub-dot phase).
                if self.dma_conflict.is_none()
                    && (self.tier2_reclock || !stamp_blocks(self.m0_access_edge, ACCESS_PHASE))
                {
                    self.intf |= self.ppu.write(addr, value) & IF_MASK;
                }
            }
            0xFEA0..=0xFEFF => self.prohibited_write(addr, value),
            0xFF00..=0xFF7F => self.io_write(addr, value),
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)] = value,
            0xFFFF => self.ie = value,
        }
    }

    /// Read for the debugger views: like [`Self::peek`] but resolves the IO
    /// registers (FF00-FF7F) to their live hardware values via [`Self::io_read`]
    /// — the bgb debugger/IO-map want to *show* register state, not the `$FF`
    /// `peek` returns to keep test harnesses from reading IO out of band.
    /// Side-effect-free (`&self`); the value is what the CPU would read now.
    pub(crate) fn debug_read(&self, addr: u16) -> u8 {
        match addr {
            0xFF00..=0xFF7F => self.io_read(addr),
            _ => self.peek(addr),
        }
    }

    fn io_read(&self, addr: u16) -> u8 {
        match addr {
            0xFF00 => self.joypad.read(),
            0xFF01 | 0xFF02 => self.serial.read(addr),
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF0F => 0xE0 | self.intf,
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF46 => self.dma_reg,
            // The STAT mode bits read at the cc+2 MID phase: in double speed
            // a read whose M-cycle straddles a sprite-line mode-3→mode-0 flip
            // still reads mode 3 for the WHOLE straddle M-cycle
            // (`event_phase(StatMode)=END_PHASE`, INC-G3 task 6 — not only a
            // 2nd-half flip), where the whole-dot end view has already flipped
            // to mode 0 (gambatte sprites m3stat_ds). Only the low mode bits
            // move; the enable bits and the live LYC compare keep the end view.
            // The `double_speed` gate is load-bearing, not belt-and-braces:
            // single-speed FF41 m3stat reads share this one stamp but want the
            // end view (cross-oracle — see
            // `stat_mode_override_requires_double_speed`). The LCD-on guard is
            // belt-and-braces: the flag is only set by a live flip (LCD on) and
            // reset every tick, so an LCD-off read can never carry it, but the
            // guard keeps the override from ever forcing mode 3 over the LCD-off
            // mode 0. Sub-dot event-phase model, INC-DS-1 + INC-G3 task 6.
            0xFF41
                if stamp_blocks(self.stat_mode_edge, ACCESS_PHASE)
                    && self.double_speed
                    && self.ppu.lcd_enabled() =>
            {
                self.ppu.read(0xFF41) | 0x03
            }
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B => self.ppu.read(addr),
            0xFF4D if self.cgb_mode => {
                0x7E | (u8::from(self.double_speed) << 7) | u8::from(self.key1_armed)
            }
            // VBK reads $FE|bank on CGB even in DMG mode (boot_hwio-C).
            0xFF4F => self.ppu.read(addr),
            // FF55 reads the live register verbatim: remaining blocks - 1
            // while a HBlank transfer runs, bit 7 set when none is
            // registered (gambatte ioamhram_[0x155]).
            0xFF55 if self.cgb_mode => self.hdma5,
            // RP: bits 2-5 unimplemented (1), bit 1 = received signal,
            // active low — no peer, so never receiving.
            0xFF56 if self.cgb_mode => 0x3C | (self.rp & 0xC1) | 0x02,
            // BCPS/OCPS stay readable in DMG-compat mode (boot_hwio-C reads
            // the boot leftovers $C8/$D0); the data ports do not.
            0xFF68 | 0xFF6A => self.ppu.read(addr),
            // cc+2 MID-phase CGB palette read: the pipe-end unblock commits at
            // the M-cycle end (whole-M-cycle block, see `pal_access_edge` /
            // [`event_phase`]), so the read stays $FF for the entire straddle
            // M-cycle and becomes readable only next M-cycle (sub-dot
            // event-phase model, INC-G3 task 5). Tier-2 BYPASSES the stamp:
            // the deferred (cc+0) read is resolved to its exact half-dot
            // BEFORE sampling, so the straddle-M-cycle approximation would
            // re-block a read landing legitimately past the unblock (the
            // `cgbpal_m3end_scx*_2` SameBoy-passes); the deferred frame's
            // trailing unblock lives in `Ppu::pal_ram_blocked` instead
            // (`pal_open_dot` + 1 dot SS / + 0 DS).
            0xFF69 | 0xFF6B
                if self.cgb_mode
                    && !self.tier2_reclock
                    && stamp_blocks(self.pal_access_edge, ACCESS_PHASE) =>
            {
                0xFF
            }
            0xFF69 | 0xFF6B if self.cgb_mode => self.ppu.read(addr),
            0xFF6C => self.ppu.read(addr),
            0xFF70 if self.cgb_mode => 0xF8 | self.svbk,
            0xFF72 if self.model.is_cgb() => self.ff72,
            0xFF73 if self.model.is_cgb() => self.ff73,
            0xFF74 if self.cgb_mode => self.ff74,
            0xFF75 if self.model.is_cgb() => 0x8F | (self.ff75 & 0x70),
            // FF76/FF77 (PCM12/PCM34): read-only per-channel 4-bit digital
            // outputs (Pan Docs "PCM amplitude readouts").
            0xFF76 if self.model.is_cgb() => self.apu.pcm12(),
            0xFF77 if self.model.is_cgb() => self.apu.pcm34(),
            // FF50 (boot ROM disable) and everything unmapped: $FF.
            _ => 0xFF,
        }
    }

    fn io_write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF00 => self.joypad.write(value),
            0xFF01 | 0xFF02 => self.serial.write(addr, value),
            // A timer write never requests IF directly: a write-induced TIMA
            // overflow raises it only at the reload, from `Timer::tick`.
            0xFF04..=0xFF07 => {
                if addr == 0xFF04 {
                    // The DIV-reset falling edge must reach the serial
                    // clock within this cycle: the once-per-M-cycle
                    // sampled tick would miss it for the CGB fast clock,
                    // whose DIV bit is high again by the next sample
                    // (`Serial::div_write`; gambatte serial/
                    // start83_late_div_write_*).
                    let iff = self.serial.div_write(self.timer.div_counter());
                    self.intf |= iff & IF_MASK;
                    // Same for the frame sequencer: the reset's DIV-APU
                    // falling edge lands in this cycle (`Apu::div_write`).
                    self.apu.div_write(self.double_speed);
                }
                self.timer.write(addr, value)
            }
            0xFF0F => self.intf = value & IF_MASK,
            0xFF10..=0xFF3F => self.apu.write(addr, value),
            0xFF46 => {
                self.dma_reg = value;
                let src = u16::from(value) << 8;
                // A rewrite mid-flight retargets the running transfer
                // immediately: the handover copies before the new
                // transfer's byte 0 read the NEW source at the old
                // indices, and conflict like it (gambatte-core memory.cpp
                // FF46 handler updates ioamhram_[0x146] and re-runs
                // oamDmaInitSetup before the next copy; pinned by gambatte
                // oamdma_src8000_srcchange0000_busyread0000_1/2 — mooneye
                // oam_dma_restart restarts with the same page and cannot
                // discriminate).
                if let Some(run) = &mut self.dma_run {
                    run.src = src;
                }
                self.dma_start = Some(OamDmaStart { src, delay: 1 });
            }
            // PPU register writes can raise the STAT line in this very
            // cycle (stat_lyc_onoff round 4): `Ppu::write` returns the IF
            // bits the write raised, OR-ed in immediately.
            0xFF40 => {
                let was_on = self.ppu.lcd_enabled();
                self.intf |= self.ppu.write(addr, value) & IF_MASK;
                let now_on = self.ppu.lcd_enabled();
                if was_on && !now_on && self.hdma_mode == HdmaMode::ArmedLcdOn {
                    // Display disabled while HBlank DMA is armed: the
                    // arming dies with the LCD — FF55 keeps reading
                    // "active" but no further block ever copies until
                    // FF55 is rewritten (gambatte video.cpp lcdcChange:
                    // the disable branch sets every memevent, including
                    // memevent_hdma, to disabled_time).
                    self.hdma_mode = HdmaMode::Disabled;
                } else if !was_on && now_on && self.hdma_mode == HdmaMode::ArmedLcdOff {
                    // HBlank DMA armed while the LCD was off resumes at
                    // the new frame's mode-0 entries (gambatte
                    // lcdcChange enable branch: `if (hdmaIsEnabled())
                    // setm<memevent_hdma>(predictedNextM0Time())`).
                    self.hdma_mode = HdmaMode::ArmedLcdOn;
                }
            }
            0xFF41..=0xFF45 | 0xFF47..=0xFF4B => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF4D if self.cgb_mode => self.key1_armed = value & 1 != 0,
            0xFF4F if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            // FF51-FF54 are the *live* DMA address counters, not start
            // latches: `hdma_src`/`hdma_dst` advance as blocks copy, and a
            // register write merges into the current counter value
            // (gambatte memory.cpp cases 0x51-0x54; SameBoy GB_IO_HDMA1-4
            // agree). The high-byte writes keep the counter's full low
            // byte and apply no destination mask — the dest counter is a
            // full 16-bit register, masked only at the VRAM write
            // (`hdma_partial_src_rewrite_blends_live_counter`,
            // `gdma_continues_from_incremented_addresses`,
            // `gdma_terminates_at_dest_0x10000_crossing`).
            0xFF51 if self.cgb_mode => {
                self.hdma_src = (self.hdma_src & 0x00FF) | (u16::from(value) << 8)
            }
            0xFF52 if self.cgb_mode => {
                self.hdma_src = (self.hdma_src & 0xFF00) | u16::from(value & 0xF0)
            }
            0xFF53 if self.cgb_mode => {
                self.hdma_dst = (self.hdma_dst & 0x00FF) | (u16::from(value) << 8)
            }
            0xFF54 if self.cgb_mode => {
                self.hdma_dst = (self.hdma_dst & 0xFF00) | u16::from(value & 0xF0)
            }
            0xFF55 if self.cgb_mode => self.hdma5_write(value),
            0xFF56 if self.cgb_mode => self.rp = value & 0xC1,
            0xFF68 | 0xFF6A => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF69 | 0xFF6B if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            // OPRI is set up by the boot ROM and locked outside CGB mode.
            0xFF6C if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF70 if self.cgb_mode => self.svbk = value & 7,
            0xFF72 if self.model.is_cgb() => self.ff72 = value,
            0xFF73 if self.model.is_cgb() => self.ff73 = value,
            0xFF74 if self.cgb_mode => self.ff74 = value,
            0xFF75 if self.model.is_cgb() => self.ff75 = value & 0x70,
            // KEY0 (FF4C): the CGB boot ROM writes bit 2 to lock DMG-compat mode
            // for a DMG cart, AFTER installing the compat palettes/OPRI in CGB
            // mode. Honoured only while the boot ROM is mapped — on a post-boot/
            // `new` machine FF4C falls through to the ignored `_` arm (golden-safe;
            // FF4C is locked out of the normal runtime, `boot_active` false here).
            0xFF4C if self.boot_active && value & 0x04 != 0 => self.set_cgb_mode(false),
            // FF50 boot-disable: while a boot ROM is mapped (opt-in), a write
            // with bit 0 set hands off — the boot ROM unmaps itself permanently.
            // With no boot ROM we start post-boot, so this is never taken and
            // the write is ignored (golden-safe; `boot_active` false by default).
            0xFF50 if self.boot_active && value & 1 != 0 => self.boot_active = false,
            _ => {}
        }
    }
}
