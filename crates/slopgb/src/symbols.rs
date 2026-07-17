//! bgb/rgbds-style `.sym` symbol files: one `BB:AAAA name` line per symbol
//! (`BB` = bank hex, `AAAA` = 16-bit address hex). Loaded into the debugger so
//! the disassembly, `Go to…`, and the breakpoint manager can show names.
//!
//! Parsing is deliberately tolerant: blank lines, `;` comments (whole-line or
//! trailing), section headers like `[symbols]`, and any malformed line are
//! skipped rather than rejected, so a real-world `.sym` loads what it can.

/// One loaded symbol: its ROM/RAM bank, 16-bit address, and name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub bank: u16,
    pub addr: u16,
    pub name: String,
}

/// A parsed `.sym` table, kept sorted by address for nearest-symbol lookups.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SymbolTable {
    /// Sorted by `addr` (ascending), so lookups can binary-search.
    syms: Vec<Symbol>,
}

impl SymbolTable {
    /// Parse the text of a `.sym` file. Unparseable lines are skipped.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut syms = Vec::new();
        for raw in text.lines() {
            // Strip a `;` comment (whole-line or trailing) and surrounding space.
            let line = raw.split(';').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut it = line.split_whitespace();
            let (Some(loc), Some(name)) = (it.next(), it.next()) else {
                continue;
            };
            let Some((bank, addr)) = parse_loc(loc) else {
                continue;
            };
            syms.push(Symbol {
                bank,
                addr,
                name: name.to_string(),
            });
        }
        syms.sort_by_key(|s| s.addr);
        Self { syms }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.syms.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.syms.is_empty()
    }

    /// The name of the symbol at exactly `addr` in `bank`, if any. Banks
    /// discriminate: a `0x4000-0x7FFF` symbol in ROM bank `0B` is not shown while
    /// bank `24` is mapped there, matching bgb (the caller passes the live-mapped
    /// bank for `addr`'s region — bank 0 for the fixed regions). On a same-address
    /// multi-bank collision the matching-bank symbol wins.
    #[must_use]
    pub fn name_at(&self, bank: u16, addr: u16) -> Option<&str> {
        let lo = self.syms.partition_point(|s| s.addr < addr);
        self.syms[lo..]
            .iter()
            .take_while(|s| s.addr == addr)
            .find(|s| s.bank == bank)
            .map(|s| s.name.as_str())
    }

    /// The nearest symbol in `bank` at or before `addr` (name + its address), for
    /// the standalone memory viewer's status bar ("Name+offset"). Bank-filtered
    /// like [`Self::name_at`].
    #[must_use]
    pub fn nearest_before(&self, bank: u16, addr: u16) -> Option<(&str, u16)> {
        let end = self.syms.partition_point(|s| s.addr <= addr);
        self.syms[..end]
            .iter()
            .rev()
            .find(|s| s.bank == bank)
            .map(|s| (s.name.as_str(), s.addr))
    }

    /// The `(bank, addr)` of the symbol named `name` (case-insensitive), for `Go
    /// to…`. The bank lets a Go-to pin the disasm/memory bank browser to the
    /// symbol's own bank (so `01:6401 SomeWhere` jumps into bank 1, not whatever
    /// is mapped).
    #[must_use]
    pub fn resolve(&self, name: &str) -> Option<(u16, u16)> {
        self.syms
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .map(|s| (s.bank, s.addr))
    }
}

/// Parse a `BB:AAAA` (or bare `AAAA`, bank 0) location field into `(bank, addr)`.
fn parse_loc(loc: &str) -> Option<(u16, u16)> {
    match loc.split_once(':') {
        Some((b, a)) => Some((
            u16::from_str_radix(b, 16).ok()?,
            u16::from_str_radix(a, 16).ok()?,
        )),
        None => Some((0, u16::from_str_radix(loc, 16).ok()?)),
    }
}

#[cfg(test)]
#[path = "symbols_tests.rs"]
mod tests;
