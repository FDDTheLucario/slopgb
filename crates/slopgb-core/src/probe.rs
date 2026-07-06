//! Opt-in SameBoy-port measurement harness.
//!
//! None of this compiles into a default build. The `port_probe` Cargo feature
//! is off by default, so [`probe!`] discards its body and every tune hook below
//! folds to its production default — the timing core stays byte-identical and
//! free of measurement clutter. Build or test with `--features port_probe` and
//! set the `SLOPGB_*` environment variables to re-arm the read traces and the
//! tier2-reclock sweep knobs used during the cc-exact port:
//!
//! * `SLOPGB_S5DBG` / `SLOPGB_ISRTRACE` — enable the read/wake and ISR traces.
//! * `SLOPGB_STOPADV` — override the STOP realignment advance.
//! * `SLOPGB_LCDPH` — inject an LCD-phase offset on a non-DS line.
//! * `SLOPGB_P2TBL` — override the halt LY-phase carry table (4-char string).
//! * `SLOPGB_P2HH` — override the mode-0 halt-hold value.
//! * `SLOPGB_NOXLINE` — disable the cross-line window mode-3 exit arm.
//!
//! Trace bodies live in `#[cfg(feature = "port_probe")]` methods next to the
//! state they read; the hot paths only carry a one-line `probe!(...)` guard.

/// Guard a measurement statement. Expands to its body only under
/// `--features port_probe`; otherwise it discards the tokens (the referenced
/// trace-only methods need not even exist off-feature, so there is zero cost
/// and no dead-code churn in the default build).
#[cfg(feature = "port_probe")]
macro_rules! probe {
    ($($body:tt)*) => { $($body)* };
}
#[cfg(not(feature = "port_probe"))]
macro_rules! probe {
    ($($body:tt)*) => {};
}

/// One-shot `SLOPGB_S5DBG` gate (read/wake traces).
#[cfg(feature = "port_probe")]
pub(crate) fn s5dbg_on() -> bool {
    use std::sync::OnceLock;
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::env::var_os("SLOPGB_S5DBG").is_some())
}

/// One-shot `SLOPGB_ISRTRACE` gate (ISR dispatch traces).
#[cfg(feature = "port_probe")]
pub(crate) fn isrtrace_on() -> bool {
    use std::sync::OnceLock;
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::env::var_os("SLOPGB_ISRTRACE").is_some())
}

/// STOP realignment advance override (`SLOPGB_STOPADV`); returns `default`
/// unless the knob is set.
#[cfg(feature = "port_probe")]
pub(crate) fn tune_stopadv(default: u32) -> u32 {
    std::env::var("SLOPGB_STOPADV")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}
#[cfg(not(feature = "port_probe"))]
#[inline(always)]
pub(crate) fn tune_stopadv(default: u32) -> u32 {
    default
}

/// Halt LY-phase carry-table override (`SLOPGB_P2TBL`, a 4-char digit string
/// indexed by `(cc-1)&3`); returns `default` unless the knob is set.
#[cfg(feature = "port_probe")]
pub(crate) fn tune_p2tbl(default: u8, cc: u8) -> u8 {
    match std::env::var("SLOPGB_P2TBL") {
        Ok(t) if t.len() == 4 => {
            let b = t.as_bytes()[(cc as usize - 1) & 3];
            // Non-digit char in the knob → fall back rather than wrap-subtract.
            if b.is_ascii_digit() { b - b'0' } else { default }
        }
        _ => default,
    }
}
#[cfg(not(feature = "port_probe"))]
#[inline(always)]
pub(crate) fn tune_p2tbl(default: u8, _cc: u8) -> u8 {
    default
}

/// Mode-0 halt-hold override (`SLOPGB_P2HH`); returns `default` unless set.
#[cfg(feature = "port_probe")]
pub(crate) fn tune_p2hh(default: u8) -> u8 {
    std::env::var("SLOPGB_P2HH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
#[cfg(not(feature = "port_probe"))]
#[inline(always)]
pub(crate) fn tune_p2hh(default: u8) -> u8 {
    default
}

/// Whether the cross-line window mode-3 exit arm fires. `SLOPGB_NOXLINE`
/// disables it for measurement; the production default is that it fires.
#[cfg(feature = "port_probe")]
pub(crate) fn noxline_fires() -> bool {
    std::env::var_os("SLOPGB_NOXLINE").is_none()
}
#[cfg(not(feature = "port_probe"))]
#[inline(always)]
pub(crate) fn noxline_fires() -> bool {
    true
}
