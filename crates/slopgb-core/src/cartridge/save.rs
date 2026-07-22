//! Battery-backed save images (RAM + serialized RTC) and RTC/rumble hooks.

use super::*;

impl Cartridge {
    /// Power-on init for external RAM: overwrite every byte with `f()`. Used by
    /// [`crate::GameBoy::init_ram`] to fill an unsaved cart's SRAM with a
    /// deterministic constant or seeded garbage. MBC2 masks the upper nibble on
    /// read, so a raw fill is fine. A `.sav` load ([`Self::load_save_data`])
    /// overwrites this afterwards.
    pub(crate) fn fill_ram(&mut self, mut f: impl FnMut() -> u8) {
        for b in &mut self.ram {
            *b = f();
        }
    }

    fn rtc(&self) -> Option<&Rtc> {
        match &self.mapper {
            Mapper::Mbc3 { rtc, .. } => rtc.as_ref(),
            _ => None,
        }
    }

    /// The battery SRAM alone, without the RTC trailer [`Self::save_data`]
    /// appends. `None` with no battery. Read-only.
    pub fn battery_sram(&self) -> Option<Vec<u8>> {
        self.has_battery.then(|| self.ram.clone())
    }

    /// The MBC3 RTC `(live, latched)` register files, each `[S, M, H, DL, DH]`,
    /// or `None` with no RTC. Read-only.
    pub fn rtc_state(&self) -> Option<([u8; 5], [u8; 5])> {
        self.rtc().map(|r| (r.regs, r.latched))
    }

    /// Battery-backed RAM image (+ serialized RTC for MBC3, + the flash for
    /// MBC6), None if the cartridge has no battery.
    ///
    /// Format: the raw RAM contents (MBC2: 512 bytes, low nibble valid),
    /// followed — for RTC carts (types 0x0F/0x10) — by a 16-byte block:
    /// live S,M,H,DL,DH; latched S,M,H,DL,DH; sub-second T-cycle counter as
    /// little-endian u32; the last latch register write; one zero pad byte.
    /// For MBC6 the trailer is instead the non-volatile flash chip: the
    /// 1 MiB array, the 256-byte hidden region, and one protect-flag byte
    /// (Pan Docs "MBC6": the Protect Sector 0 state "is stored non-volatile").
    pub fn save_data(&self) -> Option<Vec<u8>> {
        if !self.has_battery {
            return None;
        }
        let mut data = self.ram.clone();
        if let Some(rtc) = self.rtc() {
            data.extend_from_slice(&rtc.regs);
            data.extend_from_slice(&rtc.latched);
            data.extend_from_slice(&rtc.subsec.to_le_bytes());
            // latch_prev so an armed 0x00 -> 0x01 latch sequence survives a
            // save taken between the two writes.
            data.extend_from_slice(&[rtc.latch_prev, 0]);
        }
        if let Mapper::Mbc6 { flash, .. } = &self.mapper {
            data.extend_from_slice(&flash.data);
            data.extend_from_slice(&flash.hidden);
            data.push(u8::from(flash.protect));
        }
        Some(data)
    }

    /// Restore a [`Self::save_data`] image; also accepts the de-facto .sav
    /// layouts of other emulators. Returns whether anything was restored.
    ///
    /// The RAM prefix is loaded whenever `data` is at least RAM-sized; the
    /// trailing block is then interpreted as RTC state if the cartridge has
    /// an RTC: either our own 16-byte block ([`Self::save_data`]) or the
    /// 44/48-byte footer written by VBA/mGBA/BGB/SameBoy (five 4-byte LE
    /// live registers, five 4-byte LE latched registers, 32/64-bit
    /// timestamp). An unknown trailer size skips only the RTC restore, so
    /// e.g. a Pokemon G/S/C save imported from another emulator never loses
    /// its RAM. Data shorter than the RAM is rejected (returns false).
    pub fn load_save_data(&mut self, data: &[u8]) -> bool {
        if !self.has_battery || data.len() < self.ram.len() {
            return false;
        }
        let (ram, trailer) = data.split_at(self.ram.len());
        self.ram.copy_from_slice(ram);
        let trailer_restored = self.load_rtc_trailer(trailer) || self.load_mbc6_trailer(trailer);
        !self.ram.is_empty() || trailer_restored
    }

    /// Parse the post-RAM trailer of an MBC6 save image into the flash
    /// chip (see [`Self::save_data`]). An unknown trailer size (e.g. a
    /// foreign SRAM-only .sav) skips only the flash restore, leaving a
    /// fresh all-0xFF chip. Returns whether flash state was restored.
    fn load_mbc6_trailer(&mut self, trailer: &[u8]) -> bool {
        let Mapper::Mbc6 { flash, .. } = &mut self.mapper else {
            return false;
        };
        if trailer.len() != MBC6_FLASH_SIZE + 256 + 1 {
            return false;
        }
        flash.data.copy_from_slice(&trailer[..MBC6_FLASH_SIZE]);
        flash
            .hidden
            .copy_from_slice(&trailer[MBC6_FLASH_SIZE..MBC6_FLASH_SIZE + 256]);
        flash.protect = trailer[MBC6_FLASH_SIZE + 256] != 0;
        true
    }

    /// Parse the post-RAM trailer of a save image into the RTC, if any.
    /// Returns whether RTC state was restored.
    fn load_rtc_trailer(&mut self, trailer: &[u8]) -> bool {
        let Mapper::Mbc3 { rtc: Some(rtc), .. } = &mut self.mapper else {
            return false;
        };
        match trailer.len() {
            // Our own block, see `save_data`.
            RTC_SAVE_LEN => {
                for (i, (reg, mask)) in rtc.regs.iter_mut().zip(RTC_MASKS).enumerate() {
                    *reg = trailer[i] & mask;
                }
                for (i, (reg, mask)) in rtc.latched.iter_mut().zip(RTC_MASKS).enumerate() {
                    *reg = trailer[5 + i] & mask;
                }
                let subsec = u32::from_le_bytes(trailer[10..14].try_into().unwrap());
                rtc.subsec = subsec % CYCLES_PER_SECOND;
                rtc.latch_prev = trailer[14];
                true
            }
            // De-facto VBA footer (also mGBA/BGB/SameBoy): each register is
            // stored as a 4-byte LE word (only the low byte is meaningful),
            // five live then five latched, then a 32- or 64-bit host
            // timestamp we ignore (our RTC is deterministic and never reads
            // the host clock).
            44 | 48 => {
                for (i, (reg, mask)) in rtc.regs.iter_mut().zip(RTC_MASKS).enumerate() {
                    *reg = trailer[4 * i] & mask;
                }
                for (i, (reg, mask)) in rtc.latched.iter_mut().zip(RTC_MASKS).enumerate() {
                    *reg = trailer[20 + 4 * i] & mask;
                }
                rtc.subsec = 0;
                true
            }
            _ => false,
        }
    }

    /// Advance the cartridge's wall-time devices by `t_cycles` T-cycles
    /// (dots) of wall-clock time (4_194_304 per second; in CGB double speed
    /// mode pass dots, not CPU cycles, so wall time stays correct): the
    /// MBC3 real-time clock and the MBC6 flash's embedded-operation busy
    /// timer. Deterministic — never reads the host clock. No-op for carts
    /// with neither device.
    pub fn tick_time(&mut self, t_cycles: u32) {
        match &mut self.mapper {
            Mapper::Mbc3 { rtc: Some(rtc), .. } => rtc.tick_cycles(t_cycles),
            Mapper::Mbc6 { flash, .. } => {
                flash.busy = flash.busy.saturating_sub(t_cycles);
            }
            _ => {}
        }
    }

    /// Rumble motor state (MBC5 rumble carts, types 0x1C-0x1E); always false
    /// for other cartridges.
    pub fn rumble(&self) -> bool {
        matches!(self.mapper, Mapper::Mbc5 { rumble: true, .. })
    }
}
