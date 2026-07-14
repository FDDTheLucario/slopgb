//! Combined SGB SNES-side audio coprocessor: a clean-room WDC 65C816 (the SNES
//! CPU) driving a SPC700 (S-SMP) + S-DSP, wired together as one
//! [`slopgb_core::sgb::AudioCoprocessor`] a frontend can inject with
//! [`slopgb_core::GameBoy::set_audio_coprocessor`].
//!
//! This is the LLE route the built-in HLE `SgbApu` leaves open: the built-in
//! path never runs a 65C816, so DATA_SND/JUMP are no-ops and only a self-
//! uploaded SPC700 driver (SOU_TRN) makes sound. Here the 65C816 is present, so
//! DATA_SND lands in SNES work RAM, JUMP redirects the SNES CPU, and the four
//! SNES↔APU comm ports (`$2140-$2143`) actually carry data between the two CPUs.
//!
//! # Clean-room firmware (original, not the SGB system ROM)
//!
//! The real SGB sound program lives in Nintendo's SGB cartridge SNES ROM, which
//! slopgb does not ship and this code was never allowed to read. In its place
//! this coprocessor installs an **original** two-part firmware, authored purely
//! from the WDC W65C816S datasheet's opcode encodings and nocash *fullsnes*
//! (SNES APU I/O ports `$2140-$2143`, the SPC700 opcode table, the S-DSP
//! register map):
//!
//! - a 65C816 **shim** ([`SNES_SHIM`]) that forwards a SNES-RAM sound mailbox to
//!   the SPC700 comm ports, and
//! - a SPC700 **driver** ([`spc_firmware`]) that waits on a comm port and, on the
//!   trigger, programs the S-DSP to play a synthesized square-wave voice.
//!
//! So a bare SGB `SOUND ($08)` command produces audio with no game-supplied
//! driver — the clean-room stand-in for the default sound bank. A game that
//! ships its own SPC700 driver via `SOU_TRN` still works (that upload replaces
//! the resident driver, exactly as on real hardware).
//!
//! Clocking + PCM mirror the built-in `SgbApu` (SPC700 at `125/512` GB T-cycle,
//! one 32 kHz S-DSP sample per 32 SPC cycles, zero-order-held to the output
//! rate). See `docs/hardware-state/sgb-audio.md`.

use std::cell::RefCell;
use std::rc::Rc;

use slopgb_core::sgb::{AudioCoprocessor, SgbCommandSource};
use slopgb_core::{SgbFlags, SgbSound, StateError};
use slopgb_snes_apu::dsp::SDsp;
use slopgb_snes_apu::spc700::{Dsp, Spc700};
use slopgb_snes_apu::state::{Reader, Writer};
use slopgb_w65c816::{Bus, Cpu};

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

/// GB master clock (T-cycles/s) — mirrors `slopgb_core::CLOCK_HZ`.
const GB_CLOCK_HZ: u32 = 4_194_304;
/// GB T-cycles → SPC700 cycles is `125/512` (1.024 MHz / 4.194304 MHz).
const SPC_NUM: i64 = 125;
const SPC_DEN: i64 = 512;
/// GB T-cycles → 65C816 cycles. The SNES CPU averages ~2.68 MHz once memory-
/// access wait states are folded in; `5/8` of the GB clock is close enough for
/// this HLE bridge (the two CPUs only need to make forward progress and trade
/// comm-port bytes, not stay cycle-locked).
// ponytail: fixed HLE ratio; the SNES↔GB clock relationship is loose here, not
// cycle-exact. Tighten only if a driver turns out to be timing-sensitive.
const CPU_NUM: i64 = 5;
const CPU_DEN: i64 = 8;
/// The S-DSP emits one stereo sample every 32 SPC700 cycles (→ 32 kHz).
const DSP_PERIOD: u32 = 32;
/// Full-scale S-DSP output (±32768) → mix amplitude; half scale, matching the
/// built-in path so an injected coprocessor is no louder than the default.
const MIX_SCALE: f32 = 0.5 / 32768.0;

/// SNES bank-0 address of comm port 0 (`$2140`); ports 1-3 follow. (fullsnes,
/// "SNES APU I/O Ports".)
const PORT_BASE: u16 = 0x2140;
const N_PORTS: usize = 4;
/// Where the 65C816 shim runs from, and its emulation-mode reset vector value.
const SHIM_ORG: u16 = 0x8000;
/// Emulation-mode reset vector location (`$00FFFC-$00FFFD`).
const RESET_VEC: usize = 0xFFFC;
/// The sound mailbox the shim forwards: `[note, trigger]` in SNES work RAM.
const MB_NOTE: u16 = 0x0200;
const MB_TRIG: u16 = 0x0201;

/// The clean-room 65C816 shim (emulation mode, 8-bit). It copies the SNES-RAM
/// mailbox to the SPC700 comm ports forever, so a mailbox write reaches the
/// audio CPU. Opcodes are the WDC datasheet encodings (`AD`/`8D` = LDA/STA abs,
/// `4C` = JMP abs):
///
/// ```text
/// $8000  LDA $0200   ; A = mailbox note
/// $8003  STA $2140   ; -> APUIO0 (note the SPC700 reads at $F4)
/// $8006  LDA $0201   ; A = mailbox trigger
/// $8009  STA $2141   ; -> APUIO1 (trigger the SPC700 polls at $F5)
/// $800C  JMP $8000   ; loop
/// ```
const SNES_SHIM: [u8; 15] = [
    0xAD,
    (MB_NOTE & 0xFF) as u8,
    (MB_NOTE >> 8) as u8, // LDA $0200
    0x8D,
    (PORT_BASE & 0xFF) as u8,
    (PORT_BASE >> 8) as u8, // STA $2140
    0xAD,
    (MB_TRIG & 0xFF) as u8,
    (MB_TRIG >> 8) as u8, // LDA $0201
    0x8D,
    ((PORT_BASE + 1) & 0xFF) as u8,
    ((PORT_BASE + 1) >> 8) as u8, // STA $2141
    0x4C,
    (SHIM_ORG & 0xFF) as u8,
    (SHIM_ORG >> 8) as u8, // JMP $8000
];

/// A lightweight [`Dsp`] the SPC700 owns; forwards `$F2`/`$F3` accesses to the
/// shared [`SDsp`]. Synthesis (which needs APU RAM) is driven by [`SgbCoprocessor::clock`].
struct DspLink(Rc<RefCell<SDsp>>);

impl Dsp for DspLink {
    fn read(&mut self, addr: u8) -> u8 {
        self.0.borrow_mut().read(addr)
    }
    fn write(&mut self, addr: u8, val: u8) {
        self.0.borrow_mut().write(addr, val);
    }
}

/// SNES work RAM + the comm-port latches the 65C816 shares with the SPC700.
/// `$2140-$2143` (any bank — banks alias) route to the ports; everything else is
/// plain RAM.
#[derive(Clone)]
struct SnesBus {
    ram: Box<[u8; 0x1_0000]>,
    /// 65C816 → SPC700: what the CPU last wrote to `$2140-$2143`.
    to_apu: [u8; N_PORTS],
    /// SPC700 → 65C816: what the SPC700 last wrote back, for CPU reads of the
    /// same window.
    from_apu: [u8; N_PORTS],
}

impl SnesBus {
    fn new() -> Self {
        let mut ram = Box::new([0u8; 0x1_0000]);
        ram[SHIM_ORG as usize..SHIM_ORG as usize + SNES_SHIM.len()].copy_from_slice(&SNES_SHIM);
        ram[RESET_VEC] = SHIM_ORG as u8;
        ram[RESET_VEC + 1] = (SHIM_ORG >> 8) as u8;
        SnesBus {
            ram,
            to_apu: [0; N_PORTS],
            from_apu: [0; N_PORTS],
        }
    }

    /// The comm-port index an address maps to (`$2140-$2143` in any bank), or
    /// `None` for plain RAM.
    fn port_index(addr: u32) -> Option<usize> {
        let low = (addr & 0xFFFF) as u16;
        (low >= PORT_BASE && low < PORT_BASE + N_PORTS as u16).then(|| (low - PORT_BASE) as usize)
    }
}

impl Bus for SnesBus {
    fn read(&mut self, addr: u32) -> u8 {
        match Self::port_index(addr) {
            Some(p) => self.from_apu[p],
            None => self.ram[(addr & 0xFFFF) as usize],
        }
    }
    fn write(&mut self, addr: u32, value: u8) {
        match Self::port_index(addr) {
            Some(p) => self.to_apu[p] = value,
            None => self.ram[(addr & 0xFFFF) as usize] = value,
        }
    }
}

/// The combined coprocessor: a 65C816 over [`SnesBus`] plus a SPC700 + S-DSP,
/// clocked off the Game Boy stream and mixed into its audio.
pub struct SgbCoprocessor {
    cpu: Cpu,
    bus: SnesBus,
    spc: Spc700,
    dsp: Rc<RefCell<SDsp>>,

    /// 65C816 cycle budget carried across `clock`s (in `1/CPU_DEN` units).
    cpu_acc: i64,
    /// SPC700 cycle budget carried across `clock`s (in `1/SPC_DEN` units).
    spc_acc: i64,
    /// SPC cycles accumulated toward the next 32 kHz DSP sample.
    dsp_div: u32,
    /// Latest 32 kHz DSP sample, zero-order-held between updates.
    cur: (i16, i16),

    /// Output-rate emission accumulator (in GB T-cycles) and the cycles-per-
    /// sample law (mirrors the GB APU so the streams stay sample-aligned).
    samp_acc: f64,
    cycles_per_sample: f64,
    out: Vec<(f32, f32)>,
    max_out: usize,

    /// Command-poll throttle for the transfer getters (they persist between
    /// transfers, so edge-detect by checksum — same policy as the built-in).
    poll_ctr: u32,
    sou_trn_sig: u64,
    data_trn_sig: u64,
    jump: Option<u32>,
}

impl SgbCoprocessor {
    /// Build the coprocessor at `output_rate` Hz, with the clean-room firmware
    /// installed (65C816 shim resident in SNES RAM, SPC700 driver + sample bank
    /// resident in APU RAM) and both CPUs at their reset entry points.
    #[must_use]
    pub fn new(output_rate: u32) -> Self {
        let dsp = Rc::new(RefCell::new(SDsp::new()));
        let mut spc = Spc700::new();
        spc.attach_dsp(Box::new(DspLink(Rc::clone(&dsp))));
        let rate = output_rate.max(1);
        let mut me = SgbCoprocessor {
            cpu: Cpu::new(),
            bus: SnesBus::new(),
            spc,
            dsp,
            cpu_acc: 0,
            spc_acc: 0,
            dsp_div: 0,
            cur: (0, 0),
            samp_acc: 0.0,
            cycles_per_sample: f64::from(GB_CLOCK_HZ) / f64::from(rate),
            out: Vec::new(),
            max_out: rate as usize,
            poll_ctr: 0,
            sou_trn_sig: 0,
            data_trn_sig: 0,
            jump: None,
        };
        me.install_firmware();
        me
    }

    /// (Re)install the resident firmware and point both CPUs at their entry.
    fn install_firmware(&mut self) {
        self.cpu = Cpu::new();
        let lo = u16::from(self.bus.ram[RESET_VEC]);
        let hi = u16::from(self.bus.ram[RESET_VEC + 1]);
        self.cpu.regs.pc = lo | (hi << 8);
        install_spc_firmware(&mut self.spc);
    }

    // -- Clocking -----------------------------------------------------------

    fn clock(&mut self, gb_cycles: u64) {
        // 1. Deliver the 65C816's last comm-port writes to the SPC700.
        for p in 0..N_PORTS {
            self.spc.snes_write_port(p, self.bus.to_apu[p]);
        }

        // 2. Run the SPC700 + S-DSP, synthesizing samples into `cur`.
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

        // 3. Read the SPC700's comm-port replies back for the 65C816.
        for p in 0..N_PORTS {
            self.bus.from_apu[p] = self.spc.snes_read_port(p);
        }

        // 4. Run the 65C816 shim.
        self.cpu_acc += gb_cycles as i64 * CPU_NUM;
        while self.cpu_acc >= CPU_DEN && !self.cpu.stopped {
            let spent = self.cpu.step(&mut self.bus);
            self.cpu_acc -= spent as i64 * CPU_DEN;
        }
        if self.cpu.stopped {
            self.cpu_acc = 0;
        }

        // 5. Emit output-rate samples (zero-order-hold of the 32 kHz stream).
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

    fn mix_into(&mut self, gb: &mut [(f32, f32)]) {
        let n = gb.len().min(self.out.len());
        for (dst, src) in gb.iter_mut().zip(self.out.iter()).take(n) {
            dst.0 += src.0;
            dst.1 += src.1;
        }
        self.out.drain(..n);
    }

    fn set_output_rate(&mut self, hz: u32) {
        let hz = hz.max(1);
        self.cycles_per_sample = f64::from(GB_CLOCK_HZ) / f64::from(hz);
        self.max_out = hz as usize;
        self.samp_acc = 0.0;
        self.out.clear();
    }

    /// Drain the stereo output-rate PCM synthesized since the last drain, oldest
    /// first — the equivalent of the tier-3 plugin ABI's `drain_pcm`, for a host
    /// that would rather pull the samples than have them mixed in.
    pub fn drain_pcm(&mut self) -> Vec<(f32, f32)> {
        std::mem::take(&mut self.out)
    }

    // -- SGB command routing ------------------------------------------------

    fn poll(&mut self, cmds: &mut dyn SgbCommandSource) {
        // SOUND ($08): a play request. Deposit the effect id + a trigger in the
        // mailbox; the 65C816 shim forwards them to the SPC700 driver.
        while let Some(s) = cmds.take_sound_event() {
            self.apply_sound(s);
        }
        // DATA_SND ($0F): a write to SNES work RAM — no longer a no-op. fullsnes:
        // the packet is `dest_lo, dest_hi, len, data…`.
        while let Some(pkt) = cmds.take_data_snd() {
            self.apply_data_snd(&pkt);
        }

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
                self.upload_transfer(data, false);
            }
        }
        if let Some(flags) = cmds.flags() {
            self.apply_flags(flags);
        }
    }

    /// SOUND ($08): mailbox `note = effect_a`, `trigger = 1` (or the effect-on
    /// flags byte if non-zero), so the shim wakes the SPC700 driver.
    fn apply_sound(&mut self, s: SgbSound) {
        let trig = if s.attenuation != 0 { s.attenuation } else { 1 };
        self.bus.ram[MB_NOTE as usize] = s.effect_a;
        self.bus.ram[MB_TRIG as usize] = trig;
    }

    /// DATA_SND ($0F): copy the packet's data into SNES work RAM at its target
    /// address (bank 0), the write the 65C816 sound program would service.
    fn apply_data_snd(&mut self, pkt: &[u8]) {
        if pkt.len() < 3 {
            return;
        }
        let dest = u16::from(pkt[0]) | (u16::from(pkt[1]) << 8);
        let len = usize::from(pkt[2]);
        for (i, &b) in pkt[3..].iter().take(len).enumerate() {
            self.bus.ram[dest.wrapping_add(i as u16) as usize] = b;
        }
    }

    /// JUMP ($12): redirect the 65C816 to the SNES program target — no longer a
    /// no-op now that a real SNES CPU is present.
    fn apply_flags(&mut self, flags: SgbFlags) {
        if let Some(target) = flags.jump {
            if self.jump != Some(target) {
                self.jump = Some(target);
                self.cpu.regs.pbr = (target >> 16) as u8;
                self.cpu.regs.pc = target as u16;
                self.cpu.stopped = false;
                self.cpu.waiting = false;
            }
        }
    }

    /// Copy a self-describing `(dest, len, data…)` transfer block into APU RAM
    /// (fullsnes: SGB sound transfers begin with a destination/length pair); with
    /// `start`, point the SPC700 at the first load address. Same shape as the
    /// built-in `SgbApu` uploader, so a `SOU_TRN` game driver runs identically.
    fn upload_transfer(&mut self, data: &[u8], start: bool) {
        let ram = self.spc.apu_ram_mut();
        let mut off = 0usize;
        let mut entry = None;
        while off + 4 <= data.len() {
            let dest = u16::from_le_bytes([data[off], data[off + 1]]);
            let len = usize::from(u16::from_le_bytes([data[off + 2], data[off + 3]]));
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

    // -- Save state ---------------------------------------------------------

    fn write_state(&self, w: &mut Writer) {
        self.spc.write_state(w);
        self.dsp.borrow().write_state(w);
        write_cpu(w, &self.cpu);
        w.bytes(&self.bus.ram[..]);
        for p in 0..N_PORTS {
            w.u8(self.bus.to_apu[p]);
            w.u8(self.bus.from_apu[p]);
        }
        w.u64(self.cpu_acc as u64);
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
    }

    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        self.spc.read_state(r)?;
        self.dsp.borrow_mut().read_state(r)?;
        read_cpu(r, &mut self.cpu)?;
        r.bytes_into(&mut self.bus.ram[..])?;
        for p in 0..N_PORTS {
            self.bus.to_apu[p] = r.u8()?;
            self.bus.from_apu[p] = r.u8()?;
        }
        self.cpu_acc = r.u64()? as i64;
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

impl Clone for SgbCoprocessor {
    fn clone(&self) -> Self {
        // Deep-clone the DSP into a fresh cell and re-attach a link to the cloned
        // SPC700 (its own `Clone` drops the trait object), mirroring `SgbApu`.
        let dsp = Rc::new(RefCell::new(self.dsp.borrow().clone()));
        let mut spc = self.spc.clone();
        spc.attach_dsp(Box::new(DspLink(Rc::clone(&dsp))));
        SgbCoprocessor {
            cpu: self.cpu.clone(),
            bus: self.bus.clone(),
            spc,
            dsp,
            cpu_acc: self.cpu_acc,
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
        }
    }
}

impl AudioCoprocessor for SgbCoprocessor {
    fn clock(&mut self, gb_cycles: u64) {
        SgbCoprocessor::clock(self, gb_cycles);
    }
    fn poll(&mut self, cmds: &mut dyn SgbCommandSource) {
        SgbCoprocessor::poll(self, cmds);
    }
    fn mix_into(&mut self, out: &mut [(f32, f32)]) {
        SgbCoprocessor::mix_into(self, out);
    }
    fn set_output_rate(&mut self, hz: u32) {
        SgbCoprocessor::set_output_rate(self, hz);
    }
    fn load_bios(&mut self, _bios: &[u8]) {
        // The resident clean-room firmware is fixed; there is no user BIOS image
        // to install (and slopgb never reads the copyrighted SGB system ROM).
    }
    fn write_state(&self, w: &mut Writer) {
        SgbCoprocessor::write_state(self, w);
    }
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        SgbCoprocessor::read_state(self, r)
    }
    fn clone_box(&self) -> Box<dyn AudioCoprocessor> {
        Box::new(self.clone())
    }
}

/// Serialize the 65C816 register file + halt flags.
fn write_cpu(w: &mut Writer, cpu: &Cpu) {
    let r = &cpu.regs;
    w.u16(r.a);
    w.u16(r.x);
    w.u16(r.y);
    w.u16(r.s);
    w.u16(r.d);
    w.u16(r.pc);
    w.u8(r.pbr);
    w.u8(r.dbr);
    w.u8(r.p);
    w.bool(r.e);
    w.bool(cpu.stopped);
    w.bool(cpu.waiting);
}

fn read_cpu(r: &mut Reader<'_>, cpu: &mut Cpu) -> Result<(), StateError> {
    let regs = &mut cpu.regs;
    regs.a = r.u16()?;
    regs.x = r.u16()?;
    regs.y = r.u16()?;
    regs.s = r.u16()?;
    regs.d = r.u16()?;
    regs.pc = r.u16()?;
    regs.pbr = r.u8()?;
    regs.dbr = r.u8()?;
    regs.p = r.u8()?;
    regs.e = r.bool()?;
    cpu.stopped = r.bool()?;
    cpu.waiting = r.bool()?;
    Ok(())
}

/// Install the clean-room SPC700 driver + one-entry sample directory + a square
/// BRR sample into APU RAM and point the SPC700 at the driver entry.
fn install_spc_firmware(spc: &mut Spc700) {
    let (prog, dir, brr) = spc_firmware();
    let ram = spc.apu_ram_mut();
    ram[0x0400..0x0400 + prog.len()].copy_from_slice(&prog);
    ram[0x0200..0x0200 + dir.len()].copy_from_slice(&dir);
    ram[0x0210..0x0210 + brr.len()].copy_from_slice(&brr);
    spc.set_pc(0x0400);
}

/// The original clean-room SPC700 driver: wait on comm port 1 (the SNES
/// trigger), then program the S-DSP to key a ~2 kHz square-wave voice. Authored
/// from the SPC700 opcode table + S-DSP register map (nocash *fullsnes*), never
/// from a ROM. Returns `(program@$0400, directory@$0200, sample@$0210)`.
fn spc_firmware() -> (Vec<u8>, [u8; 4], Vec<u8>) {
    // `MOV dp,#imm` = `8F imm dp`; `MOV A,dp` = `E4 dp`; `CLRP` = `20`;
    // `BEQ rel` = `F0 rel`; `BRA rel` = `2F rel` (fullsnes opcode table).
    let mov = |dp: u8, imm: u8| [0x8F, imm, dp];
    let mut prog = Vec::new();
    prog.push(0x20); // CLRP: direct page = $00xx, so $F5 is the comm port
    // wait: MOV A,$F5 / BEQ wait  — spin until the SNES sets the trigger port.
    prog.extend_from_slice(&[0xE4, 0xF5]); // MOV A,$F5 (port_in[1])
    prog.extend_from_slice(&[0xF0, 0xFC]); // BEQ -4 -> the MOV above
    // The S-DSP program: voice 0, GAIN-direct, square sample, KON last.
    let dsp_writes: [(u8, u8); 12] = [
        (0x6C, 0x00), // FLG: unmute, no reset, noise off
        (0x5D, 0x02), // DIR = page $02 (directory at $0200)
        (0x0C, 0x7F), // MVOLL
        (0x1C, 0x7F), // MVOLR
        (0x00, 0x7F), // V0 VOLL
        (0x01, 0x7F), // V0 VOLR
        (0x02, 0x00), // V0 pitch lo
        (0x03, 0x10), // V0 pitch hi -> $1000
        (0x04, 0x00), // V0 SRCN = directory entry 0
        (0x05, 0x00), // V0 ADSR1 = 0 -> use GAIN
        (0x07, 0x7F), // V0 GAIN = direct max
        (0x4C, 0x01), // KON voice 0 (last)
    ];
    for (dp, imm) in dsp_writes {
        prog.extend_from_slice(&mov(0xF2, dp)); // select DSP register
        prog.extend_from_slice(&mov(0xF3, imm)); // write it
    }
    prog.extend_from_slice(&[0x2F, 0xFE]); // BRA * (spin so the DSP keeps playing)

    // One-entry sample directory: start = loop = $0210.
    let dir = [0x10u8, 0x02, 0x10, 0x02];
    // A 16-sample square BRR block: header shift 9 / filter 0 / loop + end, then
    // eight +7 nibbles and eight -8 nibbles -> a square wave, looped at $1000
    // pitch = 32 kHz / 16 = 2 kHz.
    let brr = vec![0x93u8, 0x77, 0x77, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88];
    (prog, dir, brr)
}

/// A cheap order-sensitive checksum for edge-detecting transfer uploads (FNV-1a).
fn checksum(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
