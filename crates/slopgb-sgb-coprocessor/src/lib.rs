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

mod commands;
mod dma;
mod state;

use state::InertCoprocessor;

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
/// The optional SNES-PPU plugin: absent = the audio-only backend, unchanged.
pub const PPU_WASM: &str = "snes-ppu.wasm";

/// The snes-ppu plugin's host window (mirrored, wasm-loaded never linked):
/// `W len 2` at `PPU_HW_LINE`: render framebuffer row y; reads at
/// `PPU_HW_FB` fetch the 256x224 RGB555 LE framebuffer.
const PPU_HW_LINE: u32 = 0x0100_0000;
const PPU_HW_FB: u32 = 0x0100_1000;
/// The plugin's fixed frame geometry.
pub const SNES_FB_W: usize = 256;
pub const SNES_FB_H: usize = 224;

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
/// `R len 3 + 3*512` (drains): the MMIO write-capture ring.
const HW_MMIO_RING: u32 = 0x0100_1000;
/// `W len L` at `+ i`: CPU-read shadows for `$4200 + i`.
const HW_SHADOW: u32 = 0x0100_2000;
/// `W len 1`: request an NMI (consumed at the next instruction boundary).
const HW_NMI: u32 = 0x0100_3000;
/// `W len 1` zero: DMA service complete, un-stall the CPU. (`R len 1`: the
/// stall flag a guest `$420B` write armed.)
const HW_DMA_STALL: u32 = 0x0100_3001;
/// The plugin's ring capacity (drain reads size to this).
const MMIO_RING_CAP: usize = 512;
/// Shadow offsets within the `$4200` block.
const SH_RDNMI: u32 = 0x10;
const SH_HVBJOY: u32 = 0x12;
const SH_JOY1: u32 = 0x18;

/// SNES NTSC frame: 262 lines, vblank beginning at V=225 in the 224-line
/// mode (fullsnes "SNES PPU Resolution" / "4212h"). The GB frame position is
/// scaled onto it — both machines run ~60 Hz (the two oscillators are not
/// locked on real hardware either).
const SNES_LINES: u64 = 262;
const SNES_VBLANK_LINE: u64 = 225;

/// Max teed packets held for deposit before the oldest is dropped (matches
/// the core-side tee cap; the guest normally consumes far faster).
const PACKET_QUEUE_CAP: usize = 16;

/// The resident BIOS-runtime WRAM contract arcade-takeover programs hook —
/// pinned black-box from the pilot's own uploaded routines (see
/// docs/hardware-state/sgb-arcade-takeover.md), never from BIOS code.
/// The current packet's 16 bytes (`$7E:0600`, via the bank-0 mirror).
const BIOS_PKT_BUF: u32 = 0x0600;
/// The last received command number (`$7E:02C2`).
const BIOS_LAST_CMD: u32 = 0x02C2;
/// 16-bit pointer to the staged DATA_TRN payload, bank `$7E` implied
/// (`$7E:0284/85`).
const BIOS_TRN_PTR: u32 = 0x0284;
/// Where the host stages DATA_TRN payloads (high WRAM, clear of the low
/// pages the uploaded stubs use; reached through the pointer above).
const BIOS_TRN_STAGING: u32 = 0x7E_D000;
/// BIOS service entries uploaded bootstraps `JSR` into (two per known BIOS
/// revision, selected on the `$00:FFDB` byte — zero in slopgb, so the first
/// of each pair is the live one). The entries sit 3 bytes apart, so each
/// holds a `JMP` thunk into a resident body below.
const BIOS_MAIN_ENTRIES: [u32; 2] = [0xBBED, 0xBBF0];
const BIOS_AUX_ENTRIES: [u32; 2] = [0xC58D, 0xC590];
/// The resident main-service body: call the game's `$0800` hook slot when
/// one is installed (`LDA $0800 / BEQ +3 / JSR $0800 / RTS` — the two-JSR
/// depth is pinned by the pilot's own `PLA PLA / RTS` stack fixup), else
/// return. The host performs the receive/transfer duties asynchronously.
const BIOS_MAIN_BODY: u32 = 0xBE00;
/// The resident aux-service body: wait for the next vblank (poll RDNMI
/// bit 7 — the flag sets regardless of NMITIMEN, fullsnes 4210h), then
/// return. Pinned black-box from the pilot's hook, which wraps the aux
/// call in its ICD2 ACK latch writes ($01 before, $00 after) on every
/// DATA_TRN delivery: the wait holds the $01 latch across a vblank so the
/// GB polling D751 observes both handshake values. It must NOT touch
/// NMITIMEN — the pilot's JUMP sets the $00BB NMI vector to its bootstrap
/// entry, so a delivered NMI would re-enter the main loop recursively
/// (probed: the stack then eats the direct page). Sits past the RTI stub
/// (the body outgrew the $BE20-$BE2F slot).
const BIOS_AUX_BODY: u32 = 0xBE60;
/// The game's hook slot the main service calls (zero until installed).
const BIOS_HOOK_SLOT: u32 = 0x0800;
/// The JUMP trampoline: `CLC / XCE / JML target` — the BIOS hands control
/// to a JUMP target in native mode (pinned by the pilot's dispatcher using
/// `REP #$30` + 16-bit index ops, impossible in emulation mode).
const JUMP_TRAMP: u32 = 0xBF00;
/// The resident NMI handler. fullsnes ("SGB Commands" notes): the BIOS's NMI
/// path dispatches through a **RAM vector at `$00:00BB-$00BD`** — "only NMIs
/// can be hooked", and JUMP is documented to clobber exactly those bytes.
/// The handler jumps through that vector when a program installed one, else
/// returns. A and P are saved at the interrupted width and restored on both
/// exits (the hook receives the original A), and the guard reads the vector
/// with long addressing so it is independent of the interrupted D and DBR:
/// `PHA / PHP / SEP #$20 / LDA $0000BB / ORA $0000BC / ORA $0000BD /
/// BEQ +5 / PLP / PLA / JML [$00BB] / PLP / PLA / RTI`.
const NMI_HANDLER: u32 = 0xBE30;
/// The RAM NMI vector the handler dispatches through.
const NMI_RAM_VEC: u8 = 0xBB;
/// A resident RTI, targeted by the BRK/COP/IRQ vectors: a stray break in an
/// uploaded program resumes instead of cascading through zeroed memory
/// (those vectors live in the unshipped BIOS ROM; fullsnes notes several
/// real BIOS entries are plain returns).
const RTI_STUB: u32 = 0xBE50;

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
    /// Teed DATA_TRN packets whose 4 KB payloads have not landed yet,
    /// oldest first (Pan Docs "SGB Command $10": dest lo, hi, bank in
    /// bytes 1-3; the payload rides the next frame's screen capture). Each
    /// payload pairs with the oldest pending packet, and the BIOS-runtime
    /// variables ($0600 packet buffer, $02C2 command) update only then — a
    /// dispatcher keying on $02C2 == $10 must never run against an empty
    /// staging buffer, and a payload must never land at a newer packet's
    /// dest (the mis-pair that overwrote the pilot's program area).
    pending_trn: VecDeque<[u8; 16]>,
    /// The guest's last NMITIMEN ($4200) write, from the MMIO capture ring
    /// (bit 7 = vblank NMI enable, bit 0 = joypad autopoll).
    nmitimen: u8,
    /// Whether the scaled SNES V counter sat in vblank at the last flush
    /// (edge detector for the RDNMI flag + NMI delivery).
    in_vblank: bool,
    /// GP-DMA channel working registers `$43x0-$43x6` (DMAP, BBAD, A1TL/H,
    /// A1B, DASL/H per channel), filled from the capture ring and stepped by
    /// transfers (see `dma.rs`).
    dma_regs: [[u8; 7]; 8],
    /// The 17-bit `$2181-$2183` WRAM access address behind the `$2180`
    /// WMDATA port (auto-increments per access — fullsnes 2180h).
    wmadd: u32,
    /// The GB-side physical matrix (active-low `dpad`/`buttons` nibbles)
    /// pushed by `AudioCoprocessor::set_input`. Transient — the core
    /// re-pushes it every step.
    input: (u8, u8),
    /// The autopoll window is open: HVBJOY bit 0 set at the vblank edge,
    /// cleared (and the JOY1 shadows published) one flush later — hardware
    /// takes ~4224 master cycles and reads mid-window are unreliable
    /// (fullsnes 4212h / "AUTO JOYPAD READ").
    joy_busy: bool,
    /// The optional SNES-PPU plugin (+ its bytes for [`Self::deep_clone`]).
    /// `None` = the audio-only backend, byte-for-byte unchanged.
    ppu: Option<RefCell<LoadedCoprocessor>>,
    ppu_wasm: Option<Vec<u8>>,
    /// The next framebuffer row the scanline pump renders (0-224).
    ppu_row: u16,
    /// A completed frame awaits [`Self::take_snes_frame`].
    frame_ready: bool,
    /// Diagnostics only (`debug_status`), transient across save states:
    /// completed-frame count + the guest's last INIDISP write.
    frames_done: u64,
    last_inidisp: u8,
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
        // The PPU plugin is optional: absent keeps the audio-only backend.
        let ppu_bytes = fs::read(dir.join(PPU_WASM)).ok();
        Self::from_wasm_full(&spc_bytes, &cpu_bytes, ppu_bytes.as_deref(), output_rate)
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
        Self::from_wasm_full(spc_bytes, cpu_bytes, None, output_rate)
    }

    /// [`Self::from_wasm`] plus the optional SNES-PPU plugin.
    pub fn from_wasm_full(
        spc_bytes: &[u8],
        cpu_bytes: &[u8],
        ppu_bytes: Option<&[u8]>,
        output_rate: u32,
    ) -> Result<Self, LoadError> {
        let mut spc = LoadedCoprocessor::load(spc_bytes)?;
        let mut cpu = LoadedCoprocessor::load(cpu_bytes)?;
        spc.reset()?;
        cpu.reset()?;
        let ppu = match ppu_bytes {
            Some(b) => {
                let mut p = LoadedCoprocessor::load(b)?;
                p.reset()?;
                Some(RefCell::new(p))
            }
            None => None,
        };
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
            pending_trn: VecDeque::new(),
            nmitimen: 0,
            in_vblank: false,
            dma_regs: [[0; 7]; 8],
            wmadd: 0,
            input: (0x0F, 0x0F),
            joy_busy: false,
            ppu,
            ppu_wasm: ppu_bytes.map(<[u8]>::to_vec),
            ppu_row: 0,
            frame_ready: false,
            frames_done: 0,
            last_inidisp: 0,
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
            // Model the entire unshipped BIOS ROM as inert returns: an RTS
            // sled across the whole program area, so an uploaded program
            // JSR-ing any service entry slopgb has not (yet) pinned returns
            // harmlessly instead of executing zeroes. Specific resident
            // routines overwrite their spots below.
            cpu.write_ram(0x8000, &[0x60u8; 0x8000])?;
            // Keep the documented revision byte: $FFDB = 0 selects the first
            // entry of each BIOS service pair (sgb-arcade-takeover.md).
            cpu.write_ram(0xFFDB, &[0x00])?;
            cpu.write_ram(u32::from(SHIM_ORG), &SNES_SHIM)?;
            cpu.write_ram(
                u32::from(RESET_VEC),
                &[SHIM_ORG as u8, (SHIM_ORG >> 8) as u8],
            )?;
            cpu.set_pc(u32::from(SHIM_ORG))?;
            // Resident BIOS service entries (JMP thunks; the entries sit 3
            // bytes apart, too tight for inline bodies). Opcodes per the WDC
            // datasheet: 4C = JMP abs, AD = LDA abs, F0 = BEQ, 20 = JSR,
            // 60 = RTS.
            for entry in BIOS_MAIN_ENTRIES {
                cpu.write_ram(
                    entry,
                    &[0x4C, BIOS_MAIN_BODY as u8, (BIOS_MAIN_BODY >> 8) as u8],
                )?;
            }
            for entry in BIOS_AUX_ENTRIES {
                cpu.write_ram(
                    entry,
                    &[0x4C, BIOS_AUX_BODY as u8, (BIOS_AUX_BODY >> 8) as u8],
                )?;
            }
            // Main body: guarded call into the game's hook slot.
            let hook_lo = BIOS_HOOK_SLOT as u8;
            let hook_hi = (BIOS_HOOK_SLOT >> 8) as u8;
            cpu.write_ram(
                BIOS_MAIN_BODY,
                &[
                    0xAD, hook_lo, hook_hi, // LDA $0800 (0 = no hook installed)
                    0xF0, 0x03, // BEQ +3 -> the RTS
                    0x20, hook_lo, hook_hi, // JSR $0800
                    0x60,    // RTS
                ],
            )?;
            // Aux body (see BIOS_AUX_BODY): PHP / SEP #$20 /
            // wait: LDA $4210 / BPL wait / PLP / RTS — the $4210 reads ride
            // the host-fed RDNMI shadow (set at every vblank edge,
            // read-clear guest-side), so the wait spans to the next edge.
            cpu.write_ram(
                BIOS_AUX_BODY,
                &[
                    0x08, // PHP (caller's register widths preserved)
                    0xE2, 0x20, // SEP #$20
                    0xAD, 0x10, 0x42, // wait: LDA $4210 (RDNMI)
                    0x10, 0xFB, // BPL wait
                    0x28, // PLP
                    0x60, // RTS
                ],
            )?;
            // The resident NMI handler + both CPU-mode NMI vectors.
            cpu.write_ram(
                NMI_HANDLER,
                &[
                    0x48, // PHA (interrupted width)
                    0x08, // PHP
                    0xE2,
                    0x20, // SEP #$20 (8-bit A for the guard)
                    0xAF,
                    NMI_RAM_VEC,
                    0x00,
                    0x00, // LDA $0000BB (long)
                    0x0F,
                    NMI_RAM_VEC + 1,
                    0x00,
                    0x00, // ORA $0000BC
                    0x0F,
                    NMI_RAM_VEC + 2,
                    0x00,
                    0x00, // ORA $0000BD
                    0xF0,
                    0x05, // BEQ +5 -> the empty-vector PLP/PLA/RTI
                    0x28, // PLP (width back to the interrupted M)
                    0x68, // PLA (original A restored for the hook)
                    0xDC,
                    NMI_RAM_VEC,
                    0x00, // JML [$00BB]
                    0x28, // PLP
                    0x68, // PLA
                    0x40, // RTI
                ],
            )?;
            let nmi_vec = [NMI_HANDLER as u8, (NMI_HANDLER >> 8) as u8];
            cpu.write_ram(0xFFEA, &nmi_vec)?; // native NMI vector
            cpu.write_ram(0xFFFA, &nmi_vec)?; // emulation NMI vector
            // Break/interrupt vectors -> the resident RTI (see RTI_STUB).
            cpu.write_ram(RTI_STUB, &[0x40])?;
            let rti = [RTI_STUB as u8, (RTI_STUB >> 8) as u8];
            cpu.write_ram(0xFFE4, &rti)?; // native COP
            cpu.write_ram(0xFFE6, &rti)?; // native BRK
            cpu.write_ram(0xFFEE, &rti)?; // native IRQ
            cpu.write_ram(0xFFF4, &rti)?; // emulation COP
            cpu.write_ram(0xFFFE, &rti)?; // emulation IRQ/BRK
            // RDNMI reads the CPU version bits from power-on (fullsnes 4210h).
            cpu.write_ram(HW_SHADOW + SH_RDNMI, &[0x02])?;
        }
        {
            let (prog, dir, brr) = spc_firmware();
            let spc = self.spc.get_mut();
            spc.write_ram(u32::from(SPC_PROG_ORG), &prog)?;
            spc.write_ram(u32::from(SPC_DIR_ORG), &dir)?;
            spc.write_ram(u32::from(SPC_BRR_ORG), &brr)?;
            // No set_pc: the SPC700 boots its own IPL ROM (the chip ships
            // the documented 64-byte boot loader at $FFC0), announcing
            // $AA/$BB and serving the standard upload protocol — the
            // arcade pilot uploads its sound driver through it. The square
            // driver above is entered host-side on a SOUND command instead
            // (see apply_sound).
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
        // Drain the guest's captured MMIO writes from the last slice and
        // apply the ones the clocking loop consumes (NMITIMEN for now; the
        // PPU/DMA routing grows here).
        let captured: Vec<(u16, u8)> = {
            let mut cpu = self.cpu.borrow_mut();
            match cpu.read_ram(HW_MMIO_RING, 3 + 3 * MMIO_RING_CAP) {
                Ok(buf) if buf.len() >= 3 => {
                    let n = usize::from(buf[0]) | usize::from(buf[1]) << 8;
                    if buf[2] != 0 {
                        eprintln!("slopgb: SNES MMIO capture ring overflowed; writes dropped");
                    }
                    buf[3..]
                        .chunks_exact(3)
                        .take(n)
                        .map(|e| (u16::from(e[0]) | u16::from(e[1]) << 8, e[2]))
                        .collect()
                }
                _ => Vec::new(),
            }
        };
        for (addr, val) in captured {
            self.apply_mmio(addr, val);
        }
        // The ring is applied — any $420B in it just ran its transfer — so
        // release a DMA-stalled CPU before this flush's run_until.
        let _ = self.cpu.get_mut().write_ram(HW_DMA_STALL, &[0]);
        // The scanline pump: render every framebuffer row the SNES beam has
        // passed since the last flush (display lines are 1-based, so row r
        // is complete once V > r). Runs after the ring apply, so this
        // flush's register writes land at ~10-line granularity.
        if let Some(ppu) = &self.ppu {
            let v = self.gb_pos * SNES_LINES / GB_FRAME_CYCLES;
            let target = v.min(SNES_FB_H as u64) as u16;
            let mut ppu = ppu.borrow_mut();
            while self.ppu_row < target {
                let _ = ppu.write_ram(PPU_HW_LINE, &self.ppu_row.to_le_bytes());
                self.ppu_row += 1;
            }
        }
        {
            let mut cpu = self.cpu.borrow_mut();
            let _ = cpu.write_ram(HW_LCD_ROW, &[row, row & 3]);
            // The SNES frame clock: scale the GB frame position onto the
            // 262-line NTSC frame; on the vblank edges maintain the RDNMI
            // flag (set at begin, auto-clear at end — fullsnes 4210h; the
            // read-acknowledge runs guest-side) and deliver the NMI when
            // NMITIMEN bit 7 enables it (fullsnes 4200h).
            let v = self.gb_pos * SNES_LINES / GB_FRAME_CYCLES;
            let vblank = v >= SNES_VBLANK_LINE;
            if vblank != self.in_vblank {
                self.in_vblank = vblank;
                if vblank {
                    let _ = cpu.write_ram(HW_SHADOW + SH_RDNMI, &[0x82]);
                    if self.nmitimen & 0x80 != 0 {
                        let _ = cpu.write_ram(HW_NMI, &[1]);
                    }
                    // Joypad autopoll begins on the first vblank line when
                    // NMITIMEN bit 0 asks for it (fullsnes 4200h).
                    self.joy_busy = self.nmitimen & 1 != 0;
                    // The scanline pump completed the frame before this
                    // edge (V >= 225 implies all 224 rows rendered).
                    self.frame_ready = self.ppu.is_some();
                    self.frames_done += u64::from(self.frame_ready);
                } else {
                    let _ = cpu.write_ram(HW_SHADOW + SH_RDNMI, &[0x02]);
                    // A new frame begins as vblank ends.
                    self.ppu_row = 0;
                }
                // HVBJOY bit 7 tracks vblank, bit 0 the autopoll window
                // (bit 6 hblank is below this pump's resolution).
                let hvbjoy = (vblank as u8) << 7 | u8::from(self.joy_busy);
                let _ = cpu.write_ram(HW_SHADOW + SH_HVBJOY, &[hvbjoy]);
            } else if self.joy_busy {
                // The poll window ends (~4224 master cycles ≈ under one
                // flush): the JOY1 shadows become valid exactly when the
                // busy bit drops — mid-window reads are unreliable on
                // hardware, so nothing is published earlier.
                self.joy_busy = false;
                let (dpad, buttons) = self.input;
                let _ = cpu.write_ram(HW_SHADOW + SH_JOY1, &joy1_bytes(dpad, buttons));
                let _ = cpu.write_ram(HW_SHADOW + SH_HVBJOY, &[(vblank as u8) << 7]);
            }
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

    /// Read `len` bytes of the 65C816 plugin's memory at the 24-bit `addr` —
    /// read-only introspection for the debugger/MCP (a `peek` into the SNES
    /// side; never advances a cycle).
    pub fn debug_cpu_ram(&self, addr: u32, len: usize) -> Vec<u8> {
        self.cpu
            .borrow_mut()
            .read_ram(addr, len)
            .unwrap_or_default()
    }

    /// The PPU plugin's raw state snapshot (the `slopgb-snes-ppu` image:
    /// VRAM, CGRAM, OAM, registers, framebuffer) — read-only introspection
    /// for the debugger/MCP; empty without a PPU plugin.
    pub fn debug_ppu_state(&self) -> Vec<u8> {
        self.ppu
            .as_ref()
            .and_then(|p| p.borrow_mut().save_state().ok())
            .unwrap_or_default()
    }

    /// Apply one captured MMIO write from the guest (also the target of DMA
    /// B-bus writes — `dma::bbus_write` routes through here). The clocking
    /// loop consumes NMITIMEN; the DMA engine consumes the channel
    /// registers, MDMAEN, and the WRAM access ports; everything else is
    /// inert until its consumer lands (PPU routing).
    fn apply_mmio(&mut self, addr: u16, val: u8) {
        match addr {
            0x2180 => self.wmdata_write(val),
            0x2181 => self.wmadd = self.wmadd & 0x1_FF00 | u32::from(val),
            0x2182 => self.wmadd = self.wmadd & 0x1_00FF | u32::from(val) << 8,
            // WMADDH: one bit — WMADD addresses 128 KB (fullsnes 2183h).
            0x2183 => self.wmadd = self.wmadd & 0xFFFF | u32::from(val & 1) << 16,
            0x4200 => self.nmitimen = val,
            0x420B => self.run_gp_dma(val),
            0x4300..=0x437F if usize::from(addr & 0xF) < 7 => {
                self.dma_regs[usize::from(addr >> 4 & 7)][usize::from(addr & 0xF)] = val;
            }
            // Every other B-bus port belongs to the PPU when one is loaded
            // (unknown ports are inert inside the chip). $2140-$2143 only
            // arrive via DMA (the CPU-side APU ports route earlier) — a
            // DMA-to-APU transfer is unimplemented and lands inert too.
            0x2100..=0x21FF => {
                if addr == 0x2100 {
                    self.last_inidisp = val; // diagnostics (debug_status)
                }
                if let Some(ppu) = &self.ppu {
                    let _ = ppu.borrow_mut().port_write((addr - 0x2100) as u8, val);
                }
            }
            _ => {}
        }
    }

    /// Fetch the last completed SNES frame (256x224 RGB555 words,
    /// row-major), at most once per vblank. `None` without a PPU plugin or
    /// until the next frame completes.
    pub fn take_snes_frame(&mut self) -> Option<Vec<u16>> {
        if !self.frame_ready {
            return None;
        }
        self.frame_ready = false;
        let ppu = self.ppu.as_ref()?;
        let bytes = ppu
            .borrow_mut()
            .read_ram(PPU_HW_FB, SNES_FB_W * SNES_FB_H * 2)
            .ok()?;
        Some(
            bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect(),
        )
    }

    // Save state, deep clone, and the inert fallback live in `state.rs`.
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
    fn set_input(&mut self, dpad: u8, buttons: u8) {
        self.input = (dpad, buttons);
    }
    fn take_frame(&mut self) -> Option<Vec<u16>> {
        self.take_snes_frame()
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
        let ppu = match &self.ppu {
            Some(_) => format!(
                "SNES PPU plugin loaded: {} frames rendered, last INIDISP ${:02X}",
                self.frames_done, self.last_inidisp
            ),
            None => "no SNES PPU plugin (audio-only)".into(),
        };
        format!(
            "wasm SGB coprocessor: SPC700 + 65C816 plugins loaded; {} \
             (65C816 ran to cyc {}, SPC700 to cyc {}); last GB->SPC ports {:02X?}; {}",
            if running {
                "RUNNING"
            } else {
                "NOT yet clocked"
            },
            self.cpu_target,
            self.spc_target,
            self.to_spc,
            ppu,
        )
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

/// Map the GB active-low matrix nibbles onto the SNES JOY1 layout, `[low,
/// high]` for `$4218/$4219` (fullsnes 4218h: bit 15 B, 13 Select, 12 Start,
/// 11-8 Up/Down/Left/Right, 7 A; 1 = pressed). GB A/B/Select/Start and the
/// d-pad map to their SNES namesakes; Y/X/L/R and the low id nibble read 0.
fn joy1_bytes(dpad: u8, buttons: u8) -> [u8; 2] {
    let d = !dpad & 0x0F;
    let b = !buttons & 0x0F;
    let high = (b >> 1 & 1) << 7 // B
        | (b >> 2 & 1) << 5 // Select
        | (b >> 3 & 1) << 4 // Start
        | (d >> 2 & 1) << 3 // Up
        | (d >> 3 & 1) << 2 // Down
        | (d >> 1 & 1) << 1 // Left
        | (d & 1); // Right
    let low = (b & 1) << 7; // A
    [low, high]
}
