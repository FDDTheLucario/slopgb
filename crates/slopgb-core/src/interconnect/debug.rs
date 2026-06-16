//! Debugger-only inherent methods on [`Interconnect`]: memory watchpoints (RM8)
//! and the execution profiler (MB5). Every one is a live-debugger control that
//! defaults inert and is never exercised on a golden/test path, so the
//! fingerprint stays byte-identical. Interconnect work package.

use super::*;

impl Interconnect {
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
