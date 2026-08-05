// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! APU save state (see `crate::state`). The output-stage config
//! (cycles_per_sample / max_samples) is NOT serialized: it is re-derived from
//! the live sample rate, so a state loads at the host's current rate.

use super::*;

impl Apu {
    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        w.bool(self.cgb);
        w.bool(self.power);
        self.ch1.write_state(w);
        self.ch2.write_state(w);
        self.ch3.write_state(w);
        self.ch4.write_state(w);
        w.u8(self.nr50);
        w.u8(self.nr51);
        w.u8(self.mute_mask);
        w.u8(self.div_divider);
        w.u8(match self.skip_div_event {
            SkipDivEvent::Inactive => 0,
            SkipDivEvent::Skip => 1,
            SkipDivEvent::Skipped => 2,
        });
        w.u8(self.phase);
        w.u16(self.prev_div);
        w.bool(self.last_double_speed);
        w.u8(self.lag);
        match self.pending_edge {
            None => w.u8(0),
            Some((offset, DivEdge::Falling)) => {
                w.u8(1);
                w.u8(offset as u8);
            }
            Some((offset, DivEdge::Rising)) => {
                w.u8(2);
                w.u8(offset as u8);
            }
        }
        w.u64(self.sample_frac.to_bits());
        w.u32(self.sum_l.to_bits());
        w.u32(self.sum_r.to_bits());
        w.u32(self.sum_count);
        w.u32(self.hp_charge.to_bits());
        w.u32(self.hp_cap_l.to_bits());
        w.u32(self.hp_cap_r.to_bits());
        // `samples`/`raw_samples` are the drained-per-frame OUTPUT queues, not
        // emulation state — a save must not carry them (raw_samples alone caps
        // at ~2 frames ≈ 1 MB of transient audio). Reset empty on load; the
        // stream resumes fresh, an imperceptible gap. (cf. `cycles_per_sample`,
        // also re-derived not serialized.)
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.cgb = r.bool()?;
        self.power = r.bool()?;
        self.ch1.read_state(r)?;
        self.ch2.read_state(r)?;
        self.ch3.read_state(r)?;
        self.ch4.read_state(r)?;
        self.nr50 = r.u8()?;
        self.nr51 = r.u8()?;
        self.mute_mask = r.u8()?;
        self.div_divider = r.u8()?;
        self.skip_div_event = match r.u8()? {
            0 => SkipDivEvent::Inactive,
            1 => SkipDivEvent::Skip,
            _ => SkipDivEvent::Skipped,
        };
        self.phase = r.u8()?;
        self.prev_div = r.u16()?;
        self.last_double_speed = r.bool()?;
        self.lag = r.u8()?;
        self.pending_edge = match r.u8()? {
            1 => Some((r.u8()? as i8, DivEdge::Falling)),
            2 => Some((r.u8()? as i8, DivEdge::Rising)),
            _ => None,
        };
        self.sample_frac = f64::from_bits(r.u64()?);
        self.sum_l = f32::from_bits(r.u32()?);
        self.sum_r = f32::from_bits(r.u32()?);
        self.sum_count = r.u32()?;
        self.hp_charge = f32::from_bits(r.u32()?);
        self.hp_cap_l = f32::from_bits(r.u32()?);
        self.hp_cap_r = f32::from_bits(r.u32()?);
        // Output queues are not serialized (see `write_state`) — start fresh.
        self.samples.clear();
        self.raw_samples.clear();
        Ok(())
    }
}
