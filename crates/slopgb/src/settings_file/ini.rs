//! std-only ordered-line INI model for bgb.ini compatibility. bgb's file is a
//! flat `Key=Value` list (no `[section]` headers), CRLF-terminated, with some
//! keys repeated as a list (`ColorScheme=`, `Recent0..9` are distinct keys).
//!
//! The model preserves every line verbatim — comments, blanks, and the ~250
//! keys slopgb doesn't model — so writing the file back never corrupts the
//! user's bgb config. Only keys we [`set`](Ini::set) are re-rendered; everything
//! else round-trips byte-for-byte. See `docs/settings-persistence-plan.md`.

/// One physical line: a recognized `key=value` pair, or a verbatim passthrough
/// (comment / blank / anything without a `=`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Line {
    Pair { key: String, val: String },
    Raw(String),
}

/// A parsed bgb-style INI, preserving line order + terminator + trailing EOL so
/// an unmodified file serializes byte-identically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ini {
    lines: Vec<Line>,
    /// The file's line terminator (`\r\n` for bgb, `\n` otherwise).
    eol: &'static str,
    /// Whether the source ended with a trailing terminator (bgb's does).
    trailing_eol: bool,
}

impl Ini {
    /// Parse `text` into the ordered-line model. A line splits into a `Pair` at
    /// the first `=` (bgb writes `key=val` with no spaces); a line with no `=`
    /// is kept as `Raw`. The terminator is detected from the text (`\r\n` if any
    /// CRLF is present, else `\n`).
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing_eol = text.ends_with('\n');
        // Split on '\n' and strip a trailing '\r' so both CRLF and LF work; the
        // final empty segment from a trailing newline is dropped (re-added on
        // serialize via `trailing_eol`).
        let mut raw: Vec<&str> = text.split('\n').collect();
        if trailing_eol {
            raw.pop();
        }
        let lines = raw
            .into_iter()
            .map(|l| {
                let l = l.strip_suffix('\r').unwrap_or(l);
                match l.split_once('=') {
                    // A leading '=' (empty key) is not a real pair — keep raw.
                    Some((k, v)) if !k.is_empty() => Line::Pair {
                        key: k.to_string(),
                        val: v.to_string(),
                    },
                    _ => Line::Raw(l.to_string()),
                }
            })
            .collect();
        Self { lines, eol, trailing_eol }
    }

    /// Serialize back to text, byte-identical to the parsed source for any line
    /// not touched by [`set`](Self::set).
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push_str(self.eol);
            }
            match line {
                Line::Pair { key, val } => {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(val);
                }
                Line::Raw(r) => out.push_str(r),
            }
        }
        if self.trailing_eol {
            out.push_str(self.eol);
        }
        out
    }

    /// The value of the first `key` occurrence, or `None`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|l| match l {
            Line::Pair { key: k, val } if k == key => Some(val.as_str()),
            _ => None,
        })
    }

    /// Set `key` to `val`: overwrite the first occurrence in place (preserving
    /// its position), or append a new pair at the end. Only touched keys change;
    /// the rest of the file is untouched.
    pub fn set(&mut self, key: &str, val: &str) {
        for line in &mut self.lines {
            if let Line::Pair { key: k, val: v } = line {
                if k == key {
                    *v = val.to_string();
                    return;
                }
            }
        }
        self.lines.push(Line::Pair {
            key: key.to_string(),
            val: val.to_string(),
        });
    }
}

// --- typed value codecs (bgb's encodings) ----------------------------------

/// bgb boolean: `"1"` is true, everything else (`"0"`, empty) false.
#[must_use]
pub fn parse_bool(v: &str) -> bool {
    v == "1"
}

/// Encode a bool the way bgb writes it.
#[must_use]
pub fn fmt_bool(b: bool) -> &'static str {
    if b {
        "1"
    } else {
        "0"
    }
}

/// Swap a COLORREF (`0x00BBGGRR`, bgb's byte order) to/from our XRGB
/// (`0x00RRGGBB`). Symmetric, so one fn does both directions.
#[must_use]
pub fn swap_bgr_rgb(v: u32) -> u32 {
    let b = (v >> 16) & 0xFF;
    let g = (v >> 8) & 0xFF;
    let r = v & 0xFF;
    (r << 16) | (g << 8) | b
}

/// Decode a `Color0..15` value (BGR hex, no `0x`, e.g. `CCFCE8`) to our XRGB;
/// `None` on a malformed value.
#[must_use]
pub fn parse_color_hex(v: &str) -> Option<u32> {
    u32::from_str_radix(v.trim(), 16).ok().map(swap_bgr_rgb)
}

/// Encode our XRGB as a `Color0..15` BGR-hex value (6 upper-case digits).
#[must_use]
pub fn fmt_color_hex(xrgb: u32) -> String {
    format!("{:06X}", swap_bgr_rgb(xrgb) & 0xFF_FFFF)
}

// (Decimal-COLORREF codec for the `Debug*Color` theme keys + repeated-key
// `get_all` for the `ColorScheme` list are deferred — no Settings field maps to
// them yet. Re-add with the debugger-theme / scheme-list mapping task.)

#[cfg(test)]
#[path = "ini_tests.rs"]
mod tests;
