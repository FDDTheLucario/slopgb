//! PPU save-state (de)serialization (a second `impl Ppu` block; see
//! `crate::state`). Covers the registers, OAM, both VRAM banks, the CGB palette
//! RAM, the STAT/LYC event ladder, and the sub-dot fetch/FIFO pipeline (`eff`,
//! `staged`, `render`). `model` is ROM-derived (not serialized — a state loads
//! into a same-model machine). Every other field of the live struct is
//! serialized, including the tier2-reclock-only ones (`leading_edge_reads`,
//! `tier2_reclock`, `eng_*`, `m0sh_*`, ...): the production/default-off path
//! keeps them at their initial 0/false, so serializing them is harmless and
//! guarantees no live field is silently dropped from a future round-trip.
//! Live-debugger/UI only, so golden-safe.

use super::*;
use crate::state::{Reader, StateError, Writer};

/// A `Option<(u8, u8)>` staged-event slot (presence byte + payload).
fn write_opt2(w: &mut Writer, o: Option<(u8, u8)>) {
    match o {
        Some((a, b)) => {
            w.bool(true);
            w.u8(a);
            w.u8(b);
        }
        None => w.bool(false),
    }
}
fn read_opt2(r: &mut Reader<'_>) -> Result<Option<(u8, u8)>, StateError> {
    Ok(if r.bool()? {
        Some((r.u8()?, r.u8()?))
    } else {
        None
    })
}

/// A `Option<(u8, u16)>` staged-event slot (presence byte + payload).
fn write_opt_u8_u16(w: &mut Writer, o: Option<(u8, u16)>) {
    match o {
        Some((a, b)) => {
            w.bool(true);
            w.u8(a);
            w.u16(b);
        }
        None => w.bool(false),
    }
}
fn read_opt_u8_u16(r: &mut Reader<'_>) -> Result<Option<(u8, u16)>, StateError> {
    Ok(if r.bool()? {
        Some((r.u8()?, r.u16()?))
    } else {
        None
    })
}

/// An `Option<EngStatPending>` staged-event slot (presence byte + payload).
/// Field order below is the on-disk layout — keep it stable.
fn write_opt_eng_stat_pending(w: &mut Writer, o: Option<EngStatPending>) {
    match o {
        Some(EngStatPending {
            phase1,
            fin,
            pre_high,
            mfi_t0,
            k,
        }) => {
            w.bool(true);
            w.u8(phase1);
            w.u8(fin);
            w.bool(pre_high);
            w.u8(mfi_t0);
            w.u8(k);
        }
        None => w.bool(false),
    }
}
fn read_opt_eng_stat_pending(r: &mut Reader<'_>) -> Result<Option<EngStatPending>, StateError> {
    Ok(if r.bool()? {
        Some(EngStatPending {
            phase1: r.u8()?,
            fin: r.u8()?,
            pre_high: r.bool()?,
            mfi_t0: r.u8()?,
            k: r.u8()?,
        })
    } else {
        None
    })
}

/// An `Option<i8>` slot (presence byte + payload, bit-cast through `u8`).
fn write_opt_i8(w: &mut Writer, o: Option<i8>) {
    match o {
        Some(v) => {
            w.bool(true);
            w.u8(v as u8);
        }
        None => w.bool(false),
    }
}
fn read_opt_i8(r: &mut Reader<'_>) -> Result<Option<i8>, StateError> {
    Ok(if r.bool()? { Some(r.u8()? as i8) } else { None })
}

impl PipeRegs {
    pub(super) fn write_state(&self, w: &mut Writer) {
        w.u8(self.lcdc);
        w.u8(self.render_lcdc);
        w.u8(self.scy);
        w.u8(self.scx);
        w.u8(self.bgp);
        w.u8(self.obp0);
        w.u8(self.obp1);
        w.u8(self.wy);
        w.u8(self.wx);
    }
    pub(super) fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        self.lcdc = r.u8()?;
        self.render_lcdc = r.u8()?;
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
    /// Serialize every field of the live struct except `model` (ROM-derived —
    /// a state loads into a same-model machine), in declaration order.
    pub(crate) fn write_state(&self, w: &mut Writer) {
        w.u64(self.frame_count);
        w.bool(self.lcd_regs_written);

        w.u8(self.lcdc);
        w.u8(self.stat_en);
        w.u8(self.eng_stat);
        write_opt_eng_stat_pending(w, self.eng_stat_pending);
        match self.eng_stat_half {
            Some((v, hd)) => {
                w.bool(true);
                w.u8(v);
                w.u8(hd);
            }
            None => w.bool(false),
        }
        w.u8(self.eng_mfi_prev);
        write_opt_u8_u16(w, self.ff41_ds_drop);
        w.u8(self.stat_if_squash);
        w.u8(self.ack_squash_ppu_mask);
        w.u8(self.ack_squash_ppu);
        w.u8(self.ly0_pulse_age);
        w.u8(self.m0sh_age);
        w.u16(self.m0sh_dot);
        w.u8(self.scy);
        w.u8(self.scx);
        w.u8(self.ly);
        w.u8(self.lyc);
        w.u8(self.bgp);
        w.u8(self.obp0);
        w.u8(self.obp1);
        w.u8(self.wy);
        w.u8(self.wx);
        w.u8(self.vbk);
        w.u8(self.opri);
        w.bool(self.dmg_compat);
        w.u8(self.bcps);
        w.u8(self.ocps);
        w.bytes(&self.bg_pal_ram);
        w.bytes(&self.obj_pal_ram);
        w.bytes(&self.vram[..]);
        w.bytes(&self.oam);
        write_opt2(w, self.dma_freeze);
        w.bool(self.oam_dma_active);

        w.bool(self.enabled);
        w.u8(self.line);
        w.u16(self.dot);
        w.u8(self.dhalf);
        w.u16(self.lcd_phase_hd as u16);
        w.u8(self.sb_dsa8);
        w.u16(self.lcd_shift_dots);
        w.bool(self.glitch_line);
        w.bool(self.frame_skip);
        w.bool(self.cmp);
        w.bool(self.stat_line);
        w.u8(self.pending_if);
        w.bool(self.stat_late);
        w.bool(self.stat_halt_late);
        w.bool(self.stat_rise_oam);
        w.bool(self.stat_rise_m0);
        w.bool(self.read_carried);
        w.bool(self.halt_refetch);
        w.bool(self.line_render_done);
        w.u16(self.flip_dot);
        w.bool(self.vis_early);
        w.u16(self.vis_hold_until);
        w.bool(self.m0_src);
        w.bool(self.m0_rise_dot);
        w.u8(self.mode_for_interrupt);
        w.bool(self.mfi_m0_prev);
        self.stat_update.write_state(w);
        w.bool(self.lyc_interrupt_line);
        w.bool(self.leading_edge_reads);
        w.bool(self.tier2_reclock);
        w.bool(self.eager_value);
        w.bool(self.m0_rise);
        write_opt_i8(w, self.m0_access_flip);
        write_opt_i8(w, self.pal_access_flip);
        write_opt_i8(w, self.m0_stat_flip);
        w.u8(self.lyc_if_delay);
        w.u16(self.l153_lyc_write_dot);
        w.u8(self.lyc_event);
        w.bool(self.cmp_irq);
        w.u8(self.stat_ev);
        write_opt2(w, self.stat_ev_staged);
        w.u8(self.lyc_ev_m);
        write_opt2(w, self.lyc_ev_m_staged);
        w.u8(self.stat_lyc_ev);
        write_opt2(w, self.stat_lyc_ev_staged);
        w.bool(self.render_finished);
        w.bool(self.hdma_lead);
        w.u16(self.pal_open_dot);

        w.bool(self.wy_latch);
        w.u8(self.wy2);
        w.u8(self.wy2_delay);
        w.bool(self.wy_trig_sb);
        w.u8(self.wy_trig_sb_line);
        w.u16(self.wy_trig_sb_dot);
        w.bool(self.stop_anchor_set);
        w.bool(self.stop_anchor_midframe);
        w.bool(self.stop_leave_lcd_on);
        w.u8(self.stop_leave_k);
        w.bool(self.lcd_enable_in_ds);
        w.bool(self.wy_trig_sb_raw);
        w.bool(self.wy_xline_trig);
        w.u8(self.vram_wr_line);
        w.u16(self.vram_wr_dot);
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
        write_opt2(w, self.render_lcdc_pending);
        self.render.write_state(w);

        w.u32_slice(&self.front[..]);
        w.u32_slice(&self.back[..]);
        w.u32_slice(&self.dmg_palette);
    }

    pub(crate) fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        self.frame_count = r.u64()?;
        self.lcd_regs_written = r.bool()?;

        self.lcdc = r.u8()?;
        self.stat_en = r.u8()?;
        self.eng_stat = r.u8()?;
        self.eng_stat_pending = read_opt_eng_stat_pending(r)?;
        self.eng_stat_half = if r.bool()? {
            Some((r.u8()?, r.u8()?))
        } else {
            None
        };
        self.eng_mfi_prev = r.u8()?;
        self.ff41_ds_drop = read_opt_u8_u16(r)?;
        self.stat_if_squash = r.u8()?;
        self.ack_squash_ppu_mask = r.u8()?;
        self.ack_squash_ppu = r.u8()?;
        self.ly0_pulse_age = r.u8()?;
        self.m0sh_age = r.u8()?;
        self.m0sh_dot = r.u16()?;
        self.scy = r.u8()?;
        self.scx = r.u8()?;
        self.ly = r.u8()?;
        self.lyc = r.u8()?;
        self.bgp = r.u8()?;
        self.obp0 = r.u8()?;
        self.obp1 = r.u8()?;
        self.wy = r.u8()?;
        self.wx = r.u8()?;
        // Mask to the FF4F-write invariant (bit 0 only): a crafted state must
        // not smuggle vbk >= 2, which would index past the 2-bank VRAM array
        // (`vram_index`). Legit saves only ever hold 0/1, so this is a no-op.
        self.vbk = r.u8()? & 1;
        self.opri = r.u8()?;
        self.dmg_compat = r.bool()?;
        self.bcps = r.u8()?;
        self.ocps = r.u8()?;
        r.bytes_into(&mut self.bg_pal_ram)?;
        r.bytes_into(&mut self.obj_pal_ram)?;
        r.bytes_into(&mut self.vram[..])?;
        r.bytes_into(&mut self.oam)?;
        self.dma_freeze = read_opt2(r)?;
        self.oam_dma_active = r.bool()?;

        self.enabled = r.bool()?;
        self.line = r.u8()?;
        self.dot = r.u16()?;
        self.dhalf = r.u8()?;
        self.lcd_phase_hd = r.u16()? as i16;
        self.sb_dsa8 = r.u8()?;
        self.lcd_shift_dots = r.u16()?;
        self.glitch_line = r.bool()?;
        self.frame_skip = r.bool()?;
        self.cmp = r.bool()?;
        self.stat_line = r.bool()?;
        self.pending_if = r.u8()?;
        self.stat_late = r.bool()?;
        self.stat_halt_late = r.bool()?;
        self.stat_rise_oam = r.bool()?;
        self.stat_rise_m0 = r.bool()?;
        self.read_carried = r.bool()?;
        self.halt_refetch = r.bool()?;
        self.line_render_done = r.bool()?;
        self.flip_dot = r.u16()?;
        self.vis_early = r.bool()?;
        self.vis_hold_until = r.u16()?;
        self.m0_src = r.bool()?;
        self.m0_rise_dot = r.bool()?;
        self.mode_for_interrupt = r.u8()?;
        self.mfi_m0_prev = r.bool()?;
        self.stat_update.read_state(r)?;
        self.lyc_interrupt_line = r.bool()?;
        self.leading_edge_reads = r.bool()?;
        self.tier2_reclock = r.bool()?;
        self.eager_value = r.bool()?;
        self.m0_rise = r.bool()?;
        self.m0_access_flip = read_opt_i8(r)?;
        self.pal_access_flip = read_opt_i8(r)?;
        self.m0_stat_flip = read_opt_i8(r)?;
        self.lyc_if_delay = r.u8()?;
        self.l153_lyc_write_dot = r.u16()?;
        self.lyc_event = r.u8()?;
        self.cmp_irq = r.bool()?;
        self.stat_ev = r.u8()?;
        self.stat_ev_staged = read_opt2(r)?;
        self.lyc_ev_m = r.u8()?;
        self.lyc_ev_m_staged = read_opt2(r)?;
        self.stat_lyc_ev = r.u8()?;
        self.stat_lyc_ev_staged = read_opt2(r)?;
        self.render_finished = r.bool()?;
        self.hdma_lead = r.bool()?;
        self.pal_open_dot = r.u16()?;

        self.wy_latch = r.bool()?;
        self.wy2 = r.u8()?;
        self.wy2_delay = r.u8()?;
        self.wy_trig_sb = r.bool()?;
        self.wy_trig_sb_line = r.u8()?;
        self.wy_trig_sb_dot = r.u16()?;
        self.stop_anchor_set = r.bool()?;
        self.stop_anchor_midframe = r.bool()?;
        self.stop_leave_lcd_on = r.bool()?;
        self.stop_leave_k = r.u8()?;
        self.lcd_enable_in_ds = r.bool()?;
        self.wy_trig_sb_raw = r.bool()?;
        self.wy_xline_trig = r.bool()?;
        self.vram_wr_line = r.u8()?;
        self.vram_wr_dot = r.u16()?;
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
        self.render_lcdc_pending = read_opt2(r)?;
        self.render.read_state(r)?;

        r.u32_slice_into(&mut self.front[..])?;
        r.u32_slice_into(&mut self.back[..])?;
        r.u32_slice_into(&mut self.dmg_palette)?;
        Ok(())
    }
}
