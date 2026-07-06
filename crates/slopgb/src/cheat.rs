//! Cheat engine, modeled on bgb's Cheat dialog (docs/bgb-reference/cheat/):
//! a list of `{ comment, code, enabled }` entries. Enabled GameShark codes are
//! re-poked into RAM once per frame — the same `debug_write` re-apply the freeze
//! list uses (`app_pacing::run_one_frame`), so it's frontend-only + golden-safe.
//!
//! GameShark GB code = 8 hex `ttvvaaaa`: type `tt` (01 = RAM write), value `vv`,
//! address `aaaa` stored little-endian. bgb renders `01FF0AC1` as `(C10A)=FF`.
//! Game Genie (ROM patch) codes are recognized + stored but not yet applied
//! (needs a core ROM-read hook); they contribute no poke.

/// The decoded effect of a cheat code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// GameShark: write `value` to `addr` every frame.
    Ram { addr: u16, value: u8 },
    /// Game Genie ROM patch: substitute `value` at `addr`, gated (9-digit codes)
    /// on the current byte matching `compare`.
    Rom { addr: u16, value: u8, compare: Option<u8> },
}

/// Parse a bgb cheat code (case-insensitive; spaces/dashes ignored). Returns the
/// decoded [`Effect`], or `None` if the format isn't recognized.
#[must_use]
pub fn parse_code(code: &str) -> Option<Effect> {
    let clean: String = code.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    // GameShark: 8 hex digits `ttvvaaaa` (addr little-endian).
    if clean.len() == 8 {
        let byte = |i: usize| u8::from_str_radix(&clean[i..i + 2], 16).ok();
        let (ty, value, lo, hi) = (byte(0)?, byte(2)?, byte(4)?, byte(6)?);
        // Type 01 = RAM write. Other GameShark types aren't supported yet.
        return (ty == 0x01).then(|| Effect::Ram {
            addr: u16::from(hi) << 8 | u16::from(lo),
            value,
        });
    }
    // Game Genie: 6 (`AAA-BBB`, unconditional) or 9 (`AAA-BBB-CCC`, with compare)
    // hex digits — the standard GB Game Genie decode (same as bgb/mGBA/VBA).
    if clean.len() == 6 || clean.len() == 9 {
        let n: Vec<u8> = clean.chars().map(|c| c.to_digit(16).unwrap() as u8).collect();
        let value = (n[0] << 4) | n[1];
        // Address: nibbles 2-4 are the low 12 bits; nibble 5 (XOR 0xF) is the top.
        let addr = u16::from(n[5] ^ 0xF) << 12
            | u16::from(n[2]) << 8
            | u16::from(n[3]) << 4
            | u16::from(n[4]);
        // Compare (9-digit): from nibbles 6 + 8 (7 is a check digit), rotate-right
        // 2 then XOR 0xBA.
        let compare = (n.len() == 9).then(|| ((n[6] << 4) | n[8]).rotate_right(2) ^ 0xBA);
        return Some(Effect::Rom { addr, value, compare });
    }
    None
}

/// One cheat list entry (bgb's Add/Edit dialog: Comment + Code).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cheat {
    pub comment: String,
    pub code: String,
    pub enabled: bool,
}

/// The App-owned cheat list (mirrors `dbg::FreezeList`): edited via the Cheat
/// dialog, its enabled RAM pokes re-applied each frame by the run loop.
#[derive(Default, Clone, Debug)]
pub struct CheatList {
    items: Vec<Cheat>,
}

impl CheatList {
    #[must_use]
    pub fn items(&self) -> &[Cheat] {
        &self.items
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Add a cheat (enabled). Returns its index.
    pub fn add(&mut self, comment: &str, code: &str) -> usize {
        self.items.push(Cheat {
            comment: comment.to_string(),
            code: code.to_string(),
            enabled: true,
        });
        self.items.len() - 1
    }

    /// Replace the comment/code of the cheat at `i` (no-op if out of range).
    pub fn edit(&mut self, i: usize, comment: &str, code: &str) {
        if let Some(c) = self.items.get_mut(i) {
            c.comment = comment.to_string();
            c.code = code.to_string();
        }
    }

    /// Remove the cheat at `i` (no-op if out of range).
    pub fn remove(&mut self, i: usize) {
        if i < self.items.len() {
            self.items.remove(i);
        }
    }

    /// Set the enabled flag of the cheat at `i`.
    pub fn set_enabled(&mut self, i: usize, on: bool) {
        if let Some(c) = self.items.get_mut(i) {
            c.enabled = on;
        }
    }

    /// Toggle the cheat at `i`; returns its new state (false if out of range).
    pub fn toggle(&mut self, i: usize) -> bool {
        if let Some(c) = self.items.get_mut(i) {
            c.enabled = !c.enabled;
            c.enabled
        } else {
            false
        }
    }

    pub fn enable_all(&mut self) {
        self.items.iter_mut().for_each(|c| c.enabled = true);
    }

    pub fn disable_all(&mut self) {
        self.items.iter_mut().for_each(|c| c.enabled = false);
    }

    /// The `(addr, value)` RAM pokes for every enabled, GameShark cheat — the
    /// run loop re-applies these each frame via `debug_write` (like freezes).
    #[must_use]
    pub fn pokes(&self) -> Vec<(u16, u8)> {
        self.items
            .iter()
            .filter(|c| c.enabled)
            .filter_map(|c| match parse_code(&c.code) {
                Some(Effect::Ram { addr, value }) => Some((addr, value)),
                _ => None,
            })
            .collect()
    }

    /// The Game Genie ROM patches for every enabled Game-Genie cheat, pushed to
    /// the core (`GameBoy::set_gg_patches`) whenever the list changes.
    #[must_use]
    pub fn gg_patches(&self) -> Vec<slopgb_core::GgPatch> {
        self.items
            .iter()
            .filter(|c| c.enabled)
            .filter_map(|c| match parse_code(&c.code) {
                Some(Effect::Rom { addr, value, compare }) => {
                    Some(slopgb_core::GgPatch { addr, value, compare })
                }
                _ => None,
            })
            .collect()
    }

    /// The one-time poke of the cheat at `i` (bgb's "Poke" button — apply once
    /// without enabling), or `None` if it isn't a RAM cheat.
    #[must_use]
    pub fn poke_once(&self, i: usize) -> Option<(u16, u8)> {
        match self.items.get(i).and_then(|c| parse_code(&c.code)) {
            Some(Effect::Ram { addr, value }) => Some((addr, value)),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "cheat_tests.rs"]
mod tests;
