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
use std::path::{Path, PathBuf};

use slopgb_core::sgb::{AudioCoprocessor, SgbCommandSource};
use slopgb_core::{Reader, SgbFlags, SgbSound, StateError, Writer};
use slopgb_plugin_host::{LoadError, LoadedCoprocessor};

mod audio_coprocessor;
mod commands;
mod dma;
mod perf;
mod render;
mod samples;
mod state;

pub use samples::{Engine, SampleRegions, parse_sgb_apu_blocks};
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
/// Full-scale S-DSP output (±32768) → mix amplitude. Tuned above unity (1.2):
/// real N-SPC songs mix conservatively (master volume + few active voices), so
/// their DSP output sits well below full scale and needs lifting to match a
/// normal GB game's loudness. >1.0 only clips a song whose DSP output nears full
/// scale, which is rare. ponytail: fixed loudness; expose a per-game knob if one
/// clips or still reads quiet.
const MIX_SCALE: f32 = 1.2 / 32768.0;
/// The GB APU feed into the SGB mix. Real SGB routes the Game Boy audio through
/// the SNES mixer below the SNES APU's own level, so the enhanced music sits on
/// top of the GB channels rather than under them. ponytail: fixed ratio; a
/// per-game knob if one needs a different balance.
const GB_GAIN: f32 = 0.6;
/// The clean-room N-SPC music engine (original SPC700 code, built offline from a
/// format spec by a walled-off implementer — see `nspc/README.md`; not derived
/// from any SGB ROM). Uploaded to APU $0400 over the ROM's own engine, driving
/// the ROM-supplied samples. `nspc/driver.bin` (WLA-DX; `make` in `nspc/`).
pub const NSPC_ENGINE: &[u8] = include_bytes!("../nspc/driver.bin");
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
/// The optional MSU-1 streaming-audio plugin: absent = no MSU-1, unchanged. A
/// game reaches it the real-hardware way — its SGB driver uploads a resident
/// 65C816 handler (`DATA_SND` + `JUMP`) that drives the MSU-1 registers at SNES
/// `$2000-$2007`, which the host serves from this plugin (see `apply_mmio` +
/// [`SgbCoprocessor::flush`]).
pub const MSU_WASM: &str = "msu1.wasm";
/// MSU-1 `.pcm` tracks stream at CD rate (44.1 kHz), one chip cycle per sample.
const MSU_RATE: f64 = 44_100.0;
/// Full-scale MSU-1 i16 → mix amplitude. MSU-1 is the game's replacement
/// soundtrack (a GB game mutes its own music when it hands off to MSU-1), so it
/// carries the music while the GB channels carry only SFX (ducked below, as real
/// SGB routes GB audio under the cartridge-in level). `.pcm` tracks are typically
/// mastered with headroom (peak ~-6 dB), so unity would read quiet against a GB
/// SFX near full scale — lift ~2x to match. ponytail: fixed loudness; a per-game
/// knob if a hot track clips or a quiet one still reads low.
const MSU_MIX_SCALE: f32 = 2.0 / 32768.0;
/// MSU_STATUS bit 4 (audio playing) — mirrors the plugin's `ST_AUDIO_PLAYING`;
/// while set, the GB channels duck so the music sits on top.
const MSU_ST_PLAYING: u8 = 0x10;
/// The `S-MSU1` id string a driver reads back at `$2002-$2007` to detect the
/// chip (mirrors the plugin's own `ID`; the driver's presence check compares it).
const MSU_ID: [u8; 6] = *b"S-MSU1";
/// The MSU-1 data-ROM host-file key (mirrors `slopgb_msu1_plugin::DATA_FILE_KEY`;
/// a reserved 32-bit key a 16-bit track number can never collide with).
const MSU_DATA_FILE_KEY: u32 = 0xFFFF_FFFF;

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
/// `W len 8`: the MSU-1 read shadows for `$2000-$2007` (status + `S-MSU1` id).
const HW_MSU: u32 = 0x0100_8000;
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
/// SPC700 cycles (~1.024 MHz) to let a resident N-SPC engine run past a play
/// command before grabbing the [from-start snapshot](SgbCoprocessor::song_start_spc)
/// — about one 60 Hz frame, enough for the driver's song-init to finish (timers
/// enabled, sequencer pointer reset to the score top) so the captured `.spc` is
/// self-sustaining, while losing only an imperceptible sliver of the first note.
const SONG_START_CAPTURE_DELAY: u64 = 17_000;

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

    /// The optional MSU-1 streaming-audio plugin (loaded from `msu1.wasm` in the
    /// plugins dir) + its bytes for [`Self::deep_clone`]. `None` = no MSU-1, the
    /// SGB path unchanged. Reached via the SNES `$2000-$2007` bus, not GB `$A000`.
    msu: Option<RefCell<LoadedCoprocessor>>,
    msu_wasm: Option<Vec<u8>>,
    /// The MSU-1 pack directory (`.pcm` tracks + optional `.msu`), retained so
    /// [`Self::deep_clone`] can re-attach the pack to the cloned plugin.
    msu_pack_dir: Option<PathBuf>,
    /// A track pack with ≥1 `.pcm` is loaded: gates the `S-MSU1` presence shadow
    /// so a game only detects the chip when tracks are actually available.
    msu_present: bool,
    /// The MSU-1 chip cycle (== 44.1 kHz output-sample index) + the GB→44.1 kHz
    /// fractional carry that advances it each flush.
    msu_cycle: u64,
    msu_acc: i64,
    /// Undrained 44.1 kHz MSU-1 samples (oldest first), the 44.1 kHz→output-rate
    /// resample carry, and the held sample — mixed into `out` in `emit_output`
    /// alongside the S-DSP source.
    msu_src: VecDeque<(i16, i16)>,
    msu_src_acc: f64,
    msu_cur: (i16, i16),
    /// The MSU-1 chip reported audio-playing at the last flush: while set, the GB
    /// channels duck in `mix_into` so the streamed music sits above the SFX.
    msu_playing: bool,

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
    /// Fast-forward render opt-out for [`Self::pump_ppu_scanlines`] (see
    /// `render.rs`); defaults `true` in every constructor.
    render_enabled: bool,
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
    /// The real SGB resident sound driver (N-SPC engine + soundfont) was
    /// installed from a user-supplied `--sgb-bios`. When set, SGB SOUND/SOU_TRN
    /// feed that resident engine over the plugin comm ports instead of driving
    /// the clean-room square driver. Default false = clean-room behavior.
    nspc_resident: bool,
    /// A pending N-SPC command word `[cmd_lo, cmd_hi, data_lo, data_hi]` = ports
    /// 0/1 (command) + 2/3 (data), set by a SOUND packet, delivered by
    /// [`Self::nspc_flush`] with the SGB BIOS echo-ack handshake.
    nspc_cmd: [u8; 4],
    /// The last command word actually sent to the engine — the ack shadow the
    /// handshake compares the engine's echo against (BIOS `$0344`/`$0346`).
    nspc_shadow: [u8; 4],
    /// A SOUND command is queued in `nspc_cmd` and not yet acked.
    nspc_pending: bool,
    /// Diagnostics (`debug_status`): how many SOUND / SOU_TRN / DATA_SND commands
    /// have been drained from the core, and the last SOUND packet's four bytes.
    dbg_sound: u32,
    dbg_soutrn: u32,
    dbg_datasnd: u32,
    dbg_last_sound: [u8; 4],
    /// First block dest + total bytes of the last SOU_TRN upload, and the loudest
    /// S-DSP sample amplitude mixed so far (0.0 = the engine produced only
    /// silence) — `debug_status` diagnostics for the N-SPC audio path.
    dbg_soutrn_dest: u16,
    dbg_soutrn_len: u32,
    dbg_soutrn_nonzero: u32,
    dbg_soutrn_head: [u8; 16],
    dbg_pcm_peak: f32,
    /// A self-sustaining `.spc` grabbed shortly after the resident engine
    /// (re)started a song, so an export reproduces that song from its opening
    /// rather than from the instant the user clicks. `None` until a recognized
    /// resident engine reaches a play command — which doubles as the export
    /// gate ([`Self::can_export_spc`]): the built-in square driver and any
    /// game-uploaded foreign engine never fill it, so they stay un-exportable
    /// (the menu greys the row) instead of yielding a broken mid-song dump.
    song_start_spc: Option<Vec<u8>>,
    /// When armed (a play command just landed), the `spc_pos` target at which
    /// [`Self::flush`] grabs [`Self::song_start_spc`].
    capture_at: Option<u64>,
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
        let mut me =
            Self::from_wasm_full(&spc_bytes, &cpu_bytes, ppu_bytes.as_deref(), output_rate)
                .map_err(|e| format!("cannot load SGB coprocessor plugins: {e}"))?;
        // The MSU-1 plugin is optional and loads from the *same* plugins dir as
        // the other coprocessor chips (its `.pcm` pack comes separately via
        // [`Self::set_msu_pack`]). Absent = no MSU-1.
        if let Ok(bytes) = fs::read(dir.join(MSU_WASM)) {
            if let Err(e) = me.attach_msu(&bytes) {
                eprintln!("slopgb: MSU-1 plugin '{MSU_WASM}' present but failed to load: {e}");
            }
        }
        Ok(me)
    }

    /// Load the MSU-1 streaming-audio plugin from its `.wasm` bytes (absent when
    /// `msu1.wasm` is not in the plugins dir). A game's SGB driver detects it at
    /// SNES `$2000-$2007` and drives it there; no track pack is advertised until
    /// [`Self::set_msu_pack`] finds `.pcm` files.
    pub fn attach_msu(&mut self, wasm: &[u8]) -> Result<(), LoadError> {
        let mut cop = LoadedCoprocessor::load(wasm)?;
        cop.reset()?;
        self.msu = Some(RefCell::new(cop));
        self.msu_wasm = Some(wasm.to_vec());
        self.msu_present = false;
        Ok(())
    }

    /// Point the loaded MSU-1 plugin at a pack directory: every `game-N.pcm`
    /// track (keyed by its trailing number, the index the game's `MSU_TRACK`
    /// write selects) and an optional `.msu` data ROM are streamed to the chip
    /// through the plugin's bulk-file channel. Presence (`S-MSU1`) is advertised
    /// only when ≥1 track loads, so a game finds the chip exactly when a pack is
    /// there. No-op without a loaded plugin.
    pub fn set_msu_pack(&mut self, dir: &Path) {
        let Some(msu) = &self.msu else { return };
        let mut tracks = 0usize;
        if let Ok(entries) = fs::read_dir(dir) {
            let mut cop = msu.borrow_mut();
            // ponytail: every track's full bytes are read into the plugin up
            // front (the `set_file` model). Fine for a handful of tracks; stream
            // a track on demand if a large pack's memory ever matters.
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(track) = track_number(&name) {
                    if let Ok(data) = fs::read(&path) {
                        cop.set_file(u32::from(track), data);
                        tracks += 1;
                    }
                } else if name.ends_with(".msu") {
                    if let Ok(data) = fs::read(&path) {
                        cop.set_file(MSU_DATA_FILE_KEY, data);
                    }
                }
            }
        }
        self.msu_pack_dir = Some(dir.to_path_buf());
        self.msu_present = tracks > 0;
        if tracks > 0 {
            eprintln!(
                "slopgb: MSU-1 pack '{}' loaded ({tracks} track(s)); served on the SGB SNES $2000-$2007 bus",
                dir.display(),
            );
        }
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
            spc_pos: 0,
            cpu_pos: 0,
            char_write_row: 0,
            char_queue: VecDeque::new(),
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
            msu: None,
            msu_wasm: None,
            msu_pack_dir: None,
            msu_present: false,
            msu_cycle: 0,
            msu_acc: 0,
            msu_src: VecDeque::new(),
            msu_src_acc: 0.0,
            msu_cur: (0, 0),
            msu_playing: false,
            poll_ctr: 0,
            sou_trn_sig: 0,
            data_trn_sig: 0,
            data_trn_seq_seen: None,
            jump: None,
            pending_packets: VecDeque::new(),
            pads_taken: false,
            pads_shadow: [0xFF; 4],
            feed_queue: VecDeque::new(),
            feed_hold: 0,
            gb_pos: 0,
            pending_trn: VecDeque::new(),
            trn_flip: false,
            nmitimen: 0,
            in_vblank: false,
            dma_regs: [[0; 7]; 8],
            wmadd: 0,
            input: (0x0F, 0x0F),
            joy_busy: false,
            ppu,
            ppu_wasm: ppu_bytes.map(<[u8]>::to_vec),
            ppu_row: 0,
            render_enabled: true,
            frame_ready: false,
            snes_live: false,
            frames_done: 0,
            last_inidisp: 0,
            nspc_resident: false,
            nspc_cmd: [0; 4],
            nspc_shadow: [0; 4],
            nspc_pending: false,
            dbg_sound: 0,
            dbg_soutrn: 0,
            dbg_datasnd: 0,
            dbg_last_sound: [0; 4],
            dbg_soutrn_dest: 0,
            dbg_soutrn_len: 0,
            dbg_soutrn_nonzero: 0,
            dbg_soutrn_head: [0; 16],
            dbg_pcm_peak: 0.0,
            song_start_spc: None,
            capture_at: None,
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
            // Main body (see BIOS_MAIN_BODY): consume the delivery mailbox
            // with long addressing (caller's DBR unknown), then the guarded
            // hook call. PLP precedes the JSR so the hook sees exactly the
            // caller's two return addresses (its PLA PLA / RTS fixup).
            let hook_lo = BIOS_HOOK_SLOT as u8;
            let hook_hi = (BIOS_HOOK_SLOT >> 8) as u8;
            let mb = |off: u32| {
                let a = BIOS_DELIVERY + off;
                [a as u8, (a >> 8) as u8, (a >> 16) as u8]
            };
            let [d0, d1, d2] = mb(0);
            let [c0, c1, c2] = mb(0x10);
            let [p0, p1, p2] = mb(0x11);
            let [q0, q1, q2] = mb(0x12);
            let [f0, f1, f2] = mb(0x16);
            cpu.write_ram(
                BIOS_MAIN_BODY,
                &[
                    0x08, // BE80 PHP
                    0xE2,
                    0x30, // BE81 SEP #$30
                    0xAF,
                    f0,
                    f1,
                    f2, // BE83 LDA long flag
                    // No delivery pending: skip the publish, still call the
                    // hook — the BIOS invokes it every service loop, and
                    // the pilot's hook re-ACKs on stale $02C2 (its own
                    // pacing protocol with the GB).
                    0xF0,
                    0x2B, // BE87 BEQ hookcall (BEB4)
                    0xA2,
                    0x0F, // BE89 LDX #$0F
                    0xBF,
                    d0,
                    d1,
                    d2, // BE8B loop: LDA long packet,X
                    0x9F,
                    BIOS_PKT_BUF as u8,
                    (BIOS_PKT_BUF >> 8) as u8,
                    0x00, // BE8F STA long $0600,X
                    0xCA, // BE93 DEX
                    0x10,
                    0xF5, // BE94 BPL loop
                    0xAF,
                    c0,
                    c1,
                    c2, // BE96 LDA long cmd
                    0x8F,
                    BIOS_LAST_CMD as u8,
                    (BIOS_LAST_CMD >> 8) as u8,
                    0x00, // BE9A STA long $02C2
                    0xAF,
                    p0,
                    p1,
                    p2, // BE9E LDA long ptr lo
                    0x8F,
                    BIOS_TRN_PTR as u8,
                    (BIOS_TRN_PTR >> 8) as u8,
                    0x00, // BEA2 STA long $0284
                    0xAF,
                    q0,
                    q1,
                    q2, // BEA6 LDA long ptr hi
                    0x8F,
                    (BIOS_TRN_PTR + 1) as u8,
                    ((BIOS_TRN_PTR + 1) >> 8) as u8,
                    0x00, // BEAA
                    0xA9,
                    0x00, // BEAE LDA #$00
                    0x8F,
                    f0,
                    f1,
                    f2, // BEB0 STA long flag (consumed)
                    0xAF,
                    hook_lo,
                    hook_hi,
                    0x00, // BEB4 LDA long $0800 (hook?)
                    0xF0,
                    0x05, // BEB8 BEQ exit (BEBF: the PLP)
                    0x28, // BEBA PLP
                    0x20,
                    hook_lo,
                    hook_hi, // BEBB JSR $0800
                    0x60,    // BEBE RTS
                    0x28,    // BEBF exit: PLP
                    0x60,    // BEC0 RTS
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
            // No set_pc: the SPC700 boots its own IPL ROM (the chip
            // ships the documented 64-byte boot loader at $FFC0). The
            // pilot's loader speaks exactly the standard uploader protocol
            // ($2142/43 dest, $2141 nonzero cmd = its length bytes ORed,
            // kick chain, terminator with cmd 0 + entry dest) — and its
            // own entry-jumped APU driver re-announces $AA/$BB to serve
            // the next upload round. The square driver above is entered
            // host-side on a SOUND command instead (see apply_sound).
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
        let mut tp = perf::PerfTimer::start();
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
            // Two-phase drain: a 3-byte header probe reports the pending
            // count without draining; a sized read drains exactly `pending`
            // entries (clamped to full-window capacity).
            let pending = match cpu.read_ram(HW_MMIO_RING, 3) {
                Ok(h) if h.len() >= 2 => usize::from(h[0]) | usize::from(h[1]) << 8,
                _ => 0,
            };
            let bulk = if pending > 0 {
                let size = (3 + 3 * pending).min(3 + 3 * MMIO_RING_CAP);
                cpu.read_ram(HW_MMIO_RING, size)
            } else {
                Ok(Vec::new())
            };
            match bulk {
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
        // Consecutive pure-PPU writes ($2100-$213F; the WMDATA quartet and
        // the CPU I/O block are host-consumed) apply as one batched wasm
        // call; any host-consumed register is an order barrier that flushes
        // the run first, so the guest-observable sequence is unchanged.
        let mut ppu_run: Vec<u8> = Vec::new();
        for (addr, val) in captured {
            if (0x2100..=0x213F).contains(&addr) {
                if addr == 0x2100 {
                    self.last_inidisp = val; // diagnostics (debug_status)
                    // The display shows a picture: not force-blanked and
                    // brightness above zero (fullsnes 2100h). Arms the
                    // frame handoff — see `take_snes_frame`.
                    if val & 0x80 == 0 && val & 0x0F != 0 {
                        self.snes_live = true;
                    }
                }
                if (0x2102..=0x2103).contains(&addr) && std::env::var_os("SLOPGB_OAMDBG").is_some()
                {
                    eprintln!("OAMREG {addr:04X}={val:02X}");
                }
                ppu_run.push((addr - 0x2100) as u8);
                ppu_run.push(val);
            } else {
                self.flush_ppu_run(&mut ppu_run);
                self.apply_mmio(addr, val);
            }
        }
        self.flush_ppu_run(&mut ppu_run);
        // The ring is applied — any $420B in it just ran its transfer — so
        // release a DMA-stalled CPU before this flush's run_until.
        let _ = self.cpu.get_mut().write_ram(HW_DMA_STALL, &[0]);
        tp.lap(0);
        // The scanline pump: render every framebuffer row the SNES beam has
        // passed since the last flush (display lines are 1-based, so row r
        // is complete once V > r) — one span render per flush. Runs after
        // the ring apply, so this flush's register writes land at ~10-line
        // granularity. See `render.rs` for the fast-forward render-skip.
        self.pump_ppu_scanlines();
        tp.lap(1);
        {
            let mut cpu = self.cpu.borrow_mut();
            // One character row per flush: land it in its rotating buffer
            // and advance the $6000 write-row shadow so the guest observes
            // every band value in sequence (18 bands/frame vs ~17 flushes
            // — near-lockstep with the real stream).
            if let Some((band, data)) = self.char_queue.pop_front() {
                let _ = cpu.write_ram(HW_CHAR_ROWS + u32::from(band % 4) * 320, &data[..]);
                self.char_write_row = band % 4;
            }
            let _ = cpu.write_ram(HW_LCD_ROW, &[row, self.char_write_row]);
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
        tp.lap(2);
        // Advance the chips' absolute cycle targets by the GB→chip ratios.
        self.spc_acc += span as i64 * SPC_NUM;
        let spc_adv = self.spc_acc.div_euclid(SPC_DEN).max(0) as u64;
        self.spc_acc -= spc_adv as i64 * SPC_DEN;
        self.spc_target += spc_adv;
        self.cpu_acc += span as i64 * CPU_NUM;
        let cpu_adv = self.cpu_acc.div_euclid(CPU_DEN).max(0) as u64;
        self.cpu_acc -= cpu_adv as i64 * CPU_DEN;
        self.cpu_target += cpu_adv;

        // 1+2. Interleaved co-simulation, in rounds: drain the 65C816's
        // ordered APU-port write ring, replay each write into the SPC700 with
        // a consume slice, hand the echoes back and give the CPU a produce
        // slice — then drain again, because those CPU slices produce the
        // *next* writes (an echo-paced upload advances exactly one byte per
        // round). Without the rounds a whole flush moved one byte; the bound
        // keeps a flush finite. Ordered per-event replay matters: the IPL
        // upload's index/ack pump repeats mod-256 values, so delivering only
        // final latch states aliases the handshake and desyncs the chips.
        //
        // A comm port is a register, not a queue: each write needs the SPC700
        // to run enough cycles to consume it before the next lands (the IPL's
        // per-byte pump is ~25 cycles — fullsnes "Boot ROM"), and the CPU
        // needs enough to check the echo and produce the next byte. The
        // per-event floors let both chips run ahead of their clock targets
        // during a burst; the positions are anchored across flushes
        // (`spc_pos`/`cpu_pos`), and later flush targets simply no-op until
        // real time catches back up.
        {
            let spc_start = self.spc_target - spc_adv;
            let cpu_start = self.cpu_target - cpu_adv;
            let dbg = std::env::var_os("SLOPGB_APUDBG").is_some();
            const SPC_MIN_EVENT_CYCLES: u64 = 64;
            const CPU_MIN_EVENT_CYCLES: u64 = 64;
            const MAX_MEDIATION_ROUNDS: usize = 512;
            let mut spc_pos = self.spc_pos.max(spc_start);
            let mut cpu_pos = self.cpu_pos.max(cpu_start);
            for _ in 0..MAX_MEDIATION_ROUNDS {
                let events: Vec<(u8, u8)> = {
                    let mut cpu = self.cpu.borrow_mut();
                    // Two-phase drain (see the MMIO ring): header probe
                    // first, a sized read drains exactly `pending` entries.
                    let pending = match cpu.read_ram(HW_PORT_RING, 3) {
                        Ok(h) if h.len() >= 2 => usize::from(h[0]) | usize::from(h[1]) << 8,
                        _ => 0,
                    };
                    let bulk = if pending > 0 {
                        let size = (3 + 2 * pending).min(3 + 2 * PORT_RING_CAP);
                        cpu.read_ram(HW_PORT_RING, size)
                    } else {
                        Ok(Vec::new())
                    };
                    match bulk {
                        Ok(buf) if buf.len() >= 3 => {
                            let n = usize::from(buf[0]) | usize::from(buf[1]) << 8;
                            if buf[2] != 0 {
                                eprintln!("slopgb: SNES APU port ring overflowed; writes dropped");
                            }
                            buf[3..]
                                .chunks_exact(2)
                                .take(n)
                                .map(|e| (e[0], e[1]))
                                .collect()
                        }
                        _ => Vec::new(),
                    }
                };
                if events.is_empty() {
                    break;
                }
                for &(p, v) in &events {
                    let mut spc = self.spc.borrow_mut();
                    let _ = spc.port_write(p, v);
                    if usize::from(p) < N_PORTS {
                        self.to_spc[usize::from(p)] = v;
                    }
                    spc_pos += SPC_MIN_EVENT_CYCLES;
                    let _ = spc.run_until(spc_pos);
                    let mut echoes = [0u8; N_PORTS];
                    for (q, slot) in echoes.iter_mut().enumerate() {
                        *slot = spc.port_read(q as u8).unwrap_or(0);
                    }
                    if dbg {
                        eprintln!(
                            "apu<- p{p}={v:02X} | out {:02X} {:02X}",
                            echoes[0], echoes[1]
                        );
                    }
                    drop(spc);
                    let mut cpu = self.cpu.borrow_mut();
                    for (q, &e) in echoes.iter().enumerate() {
                        let _ = cpu.port_write(q as u8, e);
                    }
                    cpu_pos += CPU_MIN_EVENT_CYCLES;
                    let _ = cpu.run_until(cpu_pos);
                }
            }
            tp.lap(3);
            // Run the SPC700 + S-DSP to the target (or wherever the event
            // floor left it, if that is already past) and pull the PCM.
            self.spc_pos = self.spc_target.max(spc_pos);
            self.cpu_pos = self.cpu_target.max(cpu_pos);
            let mut spc = self.spc.borrow_mut();
            let _ = spc.run_until(self.spc_pos);
            if let Ok(batch) = spc.drain_pcm() {
                self.src.extend(batch);
            }
        }
        tp.lap(4);
        // 3. Read the SPC700's final comm-port replies back for the 65C816.
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
            // 4. Run the 65C816 to its target (or the event floor's position).
            let _ = cpu.run_until(self.cpu_target.max(self.cpu_pos));
            // ICD2, SNES→GB half: drain the ordered pad-latch write ring.
            // Every write becomes one queued feed snapshot, so a sub-flush
            // protocol sequence (the takeover init's one-shot Select+Start
            // trigger chased by the hook's ACK sandwich) reaches the GB one
            // value per step instead of collapsing to the final latch state.
            // The sticky written flag still gates the takeover as a whole —
            // before the SNES side writes a latch, the GB's local matrix
            // must stay live.
            if let Ok(v) = cpu.read_ram(HW_PADS, 5) {
                if let [p1, p2, p3, p4, written] = v[..] {
                    if written != 0 {
                        self.pads_taken = true;
                        self.pads_shadow = [p1, p2, p3, p4];
                    }
                }
            }
            // Two-phase drain (see the MMIO ring): header probe first, sized read.
            let pad_pending = match cpu.read_ram(HW_PAD_RING, 2) {
                Ok(h) if !h.is_empty() => usize::from(h[0]),
                _ => 0,
            };
            let pad_bulk = if pad_pending > 0 {
                let size = (2 + 2 * pad_pending).min(2 + 2 * PAD_RING_CAP);
                cpu.read_ram(HW_PAD_RING, size)
            } else {
                Ok(Vec::new())
            };
            if let Ok(ring) = pad_bulk {
                if ring.len() >= 2 {
                    let n = usize::from(ring[0]);
                    let mut pads = self.feed_queue.back().copied().unwrap_or(self.pads_shadow);
                    for i in 0..n {
                        let (r, v) = (ring[2 + i * 2], ring[3 + i * 2]);
                        if usize::from(r) < 4 {
                            pads[usize::from(r)] = v;
                            self.feed_queue.push_back(pads);
                        }
                    }
                    while self.feed_queue.len() > FEED_QUEUE_CAP {
                        self.feed_queue.pop_front();
                    }
                }
            }
        }
        tp.lap(5);
        // 5. Advance the MSU-1 chip + refresh its $2000 read shadow, then emit
        // output-rate samples (S-DSP + MSU-1 → output rate, zero-order-hold).
        self.pump_msu(span);
        self.emit_output(span);
        // From-start capture: once the resident engine has run a frame past the
        // play command (`nspc_flush` armed `capture_at`), the song-init has
        // finished — grab a self-sustaining snapshot for a from-the-top export.
        if let Some(at) = self.capture_at {
            if self.spc_pos >= at {
                self.capture_at = None;
                if let Ok(spc) = self.spc.borrow_mut().dump_spc() {
                    if !spc.is_empty() {
                        self.song_start_spc = Some(spc);
                    }
                }
            }
        }
        tp.lap(6);
        tp.finish();
    }

    /// Advance the MSU-1 chip a `span` of GB T-cycles' worth of 44.1 kHz samples,
    /// drain its PCM into `msu_src` for the mix, and refresh the CPU's read shadow
    /// for `$2000-$2007` (MSU_STATUS + the `S-MSU1` id) so the game's resident
    /// SNES-side handler can detect the chip and poll its status. No-op — and no
    /// presence advertised — without a loaded plugin + a track pack.
    fn pump_msu(&mut self, span: u64) {
        if self.msu.is_none() {
            return;
        }
        // GB T-cycles → 44.1 kHz chip cycles (one cycle == one output sample),
        // fractional carry in `msu_acc` — the same ratio law as the SPC/CPU.
        self.msu_acc += span as i64 * MSU_RATE as i64;
        let adv = self.msu_acc.div_euclid(i64::from(GB_CLOCK_HZ)).max(0) as u64;
        self.msu_acc -= adv as i64 * i64::from(GB_CLOCK_HZ);
        self.msu_cycle += adv;
        let present = self.msu_present;
        let (batch, status) = {
            let mut cop = self.msu.as_ref().unwrap().borrow_mut();
            let _ = cop.run_until(self.msu_cycle);
            let batch = cop.drain_pcm().unwrap_or_default();
            // port_read(0) = MSU_STATUS (a pure read; the data-read port 1 is the
            // only one with a side effect, and it is never mirrored).
            let status = if present {
                cop.port_read(0).unwrap_or(0)
            } else {
                0
            };
            (batch, status)
        };
        self.msu_src.extend(batch);
        self.msu_playing = present && status & MSU_ST_PLAYING != 0;
        let mut shadow = [0u8; 8];
        if present {
            shadow[0] = status;
            shadow[2..8].copy_from_slice(&MSU_ID);
        }
        let _ = self.cpu.borrow_mut().write_ram(HW_MSU, &shadow);
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
            // The MSU-1 source (44.1 kHz) rides the same output timeline. Unlike
            // the S-DSP hold, an underrun/stop drops to silence (pop-or-zero) so a
            // finished track leaves no held-DC click.
            self.msu_src_acc += MSU_RATE;
            while self.msu_src_acc >= f64::from(self.out_rate) {
                self.msu_src_acc -= f64::from(self.out_rate);
                self.msu_cur = self.msu_src.pop_front().unwrap_or((0, 0));
            }
            let amp = f32::from(self.cur.0.unsigned_abs().max(self.cur.1.unsigned_abs()));
            if amp > self.dbg_pcm_peak {
                self.dbg_pcm_peak = amp;
            }
            if self.out.len() < self.max_out {
                self.out.push((
                    f32::from(self.cur.0) * MIX_SCALE + f32::from(self.msu_cur.0) * MSU_MIX_SCALE,
                    f32::from(self.cur.1) * MIX_SCALE + f32::from(self.msu_cur.1) * MSU_MIX_SCALE,
                ));
            }
        }
    }

    fn mix_into(&mut self, gb: &mut [(f32, f32)]) {
        // The SGB hardware mix routes the GB audio into the SNES mixer below the
        // SNES-side level: duck the GB channels when the SNES side is the primary
        // voice — the resident N-SPC driver (--sgb-bios) or a playing MSU-1 track
        // (whose game has already muted its own GB music, leaving only SFX). With
        // neither, leave the GB channels at full so nothing is quieted for no
        // benefit (clean-room square SFX / no enhancement).
        if self.nspc_resident || self.msu_playing {
            for s in gb.iter_mut() {
                s.0 *= GB_GAIN;
                s.1 *= GB_GAIN;
            }
        }
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
        self.msu_src_acc = 0.0;
        self.msu_src.clear();
        self.msu_cur = (0, 0);
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
    /// Apply a buffered run of pure-PPU `(port, val)` pairs as one batched
    /// plugin call, in order, and clear the buffer. No-op when empty or
    /// without a PPU plugin (matching the unbatched path's routing).
    fn flush_ppu_run(&mut self, run: &mut Vec<u8>) {
        if run.is_empty() {
            return;
        }
        if let Some(ppu) = &self.ppu {
            let _ = ppu.borrow_mut().write_ram(PPU_HW_PORTS, run);
        }
        run.clear();
    }

    fn apply_mmio(&mut self, addr: u16, val: u8) {
        match addr {
            // MSU-1 register window: the resident 65C816 handler a game's SGB
            // driver uploaded writes seek/track/volume/control here, and the host
            // forwards them to the MSU-1 plugin (comm port n == `$2000 + n`).
            0x2000..=0x2007 => {
                if let Some(msu) = &self.msu {
                    let _ = msu.borrow_mut().port_write((addr - 0x2000) as u8, val);
                }
            }
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
                if (0x2102..=0x2103).contains(&addr) && std::env::var_os("SLOPGB_OAMDBG").is_some()
                {
                    eprintln!("OAMREG {addr:04X}={val:02X}");
                }
                if addr == 0x2100 {
                    self.last_inidisp = val; // diagnostics (debug_status)
                    // The display shows a picture: not force-blanked and
                    // brightness above zero (fullsnes 2100h). Arms the
                    // frame handoff — see `take_snes_frame`.
                    if val & 0x80 == 0 && val & 0x0F != 0 {
                        self.snes_live = true;
                    }
                }
                if let Some(ppu) = &self.ppu {
                    let _ = ppu.borrow_mut().port_write((addr - 0x2100) as u8, val);
                }
            }
            _ => {}
        }
    }

    /// Fetch the last completed SNES frame (256x224 RGB555 words,
    /// row-major), at most once per vblank. `None` without a PPU plugin,
    /// until the next frame completes, or until the SNES display has ever
    /// shown a picture (`snes_live`) — before a takeover programs the PPU
    /// the framebuffer is permanently black, and surfacing it would black
    /// out the frontend over the live HLE presentation. Sticky once live:
    /// the takeover's own blank stretches present as a real TV would.
    pub fn take_snes_frame(&mut self) -> Option<Vec<u16>> {
        if !self.frame_ready || !self.snes_live {
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
    // The `AudioCoprocessor` impl (clock/poll/mix/status + the manifest and
    // its "Export SPC" menu row) lives in `audio_coprocessor.rs`.
}

/// A parsed SNES APU upload table: the entry point + `(dest, bytes)` blocks.
type ApuBlocks = (u16, Vec<(u16, Vec<u8>)>);

/// Parse a standard SNES APU upload table (`[u16 len, u16 dest, len bytes]*`
/// terminated by `[0000, entry]`) starting at `off`, returning `(entry,
/// blocks)`. Rejects a malformed table (out-of-bounds length, no terminator, or
/// no block loading the N-SPC engine entry `$0400`) so a wrong offset / wrong
/// ROM falls back to the clean-room firmware rather than uploading garbage.
fn parse_apu_blocks(rom: &[u8], mut off: usize) -> Option<ApuBlocks> {
    let mut blocks: Vec<(u16, Vec<u8>)> = Vec::new();
    loop {
        let len = u16::from_le_bytes([*rom.get(off)?, *rom.get(off + 1)?]);
        let dest = u16::from_le_bytes([*rom.get(off + 2)?, *rom.get(off + 3)?]);
        off += 4;
        if len == 0 {
            // Terminator: `dest` is the SPC700 execution entry. A valid driver
            // table loads the engine to $0400 and jumps there.
            return (dest == 0x0400 && blocks.iter().any(|(d, _)| *d == 0x0400))
                .then_some((dest, blocks));
        }
        let end = off.checked_add(usize::from(len))?;
        blocks.push((dest, rom.get(off..end)?.to_vec()));
        off = end;
        if blocks.len() > 64 {
            return None; // runaway guard: no real driver table is this long
        }
    }
}

/// `install_firmware`). The original clean-room driver waits on comm port 1
/// (the SNES trigger), then programs the S-DSP to key a ~2 kHz square-wave
/// voice. Authored from the SPC700 opcode table + S-DSP register map
/// (nocash *fullsnes*), never from a ROM.
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

/// The track number a `.pcm` filename ends with (`track_1.pcm` / `game-3.pcm` /
/// `5.pcm` → `1` / `3` / `5`), or `None` for a non-`.pcm` / un-numbered name.
/// The number is the plugin host-file key a game selects via `MSU_TRACK`.
fn track_number(name: &str) -> Option<u16> {
    let stem = name.strip_suffix(".pcm")?;
    // Trailing digits are ASCII (1 byte each), so the char count is the byte
    // length of the run — slice it straight off the end.
    let run = stem.chars().rev().take_while(char::is_ascii_digit).count();
    stem.get(stem.len() - run..).and_then(|d| d.parse().ok())
}
