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
pub mod debug;
pub(crate) mod interconnect;
pub(crate) mod joypad;
pub(crate) mod model;
pub(crate) mod ppu;
pub(crate) mod serial;
pub(crate) mod timer;

pub use apu::DEFAULT_SAMPLE_RATE;
pub use cartridge::CartridgeError;
pub use cpu::Registers;
pub use joypad::Button;
pub use model::Model;

// Escape hatch for the crate's integration tests, which drive the CPU and
// interconnect directly (OAM DMA freeze/timing tests). Not public API.
#[doc(hidden)]
pub use cartridge::Cartridge;
#[doc(hidden)]
pub use cpu::{Bus, Cpu};
#[doc(hidden)]
pub use interconnect::Interconnect;

/// Screen width in pixels.
pub const SCREEN_W: usize = 160;
/// Screen height in pixels.
pub const SCREEN_H: usize = 144;
/// Pixels per frame.
pub const SCREEN_PIXELS: usize = SCREEN_W * SCREEN_H;
/// T-cycles (dots) per frame with the LCD on.
pub const CYCLES_PER_FRAME: u32 = 70224;
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
#[derive(Clone)]
pub struct GameBoy {
    cpu: cpu::Cpu,
    bus: interconnect::Interconnect,
}

impl GameBoy {
    /// Build a machine for `model` with the given cartridge ROM image.
    ///
    /// No boot ROM is executed: CPU registers, hardware registers and timers
    /// are initialised to the exact post-boot state of `model`.
    pub fn new(model: Model, rom: Vec<u8>) -> Result<Self, CartridgeError> {
        let cart = cartridge::Cartridge::from_bytes(rom)?;
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
        Ok(Self { cpu, bus })
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

    /// Execute one CPU instruction (or one halted/stopped M-cycle).
    pub fn step(&mut self) {
        self.cpu.step(&mut self.bus);
    }

    /// Run until the next frame is complete (vblank reached), or — with the
    /// LCD off — until an equivalent number of cycles has elapsed.
    pub fn run_frame(&mut self) {
        let target = self.bus.frame_count().wrapping_add(1);
        let deadline = self.bus.cycles().wrapping_add(u64::from(CYCLES_PER_FRAME));
        while self.bus.frame_count() != target && self.bus.cycles() < deadline {
            self.step();
        }
    }

    /// Like [`Self::run_frame`], but stop early (returning that address) if `PC`
    /// reaches one of `breakpoints` after a step — the debugger's free-running
    /// auto-halt. The PC check is *after* each step, matching
    /// [`Self::run_until_breakpoint`], so a breakpoint on the current line
    /// doesn't fire instantly (the loop always advances off it; a loop back to
    /// the breakpoint still stops). With no breakpoints it is exactly a
    /// `run_frame`. Returns `None` if the frame completed without a hit. This
    /// drives emulation forward; only call it for a live debugger run, never on
    /// a golden/test path.
    pub fn run_frame_until_breakpoint(&mut self, breakpoints: &[u16]) -> Option<u16> {
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
            if breakpoints.contains(&pc) {
                return Some(pc);
            }
        }
        None
    }

    /// Set (replacing any previous) the debugger memory watchpoints the free run
    /// halts on (bgb's "Set watchpoint"). A live-debugger-only control — the list
    /// defaults empty and is never set on a golden/test path, so it is
    /// golden-safe (an empty list is a zero-overhead no-op in the access path).
    pub fn set_watchpoints(&mut self, wps: &[Watchpoint]) {
        self.bus.set_watchpoints(wps);
    }

    /// Set the debugger exception-break mask (bgb's Options → Exceptions): the
    /// free run halts when an armed `EXC_*` condition occurs. `0` (the default)
    /// disarms every check, so it is golden-safe (never set on a golden/test
    /// path; an unset mask is a zero-overhead no-op in the exec/access paths).
    pub fn set_exceptions(&mut self, mask: u16) {
        self.bus.set_exceptions(mask);
    }

    /// The current exception-break mask (`0` when no exception is armed).
    #[must_use]
    pub fn exceptions(&self) -> u16 {
        self.bus.exceptions()
    }

    /// Enable/disable the execution profiler (bgb's "logging mode"/"stop"): a
    /// per-PC instruction tally. Off by default and never set on a golden/test
    /// path, so it is golden-safe (an unset tally is a zero-overhead no-op in
    /// the CPU fetch path).
    pub fn set_profiling(&mut self, on: bool) {
        self.bus.set_profiling(on);
    }

    /// Zero the profiler tally without disabling logging (bgb's "clear buffer").
    pub fn clear_profile(&mut self) {
        self.bus.clear_profile();
    }

    /// Arm/disarm profiler "break mode": the free run halts the first time each
    /// address executes (bgb's coverage break). Only meaningful with profiling
    /// on; live-debugger-only, golden-safe.
    pub fn set_profile_break(&mut self, on: bool) {
        self.bus.set_profile_break(on);
    }

    /// Whether profiler break mode is armed.
    #[must_use]
    pub fn profile_break(&self) -> bool {
        self.bus.profile_break()
    }

    /// Whether the execution profiler is currently logging.
    #[must_use]
    pub fn profiling(&self) -> bool {
        self.bus.profiling()
    }

    /// Times the instruction at `pc` has executed since the last clear (0 if
    /// unseen or profiling is off).
    #[must_use]
    pub fn profile_count(&self, pc: u16) -> u64 {
        self.bus.profile_count(pc)
    }

    /// Distinct instruction addresses the profiler has seen since the last clear
    /// (bgb's "N addresses seen").
    #[must_use]
    pub fn profile_seen(&self) -> usize {
        self.bus.profile_seen()
    }

    /// XRGB8888 pixels of the most recently completed frame, row-major.
    pub fn frame(&self) -> &[u32; SCREEN_PIXELS] {
        self.bus.ppu().frame()
    }

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
        self.bus.apu_mut().drain_samples(out);
    }

    /// Set the audio output sample rate in Hz (default
    /// [`DEFAULT_SAMPLE_RATE`]).
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.bus.apu_mut().set_sample_rate(hz);
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

    /// Map the four DMG shades to XRGB8888 colors (ignored on CGB models).
    pub fn set_dmg_palette(&mut self, palette: [u32; 4]) {
        self.bus.ppu_mut().set_dmg_palette(palette);
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

    /// Side-effect-free memory peek: no M-cycle passes and nothing is
    /// mutated (`&self`). Follows live ROM/VRAM/cart-RAM/WRAM banking and
    /// intentionally ignores PPU VRAM/OAM access blocking; IO registers
    /// (FF00-FF7F) are not peekable and read $FF (see
    /// `Interconnect::peek`).
    #[doc(hidden)]
    pub fn peek(&self, addr: u16) -> u8 {
        self.bus.peek(addr)
    }

    /// Read for the bgb-style debugger views: like [`Self::peek`] but resolves
    /// the IO registers (FF00-FF7F) to their live hardware values instead of
    /// the `$FF` `peek` returns. Side-effect-free; the value is what the CPU
    /// would read at this instant. Use this for the memory dump and I/O map.
    #[must_use]
    pub fn debug_read(&self, addr: u16) -> u8 {
        self.bus.debug_read(addr)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with_cgb_flag(flag: u8) -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x143] = flag;
        rom
    }

    /// Pan Docs "CPU registers" (Power-Up Sequence): on CGB/AGB hardware
    /// the boot ROM hands a CGB-flagged cart off with DE=$FF56 HL=$000D;
    /// a DMG cart gets DE=$0008 HL=$007C (mooneye misc/boot_regs-cgb/-A —
    /// every mooneye ROM is DMG-flagged). A/F/B/C are cart-independent:
    /// AGB's extra `inc b` gives B=$01/F=$00 for both cart kinds.
    #[test]
    fn cgb_flagged_cart_boot_regs() {
        for (model, af, bc) in [(Model::Cgb, 0x1180, 0x0000), (Model::Agb, 0x1100, 0x0100)] {
            let gb = GameBoy::new(model, rom_with_cgb_flag(0x80)).unwrap();
            let r = gb.cpu_regs();
            assert_eq!(r.af(), af, "{model:?} CGB cart AF");
            assert_eq!(r.bc(), bc, "{model:?} CGB cart BC");
            assert_eq!(r.de(), 0xFF56, "{model:?} CGB cart DE");
            assert_eq!(r.hl(), 0x000D, "{model:?} CGB cart HL");

            let gb = GameBoy::new(model, rom_with_cgb_flag(0x00)).unwrap();
            let r = gb.cpu_regs();
            assert_eq!(r.af(), af, "{model:?} DMG cart AF");
            assert_eq!(r.bc(), bc, "{model:?} DMG cart BC");
            assert_eq!(r.de(), 0x0008, "{model:?} DMG cart DE");
            assert_eq!(r.hl(), 0x007C, "{model:?} DMG cart HL");
        }
    }

    /// A ROM that writes 0x42 to 0xC000 then self-loops:
    /// `ld a,42 ; ld (C000),a ; jr -2`.
    fn write_c000_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0100..0x0107].copy_from_slice(&[0x3E, 0x42, 0xEA, 0x00, 0xC0, 0x18, 0xFE]);
        rom
    }

    #[test]
    fn watchpoint_halts_the_free_run_on_a_matching_access() {
        let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
        gb.set_watchpoints(&[Watchpoint {
            addr: 0xC000,
            read: false,
            write: true,
        }]);
        // The write to 0xC000 halts the frame at that address.
        assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0xC000));
    }

    #[test]
    fn watchpoint_kind_and_emptiness_are_respected() {
        // A read-only watchpoint at 0xC000 does NOT fire on the write.
        let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
        gb.set_watchpoints(&[Watchpoint {
            addr: 0xC000,
            read: true,
            write: false,
        }]);
        assert_eq!(
            gb.run_frame_until_breakpoint(&[]),
            None,
            "a read watchpoint ignores the write"
        );
        // Golden-safety: with no watchpoints set, the frame runs to completion.
        let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
        assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
    }

    #[test]
    fn profiler_tallies_executed_instruction_addresses() {
        // The execution profiler (MB5): an opt-in per-PC instruction tally that
        // is inert (no map) until enabled, so it never perturbs a golden run.
        let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
        assert!(!gb.profiling(), "off by default");
        assert_eq!(gb.profile_seen(), 0);
        assert_eq!(gb.profile_count(0x0100), 0);

        gb.set_profiling(true);
        assert!(gb.profiling());
        // ld a,42 @0100 ; ld (C000),a @0102 ; jr -2 @0105 (then self-loops).
        gb.step();
        gb.step();
        gb.step();
        assert_eq!(gb.profile_count(0x0100), 1, "ld a,42 executed once");
        assert_eq!(gb.profile_count(0x0102), 1, "ld (C000),a executed once");
        assert_eq!(gb.profile_count(0x0105), 1, "jr executed once");
        assert_eq!(gb.profile_seen(), 3, "three distinct addresses seen");
        gb.step(); // the jr self-loops back to 0x0105
        assert_eq!(gb.profile_count(0x0105), 2);
        assert_eq!(
            gb.profile_seen(),
            3,
            "seen counts distinct addresses, not hits"
        );

        // "clear buffer" keeps logging on but zeroes the counts.
        gb.clear_profile();
        assert!(gb.profiling());
        assert_eq!(gb.profile_seen(), 0);
        assert_eq!(gb.profile_count(0x0105), 0);

        // Disabling drops the tally; stepping no longer records anything.
        gb.set_profiling(false);
        assert!(!gb.profiling());
        gb.step();
        assert_eq!(gb.profile_seen(), 0, "no tally while profiling is off");
    }

    #[test]
    fn profiler_break_mode_halts_on_first_execution() {
        // bgb's coverage break: the free run stops the first time each address
        // executes, then continues past it.
        let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
        gb.set_profiling(true);
        gb.set_profile_break(true);
        assert!(gb.profile_break());
        // 0100 (ld a), 0102 (ld (C000),a), 0105 (jr) each halt once on first run.
        assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0100));
        assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0102));
        assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0105));
        // The jr self-loops over only already-seen addresses → no more halts.
        assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
        // Disabling break mode keeps logging: no halts, but the tally still grows.
        let before = gb.profile_count(0x0105);
        gb.set_profile_break(false);
        assert!(!gb.profile_break());
        assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
        assert!(gb.profile_count(0x0105) > before);
    }

    /// A 32 KiB ROM with `bytes` placed at the entry point (0x0100).
    fn exc_rom(bytes: &[u8]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0100..0x0100 + bytes.len()].copy_from_slice(bytes);
        rom
    }

    #[test]
    fn exception_break_defaults_inert() {
        // Options → Exceptions: nothing armed by default ⇒ the free run never
        // halts on these conditions (golden-safe — the mask is 0 on every
        // golden/test path).
        let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0x40, 0x18, 0xFE])).unwrap();
        assert_eq!(gb.exceptions(), 0, "no exception armed by default");
        assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
    }

    #[test]
    fn exception_break_on_ld_b_b() {
        // ld b,b (40h) ; jr -2 — halts at the ld b,b when armed.
        let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0x40, 0x18, 0xFE])).unwrap();
        gb.set_exceptions(EXC_LD_B_B);
        assert_eq!(gb.exceptions(), EXC_LD_B_B);
        assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0100));
        // The invalid-opcode mask does NOT fire on a (legal) ld b,b.
        let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0x40, 0x18, 0xFE])).unwrap();
        gb.set_exceptions(EXC_INVALID_OPCODE);
        assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
    }

    #[test]
    fn exception_break_on_invalid_opcode() {
        // 0xDD is one of the 11 undefined SM83 opcodes (the CPU hard-locks).
        let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0xDD])).unwrap();
        gb.set_exceptions(EXC_INVALID_OPCODE);
        assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0x0100));
        // The ld-b,b mask does NOT fire on an invalid opcode.
        let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0xDD])).unwrap();
        gb.set_exceptions(EXC_LD_B_B);
        assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
    }

    #[test]
    fn exception_break_on_echo_ram_access() {
        // ld a,(E000) ; jr -5 — a CPU read of echo RAM (E000-FDFF) halts.
        let mut gb = GameBoy::new(Model::Dmg, exc_rom(&[0xFA, 0x00, 0xE0, 0x18, 0xFB])).unwrap();
        gb.set_exceptions(EXC_ECHO_RAM);
        assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0xE000));
        // A work-RAM (C000) access is NOT echo RAM → no halt.
        let mut gb = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
        gb.set_exceptions(EXC_ECHO_RAM);
        assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
    }

    #[test]
    fn exception_break_on_lcd_off_outside_vblank() {
        // 16 NOPs (the DMG boot hands off mid-vblank at LY 0; the PPU leaves
        // mode 1 a few M-cycles in) then: xor a ; ldh (40),a ; ldh (40),a ;
        // jr -2 — two writes of FF40←0 well outside vblank.
        let mut prog = vec![0x00u8; 16];
        prog.extend_from_slice(&[0xAF, 0xE0, 0x40, 0xE0, 0x40, 0x18, 0xFE]);
        let rom = exc_rom(&prog);
        // Armed from boot: the LCD is on (LCDC=0x91) and the first FF40←0 write
        // lands outside vblank, so it halts.
        let mut gb = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
        gb.set_exceptions(EXC_LCD_OFF_VBLANK);
        assert_eq!(gb.run_frame_until_breakpoint(&[]), Some(0xFF40));
        // Already off: step the NOPs + xor + first write disarmed (LCD now off),
        // then arm — the second FF40←0 write must NOT halt (LCD already off).
        let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
        for _ in 0..18 {
            gb.step(); // 16 NOPs, xor a, first ldh (40),a -> LCD off
        }
        gb.set_exceptions(EXC_LCD_OFF_VBLANK);
        assert_eq!(gb.run_frame_until_breakpoint(&[]), None);
    }

    #[test]
    fn clone_is_an_independent_machine_snapshot() {
        // The Quick Save/Load primitive (MN6): GameBoy: Clone must be a deep,
        // independent copy — advancing one must not touch the other.
        let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
        gb.run_frame();
        let snap = gb.clone();
        let (pc0, cyc0) = (snap.cpu_regs().pc, snap.cycles());
        for _ in 0..10 {
            gb.run_frame();
        }
        assert_ne!(gb.cycles(), cyc0, "original advanced");
        assert_eq!(snap.cycles(), cyc0, "clone is frozen at the snapshot");
        assert_eq!(
            snap.cpu_regs().pc,
            pc0,
            "clone PC unchanged by the original"
        );
        // Restoring rewinds the machine exactly to the snapshot.
        let restored = snap.clone();
        assert_eq!(restored.cycles(), cyc0);
        assert_eq!(restored.cpu_regs().pc, pc0);
    }

    #[test]
    fn debug_set_reg_writes_each_register_pair() {
        let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
        gb.debug_set_reg(DebugReg::Af, 0x12FF); // F low nibble must mask to 0
        gb.debug_set_reg(DebugReg::Bc, 0x1234);
        gb.debug_set_reg(DebugReg::De, 0x5678);
        gb.debug_set_reg(DebugReg::Hl, 0x9ABC);
        gb.debug_set_reg(DebugReg::Sp, 0xD000);
        gb.debug_set_reg(DebugReg::Pc, 0x0150);
        let r = gb.cpu_regs();
        assert_eq!(r.af(), 0x12F0, "AF written, F low nibble masked");
        assert_eq!(r.bc(), 0x1234);
        assert_eq!(r.de(), 0x5678);
        assert_eq!(r.hl(), 0x9ABC);
        assert_eq!(r.sp, 0xD000);
        assert_eq!(r.pc, 0x0150);
    }

    #[test]
    fn debug_call_pushes_return_addr_and_jumps() {
        // bgb "Call cursor": push the current PC (little-endian) and set
        // PC=target, so a later RET returns to where execution was.
        let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
        gb.debug_set_reg(DebugReg::Sp, 0xD000);
        gb.debug_set_reg(DebugReg::Pc, 0x1234);
        gb.debug_call(0x4000);
        let r = gb.cpu_regs();
        assert_eq!(r.sp, 0xCFFE, "SP descended by 2");
        assert_eq!(r.pc, 0x4000, "PC jumped to the target");
        assert_eq!(gb.debug_read(0xCFFE), 0x34, "return low byte");
        assert_eq!(gb.debug_read(0xCFFF), 0x12, "return high byte");
    }

    #[test]
    fn channel_mute_round_trips_and_defaults_off() {
        let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
        for ch in 1..=4 {
            assert!(!gb.channel_muted(ch), "ch{ch} audible at power-on");
        }
        gb.set_channel_mute(3, true);
        assert!(gb.channel_muted(3));
        assert!(!gb.channel_muted(2), "only ch3 muted");
        gb.set_channel_mute(3, false);
        assert!(!gb.channel_muted(3));
        // Out-of-range channels are ignored (no panic).
        gb.set_channel_mute(0, true);
        gb.set_channel_mute(9, true);
        assert!(!gb.channel_muted(0) && !gb.channel_muted(9));
    }

    #[test]
    fn debug_read_resolves_io_but_peek_does_not() {
        let gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
        // peek keeps IO out of band ($FF); debug_read returns the live value.
        // Post-boot LY is a valid scanline (0..=153), so it can't be the $FF
        // peek hands back — proving debug_read took the io_read path.
        assert_eq!(gb.peek(0xFF44), 0xFF, "peek must not read IO");
        assert!(
            gb.debug_read(0xFF44) <= 153,
            "debug_read should give live LY"
        );
        // Outside IO, debug_read is identical to peek (and to ROM contents).
        assert_eq!(gb.debug_read(0x0143), gb.peek(0x0143));
        assert_eq!(gb.debug_read(0x0143), 0x00); // the CGB flag we wrote
        for addr in [0x0000u16, 0x4000, 0xC000, 0xFF80, 0xFFFF] {
            assert_eq!(gb.debug_read(addr), gb.peek(addr), "non-IO {addr:#06x}");
        }
    }

    #[test]
    fn stack_descends_from_sp_little_endian() {
        let gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
        let sp = gb.cpu_regs().sp;
        let s = gb.stack(3);
        assert_eq!(s.len(), 3);
        // Addresses descend by two from SP (bgb's stack pane order).
        assert_eq!(s[0].0, sp);
        assert_eq!(s[1].0, sp.wrapping_sub(2));
        assert_eq!(s[2].0, sp.wrapping_sub(4));
        // Each word is the little-endian pair at its address.
        for &(addr, word) in &s {
            let want = u16::from(gb.debug_read(addr))
                | (u16::from(gb.debug_read(addr.wrapping_add(1))) << 8);
            assert_eq!(word, want, "word @ {addr:#06x}");
        }
    }

    /// A ROM whose entry (`0x100`) is `nop; jp 0x150` and `0x150..` is nops,
    /// so PC walks 0x100 -> 0x101 -> 0x150 -> 0x151 -> 0x152 … deterministically.
    fn linear_code_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x100] = 0x00; // nop
        rom[0x101..0x104].copy_from_slice(&[0xC3, 0x50, 0x01]); // jp 0150
        // 0x150.. already 0x00 (nop) from the zero-fill.
        rom
    }

    #[test]
    fn run_until_breakpoint_stops_at_the_address() {
        let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        assert_eq!(gb.cpu_regs().pc, 0x100);
        // 0x100 nop -> 0x101 jp -> 0x150 nop -> 0x151. bp at 0x151.
        assert_eq!(gb.run_until_breakpoint(&[0x151], 100), Some(0x151));
        assert_eq!(gb.cpu_regs().pc, 0x151);
    }

    #[test]
    fn run_until_breakpoint_respects_the_step_limit() {
        let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        // No reachable breakpoint -> runs the cap, returns None.
        assert_eq!(gb.run_until_breakpoint(&[0xBEEF], 5), None);
        assert_eq!(gb.run_until_breakpoint(&[], 3), None);
    }

    #[test]
    fn run_until_breakpoint_advances_off_the_current_pc() {
        let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        // A breakpoint on the *current* PC must not stop instantly — one step
        // moves to 0x101, which isn't the (already-left) 0x100.
        assert_eq!(gb.run_until_breakpoint(&[0x100], 1), None);
        assert_eq!(gb.cpu_regs().pc, 0x101);
    }

    #[test]
    fn run_frame_until_breakpoint_halts_at_a_breakpoint_mid_frame() {
        let mut gb = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        assert_eq!(gb.cpu_regs().pc, 0x100);
        let frames_before = gb.frame_count();
        // 0x100 nop -> 0x101 jp -> 0x150 nop -> 0x151: stops within a handful of
        // cycles, far short of a full frame's worth of dots.
        assert_eq!(gb.run_frame_until_breakpoint(&[0x151]), Some(0x151));
        assert_eq!(gb.cpu_regs().pc, 0x151);
        assert_eq!(
            gb.frame_count(),
            frames_before,
            "halted before the frame completed"
        );
    }

    #[test]
    fn run_frame_until_breakpoint_with_no_hit_completes_a_frame_like_run_frame() {
        // No reachable breakpoint -> runs a whole frame and returns None,
        // leaving the machine exactly where a plain run_frame would.
        let mut a = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        let mut b = GameBoy::new(Model::Dmg, linear_code_rom()).unwrap();
        assert_eq!(a.run_frame_until_breakpoint(&[0xBEEF]), None);
        b.run_frame();
        assert_eq!(a.frame_count(), b.frame_count());
        assert_eq!(a.cycles(), b.cycles());
        assert_eq!(a.cpu_regs().pc, b.cpu_regs().pc);
        // Empty breakpoint list is just a run_frame.
        assert_eq!(a.run_frame_until_breakpoint(&[]), None);
    }

    #[test]
    fn ime_accessors_track_the_ei_delay() {
        let mut rom = vec![0u8; 0x8000];
        rom[0x100] = 0xFB; // ei; 0x101.. stay nop
        let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
        assert!(!gb.ime() && !gb.ime_pending(), "post-boot: interrupts off");
        assert!(!gb.double_speed(), "DMG is never double-speed");
        gb.step(); // ei: arms the pending enable, IME still off
        assert!(!gb.ime(), "IME stays off the instruction after EI");
        assert!(gb.ime_pending(), "EI arms the pending enable");
        gb.step(); // the following instruction commits IME
        assert!(gb.ime(), "IME enabled one instruction after EI");
        assert!(!gb.ime_pending(), "pending cleared once applied");
    }
}
