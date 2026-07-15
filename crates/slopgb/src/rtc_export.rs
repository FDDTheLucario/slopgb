//! VBA-compatible RTC footer for the `.sav` (Options → System → "Save RTC in
//! SAV file (VBA compatible)"). Lets VBA / mGBA / SameBoy read an MBC3 cart's
//! clock out of a slopgb save. The byte layout is exactly what
//! `slopgb_core`'s `load_save_data` parses back: five 4-byte-LE live registers,
//! five 4-byte-LE latched registers, then an 8-byte-LE host timestamp — which
//! the deterministic core writes for other emulators but ignores on load.

/// Build the 48-byte VBA RTC footer from the `(live, latched)` register files
/// (each `[S, M, H, DL, DH]`) and a Unix timestamp in seconds. Each register is
/// stored as a little-endian `u32` (only the low byte is meaningful).
pub(crate) fn vba_footer(live: [u8; 5], latched: [u8; 5], unix_secs: u64) -> [u8; 48] {
    let mut out = [0u8; 48];
    for (i, &r) in live.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&u32::from(r).to_le_bytes());
    }
    for (i, &r) in latched.iter().enumerate() {
        out[20 + 4 * i..20 + 4 * i + 4].copy_from_slice(&u32::from(r).to_le_bytes());
    }
    out[40..48].copy_from_slice(&unix_secs.to_le_bytes());
    out
}

#[cfg(test)]
#[path = "rtc_export_tests.rs"]
mod tests;
