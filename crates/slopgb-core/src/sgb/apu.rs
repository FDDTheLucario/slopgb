//! The Super Game Boy audio subsystem: an [`Spc700`] + [`SDsp`] clocked off the
//! Game Boy's cycle stream, plus the SNES-side SGB command routing (SOUND /
//! SOU_TRN / DATA_SND / JUMP) and the optional audio-BIOS gating.
//!
//! # Clocking
//!
//! The SPC700 runs at 1.024 MHz; the Game Boy at 4.194304 MHz, so one GB
//! T-cycle is exactly `125/512` SPC cycle. Each Game Boy instruction, the SGB
//! APU advances the SPC700 by that many cycles; the S-DSP produces one 32 kHz
//! stereo sample every 32 SPC cycles (1.024 MHz ÷ 32). That 32 kHz stream is
//! zero-order-held into the Game Boy APU's output rate (48 kHz by default) so
//! the two streams mix sample-for-sample in [`crate::GameBoy::drain_audio`].
//!
//! # Data paths & BIOS gating (be honest about this)
//!
//! - **SOU_TRN ($09)** uploads a self-describing SPC700 program into APU RAM and
//!   starts it — a game that ships its own driver (e.g. Space Invaders) gets
//!   real audio with **no BIOS**.
//! - **SOUND ($08)** is decoded to the four SNES↔APU comm ports. The exact
//!   effect-code → port encoding used by the *standard* SGB driver is **not**
//!   publicly documented (it lives in the SGB system ROM), so this is a
//!   best-effort seam that drives whatever driver is resident in APU RAM.
//! - **DATA_SND ($0F)** targets SNES **work RAM**, not the APU; with no 65816
//!   core it has no audio effect and is drained/ignored.
//! - **JUMP ($12)** records the SNES program-jump target (no 65816 to run it).
//! - **The SGB default sound driver + sample bank live in the SGB cartridge's
//!   SNES ROM**, which slopgb does not ship. [`SgbApu::load_bios`] accepts a
//!   user-supplied image and stores it, but without a 65816 core the default
//!   driver is not executed — so **absent a self-uploaded driver, SGB audio is
//!   silent** while everything else works. No sample data is ever fabricated.
//!
//! Only ever constructed on `Model::Sgb`/`Sgb2`, so `Dmg`/`Cgb` are unaffected.
//! See `docs/hardware-state/sgb-audio.md`.

use std::cell::RefCell;
use std::rc::Rc;

use super::dsp::SDsp;
use super::spc700::{Dsp, Spc700};
use crate::SgbSound;

/// The S-DSP emits one stereo sample every 32 SPC700 cycles (→ 32 kHz).
const DSP_PERIOD: u32 = 32;
/// GB T-cycles → SPC700 cycles is `125/512` (1.024 MHz / 4.194304 MHz). Budget
/// is accumulated in `1/512`-SPC-cycle units to stay exact.
const SPC_NUM: i64 = 125;
const SPC_DEN: i64 = 512;
/// Full-scale S-DSP output (±32768) → mix amplitude. Unity: full-scale DSP maps
/// to ±1.0, the same headroom as the GB APU's full-scale output, so SGB music
/// sits at parity with the GB channels instead of half-volume under them.
// ponytail: fixed loudness balance; expose a per-game knob if one needs it.
const MIX_SCALE: f32 = 1.0 / 32768.0;

/// A lightweight [`Dsp`] the SPC700 owns; it forwards `$F2`/`$F3` register
/// accesses to the shared [`SDsp`]. Synthesis (which needs APU RAM) is driven
/// externally by [`SgbApu::clock`], so [`Dsp::tick`] is a no-op here.
struct DspLink(Rc<RefCell<SDsp>>);

impl Dsp for DspLink {
    fn read(&mut self, addr: u8) -> u8 {
        self.0.borrow_mut().read(addr)
    }
    fn write(&mut self, addr: u8, val: u8) {
        self.0.borrow_mut().write(addr, val);
    }
}

/// The SGB audio subsystem.
pub(crate) struct SgbApu {
    spc: Spc700,
    /// The S-DSP, shared with the [`DspLink`] attached to `spc`.
    dsp: Rc<RefCell<SDsp>>,

    /// SPC-cycle budget carried across instructions, in `1/512`-cycle units.
    spc_acc: i64,
    /// SPC cycles accumulated toward the next 32 kHz DSP sample.
    dsp_div: u32,
    /// Latest 32 kHz DSP sample, zero-order-held between updates.
    cur: (i16, i16),

    /// Output-rate emission accumulator (in GB T-cycles).
    samp_acc: f64,
    /// GB T-cycles per output sample (`CLOCK_HZ / output_rate`).
    cycles_per_sample: f64,
    /// Output-rate stereo samples awaiting mix, capped at ~1 s.
    out: Vec<(f32, f32)>,
    max_out: usize,

    /// Command-poll throttle for the transfer getters.
    poll_ctr: u32,
    /// Checksums of the last applied SOU_TRN / DATA_TRN payloads (edge detect).
    sou_trn_sig: u64,
    data_trn_sig: u64,
    /// Latched JUMP ($12) SNES target (recorded; not executed — no 65816).
    jump: Option<u32>,
    /// User-supplied SGB audio BIOS (SNES ROM image). Stored for forward
    /// compatibility; see the module docs for what it does and does not enable.
    bios: Option<Vec<u8>>,
}

impl SgbApu {
    /// Build an SGB APU targeting `output_rate` Hz. Only `Model::Sgb`/`Sgb2`
    /// machines create one (see [`Self::for_model`]).
    fn new(output_rate: u32) -> Self {
        let dsp = Rc::new(RefCell::new(SDsp::new()));
        let mut spc = Spc700::new();
        spc.attach_dsp(Box::new(DspLink(Rc::clone(&dsp))));
        let rate = output_rate.max(1);
        SgbApu {
            spc,
            dsp,
            spc_acc: 0,
            dsp_div: 0,
            cur: (0, 0),
            samp_acc: 0.0,
            cycles_per_sample: f64::from(crate::CLOCK_HZ) / f64::from(rate),
            out: Vec::new(),
            max_out: rate as usize,
            poll_ctr: 0,
            sou_trn_sig: 0,
            data_trn_sig: 0,
            jump: None,
            bios: None,
        }
    }

    /// `Some(SgbApu)` on `Model::Sgb`/`Sgb2`, else `None` (SGB audio exists only
    /// on the Super Game Boy).
    pub(crate) fn for_model(model: crate::Model) -> Option<Self> {
        matches!(model, crate::Model::Sgb | crate::Model::Sgb2)
            .then(|| Self::new(crate::DEFAULT_SAMPLE_RATE))
    }

    /// Retarget the output sample rate (mirrors the Game Boy APU rate so the two
    /// streams stay aligned for the mix). Clears the pending output.
    pub(crate) fn set_output_rate(&mut self, hz: u32) {
        let hz = hz.max(1);
        self.cycles_per_sample = f64::from(crate::CLOCK_HZ) / f64::from(hz);
        self.max_out = hz as usize;
        self.samp_acc = 0.0;
        self.out.clear();
    }

    // -- Clocking -----------------------------------------------------------

    /// Advance the SPC700 + S-DSP by `gb_cycles` Game Boy T-cycles, emitting
    /// output-rate samples.
    pub(crate) fn clock(&mut self, gb_cycles: u64) {
        self.spc_acc += gb_cycles as i64 * SPC_NUM;
        while self.spc_acc >= SPC_DEN {
            let cyc = self.spc.step();
            self.spc_acc -= i64::from(cyc) * SPC_DEN;
            self.dsp_div += cyc;
            while self.dsp_div >= DSP_PERIOD {
                self.dsp_div -= DSP_PERIOD;
                self.cur = self.dsp.borrow_mut().sample(self.spc.apu_ram_mut());
            }
        }

        // Emit output-rate samples (zero-order-hold of the 32 kHz DSP stream),
        // using the same accumulator law as the GB APU so counts stay aligned.
        self.samp_acc += gb_cycles as f64;
        while self.samp_acc >= self.cycles_per_sample {
            self.samp_acc -= self.cycles_per_sample;
            if self.out.len() < self.max_out {
                self.out.push((
                    f32::from(self.cur.0) * MIX_SCALE,
                    f32::from(self.cur.1) * MIX_SCALE,
                ));
            }
        }
    }

    /// Add the pending SGB samples into `gb` (the Game Boy samples just drained),
    /// sample-for-sample, keeping any surplus for the next drain.
    pub(crate) fn mix_into(&mut self, gb: &mut [(f32, f32)]) {
        let n = gb.len().min(self.out.len());
        for (dst, src) in gb.iter_mut().zip(self.out.iter()).take(n) {
            dst.0 += src.0;
            dst.1 += src.1;
        }
        self.out.drain(..n);
    }

    // -- SGB command routing ------------------------------------------------

    /// Drain the SGB command seams and apply them.
    pub(crate) fn poll(&mut self, cmds: &mut dyn super::SgbCommandSource) {
        // Best-effort sound-effect events → comm ports.
        while let Some(sound) = cmds.take_sound_event() {
            self.apply_sound(sound);
        }
        // DATA_SND targets SNES work RAM; no audio effect without a 65816.
        while cmds.take_data_snd().is_some() {}

        // Poll the transfer getters + JUMP occasionally (they persist between
        // transfers, so edge-detect by checksum).
        self.poll_ctr = self.poll_ctr.wrapping_add(1);
        if self.poll_ctr & 0x3F != 0 {
            return;
        }
        if let Some(data) = cmds.sou_trn_data() {
            let sig = checksum(data);
            if sig != self.sou_trn_sig {
                self.sou_trn_sig = sig;
                self.upload_transfer(data, true);
            }
        }
        if let Some(data) = cmds.data_trn_data() {
            let sig = checksum(data);
            if sig != self.data_trn_sig {
                self.data_trn_sig = sig;
                // DATA_TRN really targets SNES work RAM; with no 65816 we push it
                // through the same self-describing uploader as SOU_TRN. There is
                // NO gate that the block actually carries APU descriptors —
                // `upload_transfer` unconditionally reads the first 4 bytes as a
                // (dest,len) pair and only stops on len==0 or an overrun, so a
                // non-descriptor DATA_TRN can scribble APU RAM. `start=false` at
                // least keeps it from redirecting the SPC700's PC. (Limitation:
                // the true DATA_TRN format/destination is not modeled.)
                self.upload_transfer(data, false);
            }
        }
        if let Some(flags) = cmds.flags() {
            if flags.jump.is_some() {
                self.jump = flags.jump;
            }
        }
    }

    /// SOUND ($08): drive the four comm ports. The standard SGB driver's exact
    /// port encoding is undocumented, so this is a best-effort mapping that
    /// feeds whatever driver is resident in APU RAM.
    fn apply_sound(&mut self, s: SgbSound) {
        self.spc.snes_write_port(0, s.effect_a);
        self.spc.snes_write_port(1, s.effect_b);
        self.spc.snes_write_port(2, s.attenuation);
        self.spc.snes_write_port(3, s.effect_bank);
    }

    /// Copy a self-describing transfer block into APU RAM. The block is a run of
    /// `(dest:u16, len:u16, data[len])` descriptors (fullsnes: SGB sound
    /// transfers begin with a destination/length pair). `start` requests that
    /// the SPC700 begin executing at the first descriptor's address (typically
    /// the Program Area `0x0400`); the true entry point is not publicly
    /// documented, so this is best-effort.
    fn upload_transfer(&mut self, data: &[u8], start: bool) {
        let ram = self.spc.apu_ram_mut();
        let mut off = 0usize;
        let mut entry = None;
        while off + 4 <= data.len() {
            // SBN / SNES APU block header is `[u16 len, u16 dest]` (SBN2SPC; the
            // SGB system ROM's loader) — length first, destination second.
            let len = usize::from(u16::from_le_bytes([data[off], data[off + 1]]));
            let dest = u16::from_le_bytes([data[off + 2], data[off + 3]]);
            off += 4;
            if len == 0 || off + len > data.len() {
                break;
            }
            for (i, &b) in data[off..off + len].iter().enumerate() {
                ram[dest.wrapping_add(i as u16) as usize] = b;
            }
            entry.get_or_insert(dest);
            off += len;
        }
        if let (true, Some(e)) = (start, entry) {
            self.spc.set_pc(e);
        }
    }

    /// Store the optional user-supplied SGB audio BIOS (see the module docs for
    /// what it does and does not enable).
    pub(crate) fn load_bios(&mut self, bios: &[u8]) {
        self.bios = Some(bios.to_vec());
    }

    // -- Output drain -------------------------------------------------------

    // (mix is done via `mix_into`; the GB APU stream is the drain driver.)

    // -- Save state ---------------------------------------------------------

    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        self.spc.write_state(w);
        self.dsp.borrow().write_state(w);
        w.u64(self.spc_acc as u64);
        w.u32(self.dsp_div);
        w.u16(self.cur.0 as u16);
        w.u16(self.cur.1 as u16);
        w.u64(self.samp_acc.to_bits());
        w.u32(self.poll_ctr);
        w.u64(self.sou_trn_sig);
        w.u64(self.data_trn_sig);
        w.bool(self.jump.is_some());
        w.u32(self.jump.unwrap_or(0));
        // `out` is transient output (like the GB APU sample queue) and the BIOS
        // is host configuration — neither is emulation state, so neither is
        // serialized; the output rate is re-derived from the live host rate.
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::StateError> {
        self.spc.read_state(r)?;
        self.dsp.borrow_mut().read_state(r)?;
        self.spc_acc = r.u64()? as i64;
        self.dsp_div = r.u32()?;
        self.cur.0 = r.u16()? as i16;
        self.cur.1 = r.u16()? as i16;
        self.samp_acc = f64::from_bits(r.u64()?);
        self.poll_ctr = r.u32()?;
        self.sou_trn_sig = r.u64()?;
        self.data_trn_sig = r.u64()?;
        let has_jump = r.bool()?;
        let j = r.u32()?;
        self.jump = has_jump.then_some(j);
        self.out.clear();
        Ok(())
    }
}

// The swap seam (`super::AudioCoprocessor`) bridges to the built-in
// implementation: each method forwards to the identically-named inherent method
// above, so the built-in path is byte-identical whether reached directly (unit
// tests hold a concrete `SgbApu`) or through the trait object `GameBoy` holds.
impl super::AudioCoprocessor for SgbApu {
    fn clock(&mut self, gb_cycles: u64) {
        SgbApu::clock(self, gb_cycles);
    }

    fn poll(&mut self, cmds: &mut dyn super::SgbCommandSource) {
        SgbApu::poll(self, cmds);
    }

    fn mix_into(&mut self, out: &mut [(f32, f32)]) {
        SgbApu::mix_into(self, out);
    }

    fn set_output_rate(&mut self, hz: u32) {
        SgbApu::set_output_rate(self, hz);
    }

    fn load_bios(&mut self, bios: &[u8]) {
        SgbApu::load_bios(self, bios);
    }

    fn write_state(&self, w: &mut crate::state::Writer) {
        SgbApu::write_state(self, w);
    }

    fn read_state(&mut self, r: &mut crate::state::Reader<'_>) -> Result<(), crate::StateError> {
        SgbApu::read_state(self, r)
    }

    fn clone_box(&self) -> Box<dyn super::AudioCoprocessor> {
        Box::new(self.clone())
    }
    // No `export_spc`: this HLE square driver plays SGB sound effects, not
    // sequenced music with a song-start to snapshot from — the trait default
    // (`None` / not exportable) greys the export action for it.
}

impl Clone for SgbApu {
    fn clone(&self) -> Self {
        // Deep-clone the DSP into a fresh shared cell and re-attach a link to the
        // cloned SPC700 (its own `Clone` drops the trait object).
        let dsp = Rc::new(RefCell::new(self.dsp.borrow().clone()));
        let mut spc = self.spc.clone();
        spc.attach_dsp(Box::new(DspLink(Rc::clone(&dsp))));
        SgbApu {
            spc,
            dsp,
            spc_acc: self.spc_acc,
            dsp_div: self.dsp_div,
            cur: self.cur,
            samp_acc: self.samp_acc,
            cycles_per_sample: self.cycles_per_sample,
            out: self.out.clone(),
            max_out: self.max_out,
            poll_ctr: self.poll_ctr,
            sou_trn_sig: self.sou_trn_sig,
            data_trn_sig: self.data_trn_sig,
            jump: self.jump,
            bios: self.bios.clone(),
        }
    }
}

/// A cheap order-sensitive checksum for edge-detecting transfer uploads.
fn checksum(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64; // FNV-1a offset basis
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
#[path = "apu_tests.rs"]
mod tests;
