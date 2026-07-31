//! Debugger/introspection accessors on [`GameBoy`]: memory watchpoints, the
//! exception-break mask, save-states, the execution profiler, the FCEUX-style
//! code/data log (CDL), and the WRAM/VRAM bank indicators.
//!
//! A second `impl GameBoy` block, split out of `lib.rs` to keep it under the
//! 1000-line cap. `use super::*` pulls in
//! `GameBoy`, `Watchpoint`, `StateError`, and the save-state constants; as a
//! child module it reaches `GameBoy`'s private `cpu`/`bus`/`sgb_apu` fields
//! directly.
//!
//! Every accessor here is read-only `&self` introspection or a default-off
//! mutating debug hook (never armed on a golden/test path), so it is
//! golden-safe (the golden-safe law).

use super::*;

impl GameBoy {
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

    /// Drop any pending watchpoint / exception-break / profiler-break hit without
    /// halting. The frontend calls this when the debugger *opens* (an armed wake
    /// begins): watchpoints and the exception mask stay armed even while the
    /// debugger is closed, so `check_access` keeps recording hits that the plain
    /// `run_frame` path never consumes — opening would otherwise replay a stale,
    /// wrongly-timed hit as a spurious halt. Golden-safe: it only `take`s the
    /// debug `Option` fields (the same accessors `run_frame_until_breakpoint`
    /// uses), advancing no cycle and touching no emulated state.
    pub fn clear_debug_hits(&mut self) {
        self.bus.take_watch_hit();
        self.bus.take_exc_hit();
        self.bus.take_prof_break_hit();
    }

    /// Serialize the whole machine to bytes (bgb's File → Save state). A
    /// magic + version + ROM-fingerprint header precedes the volatile state
    /// (CPU + all peripherals). ROM bytes are *not* included — a state restores
    /// into a machine already built from the same ROM. `&self`/read-only, so it
    /// is golden-safe (never reached on a golden/test path).
    #[must_use]
    pub fn save_state(&self) -> Vec<u8> {
        let mut w = state::Writer::new();
        w.bytes(STATE_MAGIC);
        w.u16(STATE_VERSION);
        let id = self.bus.cartridge().rom_id();
        w.u32(id.len() as u32);
        w.bytes(&id);
        // Is-SGB-model flag: the ROM header pins the ROM but not the model, yet
        // the same ROM runs as SGB (whose PPU carries the SGB view) or DMG/CGB.
        // Record it so `load_state` can reject a cross-model load.
        w.bool(self.model().is_sgb());
        self.cpu.write_state(&mut w);
        self.bus.write_state(&mut w);
        // The installed SNES coprocessor's own state, appended only when the
        // slot is filled — so `Dmg`/`Cgb` (and a pluginless SGB) states carry
        // no tail at all. Its length is the coprocessor's business, so the
        // reader can only consume it with the same kind of coprocessor
        // installed; the flag lets a mismatch be rejected instead of misparsed.
        w.bool(self.sgb_apu.is_some());
        if let Some(apu) = &self.sgb_apu {
            apu.write_state(&mut w);
        }
        w.into_vec()
    }

    /// Restore a machine from [`Self::save_state`] bytes (bgb's File → Load
    /// state). Validates the magic/version/ROM fingerprint against the *loaded*
    /// ROM, then restores the volatile state. The debugger state (breakpoints,
    /// watchpoints, profiler, exception mask) is left untouched. Live-debugger/UI
    /// only.
    ///
    /// **Atomic for the Game Boy itself**: CPU + bus land in a scratch machine
    /// that only replaces `self` on full success, so any error leaves them
    /// unchanged. The installed SGB coprocessor (if any) is *moved* into that
    /// scratch machine rather than cloned — [`sgb::AudioCoprocessor::clone_box`]
    /// re-instantiates a plugin-backed coprocessor's wasm, and the tail read
    /// below overwrites the chips' state anyway, so the rebuild was pure waste on
    /// a path the player rewind runs once per rewound frame. What that costs: a
    /// state whose coprocessor tail is truncated leaves those chips part-restored
    /// (the tail is read last, after the Game Boy side is already safely parsed),
    /// and the next successful load overwrites them wholesale.
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        let apu = self.sgb_apu.take();
        let mut restored = self.clone();
        restored.sgb_apu = apu;
        match restored.load_state_into(bytes) {
            Ok(()) => {
                *self = restored;
                Ok(())
            }
            Err(e) => {
                self.sgb_apu = restored.sgb_apu.take();
                Err(e)
            }
        }
    }

    fn load_state_into(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        let mut r = state::Reader::new(bytes);
        let mut magic = [0u8; 4];
        r.bytes_into(&mut magic)?;
        if &magic != STATE_MAGIC {
            return Err(StateError::BadMagic);
        }
        if r.u16()? != STATE_VERSION {
            return Err(StateError::BadVersion);
        }
        let id_len = r.u32()? as usize;
        let id = r.bytes_vec(id_len)?;
        if id != self.bus.cartridge().rom_id() {
            return Err(StateError::RomMismatch);
        }
        // Reject a cross-model load before touching state: a DMG/CGB state
        // restored into SGB would silently drop that machine's PPU SGB view
        // (the PPU's own presence flag would clear it), and the reverse would
        // load SGB-only payload into a machine that has none.
        if r.bool()? != self.model().is_sgb() {
            return Err(StateError::ModelMismatch);
        }
        self.cpu.read_state(&mut r)?;
        self.bus.read_state(&mut r)?;
        // The SNES-coprocessor tail is only readable by a coprocessor of the
        // same kind: its length is opaque here, so a state whose tail presence
        // disagrees with this machine's slot is rejected, never guessed at.
        let has_tail = r.bool()?;
        match (has_tail, self.sgb_apu.as_mut()) {
            (true, Some(apu)) => apu.read_state(&mut r)?,
            (false, None) => {}
            _ => return Err(StateError::ModelMismatch),
        }
        Ok(())
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

    /// Enable/disable the FCEUX-style code/data log (CDL): per-CPU-address R/W/X
    /// access flags. Off by default and never set on a golden/test path, so it is
    /// golden-safe (a `None` log is a zero-overhead no-op in every access hook).
    pub fn set_cdl(&mut self, on: bool) {
        self.bus.set_cdl(on);
    }

    /// The CDL access flags at `addr` (R=1, W=2, X=4), 0 when off/unvisited.
    #[must_use]
    pub fn cdl_flag(&self, addr: u16) -> u8 {
        self.bus.cdl_flag(addr)
    }

    /// The CDL access flags at an **explicit** bank of the banked regions
    /// (ROMX / VRAM / WRAMX); elsewhere `bank` is ignored (== [`Self::cdl_flag`]).
    /// For the MCP/debug `cdl` tool. Read-only, golden-safe.
    #[must_use]
    pub fn cdl_flag_banked(&self, bank: u16, addr: u16) -> u8 {
        self.bus.cdl_flag_banked(bank, addr)
    }

    /// The whole CDL flag buffer (for a save), or `None` when the log is off.
    #[must_use]
    pub fn cdl_flags(&self) -> Option<&[u8]> {
        self.bus.cdl_flags()
    }

    /// Every continuous span of logged (non-`.`) CPU addresses, one
    /// [`CdlRange`](crate::CdlRange) per span (bank-tagged for the banked
    /// regions). Empty when the log is off. For the MCP/debug `cdl-ranges` tool.
    /// Read-only, golden-safe.
    #[must_use]
    pub fn cdl_logged_ranges(&self) -> Vec<crate::CdlRange> {
        self.bus.cdl_logged_ranges()
    }

    /// Zero the CDL flags without disabling logging.
    pub fn cdl_clear(&mut self) {
        self.bus.cdl_clear();
    }

    /// Load a physical CDL flag buffer (a decoded `.cdl` file), enabling the
    /// log. Returns false (leaving the log unchanged) if the buffer's length
    /// doesn't match this machine's layout — a `.cdl` from another ROM/RAM
    /// configuration.
    #[must_use]
    pub fn load_cdl(&mut self, flags: &[u8]) -> bool {
        self.bus.load_cdl(flags)
    }

    /// The live WRAM bank at `0xD000-0xDFFF` (CGB SVBK, 1 on DMG), for the
    /// memory-viewer bank indicator.
    #[must_use]
    pub fn wram_bank(&self) -> usize {
        self.bus.wram_bank()
    }

    /// The live VRAM bank (CGB VBK, 0 on DMG), for the viewer bank indicator.
    #[must_use]
    pub fn vram_bank(&self) -> usize {
        self.bus.vram_bank()
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
}
