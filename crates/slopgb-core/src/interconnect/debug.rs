//! Debugger-only inherent methods on [`Interconnect`]: memory watchpoints (RM8),
//! the execution profiler (MB5), and the exception-break checks (Options →
//! Exceptions). Every one is a live-debugger control that defaults inert and is
//! never exercised on a golden/test path, so the fingerprint stays
//! byte-identical. Interconnect work package.

use super::*;
use crate::{EXC_ECHO_RAM, EXC_INVALID_OPCODE, EXC_LCD_OFF_VBLANK, EXC_LD_B_B};

impl Interconnect {
    /// Per-access debugger check on a CPU bus access: memory watchpoints (RM8)
    /// and the echo-RAM exception break. Both halves early-out when their
    /// feature is unarmed (empty watch list / `exc_mask == 0`), so this is a
    /// no-op on every golden path (golden-safe). Replaces the former
    /// `check_watch`, called from the ticked `Bus` read/read_inc/write.
    pub(super) fn check_access(&mut self, addr: u16, is_write: bool) {
        // CDL: record a CPU read/write of this byte (R=1, W=2). `None` when the
        // log is off → no-op, so the golden path is byte-identical.
        if let Some(b) = &mut self.cdl {
            b[addr as usize] |= if is_write { 2 } else { 1 };
        }
        if !self.watchpoints.is_empty()
            && self
                .watchpoints
                .iter()
                .any(|w| w.addr == addr && if is_write { w.write } else { w.read })
        {
            self.watch_hit = Some(addr);
        }
        // Echo RAM is C000-DDFF mirrored at E000-FDFF; any CPU access there is
        // bgb's "break on ram echo (E000-FDFF) access".
        if self.exc_mask & EXC_ECHO_RAM != 0 && (0xE000..=0xFDFF).contains(&addr) {
            self.exc_hit = Some(addr);
        }
    }

    /// Exception break on a write: disabling the LCD (`FF40` bit 7 → 0) while it
    /// is on and the PPU is outside vblank (mode ≠ 1). The caller passes the
    /// *new* value before committing it, so `lcd_enabled()` still reads the old
    /// LCDC. Inert when the bit is unarmed.
    pub(super) fn check_exc_lcd(&mut self, addr: u16, value: u8) {
        if self.exc_mask & EXC_LCD_OFF_VBLANK != 0
            && addr == 0xFF40
            && value & 0x80 == 0
            && self.ppu.lcd_enabled()
            && self.ppu.mode_bits() != 1
        {
            self.exc_hit = Some(addr);
        }
    }

    /// Exception break on the opcode about to execute at `pc`: `LD B,B` (`40h`)
    /// or an undefined opcode. The undefined set is exactly the 11 opcodes the
    /// CPU hard-locks on (`cpu::execute`). Inert when no opcode exception is
    /// armed (`exc_mask == 0`).
    pub(super) fn exec_exception(&mut self, pc: u16, opcode: u8) {
        if self.exc_mask & (EXC_LD_B_B | EXC_INVALID_OPCODE) == 0 {
            return;
        }
        let hit = (self.exc_mask & EXC_LD_B_B != 0 && opcode == 0x40)
            || (self.exc_mask & EXC_INVALID_OPCODE != 0
                && matches!(
                    opcode,
                    0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD
                ));
        if hit {
            self.exc_hit = Some(pc);
        }
    }

    /// Set the debugger exception-break mask (the `EXC_*` bits). `0` disarms
    /// every check (golden-safe). Clears any pending hit (like
    /// [`Self::set_watchpoints`]) so re-arming can't replay a stale one.
    /// Live-debugger-only.
    pub fn set_exceptions(&mut self, mask: u16) {
        self.exc_mask = mask;
        self.exc_hit = None;
    }

    /// The current exception-break mask (`0` when nothing is armed).
    pub fn exceptions(&self) -> u16 {
        self.exc_mask
    }

    /// Take the pending exception-break hit address (cleared by the read).
    pub fn take_exc_hit(&mut self) -> Option<u16> {
        self.exc_hit.take()
    }

    /// Replace the debugger memory watchpoints (RM8). Empty disables the
    /// access-path check entirely (golden-safe).
    pub fn set_watchpoints(&mut self, wps: &[crate::Watchpoint]) {
        self.watchpoints = wps.to_vec();
        self.watch_hit = None;
    }

    /// Take the pending watchpoint hit address (cleared by the read).
    pub fn take_watch_hit(&mut self) -> Option<u16> {
        self.watch_hit.take()
    }

    /// Enable/disable the execution profiler (MB5). Enabling allocates the tally
    /// (preserving an existing one); disabling drops it and any break-mode state.
    /// Live-debugger-only.
    pub fn set_profiling(&mut self, on: bool) {
        match (on, self.prof.is_some()) {
            (true, false) => self.prof = Some(std::collections::BTreeMap::new()),
            (false, true) => {
                self.prof = None;
                self.prof_break = false;
                self.prof_break_hit = None;
            }
            _ => {}
        }
    }

    /// Arm/disarm profiler break mode (halt the free run on each address's first
    /// execution). Only meaningful while profiling is on.
    pub fn set_profile_break(&mut self, on: bool) {
        self.prof_break = on;
        if !on {
            self.prof_break_hit = None;
        }
    }

    /// Whether profiler break mode is armed.
    pub fn profile_break(&self) -> bool {
        self.prof_break
    }

    /// Take the pending break-mode hit address (cleared by the read).
    pub fn take_prof_break_hit(&mut self) -> Option<u16> {
        self.prof_break_hit.take()
    }

    /// Zero the profiler tally without disabling logging (bgb's "clear buffer").
    pub fn clear_profile(&mut self) {
        if let Some(m) = &mut self.prof {
            m.clear();
        }
    }

    /// Enable/disable the code/data log (CDL). Enabling allocates the 64 KiB flag
    /// buffer (preserving an existing one); disabling drops it. Live-debugger-only,
    /// golden-safe (a `None` log is a no-op in every CDL hook).
    pub fn set_cdl(&mut self, on: bool) {
        match (on, self.cdl.is_some()) {
            (true, false) => self.cdl = Some(Box::new([0u8; 65536])),
            (false, true) => self.cdl = None,
            _ => {}
        }
    }

    /// The CDL access flags at `addr` (R=1, W=2, X=4), or 0 when the log is
    /// off/the byte is unvisited.
    #[must_use]
    pub fn cdl_flag(&self, addr: u16) -> u8 {
        self.cdl.as_ref().map_or(0, |b| b[addr as usize])
    }

    /// The whole 64 KiB flag buffer (for a save), or `None` when the log is off.
    #[must_use]
    pub fn cdl_flags(&self) -> Option<&[u8]> {
        self.cdl.as_deref().map(|b| &b[..])
    }

    /// Zero the CDL flags without disabling logging (bgb's "clear buffer").
    pub fn cdl_clear(&mut self) {
        if let Some(b) = &mut self.cdl {
            b.fill(0);
        }
    }

    /// Load a CDL flag buffer (a loaded `.cdl` file), enabling the log.
    pub fn load_cdl(&mut self, flags: &[u8; 65536]) {
        self.cdl = Some(Box::new(*flags));
    }

    /// Whether the profiler is currently logging.
    pub fn profiling(&self) -> bool {
        self.prof.is_some()
    }

    /// Times the instruction at `pc` has executed since the last clear (0 if
    /// unseen or profiling is off).
    pub fn profile_count(&self, pc: u16) -> u64 {
        self.prof
            .as_ref()
            .and_then(|m| m.get(&pc))
            .copied()
            .unwrap_or(0)
    }

    /// Distinct instruction addresses seen since the last clear.
    pub fn profile_seen(&self) -> usize {
        self.prof
            .as_ref()
            .map_or(0, std::collections::BTreeMap::len)
    }
}
