//! MCP address parsing — the `BB:AAAA` / `AAAA` operand shared by the
//! `disassemble` / `peek` / `cdl` / `breakpoint` tools.
//!
//! `AAAA` is a bare 4-hex CPU address (bank implied 0); `BB:AAAA` prefixes a
//! 2-hex bank. Which form is legal depends on the region:
//!
//! | Region | Range | Form |
//! |---|---|---|
//! | ROM0 | 0000-3FFF | `AAAA` |
//! | ROMX | 4000-7FFF | `BB:AAAA` |
//! | VRAM | 8000-9FFF | `BB:AAAA` |
//! | WRAM0 | C000-CFFF | `AAAA` |
//! | WRAMX | D000-DFFF | `BB:AAAA` |
//! | echo+ | E000-FFFF | `AAAA` |
//!
//! Cart SRAM (A000-BFFF) is addressable by neither form (matching the tool
//! spec) — a query there is rejected. A range must stay inside one region (and,
//! for the banked regions, one bank), so a caller can't accidentally read across
//! a bank boundary; it splits the query instead.

/// A parsed address: an explicit bank plus the 16-bit CPU address. For the
/// bare `AAAA` form `bank` is 0 (and the region is unbanked, so it's ignored on
/// read anyway; it is what the `BB:` column shows).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Addr {
    pub bank: u16,
    pub addr: u16,
}

/// The memory region an address lands in — fixes the legal address form and
/// whether a bank is meaningful.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    Rom0,
    RomX,
    Vram,
    Sram,
    Wram0,
    WramX,
    Echo,
}

impl Region {
    #[must_use]
    pub fn of(addr: u16) -> Self {
        match addr {
            0x0000..=0x3FFF => Region::Rom0,
            0x4000..=0x7FFF => Region::RomX,
            0x8000..=0x9FFF => Region::Vram,
            0xA000..=0xBFFF => Region::Sram,
            0xC000..=0xCFFF => Region::Wram0,
            0xD000..=0xDFFF => Region::WramX,
            _ => Region::Echo,
        }
    }

    /// Whether the region needs an explicit bank (the `BB:AAAA` form).
    #[must_use]
    pub fn banked(self) -> bool {
        matches!(self, Region::RomX | Region::Vram | Region::WramX)
    }

    fn label(self) -> &'static str {
        match self {
            Region::Rom0 => "ROM0",
            Region::RomX => "ROMX",
            Region::Vram => "VRAM",
            Region::Sram => "SRAM",
            Region::Wram0 => "WRAM0",
            Region::WramX => "WRAMX",
            Region::Echo => "echo/OAM/IO/HRAM",
        }
    }
}

/// Parse one `AAAA` or `BB:AAAA` token, enforcing the per-region form. Total —
/// every malformed input is an `Err(String)`, never a panic.
pub fn parse_one(s: &str) -> Result<Addr, String> {
    let s = s.trim();
    if let Some((b, a)) = s.split_once(':') {
        let bank = u16::from_str_radix(b.trim(), 16)
            .map_err(|_| format!("bad bank in '{s}' (want 2-hex BB:AAAA)"))?;
        let addr = u16::from_str_radix(a.trim(), 16)
            .map_err(|_| format!("bad address in '{s}' (want 2-hex BB:AAAA)"))?;
        let region = Region::of(addr);
        if !region.banked() {
            return Err(format!(
                "{addr:04X} is {} — use the bare AAAA form, not BB:AAAA",
                region.label()
            ));
        }
        Ok(Addr { bank, addr })
    } else {
        let addr = u16::from_str_radix(s, 16)
            .map_err(|_| format!("bad address '{s}' (want AAAA or BB:AAAA hex)"))?;
        let region = Region::of(addr);
        if region.banked() {
            return Err(format!(
                "{addr:04X} is {} — needs the BB:AAAA form (a bank)",
                region.label()
            ));
        }
        if region == Region::Sram {
            return Err(format!(
                "{addr:04X} is cart SRAM — not addressable by these tools"
            ));
        }
        Ok(Addr { bank: 0, addr })
    }
}

/// Parse a `from`/`to` pair for a range query. Rejects a range that straddles a
/// region boundary or (for banked regions) a bank boundary, and `from > to`.
/// Returns the two endpoints (same bank); iterate `from.addr..=to.addr` reading
/// `debug_read_banked(from.bank, addr)`.
pub fn parse_range(from: &str, to: &str) -> Result<(Addr, Addr), String> {
    let a = parse_one(from)?;
    let b = parse_one(to)?;
    let (ra, rb) = (Region::of(a.addr), Region::of(b.addr));
    if ra != rb {
        return Err(format!(
            "range {a:?}..{b:?} straddles a region boundary ({} vs {}) — split the query",
            ra.label(),
            rb.label()
        ));
    }
    if ra.banked() && a.bank != b.bank {
        return Err(format!(
            "range straddles a bank boundary (bank {:02X} vs {:02X}) — split the query",
            a.bank, b.bank
        ));
    }
    if a.addr > b.addr {
        return Err(format!("from {:04X} is after to {:04X}", a.addr, b.addr));
    }
    Ok((a, b))
}

#[cfg(test)]
#[path = "addr_tests.rs"]
mod tests;
