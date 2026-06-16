//! Pure search + bookmark logic for the debugger Search menu (MB3): a forward
//! byte-or-mnemonic scan over the live machine (`find_match`) and the
//! next/previous-mark cursor walk (`next_mark`). The view state + machine glue
//! live in [`super::DebuggerState`] / `toolwin`; these are side-effect-free so
//! they unit-test headless.

use slopgb_core::debug;

/// Search forward from `from` (wrapping once through the whole address space) for
/// the first address matching `query`. A query that parses as whitespace-
/// separated hex byte pairs (`"3E 01"`, `"3E01"`) matches that byte sequence;
/// anything else is a case-insensitive substring match against the decoded
/// mnemonic at each address (bgb's `search string 'ld a,'`). `read(addr)` yields
/// the byte at `addr` (`GameBoy::debug_read`). Returns the matching address.
#[must_use]
pub fn find_match(read: impl Fn(u16) -> u8, from: u16, query: &str) -> Option<u16> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    match parse_hex_bytes(q) {
        Some(bytes) if !bytes.is_empty() => find_bytes(&read, from, &bytes),
        _ => find_mnemonic(&read, from, &q.to_ascii_lowercase()),
    }
}

/// Parse a query as whitespace-separated hex byte pairs. Returns `None` (so the
/// caller falls back to a mnemonic search) unless every token is an even number
/// of hex digits — so `"add"` (odd) and `"ld a,"` (non-hex) are mnemonics, while
/// `"3E 01"` / `"3E01"` are byte sequences.
fn parse_hex_bytes(q: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for tok in q.split_whitespace() {
        if tok.is_empty() || tok.len() % 2 != 0 || !tok.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        for pair in tok.as_bytes().chunks(2) {
            let s = std::str::from_utf8(pair).ok()?;
            bytes.push(u8::from_str_radix(s, 16).ok()?);
        }
    }
    Some(bytes)
}

/// First address (scanning all 65536 positions from `from`, wrapping) where
/// `bytes` occur consecutively.
fn find_bytes(read: &impl Fn(u16) -> u8, from: u16, bytes: &[u8]) -> Option<u16> {
    (0..=0xFFFFu32)
        .map(|o| from.wrapping_add(o as u16))
        .find(|&addr| {
            bytes
                .iter()
                .enumerate()
                .all(|(i, &b)| read(addr.wrapping_add(i as u16)) == b)
        })
}

/// First address (scanning all positions from `from`, wrapping) whose decoded
/// instruction text contains `needle` (already lower-cased).
fn find_mnemonic(read: &impl Fn(u16) -> u8, from: u16, needle: &str) -> Option<u16> {
    (0..=0xFFFFu32)
        .map(|o| from.wrapping_add(o as u16))
        .find(|&addr| {
            let bytes = [
                read(addr),
                read(addr.wrapping_add(1)),
                read(addr.wrapping_add(2)),
            ];
            debug::decode(&bytes, addr)
                .text
                .to_ascii_lowercase()
                .contains(needle)
        })
}

/// The next mark strictly after `from` (or, with `forward` false, strictly
/// before), wrapping around the ends. `marks` need not be sorted or unique;
/// used for "go to next/previous bookmark or breakpoint" (Ctrl+N / Ctrl+B).
/// `None` only when `marks` is empty.
#[must_use]
pub fn next_mark(marks: &[u16], from: u16, forward: bool) -> Option<u16> {
    if forward {
        marks
            .iter()
            .copied()
            .filter(|&m| m > from)
            .min()
            .or_else(|| marks.iter().copied().min())
    } else {
        marks
            .iter()
            .copied()
            .filter(|&m| m < from)
            .max()
            .or_else(|| marks.iter().copied().max())
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
