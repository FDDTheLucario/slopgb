//! slopgb-core: cycle-accurate Game Boy (DMG) and Game Boy Color (CGB) emulator core.
//!
//! Zero dependencies, `forbid(unsafe_code)`, fully deterministic. The frontend
//! drives emulation via [`GameBoy::run_frame`] (or [`GameBoy::step`] for
//! instruction granularity), reads pixels from [`GameBoy::frame`], and drains
//! audio samples from [`GameBoy::drain_audio`].
//!
//! # Timing model
//!
//! The CPU is the clock master. Every CPU M-cycle advances the rest of the
//! machine (PPU, timer, DMA, APU, serial) by one M-cycle *before* the memory
//! access of that cycle is performed. See `docs/ARCHITECTURE.md` for the full
//! contract.

pub(crate) mod apu;
pub(crate) mod cartridge;
pub(crate) mod cpu;
pub(crate) mod cycle_clock;
pub mod debug;
pub(crate) mod interconnect;
pub(crate) mod joypad;
// Test-only cross-check oracle (SameBoy's decoupled visible/interrupt-mode
// timeline); the live PPU never consults it, so it is gated out of the shipped
// core. See its module doc.
#[cfg(test)]
pub(crate) mod mode_timeline;
pub(crate) mod model;
pub(crate) mod ppu;
pub(crate) mod serial;
pub mod sgb;
pub(crate) mod stat_update;
pub(crate) mod timer;

// The save-state serializer lives in the shared SNES-APU crate (so the SPC700 +
// S-DSP there can save through the same `Writer`/`Reader` the rest of the
// machine uses); re-exported as `crate::state` so every subsystem keeps naming
// it unchanged.
pub(crate) use slopgb_snes_apu::state;

pub use apu::DEFAULT_SAMPLE_RATE;
pub use cartridge::CartridgeError;
pub use cpu::Registers;
pub use joypad::Button;
pub use model::Model;
pub use slopgb_snes_apu::StateError;
// The save-state (de)serializers the `AudioCoprocessor` trait's `write_state` /
// `read_state` take. Re-exported publicly so an out-of-core coprocessor (e.g. a
// plugin-backed one) can name the parameter types without depending on
// `slopgb-snes-apu` directly.
pub use slopgb_snes_apu::state::{Reader, Writer};

// A Game Genie ROM patch — the element type of the public
// [`GameBoy::set_gg_patches`], built by the frontend cheat engine.
pub use cartridge::GgPatch;

// Escape hatch for the crate's integration tests, which drive the CPU and
// interconnect directly (OAM DMA freeze/timing tests). Not public API.
#[doc(hidden)]
pub use cartridge::Cartridge;
#[doc(hidden)]
pub use cpu::{Bus, Cpu};
pub use interconnect::CdlRange;
#[doc(hidden)]
pub use interconnect::Interconnect;

/// Screen width in pixels.
pub const SCREEN_W: usize = 160;
/// Screen height in pixels.
pub const SCREEN_H: usize = 144;
/// Pixels per frame.
pub const SCREEN_PIXELS: usize = SCREEN_W * SCREEN_H;
/// SGB border surface width (32 tiles of 8px).
pub const SGB_BORDER_W: usize = 256;
/// SGB border surface height (28 tiles of 8px).
pub const SGB_BORDER_H: usize = 224;
/// Pixels in the SGB border surface.
pub const SGB_BORDER_PIXELS: usize = SGB_BORDER_W * SGB_BORDER_H;
/// T-cycles (dots) per frame with the LCD on.
pub const CYCLES_PER_FRAME: u32 = 70224;

/// A game's SGB border (CHR_TRN tiles + PCT_TRN tilemap/palettes) captured from
/// an initial SGB run, for the "GBC + initial SGB border" mode. Opaque — grab it
/// with [`GameBoy::capture_initial_sgb_border`], show it on a CGB machine with
/// [`GameBoy::install_sgb_border`].
pub struct SgbBorder {
    tiles: Box<[u8; 8192]>,
    raw: Box<[u8; 2176]>,
}

/// How to initialise power-on RAM, applied via [`GameBoy::init_ram`] before the
/// machine runs. The default machine ([`GameBoy::new`]) leaves cartridge SRAM at
/// `0xFF` and work/video RAM zeroed (a stable, golden reference); a frontend can
/// override that for the play experience.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RamInit {
    /// Fill the cartridge's external (battery) SRAM with a constant byte, leaving
    /// work/video/OAM RAM at their zeroed power-on. The deterministic default
    /// most emulators use; `Fill(0xFF)` reproduces [`GameBoy::new`]. A loaded
    /// `.sav` overwrites the SRAM afterwards.
    Fill(u8),
    /// Fill ALL RAM — cartridge SRAM, work RAM, HRAM, VRAM and OAM — with a
    /// seeded xorshift PRNG: realistic power-on garbage (e.g. the garbage tiles a
    /// game briefly shows before clearing VRAM). Deterministic per `seed` so runs
    /// stay reproducible; std-only, not cryptographic.
    Random(u64),
}

/// Tiny non-cryptographic xorshift64 PRNG for [`RamInit::Random`]. std-only, no
/// deps; deterministic per seed.
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        // A zero state is a xorshift fixed point; force it non-zero.
        Self(seed | 1)
    }

    fn next_u8(&mut self) -> u8 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 24) as u8
    }
}

/// A decoded SGB SOUND ($08) command: two sound-effect IDs, an
/// attenuation/flags byte and the effect-bank selector. The core decodes and
/// queues these; Phase 3 (the S-DSP) drains the queue via
/// [`GameBoy::sgb_take_sound_event`]. (Pan Docs "SGB Command $08 — SOUND".)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SgbSound {
    /// Sound Effect A (port 1) identifier.
    pub effect_a: u8,
    /// Sound Effect B (port 2) identifier.
    pub effect_b: u8,
    /// Attenuation / effect-on flags byte.
    pub attenuation: u8,
    /// Effect-bank selector byte.
    pub effect_bank: u8,
}

/// A read-only snapshot of the SGB flag commands (ATRC_EN/TEST_EN/ICON_EN/
/// PAL_PRI) and the latched JUMP target — SNES-side state with no Game-Boy-bus
/// effect, exposed for Phase-2/3 consumers. (Pan Docs "SGB Command $0C-$0E /
/// $12 / $19".)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SgbFlags {
    /// ATRC_EN ($0C): attraction / screen-saver enable.
    pub atrc_en: bool,
    /// TEST_EN ($0D): SNES speed test enable.
    pub test_en: bool,
    /// ICON_EN ($0E): SGB built-in icon / menu enable.
    pub icon_en: bool,
    /// PAL_PRI ($19): application-vs-user palette priority.
    pub pal_pri: bool,
    /// JUMP ($12) SNES program target (24-bit PC), if one was issued.
    pub jump: Option<u32>,
}
/// Master clock in Hz (T-cycles / dots per second, normal speed).
pub const CLOCK_HZ: u32 = 4_194_304;

// Debugger exception-break mask bits (bgb's Options → Exceptions "break on X").
// Set via [`GameBoy::set_exceptions`]; the free run halts when an armed
// condition occurs. The mask is 0 on every golden/test path, so the
// exec/access checks are single-branch no-ops there (golden-safe).
/// Break on `LD B,B` (opcode `40h`).
pub const EXC_LD_B_B: u16 = 1 << 0;
/// Break on an undefined opcode (the 11 illegal SM83 opcodes).
pub const EXC_INVALID_OPCODE: u16 = 1 << 1;
/// Break on any CPU access to echo RAM (`E000-FDFF`).
pub const EXC_ECHO_RAM: u16 = 1 << 2;
/// Break on disabling the LCD (`FF40` bit 7 → 0) outside vblank.
pub const EXC_LCD_OFF_VBLANK: u16 = 1 << 3;

/// Leading bytes of a slopgb save state (see [`GameBoy::save_state`]).
const STATE_MAGIC: &[u8; 4] = b"SLPS";
/// Save-state format version (bumped on any layout change). v3 dropped the
/// APU output queues (`samples`/`raw_samples`) from the payload; v4 appends the
/// SGB audio subsystem (SPC700 + S-DSP) on `Model::Sgb`/`Sgb2` states; v6 dropped
/// the retired eager-clock flags (`leading-edge`/`eager`) from the
/// interconnect + PPU payloads; v7 records a has-SGB-audio-tail flag byte right
/// after the header so a cross-model load (SGB state into DMG/CGB or vice versa)
/// is rejected with `StateError::ModelMismatch` instead of silently dropping the
/// tail or failing as `Truncated`.
const STATE_VERSION: u16 = 7;

/// A debugger memory watchpoint (bgb's "Set watchpoint"): the free run halts
/// after the CPU accesses `addr` with a matching access kind. A frontend/
/// debugger control — the watch list defaults empty (zero overhead, no behavior
/// change) and is never populated on a golden/test path, so it is golden-safe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watchpoint {
    pub addr: u16,
    /// Halt when the CPU reads `addr`.
    pub read: bool,
    /// Halt when the CPU writes `addr`.
    pub write: bool,
}

/// A CPU register pair the debugger can write via [`GameBoy::debug_set_reg`]
/// (bgb's registers-pane "edit register"). The 8-bit halves are always edited
/// as their 16-bit pair, matching bgb's `af`/`bc`/`de`/`hl`/`sp`/`pc` rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugReg {
    Af,
    Bc,
    De,
    Hl,
    Sp,
    Pc,
}

/// A complete emulated Game Boy.
pub struct GameBoy {
    cpu: cpu::Cpu,
    bus: interconnect::Interconnect,
    /// SGB audio subsystem (SPC700 + S-DSP), behind the [`sgb::AudioCoprocessor`]
    /// swap seam. `Some` only on `Model::Sgb`/`Sgb2` (the built-in `SgbApu`);
    /// `None` elsewhere, so `Dmg`/`Cgb` are byte-identical (golden-safe).
    sgb_apu: Option<Box<dyn sgb::AudioCoprocessor>>,
}

impl Clone for GameBoy {
    fn clone(&self) -> Self {
        // Manual (not derived) because `Box<dyn AudioCoprocessor>` is not `Clone`;
        // deep-clone the coprocessor via its `clone_box`.
        GameBoy {
            cpu: self.cpu.clone(),
            bus: self.bus.clone(),
            sgb_apu: self.sgb_apu.as_ref().map(|a| a.clone_box()),
        }
    }
}

/// Build the default (built-in) SGB audio coprocessor for `model`, boxed behind
/// the [`sgb::AudioCoprocessor`] swap seam. `None` off `Model::Sgb`/`Sgb2`.
fn build_sgb_apu(model: Model) -> Option<Box<dyn sgb::AudioCoprocessor>> {
    sgb::apu::SgbApu::for_model(model).map(|a| Box::new(a) as Box<dyn sgb::AudioCoprocessor>)
}

impl GameBoy {
    /// Build a machine for `model` with the given cartridge ROM image.
    ///
    /// No boot ROM is executed: CPU registers, hardware registers and timers
    /// are initialised to the exact post-boot state of `model`.
    pub fn new(model: Model, rom: Vec<u8>) -> Result<Self, CartridgeError> {
        let cart = cartridge::Cartridge::from_bytes(rom)?;
        Ok(Self::post_boot(model, cart))
    }

    /// Initialise power-on RAM per `init` (see [`RamInit`]). Call this BEFORE the
    /// machine runs any frame and before any `.sav` load — a frontend seam for
    /// the play experience. The golden/test path never calls it, so [`Self::new`]'s
    /// stable RAM (cartridge SRAM `0xFF`, work/video zeroed) stays byte-identical.
    pub fn init_ram(&mut self, init: RamInit) {
        match init {
            RamInit::Fill(byte) => self.bus.cartridge_mut().fill_ram(|| byte),
            RamInit::Random(seed) => {
                let mut rng = XorShift64::new(seed);
                self.bus.cartridge_mut().fill_ram(|| rng.next_u8());
                self.bus.fill_work_ram(|| rng.next_u8());
                self.bus.ppu_mut().fill_video_ram(|| rng.next_u8());
            }
        }
    }

    /// The direct post-boot machine (no boot ROM executed): registers, hardware
    /// registers and timers installed at the model's post-boot state. The shared
    /// body of [`Self::new`] and the [`Self::new_with_boot`] wrong-size fallback.
    fn post_boot(model: Model, cart: cartridge::Cartridge) -> Self {
        let mut bus = interconnect::Interconnect::new(model, cart);
        let mut cpu = cpu::Cpu::new(model);
        bus.apply_post_boot_state();
        if bus.cgb_mode() {
            // CGB-flagged cart: the CGB/AGB boot ROM hands off DE=$FF56
            // HL=$000D instead of the DMG-cart values in the per-model
            // table (Pan Docs "CPU registers", Power-Up Sequence). A/F/B/C
            // are cart-independent. Pure register-file override with no
            // timing side effects; it only needs to land before the first
            // `step`.
            cpu.regs_mut().set_de(0xFF56);
            cpu.regs_mut().set_hl(0x000D);
        }
        let sgb_apu = build_sgb_apu(model);
        Self { cpu, bus, sgb_apu }
    }

    /// Build a machine that **executes `boot_rom`** from power-on (bgb's
    /// opt-in boot ROM: Nintendo logo scroll + chime + header check), instead
    /// of installing the post-boot state directly. The boot ROM is mapped over
    /// the low cart region (256 B DMG-class / 2304 B CGB-class) and runs from
    /// `PC=0x0000` in true power-on state; it writes FF50 to hand off to the
    /// cartridge. `new` (no boot ROM) is unchanged — this is a separate path,
    /// so emulation stays byte-identical when no boot ROM is supplied.
    ///
    /// A `boot_rom` whose length does not match the model class (256 B for
    /// DMG/MGB/SGB, 2304 B for CGB/AGB) cannot be mapped, so it is **ignored**
    /// and the machine falls back to the direct post-boot install (identical to
    /// [`Self::new`]) rather than running from a half-mapped, broken power-on
    /// state. [`Self::boot_active`] is then `false`.
    pub fn new_with_boot(
        model: Model,
        rom: Vec<u8>,
        boot_rom: Vec<u8>,
    ) -> Result<Self, CartridgeError> {
        let cart = cartridge::Cartridge::from_bytes(rom)?;
        let expected = if model.is_cgb() { 0x900 } else { 0x100 };
        if boot_rom.len() != expected {
            // Wrong size for the model: never produce a broken half-mapped
            // machine — install the post-boot state directly, as `new` does.
            return Ok(Self::post_boot(model, cart));
        }
        let mut bus = interconnect::Interconnect::new(model, cart);
        // Deliberately NOT apply_post_boot_state: the bus stays at its power-on
        // constructor state (LCD off, DIV 0, …) and the boot ROM brings it up.
        bus.attach_boot_rom(boot_rom);
        let cpu = cpu::Cpu::power_on();
        let sgb_apu = build_sgb_apu(model);
        Ok(Self { cpu, bus, sgb_apu })
    }

    /// Pick the best model for a ROM from its CGB-support header flag
    /// (CGB if the ROM supports or requires it, otherwise DMG). Uses the
    /// same bit-7 predicate as the interconnect's CGB-mode gate
    /// (`cartridge::cgb_flag`), matching what the CGB boot ROM checks.
    pub fn auto_model(rom: &[u8]) -> Model {
        match rom.get(0x143) {
            Some(&flag) if cartridge::cgb_flag(flag) => Model::Cgb,
            _ => Model::Dmg,
        }
    }

    /// True if the raw ROM header unlocks SGB functions: SGB flag (0x146) == 0x03
    /// *and* old licensee code (0x14B) == 0x33 (Pan Docs "SGB flag" — the SGB
    /// ignores command packets otherwise). The frontend's "automatic, prefer SGB"
    /// policy uses this to pick [`Model::Sgb`]; mirrors
    /// [`cartridge::Header::supports_sgb`] but is safe to call on any slice
    /// (`.get()` — a truncated image is simply "no SGB").
    #[must_use]
    pub fn rom_supports_sgb(rom: &[u8]) -> bool {
        rom.get(0x146) == Some(&0x03) && rom.get(0x14B) == Some(&0x33)
    }

    /// Execute one CPU instruction (or one halted/stopped M-cycle).
    pub fn step(&mut self) {
        let before = self.bus.cycles();
        self.cpu.step(&mut self.bus);
        // Advance the SGB audio subsystem by the cycles that instruction spent,
        // and drain any SGB sound commands it produced. Present only on
        // `Model::Sgb`/`Sgb2`, so `Dmg`/`Cgb` runs are byte-identical.
        if let Some(apu) = self.sgb_apu.as_mut() {
            let elapsed = self.bus.cycles().wrapping_sub(before);
            apu.clock(elapsed);
            apu.poll(&mut self.bus);
        }
    }

    /// Run until the next frame is complete (vblank reached), or — with the
    /// LCD off — until an equivalent number of cycles has elapsed. Exactly a
    /// [`Self::run_slice`] bounded to one frame's worth of cycles (see its doc
    /// for the lockstep-yield behavior).
    pub fn run_frame(&mut self) {
        self.run_slice(CYCLES_PER_FRAME);
    }

    /// Run until the frame completes, `max_cycles` elapse, or a connected master
    /// stalls (lockstep) — the frontend's chunked link pump. Running the link in
    /// sub-frame slices lets a slave exchange many bytes per frame while still
    /// advancing a full slice of emulated cycles per byte (so its serial routine
    /// has time to prepare each reply). Golden-safe: with no peer attached
    /// `link_stalled()` is always false, so this is just a cycle-bounded
    /// `run_frame` slice; only the live frontend ever calls it.
    pub fn run_slice(&mut self, max_cycles: u32) {
        debug_assert!(max_cycles > 0, "run_slice(0) makes no progress");
        let target = self.bus.frame_count().wrapping_add(1);
        let deadline = self.bus.cycles().wrapping_add(u64::from(max_cycles));
        while self.bus.frame_count() != target && self.bus.cycles() < deadline {
            self.step();
            if self.bus.link_stalled() {
                break;
            }
        }
    }

    /// Like [`Self::run_frame`], but stop early (returning that address) if `PC`
    /// reaches one of `breakpoints` after a step — the debugger's free-running
    /// auto-halt. Each breakpoint is `(addr, bank)`: `bank` is `None` to halt on
    /// `addr` in any ROM bank (the flat-address default), or `Some(b)` to halt
    /// only when the currently-mapped ROM bank ([`Self::rom_bank`]) is `b` — so a
    /// breakpoint on a `0x4000-0x7FFF` address in one ROM bank doesn't fire while
    /// a different bank is executing there. The PC check is *after* each step,
    /// matching [`Self::run_until_breakpoint`], so a breakpoint on the current
    /// line doesn't fire instantly (the loop always advances off it; a loop back
    /// to the breakpoint still stops). With no breakpoints it is exactly a
    /// `run_frame`. Returns `None` if the frame completed without a hit. This
    /// drives emulation forward; only call it for a live debugger run, never on
    /// a golden/test path.
    pub fn run_frame_until_breakpoint(
        &mut self,
        breakpoints: &[(u16, Option<u16>)],
    ) -> Option<u16> {
        let target = self.bus.frame_count().wrapping_add(1);
        let deadline = self.bus.cycles().wrapping_add(u64::from(CYCLES_PER_FRAME));
        while self.bus.frame_count() != target && self.bus.cycles() < deadline {
            self.step();
            // A memory watchpoint hit during the step halts here (RM8); the
            // returned address is the watched location. Always `None` when no
            // watchpoint is set, so this is inert on a plain run.
            if let Some(addr) = self.bus.take_watch_hit() {
                return Some(addr);
            }
            // Profiler break mode: halt on an address's first execution (MB5).
            // Always `None` unless break mode is armed, so this is inert
            // otherwise.
            if let Some(addr) = self.bus.take_prof_break_hit() {
                return Some(addr);
            }
            // Exception break (Options → Exceptions): halt on an armed
            // opcode/access condition. Always `None` with no exception armed
            // (`exc_mask == 0`), so this is inert on a plain run.
            if let Some(addr) = self.bus.take_exc_hit() {
                return Some(addr);
            }
            let pc = self.cpu_regs().pc;
            if breakpoints
                .iter()
                .any(|&(a, bank)| a == pc && bank.is_none_or(|b| self.rom_bank() as u16 == b))
            {
                return Some(pc);
            }
            // Lockstep serial stall: yield to the frontend pump (golden-safe —
            // always false when no link peer is attached).
            if self.bus.link_stalled() {
                return None;
            }
        }
        None
    }

    // The debugger/introspection accessors (watchpoints, exception-break mask,
    // save-states, execution profiler, CDL, WRAM/VRAM bank indicators) live in
    // the `lib_debug` submodule (a second `impl GameBoy`), keeping this file
    // under the 1000-line cap.

    /// XRGB8888 pixels of the most recently completed frame, row-major.
    pub fn frame(&self) -> &[u32; SCREEN_PIXELS] {
        self.bus.ppu().frame()
    }

    // The SGB border + SNES-side command seams + the audio BIOS loader live in
    // the `sgb_api` submodule (a second `impl GameBoy`), keeping this file under
    // the 1000-line cap.

    /// Count of completed frames since power-on.
    pub fn frame_count(&self) -> u64 {
        self.bus.frame_count()
    }

    /// Total elapsed T-cycles since power-on.
    pub fn cycles(&self) -> u64 {
        self.bus.cycles()
    }

    /// Press a joypad button (held until [`Self::release`]).
    pub fn press(&mut self, b: Button) {
        self.bus.joypad_mut().press(b);
    }

    /// Release a previously pressed joypad button.
    pub fn release(&mut self, b: Button) {
        self.bus.joypad_mut().release(b);
    }

    /// Whether joypad button `b` is currently held (read-only; debugger/UI —
    /// side-effect-free, never on a golden path).
    #[must_use]
    pub fn debug_button(&self, b: Button) -> bool {
        self.bus.joypad().pressed(b)
    }

    /// Move all pending stereo samples (interleaved L/R, `CLOCK_HZ / 64`-ish
    /// native rate decided by the APU) into `out`.
    pub fn drain_audio(&mut self, out: &mut Vec<(f32, f32)>) {
        let start = out.len();
        self.bus.apu_mut().drain_samples(out);
        // On SGB, mix the SNES-side stream into the GB samples just drained
        // (both emit at the same output rate off the same clock, so they align
        // sample-for-sample). Inert / absent off `Model::Sgb`/`Sgb2`.
        if let Some(apu) = self.sgb_apu.as_mut() {
            apu.mix_into(&mut out[start..]);
        }
    }

    /// Set the audio output sample rate in Hz (default
    /// [`DEFAULT_SAMPLE_RATE`]).
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.bus.apu_mut().set_sample_rate(hz);
        if let Some(apu) = self.sgb_apu.as_mut() {
            apu.set_output_rate(hz);
        }
    }

    /// Mute or un-mute one APU channel (1-4) in the mixer — a frontend/
    /// debugger control (bgb's "Sound channel" submenu), *not* hardware.
    /// The mask defaults to all-audible and is never touched on any golden/
    /// test path, so it cannot perturb golden output. Channels outside
    /// 1..=4 are ignored.
    pub fn set_channel_mute(&mut self, channel: u8, muted: bool) {
        self.bus.apu_mut().set_channel_mute(channel, muted);
    }

    /// Whether APU channel `channel` (1-4) is currently muted by
    /// [`Self::set_channel_mute`]. Out-of-range channels read `false`.
    #[must_use]
    pub fn channel_muted(&self, channel: u8) -> bool {
        self.bus.apu().channel_muted(channel)
    }

    /// Debugger register write (bgb's registers-pane "edit register"). A live-
    /// debugger-only `&mut` path — never invoked on a golden/test run, so the
    /// golden gate is untouched (same caveat as [`Self::run_until_breakpoint`]).
    /// Writing `Af` masks the F register's low nibble, which does not exist in
    /// hardware.
    pub fn debug_set_reg(&mut self, reg: DebugReg, value: u16) {
        let r = self.cpu.regs_mut();
        match reg {
            DebugReg::Af => r.set_af(value),
            DebugReg::Bc => r.set_bc(value),
            DebugReg::De => r.set_de(value),
            DebugReg::Hl => r.set_hl(value),
            DebugReg::Sp => r.sp = value,
            DebugReg::Pc => r.pc = value,
        }
    }

    /// Set PC (bgb's "Jump to cursor"): redirect execution without running.
    /// Live-debugger-only `&mut`, golden-safe (see [`Self::debug_set_reg`]).
    pub fn debug_set_pc(&mut self, pc: u16) {
        self.cpu.regs_mut().pc = pc;
    }

    /// bgb's "Call cursor": push the current PC (little-endian) onto the stack
    /// and jump to `target`, exactly like a `CALL` — so a later `RET` returns
    /// to where execution was. Live-debugger-only `&mut`, golden-safe.
    pub fn debug_call(&mut self, target: u16) {
        let pc = self.cpu.regs().pc;
        let sp = self.cpu.regs().sp.wrapping_sub(2);
        let [lo, hi] = pc.to_le_bytes();
        self.bus.debug_write(sp, lo);
        self.bus.debug_write(sp.wrapping_add(1), hi);
        let r = self.cpu.regs_mut();
        r.sp = sp;
        r.pc = target;
    }

    /// Write a byte time-free for the bgb-style memory-viewer edit and the
    /// freeze list — the write counterpart of [`Self::debug_read`]. Routes
    /// through `write_no_tick`, so it follows live banking and honors the same
    /// PPU/MBC access rules a CPU store would (VRAM/OAM dropped during a locked
    /// PPU mode; cart-RAM ignored while disabled). Live-debugger-only `&mut`,
    /// never invoked on a golden/test run, so the golden gate is untouched
    /// (same caveat as [`Self::debug_set_reg`]).
    pub fn debug_write(&mut self, addr: u16, value: u8) {
        self.bus.debug_write(addr, value);
    }

    /// Map the four DMG shades to XRGB8888 colors (ignored on CGB models).
    pub fn set_dmg_palette(&mut self, palette: [u32; 4]) {
        self.bus.ppu_mut().set_dmg_palette(palette);
    }

    /// The cartridge ROM bank currently mapped at 0x4000-0x7FFF, for the debug
    /// bank indicator (distinct from the VRAM/WRAM banks at FF4F/FF70).
    /// Side-effect-free.
    #[must_use]
    pub fn rom_bank(&self) -> usize {
        self.bus.cartridge().cur_rom_bank()
    }

    /// Set the Game Genie ROM-patch cheats (frontend cheat engine). Empty (the
    /// default / no cheats) leaves ROM reads byte-identical — a default-off
    /// mutating debug hook, never set on a golden/test path.
    pub fn set_gg_patches(&mut self, patches: Vec<GgPatch>) {
        self.bus.cartridge_mut().set_gg_patches(patches);
    }

    /// The external-RAM bank currently visible at 0xA000, or `None` when RAM is
    /// disabled/absent (or an RTC register is mapped), for the debug bank
    /// indicator. Side-effect-free.
    #[must_use]
    pub fn ram_bank(&self) -> Option<usize> {
        self.bus.cartridge().cur_ram_bank()
    }

    /// Battery-backed cartridge RAM (plus RTC state for MBC3), if any.
    pub fn save_data(&self) -> Option<Vec<u8>> {
        self.bus.cartridge().save_data()
    }

    /// Restore battery-backed RAM previously obtained from [`Self::save_data`].
    /// Returns false if the image was rejected (wrong size / no battery).
    pub fn load_save_data(&mut self, data: &[u8]) -> bool {
        self.bus.cartridge_mut().load_save_data(data)
    }

    /// True once the CPU has executed `LD B,B` (opcode 0x40) — the mooneye
    /// test suite's "test finished" software breakpoint.
    pub fn debug_breakpoint_hit(&self) -> bool {
        self.cpu.debug_breakpoint_hit()
    }

    /// CPU register snapshot, for test harnesses.
    pub fn cpu_regs(&self) -> Registers {
        self.cpu.regs()
    }

    /// Whether the opt-in boot ROM is currently mapped (false unless built via
    /// [`Self::new_with_boot`] and before the boot ROM writes FF50). Test/UI hook.
    #[must_use]
    pub fn boot_active(&self) -> bool {
        self.bus.boot_active()
    }

    /// Interrupt master enable (the debugger's `ime`). Pair with
    /// [`Self::ime_pending`] for the post-`EI` one-instruction delay.
    #[must_use]
    pub fn ime(&self) -> bool {
        self.cpu.ime()
    }

    /// True when `EI` has run but its IME-enable is still one instruction away.
    #[must_use]
    pub fn ime_pending(&self) -> bool {
        self.cpu.ime_pending()
    }

    /// CGB double-speed mode (KEY1 bit 7) — the debugger's `spd`.
    #[must_use]
    pub fn double_speed(&self) -> bool {
        self.bus.double_speed()
    }

    /// The hardware model this machine was built as.
    pub fn model(&self) -> Model {
        self.bus.model()
    }

    // ---- test-harness escape hatches (not public API) ----

    /// Drain the bytes "printed" over the link port: every completed
    /// internal-clock serial transfer (SB <- byte, then $81 to SC — the
    /// blargg test-ROM protocol) appends the byte that was shifted out.
    /// The undrained buffer is capped at 64 KiB.
    #[doc(hidden)]
    pub fn take_serial_output(&mut self) -> Vec<u8> {
        self.bus.take_serial_output()
    }

    // ---- Link cable (frontend TCP peer; all golden-safe / inert when off) --
    //
    // The frontend drives a serial link over a socket: attach a peer, push
    // bytes it receives, drain bytes to send, and complete slave transfers.
    // Every method is a no-op / `None` when no peer is attached, so emulation
    // is byte-identical on every path that never calls `link_connect`.

    /// Attach (`true`) or detach (`false`) a serial link peer. Detaching
    /// clears any pending link bytes.
    pub fn link_connect(&mut self, on: bool) {
        self.bus.link_set_connected(on);
    }

    /// Whether a link peer is attached.
    #[must_use]
    pub fn link_connected(&self) -> bool {
        self.bus.link_connected()
    }

    /// Whether a connected internal-clock (master) transfer is paused at
    /// completion awaiting the peer's byte (lockstep stall). [`Self::run_frame`]
    /// returns early in this state so the frontend can pump the link and deliver
    /// the byte ([`Self::link_push_recv`]). Always false when disconnected, so
    /// it is golden-safe.
    #[must_use]
    pub fn link_stalled(&self) -> bool {
        self.bus.link_stalled()
    }

    /// Provide the peer byte the next internal-clock (master) transfer shifts
    /// in (MSB-first). Overwrites a byte not yet consumed.
    pub fn link_push_recv(&mut self, byte: u8) {
        self.bus.link_push_recv(byte);
    }

    /// Drain the byte a completed master transfer shifted out, for the
    /// frontend to send to the peer. `None` until a transfer completes while
    /// connected.
    #[must_use]
    pub fn link_take_send(&mut self) -> Option<u8> {
        self.bus.link_take_send()
    }

    /// Complete a pending external-clock (slave) transfer with the peer's
    /// (master's) byte: raises the serial interrupt and returns the slave's
    /// outgoing byte. `None` when no slave transfer is armed (a no-op).
    pub fn link_slave_transfer(&mut self, master_byte: u8) -> Option<u8> {
        self.bus.link_slave_transfer(master_byte)
    }

    /// Side-effect-free memory peek: no M-cycle passes and nothing is
    /// mutated (`&self`). Follows live ROM/VRAM/cart-RAM/WRAM banking and
    /// intentionally ignores PPU VRAM/OAM access blocking; IO registers
    /// (FF00-FF7F) are not peekable and read $FF (see
    /// `Interconnect::peek_no_io`).
    #[doc(hidden)]
    pub fn peek_no_io(&self, addr: u16) -> u8 {
        self.bus.peek_no_io(addr)
    }

    /// Read for the bgb-style debugger views: like [`Self::peek_no_io`] but
    /// resolves the IO registers (FF00-FF7F) to their live hardware values
    /// instead of the `$FF` `peek_no_io` returns. Side-effect-free; the value
    /// is what the CPU
    /// would read at this instant. Use this for the memory dump and I/O map.
    #[must_use]
    pub fn debug_read(&self, addr: u16) -> u8 {
        self.bus.debug_read(addr)
    }

    /// Like [`Self::debug_read`] but reads a specific **bank** of the banked
    /// regions (ROMX `0x4000-0x7FFF`, VRAM `0x8000-0x9FFF`, SRAM `0xA000-0xBFFF`,
    /// WRAMX `0xD000-0xDFFF`) rather than the live one — so the MCP/debug tools
    /// can dump an arbitrary bank. Outside those regions `bank` is ignored
    /// (== [`Self::debug_read`]). Read-only, golden-safe.
    #[must_use]
    pub fn debug_read_banked(&self, bank: u16, addr: u16) -> u8 {
        self.bus.debug_read_banked(bank, addr)
    }

    /// Like [`Self::debug_write`] but pokes a specific **bank** of the banked
    /// regions (VRAM/SRAM/WRAMX), so the memory viewer's bank browser edits the
    /// bank it is showing. Other regions ignore `bank` (== [`Self::debug_write`];
    /// ROM areas still poke the mapper). Debug-only; never on a golden path.
    pub fn debug_write_banked(&mut self, bank: u16, addr: u16, value: u8) {
        self.bus.debug_write_banked(bank, addr, value);
    }

    /// Selectable bank count for the region containing `base` (1 when
    /// fixed/unbanked), for the memory viewer's bank stepper. Read-only.
    #[must_use]
    pub fn region_bank_count(&self, base: u16) -> u16 {
        self.bus.region_bank_count(base)
    }

    /// The top `n` 16-bit words of the stack as `(address, word)` pairs,
    /// descending from `SP` like bgb's stack pane: `(SP, [SP]), (SP-2, [SP-2]),
    /// …`. Words are little-endian; addresses wrap at `0x0000`.
    #[must_use]
    pub fn stack(&self, n: usize) -> Vec<(u16, u16)> {
        let sp = self.cpu_regs().sp;
        (0..n)
            .map(|i| {
                let addr = sp.wrapping_sub((2 * i) as u16);
                let lo = self.bus.debug_read(addr);
                let hi = self.bus.debug_read(addr.wrapping_add(1));
                (addr, u16::from(lo) | (u16::from(hi) << 8))
            })
            .collect()
    }

    /// Whole 16 KiB VRAM for the debug VRAM viewer: CGB bank 0 is `[..0x2000]`,
    /// bank 1 is `[0x2000..]` (DMG fills only bank 0). Decode tiles/maps with
    /// [`debug::tile_pixels`]. Side-effect-free.
    #[must_use]
    pub fn vram(&self) -> &[u8; 0x4000] {
        self.bus.ppu().debug_vram()
    }

    /// Raw 160-byte OAM (40 sprites × 4 bytes). Decode with
    /// [`debug::oam_sprites`].
    #[must_use]
    pub fn oam(&self) -> &[u8; 0xA0] {
        self.bus.ppu().debug_oam()
    }

    /// Raw 16 stored wave-RAM bytes (FF30-FF3F), for the debug I/O viewer's
    /// wave panel. Bypasses the CPU read gating of [`Self::debug_read`] (which
    /// returns 0xFF / the volatile current sample byte while channel 3 plays),
    /// so the panel shows a stable view. Side-effect-free.
    #[must_use]
    pub fn wave_ram(&self) -> [u8; 16] {
        self.bus.apu().wave_ram()
    }

    /// Raw CGB palette RAM `(BG, OBJ)`, 64 bytes each (8 palettes × 4 colors ×
    /// 2 bytes, little-endian 15-bit BGR555). DMG palettes are BGP/OBP/OBP1,
    /// readable via [`Self::debug_read`] at FF47/FF48/FF49.
    #[must_use]
    pub fn cgb_palette_ram(&self) -> (&[u8; 64], &[u8; 64]) {
        self.bus.ppu().debug_palette_ram()
    }

    /// Run instructions until `PC` matches one of `breakpoints` (returns that
    /// address) or `max_instructions` have executed (returns `None`) — the
    /// debugger's "run" / "run to cursor". The check is *after* each step, so a
    /// breakpoint on the current `PC` doesn't stop instantly; "run" always
    /// advances off the current line and a loop back to a breakpoint still
    /// stops. This drives emulation forward; only call it for a debugger run,
    /// never on a golden/test path.
    pub fn run_until_breakpoint(
        &mut self,
        breakpoints: &[u16],
        max_instructions: u64,
    ) -> Option<u16> {
        for _ in 0..max_instructions {
            self.step();
            let pc = self.cpu_regs().pc;
            if breakpoints.contains(&pc) {
                return Some(pc);
            }
        }
        None
    }

    /// True once the CPU has executed an undefined opcode (0xD3, 0xDB,
    /// 0xDD, 0xE3, 0xE4, 0xEB, 0xEC, 0xED, 0xF4, 0xFC, 0xFD) and
    /// hard-locked — wilbertpol's mooneye fork ends its tests with 0xED.
    #[doc(hidden)]
    pub fn debug_undefined_hit(&self) -> bool {
        self.cpu.debug_undefined_hit()
    }

    /// Drain the raw audio tap: one stereo sample per dot, taken straight
    /// off the APU channel mixer *before* the box-average resampler and the
    /// high-pass "output capacitor" stage (`Apu::output_cycle`). The
    /// gambatte test harness compares this stream for its `_outaudio`
    /// sample-equality verdicts, which [`Self::drain_audio`]'s filtered
    /// output would distort (a decaying high-pass tail reads as "sound", a
    /// flattened distinct input as "silence"). Capped at two frames of
    /// backlog — drain right before the frame under test.
    #[doc(hidden)]
    pub fn drain_audio_raw(&mut self, out: &mut Vec<(f32, f32)>) {
        self.bus.apu_mut().drain_raw_samples(out);
    }
}

// The SGB accessor block (border, SNES-side command seams, audio BIOS loader)
// as a second `impl GameBoy`, split out to keep this file under the 1000-line
// cap.
#[path = "lib/sgb_api.rs"]
mod sgb_api;

// The debugger/introspection accessor block (watchpoints, exception mask,
// save-states, profiler, CDL, bank indicators) as a second `impl GameBoy`,
// split out to keep this file under the 1000-line cap.
#[path = "lib/debug.rs"]
mod lib_debug;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
