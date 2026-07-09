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
