//! Reverse execution for the debugger and the player rewind, built on the
//! checkpoint ring in [`Session`]. Frame-boundary save states are too coarse to
//! *be* a reverse target, but they are exact **replay anchors**: load the
//! nearest checkpoint before where we want to land, then `step()` forward
//! deterministically to the target. The core is deterministic and joypad input
//! is baked into each save state, so replaying from a checkpoint reproduces the
//! original run exactly — which lands the *exact* instruction, not just a frame.

use super::*;

impl Session {
    /// The newest checkpoint blob taken strictly before `cycle`, cloned so the
    /// ring borrow is released before the caller mutates `self.gb`.
    fn nearest_checkpoint_before(&self, cycle: u64) -> Option<Vec<u8>> {
        self.rewind
            .iter()
            .rev()
            .find(|&&(c, _)| c < cycle)
            .map(|(_, b)| b.clone())
    }

    /// Drop checkpoints newer than `cycle` (the future we just rewound past) and
    /// re-anchor autosave + capture to the restored, earlier position.
    fn land_at(&mut self, cycle: u64) {
        while let Some(&(c, _)) = self.rewind.back() {
            if c <= cycle {
                break;
            }
            if let Some((_, old)) = self.rewind.pop_back() {
                self.rewind_bytes -= old.len();
            }
        }
        self.next_autosave = self.gb.cycles().saturating_add(AUTOSAVE_CYCLES);
        self.next_rewind_frame = self.gb.frame_count();
    }

    /// Reverse one instruction: land on the instruction boundary immediately
    /// before the current position. `false` if no checkpoint precedes it (the
    /// oldest history is exhausted). The debugger stays broken; the caller
    /// re-centers the disasm.
    pub(crate) fn reverse_step(&mut self) -> bool {
        let original = self.gb.cycles();
        let Some(bytes) = self.nearest_checkpoint_before(original) else {
            return false;
        };
        let _ = self.gb.load_state(&bytes);
        // Pass 1: the last instruction boundary strictly before `original`.
        let mut landing = self.gb.cycles();
        while self.gb.cycles() < original {
            landing = self.gb.cycles();
            self.gb.step();
        }
        // Pass 2: replay to it (no per-step snapshots — ≤ one checkpoint interval).
        let _ = self.gb.load_state(&bytes);
        while self.gb.cycles() < landing {
            self.gb.step();
        }
        self.land_at(landing);
        true
    }

    /// Reverse to the previous *frame* boundary (the player's held-Backspace
    /// rewind — frame-exact, one displayed frame per tick). `false` when history
    /// is exhausted, so the caller falls through to normal play.
    pub(crate) fn reverse_frame(&mut self) -> bool {
        let original = self.gb.cycles();
        let Some(bytes) = self.nearest_checkpoint_before(original) else {
            return false;
        };
        let _ = self.gb.load_state(&bytes);
        // The nearest checkpoint is itself a frame boundary, so it is always a
        // valid landing; refine to the latest frame boundary strictly before now.
        let mut landing = self.gb.cycles();
        let mut fc = self.gb.frame_count();
        while self.gb.cycles() < original {
            self.gb.step();
            if self.gb.frame_count() != fc {
                fc = self.gb.frame_count();
                if self.gb.cycles() < original {
                    landing = self.gb.cycles();
                }
            }
        }
        let _ = self.gb.load_state(&bytes);
        while self.gb.cycles() < landing {
            self.gb.step();
        }
        self.land_at(landing);
        true
    }

    /// Run backward to the most recent breakpoint (or watch/profiler/exception
    /// halt) strictly before the current position — the inverse of the debugger
    /// free run. Replays each checkpoint window newest-first with the same
    /// bank-aware predicate the live run uses ([`GameBoy::run_frame_until_breakpoint`]),
    /// keeping the last halt found before `original`. `false` if no halt lies in
    /// the retained history.
    ///
    /// ponytail: O(history) worst case (a halt far back re-replays the tail);
    /// interactive-fine because a per-frame breakpoint resolves in the newest
    /// window. Bound = ring depth × frame length.
    pub(crate) fn reverse_to_breakpoint(&mut self, bps: &[(u16, Option<u16>)]) -> bool {
        let original = self.gb.cycles();
        for i in (0..self.rewind.len()).rev() {
            let bytes = {
                let (c, b) = &self.rewind[i];
                if *c >= original {
                    continue;
                }
                b.clone()
            };
            let _ = self.gb.load_state(&bytes);
            let mut last_hit: Option<Vec<u8>> = None;
            while self.gb.cycles() < original {
                let hit = self.gb.run_frame_until_breakpoint(bps);
                if hit.is_some() && self.gb.cycles() < original {
                    last_hit = Some(self.gb.save_state());
                }
            }
            if let Some(state) = last_hit {
                let _ = self.gb.load_state(&state);
                let landing = self.gb.cycles();
                self.land_at(landing);
                return true;
            }
        }
        false
    }
}
