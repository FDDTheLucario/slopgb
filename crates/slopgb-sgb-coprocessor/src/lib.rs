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

mod clocking;
mod commands;
mod dma;
mod firmware;
mod perf;
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
/// `W len 2` at `PPU_HW_LINE`: render framebuffer row y (`len 3` renders a
/// `[y_lo, y_hi, count]` span in one call); reads at `PPU_HW_FB` fetch the
/// 256x224 RGB555 LE framebuffer; `W len 2N` at `PPU_HW_PORTS` applies a
/// `(port, val)` run in order — one wasm crossing per run, not per byte.
const PPU_HW_LINE: u32 = 0x0100_0000;
const PPU_HW_FB: u32 = 0x0100_1000;
const PPU_HW_PORTS: u32 = 0x0100_2000;
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
/// `W len 320` at `+ (row % 4) * 320`: load an ICD2 `$7800` character row.
const HW_CHAR_ROWS: u32 = 0x0100_0020;
/// `R len N` at `+ off`: N successive ICD2 *bus* reads (`$6000 + off+i`,
/// side effects included) — the GP-DMA A-bus source path for `$7800`.
const HW_ICD2_BUS: u32 = 0x0100_6000;
/// `R len 3 + 3*MMIO_RING_CAP` (drains): the MMIO write-capture ring.
const HW_MMIO_RING: u32 = 0x0100_1000;
/// `W len L` at `+ i`: CPU-read shadows for `$4200 + i`.
const HW_SHADOW: u32 = 0x0100_2000;
/// `W len 1`: request an NMI (consumed at the next instruction boundary).
const HW_NMI: u32 = 0x0100_3000;
/// `W len 1` zero: DMA service complete, un-stall the CPU. (`R len 1`: the
/// stall flag a guest `$420B` write armed.)
const HW_DMA_STALL: u32 = 0x0100_3001;
/// `R len 3 + 2*cap` (drains): the ordered APU-port write ring.
const HW_PORT_RING: u32 = 0x0100_4000;
/// The plugin's port-ring capacity.
const PORT_RING_CAP: usize = 16384;
/// `R len 2 + 2*PAD_RING_CAP` (drains): the ordered ICD2 pad-latch write
/// ring — `[n, overflow]` then `(reg, value)` pairs.
const HW_PAD_RING: u32 = 0x0100_5000;
/// The plugin's pad-latch ring capacity.
const PAD_RING_CAP: usize = 64;
/// Queued feed snapshots kept at most (oldest dropped).
const FEED_QUEUE_CAP: usize = 128;
/// Streamed character rows held for flush-paced delivery (oldest dropped;
/// ~a frame's worth of bands).
const CHAR_QUEUE_CAP: usize = 24;
/// GB steps each queued feed snapshot dwells on the pad — about a flush's
/// worth, so every value of a latch sequence is visible to several of the
/// GB's poll iterations (a hardware latch write persists until the BIOS's
/// next per-frame pad forward).
const FEED_DWELL_STEPS: u32 = 1024;
/// The plugin's ring capacity (drain reads size to this).
const MMIO_RING_CAP: usize = 16384;
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
/// Where the host stages DATA_TRN payloads (the mid-bank `$7E:2000-$7FFF`
/// window — the pilot's phase streams cover all of `$7F` and the upper
/// half of `$7E`, and staging inside a streamed page corrupts the game's
/// own data; reached through the pointer above). Two
/// ping-pong buffers: the guest's dispatcher caches the pointer at copy
/// start and its 4 KB copy spans several flushes, so the next payload must
/// land in the *other* buffer or an in-flight copy reads torn data (the
/// GB ACKs before copying, so transfer N+1 legitimately arrives mid-copy).
const BIOS_TRN_STAGING: [u32; 2] = [0x7E_5000, 0x7E_6000];
/// BIOS service entries uploaded bootstraps `JSR` into (two per known BIOS
/// revision, selected on the `$00:FFDB` byte — zero in slopgb, so the first
/// of each pair is the live one). The entries sit 3 bytes apart, so each
/// holds a `JMP` thunk into a resident body below.
const BIOS_MAIN_ENTRIES: [u32; 2] = [0xBBED, 0xBBF0];
const BIOS_AUX_ENTRIES: [u32; 2] = [0xC58D, 0xC590];
/// The resident main-service body: consume the host's delivery mailbox
/// (publish the packet/command/staging-pointer to the BIOS-runtime
/// variables), then call the game's `$0800` hook slot when one is
/// installed (the two-JSR depth is pinned by the pilot's own `PLA PLA /
/// RTS` stack fixup). Publishing INSIDE the service call is load-bearing:
/// the real BIOS is single-threaded, so a hook that reads its dest before
/// a vblank wait and its staging pointer after it can never see a
/// mid-delivery update — an async host publish landed exactly in that
/// window and re-routed a payload over the pilot's program area.
const BIOS_MAIN_BODY: u32 = 0xBE80;
/// The host→BIOS delivery mailbox (beside the staging buffers, in the same
/// stream-free mid-bank window):
/// `+0..15` the packet, `+$10` the command byte, `+$11/$12` the staging
/// pointer, `+$16` the pending flag the resident body consumes.
const BIOS_DELIVERY: u32 = 0x7E_4F00;
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

/// The clean-room 65C816 shim (emulation mode, 8-bit). It watches the
/// SNES-RAM mailbox `[$0200,$0201]` and, when the trigger is armed, copies it
/// to the SPC700 comm ports `$2140/$2141` once and disarms it — an idle
/// mailbox produces **no** port traffic (the written level persists on the
/// port, so the SPC700 still sees it; an unconditional forward loop would
/// flood the host's ordered port ring with identical writes). Opcodes are the
/// WDC datasheet encodings (`AD`/`8D`/`9C` = LDA/STA/STZ abs, `F0` = BEQ,
/// `4C` = JMP abs):
///
/// ```text
/// $8000  LDA $0201   ; A = mailbox trigger
/// $8003  BEQ $8000   ; idle: nothing to forward
/// $8005  LDA $0200   ; A = mailbox note
/// $8008  STA $2140   ; -> APUIO0 (the SPC700 reads at $F4)
/// $800B  LDA $0201   ; A = mailbox trigger
/// $800E  STA $2141   ; -> APUIO1 (the SPC700 polls at $F5)
/// $8011  STZ $0201   ; disarm; the port keeps the level
/// $8014  JMP $8000   ; loop
/// ```
const SNES_SHIM: [u8; 23] = [
    0xAD,
    (MB_NOTE + 1) as u8,
    ((MB_NOTE + 1) >> 8) as u8, // LDA $0201
    0xF0,
    0xFB, // BEQ $8000
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
    0x9C,
    (MB_NOTE + 1) as u8,
    ((MB_NOTE + 1) >> 8) as u8, // STZ $0201
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
    /// The `$7800` buffer (`row % 4`) the last streamed character row
    /// landed in — the `$6000` write-row shadow. Transient.
    char_write_row: u8,
    /// Streamed character rows awaiting flush delivery (one per flush, so
    /// the guest sees every `$6000` write-row value). Transient.
    char_queue: VecDeque<(u8, Box<[u8; 320]>)>,
    /// Furthest cycles each chip has actually been run to — ahead of the
    /// targets after a mediation burst (each replayed port event owes the
    /// SPC700 a consume slice and the 65C816 a produce slice). The next
    /// replay's floors start here; a floor below a chip's real position
    /// would no-op the slices and let events clobber the comm ports.
    spc_pos: u64,
    cpu_pos: u64,
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
    /// The last `data_trn_seq` observed from the command source — the cheap
    /// pre-filter that skips re-hashing an unchanged 4 KB payload on every
    /// poll (once per GB instruction). Transient: the core counter resets
    /// with a savestate load, and `None` (a source without the counter)
    /// falls back to hashing each poll.
    data_trn_seq_seen: Option<u64>,
    jump: Option<u32>,

    /// Teed GB packets awaiting deposit into the plugin's ICD2 mailbox
    /// (deposited one per flush, only when the guest cleared `$6002`).
    pending_packets: VecDeque<[u8; 16]>,
    /// The SNES program has written a pad latch at least once (the plugin's
    /// sticky flag): the joypad is taken over. Transient — refreshed from
    /// the (serialized) plugin state each flush.
    pads_taken: bool,
    /// The latches' final state as of the last flush (the base the ring's
    /// incremental writes build on). Transient like `pads_taken`.
    pads_shadow: [u8; 4],
    /// One feed snapshot per drained pad-latch write, oldest first. Each is
    /// held on the GB pad for [`FEED_DWELL_STEPS`] steps before the next
    /// pops, so the GB's polls see every value of a sub-flush latch sequence
    /// (on hardware a latch write persists until the BIOS's next per-frame
    /// pad forward). Transient: re-derives from the plugin's latch state.
    feed_queue: VecDeque<[u8; 4]>,
    /// Steps the current head of `feed_queue` (already popped into
    /// `pads_shadow`) remains on the pad.
    feed_hold: u32,
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
    /// The SNES display has shown a picture at least once (INIDISP written
    /// visible) — gates the frame handoff; see [`Self::take_snes_frame`].
    snes_live: bool,
    /// Which [`BIOS_TRN_STAGING`] buffer the next DATA_TRN payload lands in
    /// (ping-pong; see the const doc).
    trn_flip: bool,
    /// Diagnostics only (`debug_status`), transient across save states:
    /// completed-frame count + the guest's last INIDISP write.
    frames_done: u64,
    last_inidisp: u8,
}

impl SgbCoprocessor {
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

    // Save state, deep clone, and the inert fallback live in `state.rs`.
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
