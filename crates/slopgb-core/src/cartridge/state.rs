//! Save-state (manual serialization; see `crate::state`).
//!
//! ROM bytes + ROM-derived flags (has_battery, multicart/mbc30/rumble_cart, the
//! mapper variant) are NOT serialized: a state loads into a machine already
//! built from the same ROM, so only volatile RAM + banking + RTC are restored.

use super::*;

impl Rtc {
    fn write_state(&self, w: &mut crate::state::Writer) {
        w.bytes(&self.regs);
        w.bytes(&self.latched);
        w.u32(self.subsec);
        w.u8(self.latch_prev);
    }
    fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        r.bytes_into(&mut self.regs)?;
        r.bytes_into(&mut self.latched)?;
        self.subsec = r.u32()?;
        self.latch_prev = r.u8()?;
        Ok(())
    }
}

impl FlashMode {
    fn to_u8(self) -> u8 {
        match self {
            FlashMode::Read => 0,
            FlashMode::Id => 1,
            FlashMode::HiddenRead => 2,
            FlashMode::Program => 3,
            FlashMode::ProgramHidden => 4,
            FlashMode::Status => 5,
        }
    }
    fn from_u8(v: u8) -> Result<Self, crate::state::StateError> {
        Ok(match v {
            0 => FlashMode::Read,
            1 => FlashMode::Id,
            2 => FlashMode::HiddenRead,
            3 => FlashMode::Program,
            4 => FlashMode::ProgramHidden,
            5 => FlashMode::Status,
            _ => return Err(crate::state::StateError::Truncated),
        })
    }
}

impl Mbc6Flash {
    fn write_state(&self, w: &mut crate::state::Writer) {
        w.bytes(&self.data);
        w.bytes(&self.hidden);
        w.u8(self.mode.to_u8());
        w.u8(self.seq);
        w.u8(self.prefix);
        w.bool(self.protect);
        w.bytes(&self.buf);
        w.bool(self.page.is_some());
        w.u32(self.page.unwrap_or(0) as u32);
        w.u8(self.loaded);
        w.u32(self.busy);
    }
    fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        r.bytes_into(&mut self.data)?;
        r.bytes_into(&mut self.hidden)?;
        self.mode = FlashMode::from_u8(r.u8()?)?;
        self.seq = r.u8()?;
        self.prefix = r.u8()?;
        self.protect = r.bool()?;
        r.bytes_into(&mut self.buf)?;
        let has_page = r.bool()?;
        // Masked page-aligned inside the array so a corrupt state cannot
        // make a later commit index out of bounds (the hidden-page commit
        // additionally masks to its 256 bytes at use).
        let page = r.u32()? as usize & (MBC6_FLASH_SIZE - 1) & !0x7F;
        self.page = has_page.then_some(page);
        self.loaded = r.u8()?.min(128);
        self.busy = r.u32()?;
        Ok(())
    }
}

impl Mapper {
    fn write_state(&self, w: &mut crate::state::Writer) {
        match self {
            Mapper::None => {}
            Mapper::Mbc1 {
                ramg,
                bank1,
                bank2,
                mode,
                ..
            } => {
                w.bool(*ramg);
                w.u8(*bank1);
                w.u8(*bank2);
                w.bool(*mode);
            }
            Mapper::Mbc2 { ramg, romb } => {
                w.bool(*ramg);
                w.u8(*romb);
            }
            Mapper::Mbc3 {
                ramg,
                romb,
                ramb,
                rtc,
                ..
            } => {
                w.bool(*ramg);
                w.u8(*romb);
                w.u8(*ramb);
                match rtc {
                    Some(rt) => {
                        w.bool(true);
                        rt.write_state(w);
                    }
                    None => w.bool(false),
                }
            }
            Mapper::Mbc5 {
                ramg,
                romb0,
                romb1,
                ramb,
                rumble,
                ..
            } => {
                w.bool(*ramg);
                w.u8(*romb0);
                w.u8(*romb1);
                w.u8(*ramb);
                w.bool(*rumble);
            }
            Mapper::Mbc6 {
                ramg,
                ramb_a,
                ramb_b,
                romb_a,
                romb_b,
                flash_a,
                flash_b,
                flash_enable,
                flash_we,
                flash,
            } => {
                w.bool(*ramg);
                w.u8(*ramb_a);
                w.u8(*ramb_b);
                w.u8(*romb_a);
                w.u8(*romb_b);
                w.bool(*flash_a);
                w.bool(*flash_b);
                w.bool(*flash_enable);
                w.bool(*flash_we);
                flash.write_state(w);
            }
        }
    }
    fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        match self {
            Mapper::None => {}
            Mapper::Mbc1 {
                ramg,
                bank1,
                bank2,
                mode,
                ..
            } => {
                *ramg = r.bool()?;
                *bank1 = r.u8()?;
                *bank2 = r.u8()?;
                *mode = r.bool()?;
            }
            Mapper::Mbc2 { ramg, romb } => {
                *ramg = r.bool()?;
                *romb = r.u8()?;
            }
            Mapper::Mbc3 {
                ramg,
                romb,
                ramb,
                rtc,
                ..
            } => {
                *ramg = r.bool()?;
                *romb = r.u8()?;
                *ramb = r.u8()?;
                if r.bool()? {
                    let mut rt = rtc.take().unwrap_or(Rtc {
                        regs: [0; 5],
                        latched: [0; 5],
                        subsec: 0,
                        latch_prev: 0,
                    });
                    rt.read_state(r)?;
                    *rtc = Some(rt);
                } else {
                    *rtc = None;
                }
            }
            Mapper::Mbc5 {
                ramg,
                romb0,
                romb1,
                ramb,
                rumble,
                ..
            } => {
                *ramg = r.bool()?;
                *romb0 = r.u8()?;
                *romb1 = r.u8()?;
                *ramb = r.u8()?;
                *rumble = r.bool()?;
            }
            Mapper::Mbc6 {
                ramg,
                ramb_a,
                ramb_b,
                romb_a,
                romb_b,
                flash_a,
                flash_b,
                flash_enable,
                flash_we,
                flash,
            } => {
                *ramg = r.bool()?;
                // Masked like the live register writes (banking is 3/7 bits):
                // the flash read/write paths index the fixed 1 MiB array
                // without a size mask, so an unmasked bank from a corrupt
                // state blob would read/erase out of bounds and panic.
                *ramb_a = r.u8()? & 0x07;
                *ramb_b = r.u8()? & 0x07;
                *romb_a = r.u8()? & 0x7F;
                *romb_b = r.u8()? & 0x7F;
                *flash_a = r.bool()?;
                *flash_b = r.bool()?;
                *flash_enable = r.bool()?;
                *flash_we = r.bool()?;
                flash.read_state(r)?;
            }
        }
        Ok(())
    }
}

impl Cartridge {
    /// A short ROM fingerprint (title region + checksums + length) a save state
    /// is keyed to, so loading a state from a different ROM is rejected. The
    /// header bytes always exist — a cartridge under 0x150 fails construction.
    pub(crate) fn rom_id(&self) -> Vec<u8> {
        let mut id = Vec::with_capacity(26);
        id.extend_from_slice(&self.rom[0x134..0x144]); // 16-byte title region
        // Cartridge type + ROM/RAM size pin the *mapper shape* directly: the
        // mapper variant isn't serialized (read_state dispatches on the live
        // variant with a per-variant field count), so a different-mapper ROM
        // that collides on title+checksums+length would otherwise mis-decode.
        // The header checksum alone isn't enough — slopgb accepts ROMs with a
        // bad/zero header checksum.
        id.push(self.rom[0x147]); // cartridge type
        id.push(self.rom[0x148]); // ROM size
        id.push(self.rom[0x149]); // RAM size
        id.push(self.rom[0x14D]); // header checksum
        id.push(self.rom[0x14E]); // global checksum hi
        id.push(self.rom[0x14F]); // global checksum lo
        id.extend_from_slice(&(self.rom.len() as u32).to_le_bytes());
        id
    }

    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        w.u32(self.ram.len() as u32);
        w.bytes(&self.ram);
        self.mapper.write_state(w);
    }
    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        let n = r.u32()? as usize;
        if n != self.ram.len() {
            return Err(crate::state::StateError::RomMismatch);
        }
        r.bytes_into(&mut self.ram)?;
        self.mapper.read_state(r)?;
        Ok(())
    }
}
