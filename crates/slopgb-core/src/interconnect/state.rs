//! Interconnect save-state (de)serialization (a second `impl Interconnect`
//! block; see `crate::state`). Delegates the peripherals to their own
//! serializers and writes the interconnect's own volatile state (WRAM/HRAM, IF/
//! IE, the deferred-commit clock, the STAT sub-dot edges, the tier2-reclock
//! scratch fields, the OAM-DMA + HDMA engines, CGB misc regs).
//!
//! NOT serialized: `model`/`cgb_mode` (ROM-derived — a state loads into a
//! same-model machine), `boot_rom`/`boot_active` (the opt-in boot-ROM
//! attachment is machine *construction*, like the ROM itself — a state loads
//! into a machine already built with the same boot-ROM-or-not configuration),
//! and the debugger-only fields (watchpoints, profiler, exception mask) —
//! those are live UI state, left untouched by a load. Every other field,
//! including the now-inert deferred-clock scratch (`clock`,
//! `m0_halt_hold`, `ack_squash_deadline_t`, `wake_skew`, `machine_now`,
//! `vram_dma_req_pre`, `stat_vis_from_t`, `halt_ly_phase`, `deferred_squash`),
//! is serialized — production keeps these at their initial 0/false, so
//! serializing them is harmless and guarantees no live
//! field is silently dropped from a round-trip. Live-debugger/UI only, so
//! golden-safe.

use super::*;
use crate::state::{Reader, StateError, Writer};

fn write_opt_u8(w: &mut Writer, o: Option<u8>) {
    match o {
        Some(v) => {
            w.bool(true);
            w.u8(v);
        }
        None => w.bool(false),
    }
}
fn read_opt_u8(r: &mut Reader<'_>) -> Result<Option<u8>, StateError> {
    Ok(if r.bool()? { Some(r.u8()?) } else { None })
}
fn write_opt_u8x2(w: &mut Writer, o: Option<(u8, u8)>) {
    match o {
        Some((a, b)) => {
            w.bool(true);
            w.u8(a);
            w.u8(b);
        }
        None => w.bool(false),
    }
}
fn read_opt_u8x2(r: &mut Reader<'_>) -> Result<Option<(u8, u8)>, StateError> {
    Ok(if r.bool()? {
        Some((r.u8()?, r.u8()?))
    } else {
        None
    })
}

impl OamDmaRun {
    fn write_state(&self, w: &mut Writer) {
        w.u16(self.src);
        w.u8(self.idx);
    }
    fn read_state(r: &mut Reader<'_>) -> Result<Self, StateError> {
        Ok(Self {
            src: r.u16()?,
            idx: r.u8()?,
        })
    }
}
impl OamDmaStart {
    fn write_state(&self, w: &mut Writer) {
        w.u16(self.src);
        w.u8(self.delay);
    }
    fn read_state(r: &mut Reader<'_>) -> Result<Self, StateError> {
        Ok(Self {
            src: r.u16()?,
            delay: r.u8()?,
        })
    }
}
impl DmaSrcKind {
    fn tag(self) -> u8 {
        match self {
            DmaSrcKind::Rom => 0,
            DmaSrcKind::Vram => 1,
            DmaSrcKind::Sram => 2,
            DmaSrcKind::Wram => 3,
            DmaSrcKind::Invalid => 4,
        }
    }
    fn from_tag(t: u8) -> Self {
        match t {
            0 => DmaSrcKind::Rom,
            1 => DmaSrcKind::Vram,
            2 => DmaSrcKind::Sram,
            3 => DmaSrcKind::Wram,
            _ => DmaSrcKind::Invalid,
        }
    }
}
impl DmaConflict {
    fn write_state(&self, w: &mut Writer) {
        w.u8(self.kind.tag());
        w.u8(self.src_hi);
        w.u8(self.idx);
        w.u8(self.byte);
    }
    fn read_state(r: &mut Reader<'_>) -> Result<Self, StateError> {
        Ok(Self {
            kind: DmaSrcKind::from_tag(r.u8()?),
            src_hi: r.u8()?,
            idx: r.u8()?,
            byte: r.u8()?,
        })
    }
}
impl HdmaMode {
    fn tag(self) -> u8 {
        match self {
            HdmaMode::Disabled => 0,
            HdmaMode::ArmedLcdOff => 1,
            HdmaMode::ArmedLcdOn => 2,
        }
    }
    fn from_tag(t: u8) -> Self {
        match t {
            0 => HdmaMode::Disabled,
            1 => HdmaMode::ArmedLcdOff,
            _ => HdmaMode::ArmedLcdOn,
        }
    }
}
impl VramDmaReq {
    fn tag(self) -> u8 {
        match self {
            VramDmaReq::Hblank => 0,
            VramDmaReq::HblankUnhalt => 1,
            VramDmaReq::Gdma => 2,
        }
    }
    fn from_tag(t: u8) -> Self {
        match t {
            0 => VramDmaReq::Hblank,
            1 => VramDmaReq::HblankUnhalt,
            _ => VramDmaReq::Gdma,
        }
    }
}
impl HaltHdmaState {
    fn tag(self) -> u8 {
        match self {
            HaltHdmaState::Low => 0,
            HaltHdmaState::High => 1,
            HaltHdmaState::Requested => 2,
        }
    }
    fn from_tag(t: u8) -> Self {
        match t {
            0 => HaltHdmaState::Low,
            1 => HaltHdmaState::High,
            _ => HaltHdmaState::Requested,
        }
    }
}

impl Interconnect {
    pub(crate) fn write_state(&self, w: &mut Writer) {
        self.cart.write_state(w);
        self.ppu.write_state(w);
        self.apu.write_state(w);
        self.timer.write_state(w);
        self.serial.write_state(w);
        self.joypad.write_state(w);

        w.u64(self.cycles);
        self.clock.write_state(w);
        w.bool(self.double_speed);
        w.u8(self.dot_phase);
        w.bool(self.key1_armed);

        w.u32(self.wram.len() as u32);
        w.bytes(&self.wram);
        w.u8(self.svbk);
        w.bytes(&self.hram);
        w.u8(self.intf);
        w.u8(self.ie);
        w.u8(self.if_late);
        w.u8(self.m0_halt_hold);
        w.u64(self.ack_squash_deadline_t);
        w.u32(self.wake_skew);
        w.u64(self.machine_now);
        w.bool(self.vram_dma_req_pre);
        w.u64(self.stat_vis_from_t);
        w.u8(self.halt_ly_phase);
        w.u8(self.if_stat_late);
        write_opt_u8(w, self.m0_access_edge);
        write_opt_u8(w, self.pal_access_edge);
        write_opt_u8(w, self.stat_mode_edge);
        w.u8(self.ack_squash_mask);
        w.u8(self.ack_squash_ticks);
        w.u8(self.ack_squash_dots);
        w.u8(self.deferred_squash);

        w.u8(self.dma_reg);
        match &self.dma_run {
            Some(d) => {
                w.bool(true);
                d.write_state(w);
            }
            None => w.bool(false),
        }
        match &self.dma_start {
            Some(d) => {
                w.bool(true);
                d.write_state(w);
            }
            None => w.bool(false),
        }
        w.bool(self.dma_oam_owned_prev);
        write_opt_u8x2(w, self.dma_pending_oam);
        w.bool(self.cpu_halted);
        match &self.dma_conflict {
            Some(d) => {
                w.bool(true);
                d.write_state(w);
            }
            None => w.bool(false),
        }
        w.bytes(&self.extra_oam);

        w.u16(self.hdma_src);
        w.u16(self.hdma_dst);
        w.u8(self.hdma5);
        w.u8(self.hdma_mode.tag());
        match self.vram_dma_req {
            Some(v) => {
                w.bool(true);
                w.u8(v.tag());
            }
            None => w.bool(false),
        }
        w.u8(self.halt_hdma.tag());
        w.bool(self.hdma_prev_hblank);
        w.bool(self.vram_dma_stall);
        w.bool(self.vram_dma_owns_bus);

        w.u8(self.rp);
        w.u8(self.ff72);
        w.u8(self.ff73);
        w.u8(self.ff74);
        w.u8(self.ff75);
    }

    pub(crate) fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        self.cart.read_state(r)?;
        self.ppu.read_state(r)?;
        self.apu.read_state(r)?;
        self.timer.read_state(r)?;
        self.serial.read_state(r)?;
        self.joypad.read_state(r)?;

        self.cycles = r.u64()?;
        self.clock.read_state(r)?;
        self.double_speed = r.bool()?;
        self.dot_phase = r.u8()?;
        self.key1_armed = r.bool()?;

        let wram_len = r.u32()? as usize;
        if wram_len != self.wram.len() {
            return Err(StateError::RomMismatch);
        }
        r.bytes_into(&mut self.wram)?;
        self.svbk = r.u8()?;
        r.bytes_into(&mut self.hram)?;
        self.intf = r.u8()?;
        self.ie = r.u8()?;
        self.if_late = r.u8()?;
        self.m0_halt_hold = r.u8()?;
        self.ack_squash_deadline_t = r.u64()?;
        self.wake_skew = r.u32()?;
        self.machine_now = r.u64()?;
        self.vram_dma_req_pre = r.bool()?;
        self.stat_vis_from_t = r.u64()?;
        self.halt_ly_phase = r.u8()?;
        self.if_stat_late = r.u8()?;
        self.m0_access_edge = read_opt_u8(r)?;
        self.pal_access_edge = read_opt_u8(r)?;
        self.stat_mode_edge = read_opt_u8(r)?;
        self.ack_squash_mask = r.u8()?;
        self.ack_squash_ticks = r.u8()?;
        self.ack_squash_dots = r.u8()?;
        self.deferred_squash = r.u8()?;

        self.dma_reg = r.u8()?;
        self.dma_run = if r.bool()? {
            Some(OamDmaRun::read_state(r)?)
        } else {
            None
        };
        self.dma_start = if r.bool()? {
            Some(OamDmaStart::read_state(r)?)
        } else {
            None
        };
        self.dma_oam_owned_prev = r.bool()?;
        self.dma_pending_oam = read_opt_u8x2(r)?;
        self.cpu_halted = r.bool()?;
        self.dma_conflict = if r.bool()? {
            Some(DmaConflict::read_state(r)?)
        } else {
            None
        };
        r.bytes_into(&mut self.extra_oam)?;

        self.hdma_src = r.u16()?;
        self.hdma_dst = r.u16()?;
        self.hdma5 = r.u8()?;
        self.hdma_mode = HdmaMode::from_tag(r.u8()?);
        self.vram_dma_req = if r.bool()? {
            Some(VramDmaReq::from_tag(r.u8()?))
        } else {
            None
        };
        self.halt_hdma = HaltHdmaState::from_tag(r.u8()?);
        self.hdma_prev_hblank = r.bool()?;
        self.vram_dma_stall = r.bool()?;
        self.vram_dma_owns_bus = r.bool()?;

        self.rp = r.u8()?;
        self.ff72 = r.u8()?;
        self.ff73 = r.u8()?;
        self.ff74 = r.u8()?;
        self.ff75 = r.u8()?;
        Ok(())
    }
}
