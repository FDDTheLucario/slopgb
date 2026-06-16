//! PPU save-state (de)serialization (a second `impl Ppu` block; see
//! `crate::state`). Covers the registers, OAM, both VRAM banks, the CGB palette
//! RAM, the STAT/LYC event ladder, and the sub-dot fetch/FIFO pipeline (`eff`,
//! `staged`, `render`). `model` is ROM-derived (not serialized — a state loads
//! into a same-model machine). Live-debugger/UI only, so golden-safe.

use super::*;
use crate::state::{Reader, StateError, Writer};

/// A `Option<(u8, u8)>` staged-event slot (presence byte + payload).
fn write_opt(w: &mut Writer, o: Option<(u8, u8)>) {
    match o {
        Some((a, b)) => {
            w.bool(true);
            w.u8(a);
            w.u8(b);
        }
        None => w.bool(false),
    }
}
fn read_opt(r: &mut Reader<'_>) -> Result<Option<(u8, u8)>, StateError> {
    Ok(if r.bool()? {
        Some((r.u8()?, r.u8()?))
    } else {
        None
    })
}

impl PipeRegs {
    pub(super) fn write_state(&self, w: &mut Writer) {
        for b in [
            self.lcdc, self.scy, self.scx, self.bgp, self.obp0, self.obp1, self.wy, self.wx,
        ] {
            w.u8(b);
        }
    }
    pub(super) fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        self.lcdc = r.u8()?;
        self.scy = r.u8()?;
        self.scx = r.u8()?;
        self.bgp = r.u8()?;
        self.obp0 = r.u8()?;
        self.obp1 = r.u8()?;
        self.wy = r.u8()?;
        self.wx = r.u8()?;
        Ok(())
    }
}

impl StagedWrite {
    fn write_state(&self, w: &mut Writer) {
        w.u16(self.addr);
        w.u8(self.value);
        w.u8(self.dots_left);
    }
    fn read_state(r: &mut Reader<'_>) -> Result<Self, StateError> {
        Ok(Self {
            addr: r.u16()?,
            value: r.u8()?,
            dots_left: r.u8()?,
        })
    }
}

impl Ppu {
    pub(crate) fn write_state(&self, w: &mut Writer) {
        w.u64(self.frame_count);
        for b in [
            self.lcdc,
            self.stat_en,
            self.scy,
            self.scx,
            self.ly,
            self.lyc,
            self.bgp,
            self.obp0,
            self.obp1,
            self.wy,
            self.wx,
            self.vbk,
            self.opri,
        ] {
            w.u8(b);
        }
        w.bool(self.dmg_compat);
        w.u8(self.bcps);
        w.u8(self.ocps);
        w.bytes(&self.bg_pal_ram);
        w.bytes(&self.obj_pal_ram);
        w.bytes(&self.vram[..]);
        w.bytes(&self.oam);
        write_opt(w, self.dma_freeze);
        w.bool(self.oam_dma_active);

        w.bool(self.enabled);
        w.u8(self.line);
        w.u16(self.dot);
        for b in [self.glitch_line, self.frame_skip, self.cmp, self.stat_line] {
            w.bool(b);
        }
        w.u8(self.pending_if);
        for b in [
            self.stat_late,
            self.stat_halt_late,
            self.line_render_done,
            self.m0_src,
            self.m0_rise_dot,
            self.m0_rise,
            self.m0_access_flip,
            self.pal_access_flip,
            self.m0_stat_flip,
        ] {
            w.bool(b);
        }
        w.u8(self.lyc_if_delay);
        w.u8(self.lyc_event);
        w.bool(self.cmp_irq);
        w.u8(self.stat_ev);
        write_opt(w, self.stat_ev_staged);
        w.u8(self.lyc_ev_m);
        write_opt(w, self.lyc_ev_m_staged);
        w.u8(self.stat_lyc_ev);
        write_opt(w, self.stat_lyc_ev_staged);
        w.bool(self.render_finished);
        w.bool(self.hdma_lead);

        w.bool(self.wy_latch);
        w.u8(self.wy2);
        w.u8(self.wy2_delay);
        w.bool(self.staged_ds);
        w.bool(self.ds);
        w.u8(self.win_line);
        w.bool(self.win_start_pending);

        self.eff.write_state(w);
        match &self.staged {
            Some(s) => {
                w.bool(true);
                s.write_state(w);
            }
            None => w.bool(false),
        }
        self.render.write_state(w);

        w.u32_slice(&self.front[..]);
        w.u32_slice(&self.back[..]);
        w.u32_slice(&self.dmg_palette);
    }

    pub(crate) fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        self.frame_count = r.u64()?;
        self.lcdc = r.u8()?;
        self.stat_en = r.u8()?;
        self.scy = r.u8()?;
        self.scx = r.u8()?;
        self.ly = r.u8()?;
        self.lyc = r.u8()?;
        self.bgp = r.u8()?;
        self.obp0 = r.u8()?;
        self.obp1 = r.u8()?;
        self.wy = r.u8()?;
        self.wx = r.u8()?;
        self.vbk = r.u8()?;
        self.opri = r.u8()?;
        self.dmg_compat = r.bool()?;
        self.bcps = r.u8()?;
        self.ocps = r.u8()?;
        r.bytes_into(&mut self.bg_pal_ram)?;
        r.bytes_into(&mut self.obj_pal_ram)?;
        r.bytes_into(&mut self.vram[..])?;
        r.bytes_into(&mut self.oam)?;
        self.dma_freeze = read_opt(r)?;
        self.oam_dma_active = r.bool()?;

        self.enabled = r.bool()?;
        self.line = r.u8()?;
        self.dot = r.u16()?;
        self.glitch_line = r.bool()?;
        self.frame_skip = r.bool()?;
        self.cmp = r.bool()?;
        self.stat_line = r.bool()?;
        self.pending_if = r.u8()?;
        self.stat_late = r.bool()?;
        self.stat_halt_late = r.bool()?;
        self.line_render_done = r.bool()?;
        self.m0_src = r.bool()?;
        self.m0_rise_dot = r.bool()?;
        self.m0_rise = r.bool()?;
        self.m0_access_flip = r.bool()?;
        self.pal_access_flip = r.bool()?;
        self.m0_stat_flip = r.bool()?;
        self.lyc_if_delay = r.u8()?;
        self.lyc_event = r.u8()?;
        self.cmp_irq = r.bool()?;
        self.stat_ev = r.u8()?;
        self.stat_ev_staged = read_opt(r)?;
        self.lyc_ev_m = r.u8()?;
        self.lyc_ev_m_staged = read_opt(r)?;
        self.stat_lyc_ev = r.u8()?;
        self.stat_lyc_ev_staged = read_opt(r)?;
        self.render_finished = r.bool()?;
        self.hdma_lead = r.bool()?;

        self.wy_latch = r.bool()?;
        self.wy2 = r.u8()?;
        self.wy2_delay = r.u8()?;
        self.staged_ds = r.bool()?;
        self.ds = r.bool()?;
        self.win_line = r.u8()?;
        self.win_start_pending = r.bool()?;

        self.eff.read_state(r)?;
        self.staged = if r.bool()? {
            Some(StagedWrite::read_state(r)?)
        } else {
            None
        };
        self.render.read_state(r)?;

        r.u32_slice_into(&mut self.front[..])?;
        r.u32_slice_into(&mut self.back[..])?;
        r.u32_slice_into(&mut self.dmg_palette)?;
        Ok(())
    }
}
