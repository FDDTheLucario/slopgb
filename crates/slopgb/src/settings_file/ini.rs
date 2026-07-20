//! std-only ordered-line INI model for bgb.ini compatibility. bgb's file is a
//! flat `Key=Value` list (no `[section]` headers), CRLF-terminated, with some
//! keys repeated as a list (`ColorScheme=`, `Recent0..9` are distinct keys).
//!
//! The model preserves every line verbatim — comments, blanks, and the ~250
//! keys slopgb doesn't model — so writing the file back never corrupts the
//! user's bgb config. Only keys we [`set`](Ini::set) are re-rendered; everything
//! else round-trips byte-for-byte.

/// The content of one physical line: a recognized `key=value` pair, or a
/// verbatim passthrough (comment / blank / anything without a `=`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum LineKind {
    Pair { key: String, val: String },
    Raw(String),
}

/// One physical line: its content plus the terminator that followed it in the
/// source — `"\r\n"`, `"\n"`, or `""` for the final line of a file with no
/// trailing newline. The terminator is stored PER LINE (not once per file) so a
/// mixed-ending file round-trips byte-for-byte: bgb writes CRLF, but a line
/// hand-edited on a Unix tool can be lone-LF, and both must survive verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    kind: LineKind,
    eol: &'static str,
}

/// A parsed bgb-style INI, preserving line order + each line's terminator so an
/// unmodified file serializes byte-identically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ini {
    lines: Vec<Line>,
    /// Terminator for lines appended by [`set`](Self::set) (a key bgb didn't
    /// have): the file's dominant style — `\r\n` if any CRLF is present, else
    /// `\n`.
    default_eol: &'static str,
}

impl Ini {
    /// Parse `text` into the ordered-line model. A line splits into a `Pair` at
    /// the first `=` (bgb writes `key=val` with no spaces); a line with no `=`
    /// is kept as `Raw`. Each line records its own terminator (CRLF or LF), so
    /// both endings — even mixed within one file — round-trip verbatim.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let default_eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        // `split_inclusive` keeps each line's trailing '\n'; the final segment
        // has none iff the file did not end with a newline. An empty input
        // yields no segments (serializes back to "").
        let lines = text
            .split_inclusive('\n')
            .map(|seg| {
                let (content, eol) = if let Some(c) = seg.strip_suffix("\r\n") {
                    (c, "\r\n")
                } else if let Some(c) = seg.strip_suffix('\n') {
                    (c, "\n")
                } else {
                    (seg, "")
                };
                let kind = match content.split_once('=') {
                    // A leading '=' (empty key) is not a real pair — keep raw.
                    Some((k, v)) if !k.is_empty() => LineKind::Pair {
                        key: k.to_string(),
                        val: v.to_string(),
                    },
                    _ => LineKind::Raw(content.to_string()),
                };
                Line { kind, eol }
            })
            .collect();
        Self { lines, default_eol }
    }

    /// Serialize back to text, byte-identical to the parsed source for any line
    /// not touched by [`set`](Self::set) — each line re-emits its own stored
    /// terminator.
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match &line.kind {
                LineKind::Pair { key, val } => {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(val);
                }
                LineKind::Raw(r) => out.push_str(r),
            }
            out.push_str(line.eol);
        }
        out
    }

    /// The value of the first `key` occurrence, or `None`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|l| match &l.kind {
            LineKind::Pair { key: k, val } if k == key => Some(val.as_str()),
            _ => None,
        })
    }

    /// Set `key` to `val`: overwrite the first occurrence in place (preserving
    /// its position AND its terminator), or append a new pair at the end. Only
    /// touched keys change; the rest of the file is untouched.
    pub fn set(&mut self, key: &str, val: &str) {
        for line in &mut self.lines {
            if let LineKind::Pair { key: k, val: v } = &mut line.kind {
                if k == key {
                    *v = val.to_string();
                    return;
                }
            }
        }
        // Absent → append, preserving the file's trailing-newline state: if the
        // last line lacked a terminator, give it one (so the new pair stands
        // alone) and leave the new pair unterminated; otherwise terminate the
        // new pair the file's dominant way.
        let eol = match self.lines.last_mut() {
            Some(last) if last.eol.is_empty() => {
                last.eol = self.default_eol;
                ""
            }
            _ => self.default_eol,
        };
        self.lines.push(Line {
            kind: LineKind::Pair {
                key: key.to_string(),
                val: val.to_string(),
            },
            eol,
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
    if b { "1" } else { "0" }
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
// `get_all` for the `ColorScheme` list are omitted — no Settings field maps to
// those keys.)

#[cfg(test)]
#[path = "ini_tests.rs"]
mod tests;
