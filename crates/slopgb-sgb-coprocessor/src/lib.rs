//! Combined SGB SNES-side audio coprocessor, driving the SNES CPU (65C816) and
//! audio subsystem (SPC700 + S-DSP) as **loaded wasm coprocessor plugins**.
//!
//! Where the built-in HLE [`slopgb_core::sgb`] path never runs a 65C816 (so
//! `DATA_SND`/`JUMP` are no-ops and only a self-uploaded `SOU_TRN` driver makes
//! sound), this backend loads two real chips —
//! [`slopgb-w65c816-plugin`](../slopgb_w65c816_plugin/index.html) (the SNES CPU)
//! and [`slopgb-spc700-plugin`](../slopgb_spc700_plugin/index.html) (the audio
//! subsystem) — through [`LoadedCoprocessor`], and orchestrates them: it installs
//! the clean-room firmware into each chip's RAM (tier-3 `write_ram`/`set_pc`),
//! mediates the four SNES↔APU comm ports (`$2140-$2143`) between the two loaded
//! plugins each step, routes the SGB sound commands into the CPU's RAM, and mixes
//! the drained S-DSP PCM into the Game Boy stream. The chips themselves run
//! sandboxed in wasm — this crate depends on neither `slopgb-snes-apu` nor
//! `slopgb-w65c816` directly.
//!
//! # Clean-room firmware (original, not the SGB system ROM)
//!
//! The real SGB sound program lives in Nintendo's SGB cartridge SNES ROM, which
//! slopgb does not ship and this code was never allowed to read. In its place
//! this coprocessor installs an **original** two-part firmware, authored purely
//! from the WDC W65C816S datasheet's opcode encodings and nocash *fullsnes* (SNES
//! APU I/O ports `$2140-$2143`, the SPC700 opcode table, the S-DSP register map):
//!
//! - a 65C816 **shim** ([`SNES_SHIM`]) that forwards a SNES-RAM sound mailbox to
//!   the SPC700 comm ports, and
//! - a SPC700 **driver** ([`spc_firmware`]) that waits on a comm port and, on the
//!   trigger, programs the S-DSP to play a synthesized square-wave voice.
//!
//! So a bare SGB `SOUND ($08)` command produces audio with no game-supplied
//! driver. A game that ships its own SPC700 driver via `SOU_TRN` still works (the
//! upload replaces the resident driver, exactly as on real hardware).
//!
//! # Availability + fallback
//!
//! [`SgbCoprocessor::load`] reads the two plugin `.wasm` files from a directory;
//! if they are absent or fail to load it returns an error, and the frontend falls
//! back to the golden-safe built-in `SgbApu`. The `.wasm` ships with no one — a
//! user who wants this backend builds the plugin crates for `wasm32` and points
//! the backend at the directory holding them.
//!
//! See `docs/hardware-state/sgb-audio.md`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::Path;

use slopgb_core::sgb::{AudioCoprocessor, SgbCommandSource};
use slopgb_core::{Reader, SgbFlags, SgbSound, StateError, Writer};
use slopgb_plugin_host::{LoadError, LoadedCoprocessor};

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
/// this HLE bridge (the two CPUs only need forward progress + comm-port bytes).
const CPU_NUM: i64 = 5;
const CPU_DEN: i64 = 8;
/// The S-DSP emits one stereo sample every 32 SPC700 cycles → 32 kHz.
const DSP_RATE: f64 = 32_000.0;
/// Full-scale S-DSP output (±32768) → mix amplitude; half scale, matching the
/// built-in path so an injected coprocessor is no louder than the default.
const MIX_SCALE: f32 = 0.5 / 32768.0;
/// GB T-cycles of emulation accumulated before the two plugins are pumped once
/// (mediated + clocked + drained). Batching keeps the per-frame wasm-crossing
/// count low (a frame is ~17 chunks) while the comm-port handshake still
/// completes in a couple of chunks — never one crossing per emulated cycle.
const FLUSH_CHUNK: u64 = 4096;

/// The plugin `.wasm` filenames [`SgbCoprocessor::load`] looks for in its dir.
pub const SPC_WASM: &str = "spc700.wasm";
pub const CPU_WASM: &str = "w65c816.wasm";

/// GB scanline / frame lengths in T-cycles (the SGB clocks the GB at DMG
/// speed), for the ICD2 `$6000` LCD-row shadow.
const GB_LINE_CYCLES: u64 = 456;
const GB_FRAME_CYCLES: u64 = 70_224;

/// The w65c816 plugin's ICD2 host window (mirrors `slopgb-w65c816-plugin`'s
/// `HOST_WIN`/`HW_*` contract — that crate is wasm-loaded, never linked, the
/// same way `GB_CLOCK_HZ` mirrors `slopgb_core::CLOCK_HZ`).
/// `W len 16`: deposit a packet; `R len 1`: the `$6002` flag.
const HW_PACKET: u32 = 0x0100_0000;
/// `R len 5`: the `$6004-$6007` pad latches + the sticky written flag.
const HW_PADS: u32 = 0x0100_0011;
/// `W len 2`: the `$6000` shadows `[lcd_row, write_row]`.
const HW_LCD_ROW: u32 = 0x0100_0016;

/// Max teed packets held for deposit before the oldest is dropped (matches
/// the core-side tee cap; the guest normally consumes far faster).
const PACKET_QUEUE_CAP: usize = 16;

/// Comm ports (SNES APU I/O has four: `$2140-$2143` / `$F4-$F7`).
const N_PORTS: usize = 4;
/// SNES bank-0 address of comm port 0 (`$2140`). (fullsnes, "SNES APU I/O".)
const PORT_BASE: u16 = 0x2140;
/// Where the 65C816 shim runs from, and its emulation-mode reset vector value.
const SHIM_ORG: u16 = 0x8000;
/// Emulation-mode reset vector location (`$00FFFC-$00FFFD`).
const RESET_VEC: u16 = 0xFFFC;
/// The sound mailbox the shim forwards: `[note, trigger]` in SNES work RAM.
const MB_NOTE: u16 = 0x0200;
/// SPC700 APU-RAM load addresses of the resident driver / directory / sample.
const SPC_PROG_ORG: u16 = 0x0400;
const SPC_DIR_ORG: u16 = 0x0200;
const SPC_BRR_ORG: u16 = 0x0210;

/// The clean-room 65C816 shim (emulation mode, 8-bit). It copies the SNES-RAM
/// mailbox `[$0200,$0201]` to the SPC700 comm ports `$2140/$2141` forever, so a
/// mailbox write reaches the audio CPU. Opcodes are the WDC datasheet encodings
/// (`AD`/`8D` = LDA/STA abs, `4C` = JMP abs):
///
/// ```text
/// $8000  LDA $0200   ; A = mailbox note
/// $8003  STA $2140   ; -> APUIO0 (the SPC700 reads at $F4)
/// $8006  LDA $0201   ; A = mailbox trigger
/// $8009  STA $2141   ; -> APUIO1 (the SPC700 polls at $F5)
/// $800C  JMP $8000   ; loop
/// ```
const SNES_SHIM: [u8; 15] = [
    0xAD,
    MB_NOTE as u8,
    (MB_NOTE >> 8) as u8, // LDA $0200
    0x8D,
    PORT_BASE as u8,
    (PORT_BASE >> 8) as u8, // STA $2140
    0xAD,
    (MB_NOTE + 1) as u8,
    ((MB_NOTE + 1) >> 8) as u8, // LDA $0201
    0x8D,
    (PORT_BASE + 1) as u8,
    ((PORT_BASE + 1) >> 8) as u8, // STA $2141
    0x4C,
    SHIM_ORG as u8,
    (SHIM_ORG >> 8) as u8, // JMP $8000
];

/// The combined coprocessor: a 65C816 plugin + a SPC700 plugin, clocked off the
/// Game Boy stream, their comm ports mediated, the S-DSP PCM mixed into its audio.
///
/// The two [`LoadedCoprocessor`]s are held behind [`RefCell`] so the read-only
/// `AudioCoprocessor::write_state(&self)` can still call the (store-mutating)
/// wasm `save_state` export.
pub struct SgbCoprocessor {
    spc: RefCell<LoadedCoprocessor>,
    cpu: RefCell<LoadedCoprocessor>,
    /// The plugin bytes, kept so [`Self::clone_box`] can re-instantiate.
    spc_wasm: Vec<u8>,
    cpu_wasm: Vec<u8>,

    /// Absolute cycle targets handed to each plugin's `run_until` (its own domain).
    spc_target: u64,
    cpu_target: u64,
    /// Fractional cycle carries for the GB→chip clock ratios.
    spc_acc: i64,
    cpu_acc: i64,
    /// GB T-cycles accumulated since the last plugin pump.
    pending_gb: u64,
    /// Last comm-port bytes mediated into the SPC700 (host-observable).
    to_spc: [u8; N_PORTS],

    /// Undrained 32 kHz S-DSP samples pulled from the SPC plugin (oldest first).
    src: VecDeque<(i16, i16)>,
    /// Fractional 32 kHz→output-rate resample position.
    src_acc: f64,
    /// Latest 32 kHz sample, zero-order-held between source samples.
    cur: (i16, i16),
    /// Output-rate emission accumulator (GB T-cycles) + the cycles-per-sample law.
    samp_acc: f64,
    cycles_per_sample: f64,
    out_rate: u32,
    out: Vec<(f32, f32)>,
    max_out: usize,

    /// Command-poll throttle for the transfer getters (they persist between
    /// transfers, so edge-detect by checksum — same policy as the built-in).
    poll_ctr: u32,
    sou_trn_sig: u64,
    data_trn_sig: u64,
    jump: Option<u32>,

    /// Teed GB packets awaiting deposit into the plugin's ICD2 mailbox
    /// (deposited one per flush, only when the guest cleared `$6002`).
    pending_packets: VecDeque<[u8; 16]>,
    /// Pad latches the SNES program wrote (`$6004-$6007`), re-read from the
    /// plugin each flush once its sticky written flag arms. Transient — the
    /// next flush refreshes it from the (serialized) plugin state.
    feed: Option<[u8; 4]>,
    /// GB frame position (T-cycles, mod one frame) for the `$6000` LCD-row
    /// shadow.
    gb_pos: u64,
}

impl SgbCoprocessor {
    /// Load the two coprocessor plugins from `dir` (`spc700.wasm` + `w65c816.wasm`)
    /// and build the backend at `output_rate` Hz. Errors (missing / bad wasm) are
    /// returned so the frontend can log them and fall back to the built-in
    /// `SgbApu`.
    pub fn load(dir: &Path, output_rate: u32) -> Result<Self, String> {
        let spc_path = dir.join(SPC_WASM);
        let cpu_path = dir.join(CPU_WASM);
        let spc_bytes = fs::read(&spc_path)
            .map_err(|e| format!("cannot read SGB plugin '{}': {e}", spc_path.display()))?;
        let cpu_bytes = fs::read(&cpu_path)
            .map_err(|e| format!("cannot read SGB plugin '{}': {e}", cpu_path.display()))?;
        Self::from_wasm(&spc_bytes, &cpu_bytes, output_rate)
            .map_err(|e| format!("cannot load SGB coprocessor plugins: {e}"))
    }

    /// Build the backend from the two plugins' wasm bytes: instantiate, reset,
    /// install the resident clean-room firmware, and point both chips at their
    /// entry. The bytes are kept for [`Self::clone_box`].
    pub fn from_wasm(
        spc_bytes: &[u8],
        cpu_bytes: &[u8],
        output_rate: u32,
    ) -> Result<Self, LoadError> {
        let mut spc = LoadedCoprocessor::load(spc_bytes)?;
        let mut cpu = LoadedCoprocessor::load(cpu_bytes)?;
        spc.reset()?;
        cpu.reset()?;
        let rate = output_rate.max(1);
        let mut me = SgbCoprocessor {
            spc: RefCell::new(spc),
            cpu: RefCell::new(cpu),
            spc_wasm: spc_bytes.to_vec(),
            cpu_wasm: cpu_bytes.to_vec(),
            spc_target: 0,
            cpu_target: 0,
            spc_acc: 0,
            cpu_acc: 0,
            pending_gb: 0,
            to_spc: [0; N_PORTS],
            src: VecDeque::new(),
            src_acc: 0.0,
            cur: (0, 0),
            samp_acc: 0.0,
            cycles_per_sample: f64::from(GB_CLOCK_HZ) / f64::from(rate),
            out_rate: rate,
            out: Vec::new(),
            max_out: rate as usize,
            poll_ctr: 0,
            sou_trn_sig: 0,
            data_trn_sig: 0,
            jump: None,
            pending_packets: VecDeque::new(),
            feed: None,
            gb_pos: 0,
        };
        me.install_firmware()?;
        Ok(me)
    }

    /// Install the resident clean-room firmware into both chips: the 65C816 shim
    /// into SNES RAM (+ reset vector + entry PC), and the SPC700 driver + one-
    /// entry sample directory + a square BRR sample into APU RAM (+ entry PC). A
    /// failure aborts the load, so `from_wasm` reports it and the caller falls
    /// back to the built-in `SgbApu` rather than running a chip with no firmware.
    fn install_firmware(&mut self) -> Result<(), LoadError> {
        {
            let cpu = self.cpu.get_mut();
            cpu.write_ram(u32::from(SHIM_ORG), &SNES_SHIM)?;
            cpu.write_ram(
                u32::from(RESET_VEC),
                &[SHIM_ORG as u8, (SHIM_ORG >> 8) as u8],
            )?;
            cpu.set_pc(u32::from(SHIM_ORG))?;
        }
        {
            let (prog, dir, brr) = spc_firmware();
            let spc = self.spc.get_mut();
            spc.write_ram(u32::from(SPC_PROG_ORG), &prog)?;
            spc.write_ram(u32::from(SPC_DIR_ORG), &dir)?;
            spc.write_ram(u32::from(SPC_BRR_ORG), &brr)?;
            spc.set_pc(u32::from(SPC_PROG_ORG))?;
        }
        Ok(())
    }

    // -- Clocking -----------------------------------------------------------

    fn clock(&mut self, gb_cycles: u64) {
        self.pending_gb += gb_cycles;
        while self.pending_gb >= FLUSH_CHUNK {
            self.pending_gb -= FLUSH_CHUNK;
            self.flush(FLUSH_CHUNK);
        }
    }

    /// Pump both plugins once for a `span` of GB T-cycles: pump the ICD2
    /// window (LCD-row shadow + packet deposit), mediate the comm ports
    /// (65C816 → SPC700, then SPC700 → 65C816), advance each chip to its cycle
    /// target, pull the ICD2 pad latches back, drain the S-DSP PCM, and emit
    /// `span`'s worth of output samples.
    fn flush(&mut self, span: u64) {
        // ICD2, GB→SNES half: refresh the $6000 LCD-row shadow (fullsnes "SGB
        // Port 6000h": character row 0-$11, $11 = last row or vblank) and
        // deposit the next teed packet once the guest consumed the last one
        // ($6002 clear — never overwrite an unread mailbox).
        self.gb_pos = (self.gb_pos + span) % GB_FRAME_CYCLES;
        let line = self.gb_pos / GB_LINE_CYCLES;
        let row = ((line / 8) as u8).min(0x11);
        {
            let mut cpu = self.cpu.borrow_mut();
            let _ = cpu.write_ram(HW_LCD_ROW, &[row, row & 3]);
            if !self.pending_packets.is_empty() {
                let clear = matches!(cpu.read_ram(HW_PACKET, 1).as_deref(), Ok([0]));
                if clear {
                    if let Some(p) = self.pending_packets.pop_front() {
                        let _ = cpu.write_ram(HW_PACKET, &p);
                    }
                }
            }
        }
        // Advance the chips' absolute cycle targets by the GB→chip ratios.
        self.spc_acc += span as i64 * SPC_NUM;
        let spc_adv = self.spc_acc.div_euclid(SPC_DEN).max(0) as u64;
        self.spc_acc -= spc_adv as i64 * SPC_DEN;
        self.spc_target += spc_adv;
        self.cpu_acc += span as i64 * CPU_NUM;
        let cpu_adv = self.cpu_acc.div_euclid(CPU_DEN).max(0) as u64;
        self.cpu_acc -= cpu_adv as i64 * CPU_DEN;
        self.cpu_target += cpu_adv;

        // 1. Deliver the 65C816's comm-port writes to the SPC700.
        let mut cpu_out = [0u8; N_PORTS];
        {
            let mut cpu = self.cpu.borrow_mut();
            for (p, slot) in cpu_out.iter_mut().enumerate() {
                *slot = cpu.port_read(p as u8).unwrap_or(0);
            }
        }
        self.to_spc = cpu_out;
        {
            let mut spc = self.spc.borrow_mut();
            for (p, &v) in cpu_out.iter().enumerate() {
                let _ = spc.port_write(p as u8, v);
            }
            // 2. Run the SPC700 + S-DSP and pull the synthesized PCM.
            let _ = spc.run_until(self.spc_target);
            if let Ok(batch) = spc.drain_pcm() {
                self.src.extend(batch);
            }
        }
        // 3. Read the SPC700's comm-port replies back for the 65C816.
        let mut spc_out = [0u8; N_PORTS];
        {
            let mut spc = self.spc.borrow_mut();
            for (p, slot) in spc_out.iter_mut().enumerate() {
                *slot = spc.port_read(p as u8).unwrap_or(0);
            }
        }
        {
            let mut cpu = self.cpu.borrow_mut();
            for (p, &v) in spc_out.iter().enumerate() {
                let _ = cpu.port_write(p as u8, v);
            }
            // 4. Run the 65C816 shim.
            let _ = cpu.run_until(self.cpu_target);
            // ICD2, SNES→GB half: pull the pad latches the program wrote.
            // The sticky flag gates the feed — before the SNES side takes
            // over the joypad, the GB's local matrix must stay live.
            if let Ok(v) = cpu.read_ram(HW_PADS, 5) {
                if let [p1, p2, p3, p4, written] = v[..] {
                    if written != 0 {
                        self.feed = Some([p1, p2, p3, p4]);
                    }
                }
            }
        }
        // 5. Emit output-rate samples (32 kHz S-DSP → output rate, zero-order-hold).
        self.emit_output(span);
    }

    /// Emit the output-rate samples owed for a `span` of GB T-cycles, resampling
    /// the 32 kHz S-DSP source by holding the current sample (32 kHz < output).
    fn emit_output(&mut self, span: u64) {
        self.samp_acc += span as f64;
        while self.samp_acc >= self.cycles_per_sample {
            self.samp_acc -= self.cycles_per_sample;
            self.src_acc += DSP_RATE;
            while self.src_acc >= f64::from(self.out_rate) {
                self.src_acc -= f64::from(self.out_rate);
                if let Some(s) = self.src.pop_front() {
                    self.cur = s;
                }
            }
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
        self.out_rate = hz;
        self.cycles_per_sample = f64::from(GB_CLOCK_HZ) / f64::from(hz);
        self.max_out = hz as usize;
        self.samp_acc = 0.0;
        self.src_acc = 0.0;
        self.src.clear();
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
        // Raw packet tee → the ICD2 mailbox deposit queue (bounded like the
        // core-side tee; the flush pump deposits one per guest consume).
        while let Some(p) = cmds.take_packet() {
            while self.pending_packets.len() >= PACKET_QUEUE_CAP {
                self.pending_packets.pop_front();
            }
            self.pending_packets.push_back(p);
        }
        // SOUND ($08): a play request. Deposit the effect id + a trigger in the
        // CPU's mailbox; the 65C816 shim forwards them to the SPC700 driver.
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
        let _ = self
            .cpu
            .get_mut()
            .write_ram(u32::from(MB_NOTE), &[s.effect_a, trig]);
    }

    /// DATA_SND ($0F): copy the packet's data into SNES work RAM at its target
    /// address (bank 0), the write the 65C816 sound program would service.
    fn apply_data_snd(&mut self, pkt: &[u8]) {
        if pkt.len() < 3 {
            return;
        }
        let dest = u16::from(pkt[0]) | (u16::from(pkt[1]) << 8);
        let len = usize::from(pkt[2]);
        let data: Vec<u8> = pkt[3..].iter().take(len).copied().collect();
        let _ = self.cpu.get_mut().write_ram(u32::from(dest), &data);
    }

    /// JUMP ($12): redirect the 65C816 to the SNES program target — no longer a
    /// no-op now that a real SNES CPU is present.
    fn apply_flags(&mut self, flags: SgbFlags) {
        if let Some(target) = flags.jump {
            if self.jump != Some(target) {
                self.jump = Some(target);
                let _ = self.cpu.get_mut().set_pc(target);
            }
        }
    }

    /// Copy a self-describing `(dest, len, data…)` transfer block into APU RAM
    /// (fullsnes: SGB sound transfers begin with a destination/length pair); with
    /// `start`, point the SPC700 at the first load address. Same shape as the
    /// built-in `SgbApu` uploader, so a `SOU_TRN` game driver runs identically.
    fn upload_transfer(&mut self, data: &[u8], start: bool) {
        let spc = self.spc.get_mut();
        let mut off = 0usize;
        let mut entry = None;
        while off + 4 <= data.len() {
            let dest = u16::from_le_bytes([data[off], data[off + 1]]);
            let len = usize::from(u16::from_le_bytes([data[off + 2], data[off + 3]]));
            off += 4;
            if len == 0 || off + len > data.len() {
                break;
            }
            let _ = spc.write_ram(u32::from(dest), &data[off..off + len]);
            entry.get_or_insert(dest);
            off += len;
        }
        if let (true, Some(e)) = (start, entry) {
            let _ = spc.set_pc(u32::from(e));
        }
    }

    // -- Save state ---------------------------------------------------------

    fn write_state(&self, w: &mut Writer) {
        let spc = self.spc.borrow_mut().save_state().unwrap_or_else(|e| {
            eprintln!("slopgb: SGB coprocessor SPC700 save_state failed: {e}");
            Vec::new()
        });
        let cpu = self.cpu.borrow_mut().save_state().unwrap_or_else(|e| {
            eprintln!("slopgb: SGB coprocessor 65C816 save_state failed: {e}");
            Vec::new()
        });
        w.u32(spc.len() as u32);
        w.bytes(&spc);
        w.u32(cpu.len() as u32);
        w.bytes(&cpu);
        w.u64(self.spc_target);
        w.u64(self.cpu_target);
        w.u64(self.spc_acc as u64);
        w.u64(self.cpu_acc as u64);
        w.u64(self.pending_gb);
        for &b in &self.to_spc {
            w.u8(b);
        }
        w.u64(self.src_acc.to_bits());
        w.u16(self.cur.0 as u16);
        w.u16(self.cur.1 as u16);
        w.u64(self.samp_acc.to_bits());
        w.u32(self.poll_ctr);
        w.u64(self.sou_trn_sig);
        w.u64(self.data_trn_sig);
        w.bool(self.jump.is_some());
        w.u32(self.jump.unwrap_or(0));
        w.u8(self.pending_packets.len() as u8);
        for p in &self.pending_packets {
            w.bytes(p);
        }
        w.u64(self.gb_pos);
    }

    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        let n = r.u32()? as usize;
        let spc = r.bytes_vec(n)?;
        let n = r.u32()? as usize;
        let cpu = r.bytes_vec(n)?;
        if let Err(e) = self.spc.get_mut().load_state(&spc) {
            eprintln!("slopgb: SGB coprocessor SPC700 load_state failed: {e}");
        }
        if let Err(e) = self.cpu.get_mut().load_state(&cpu) {
            eprintln!("slopgb: SGB coprocessor 65C816 load_state failed: {e}");
        }
        self.spc_target = r.u64()?;
        self.cpu_target = r.u64()?;
        self.spc_acc = r.u64()? as i64;
        self.cpu_acc = r.u64()? as i64;
        self.pending_gb = r.u64()?;
        for slot in &mut self.to_spc {
            *slot = r.u8()?;
        }
        self.src_acc = f64::from_bits(r.u64()?);
        self.cur.0 = r.u16()? as i16;
        self.cur.1 = r.u16()? as i16;
        self.samp_acc = f64::from_bits(r.u64()?);
        self.poll_ctr = r.u32()?;
        self.sou_trn_sig = r.u64()?;
        self.data_trn_sig = r.u64()?;
        let has_jump = r.bool()?;
        let j = r.u32()?;
        self.jump = has_jump.then_some(j);
        let n = usize::from(r.u8()?);
        if n > PACKET_QUEUE_CAP {
            return Err(StateError::Truncated);
        }
        self.pending_packets.clear();
        for _ in 0..n {
            let mut p = [0u8; 16];
            r.bytes_into(&mut p)?;
            self.pending_packets.push_back(p);
        }
        self.gb_pos = r.u64()? % GB_FRAME_CYCLES;
        // The undrained source/output PCM is transient, not part of the
        // snapshot — and so is the pad feed (re-read from the restored plugin
        // state on the next flush).
        self.src.clear();
        self.out.clear();
        self.feed = None;
        Ok(())
    }

    /// Deep-clone: re-instantiate the two plugins from the kept wasm bytes, load
    /// this coprocessor's current chip state into them, and copy the host-side
    /// runtime. Used by [`AudioCoprocessor::clone_box`] for the save-state restore
    /// that clones the whole `GameBoy`.
    fn deep_clone(&self) -> Result<Self, LoadError> {
        let spc_state = self.spc.borrow_mut().save_state().unwrap_or_else(|e| {
            eprintln!("slopgb: SGB coprocessor SPC700 save_state failed on clone: {e}");
            Vec::new()
        });
        let cpu_state = self.cpu.borrow_mut().save_state().unwrap_or_else(|e| {
            eprintln!("slopgb: SGB coprocessor 65C816 save_state failed on clone: {e}");
            Vec::new()
        });
        let mut fresh = Self::from_wasm(&self.spc_wasm, &self.cpu_wasm, self.out_rate)?;
        if let Err(e) = fresh.spc.get_mut().load_state(&spc_state) {
            eprintln!("slopgb: SGB coprocessor SPC700 load_state failed on clone: {e}");
        }
        if let Err(e) = fresh.cpu.get_mut().load_state(&cpu_state) {
            eprintln!("slopgb: SGB coprocessor 65C816 load_state failed on clone: {e}");
        }
        fresh.spc_target = self.spc_target;
        fresh.cpu_target = self.cpu_target;
        fresh.spc_acc = self.spc_acc;
        fresh.cpu_acc = self.cpu_acc;
        fresh.pending_gb = self.pending_gb;
        fresh.to_spc = self.to_spc;
        fresh.src = self.src.clone();
        fresh.src_acc = self.src_acc;
        fresh.cur = self.cur;
        fresh.samp_acc = self.samp_acc;
        fresh.out = self.out.clone();
        fresh.poll_ctr = self.poll_ctr;
        fresh.sou_trn_sig = self.sou_trn_sig;
        fresh.data_trn_sig = self.data_trn_sig;
        fresh.jump = self.jump;
        fresh.pending_packets = self.pending_packets.clone();
        fresh.feed = self.feed;
        fresh.gb_pos = self.gb_pos;
        Ok(fresh)
    }
}

impl AudioCoprocessor for SgbCoprocessor {
    fn clock(&mut self, gb_cycles: u64) {
        SgbCoprocessor::clock(self, gb_cycles);
    }
    fn poll(&mut self, cmds: &mut dyn SgbCommandSource) {
        SgbCoprocessor::poll(self, cmds);
    }
    fn joypad_feed(&mut self) -> Option<[u8; 4]> {
        self.feed
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
        // Re-instantiating the already-validated plugin wasm can only fail on an
        // allocation error (which aborts anyway), so this is near-unreachable —
        // but a save-state clone must never panic the emulator. Degrade to a
        // silent inert coprocessor and log, rather than `.expect`.
        match self.deep_clone() {
            Ok(fresh) => Box::new(fresh),
            Err(e) => {
                eprintln!(
                    "slopgb: SGB coprocessor clone failed ({e}); audio inert for this snapshot"
                );
                Box::new(InertCoprocessor)
            }
        }
    }

    fn debug_status(&self) -> String {
        // The run-cycle targets grow only while the host clocks the chips, so a
        // zero here means the coprocessor loaded but was never driven (the
        // machine isn't in SGB mode, or the GB is sending nothing) — the exact
        // "SNES side isn't running" case. Non-zero = the chips are executing.
        let running = self.cpu_target > 0 || self.spc_target > 0;
        format!(
            "wasm SGB coprocessor: SPC700 + 65C816 plugins loaded; {} \
             (65C816 ran to cyc {}, SPC700 to cyc {}); last GB->SPC ports {:02X?}",
            if running {
                "RUNNING"
            } else {
                "NOT yet clocked"
            },
            self.cpu_target,
            self.spc_target,
            self.to_spc,
        )
    }
}

/// The on-disk state layout [`SgbCoprocessor::write_state`] emits, all zeroed
/// (empty chip-state blocks + zeroed runtime). [`InertCoprocessor`] writes this
/// so a machine saved with an inert coprocessor still reloads through
/// [`SgbCoprocessor::read_state`]. **Keep in sync with `write_state`.**
fn write_empty_state(w: &mut Writer) {
    w.u32(0); // spc state len
    w.u32(0); // cpu state len
    for _ in 0..5 {
        w.u64(0); // spc/cpu target, spc/cpu acc, pending_gb
    }
    for _ in 0..N_PORTS {
        w.u8(0); // to_spc
    }
    w.u64(0); // src_acc
    w.u16(0); // cur.0
    w.u16(0); // cur.1
    w.u64(0); // samp_acc
    w.u32(0); // poll_ctr
    w.u64(0); // sou_trn_sig
    w.u64(0); // data_trn_sig
    w.bool(false); // jump present
    w.u32(0); // jump target
    w.u8(0); // pending ICD2 packets
    w.u64(0); // gb_pos
}

/// A no-op [`AudioCoprocessor`] producing silence. Only ever the result of
/// [`SgbCoprocessor::clone_box`] failing to re-instantiate the plugin wasm
/// (near-impossible — the bytes loaded once already), so a save-state can never
/// panic the emulator.
struct InertCoprocessor;

impl AudioCoprocessor for InertCoprocessor {
    fn clock(&mut self, _gb_cycles: u64) {}
    fn poll(&mut self, _cmds: &mut dyn SgbCommandSource) {}
    fn mix_into(&mut self, _out: &mut [(f32, f32)]) {}
    fn set_output_rate(&mut self, _hz: u32) {}
    fn load_bios(&mut self, _bios: &[u8]) {}
    fn write_state(&self, w: &mut Writer) {
        write_empty_state(w);
    }
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        // Consume (and discard) the same layout, so an inert-then-restored path
        // stays byte-aligned.
        let n = r.u32()? as usize;
        r.bytes_vec(n)?;
        let n = r.u32()? as usize;
        r.bytes_vec(n)?;
        for _ in 0..5 {
            r.u64()?;
        }
        for _ in 0..N_PORTS {
            r.u8()?;
        }
        r.u64()?;
        r.u16()?;
        r.u16()?;
        r.u64()?;
        r.u32()?;
        r.u64()?;
        r.u64()?;
        r.bool()?;
        r.u32()?;
        let n = usize::from(r.u8()?);
        for _ in 0..n {
            let mut p = [0u8; 16];
            r.bytes_into(&mut p)?;
        }
        r.u64()?;
        Ok(())
    }
    fn clone_box(&self) -> Box<dyn AudioCoprocessor> {
        Box::new(InertCoprocessor)
    }
}

/// Install the clean-room SPC700 driver + one-entry sample directory + a square
/// BRR sample into APU RAM. The original clean-room driver waits on comm port 1
/// (the SNES trigger), then programs the S-DSP to key a ~2 kHz square-wave voice.
/// Authored from the SPC700 opcode table + S-DSP register map (nocash *fullsnes*),
/// never from a ROM. Returns `(program@$0400, directory@$0200, sample@$0210)`.
fn spc_firmware() -> (Vec<u8>, [u8; 4], Vec<u8>) {
    // `MOV dp,#imm` = `8F imm dp`; `MOV A,dp` = `E4 dp`; `CLRP` = `20`;
    // `BEQ rel` = `F0 rel`; `BRA rel` = `2F rel` (fullsnes opcode table).
    let mov = |dp: u8, imm: u8| [0x8F, imm, dp];
    let mut prog = Vec::new();
    prog.push(0x20); // CLRP: direct page = $00xx, so $F5 is the comm port
    // wait: MOV A,$F5 / BEQ wait — spin until the SNES sets the trigger port.
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
    let dir = [
        SPC_BRR_ORG as u8,
        (SPC_BRR_ORG >> 8) as u8,
        SPC_BRR_ORG as u8,
        (SPC_BRR_ORG >> 8) as u8,
    ];
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
