//! Minimal std-only JSON: a value type with an escaping renderer and a total
//! recursive-descent parser — enough for the MCP JSON-RPC envelope. No serde
//! (the frontend stays winit/softbuffer/cpal-only, like `link.rs` /
//! `settings_file`). The parser is defensive: it caps nesting depth so a crafted
//! deeply-nested request can't blow the stack (an untrusted-network boundary).

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A JSON value. Objects keep insertion order (a `Vec` of pairs) so rendered
/// responses read predictably; `get` does a linear scan (objects here are tiny).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// Numbers are stored as `f64`; integral values render without a decimal
    /// point (so a JSON-RPC `id` of `1` echoes back as `1`, not `1.0`).
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// A string value; convenience for building responses.
    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    /// Build an object from key/value pairs.
    pub fn obj<const N: usize>(pairs: [(&str, Json); N]) -> Json {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    /// The value at `key` in an object, or `None` (also `None` for non-objects).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The string, if this is a `Str`.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Render to a compact JSON string.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if !n.is_finite() {
                    // JSON has no inf/nan; `null` is the standard fallback (and
                    // what serde_json emits). Non-finite is wire-reachable via an
                    // overflowing id like `1e999`, so this must never render `inf`.
                    out.push_str("null");
                } else if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{n}");
                }
            }
            Json::Str(s) => write_escaped(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Json::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parse a JSON document. Total: any malformed input is an `Err(String)`,
/// never a panic. Trailing non-whitespace is an error.
pub fn parse(s: &str) -> Result<Json, String> {
    let mut p = P {
        b: s.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("trailing data at byte {}", p.i));
    }
    Ok(v)
}

/// Max object/array nesting the parser accepts before bailing — a stack-safety
/// bound against adversarial deeply-nested input, far above any real request.
const MAX_DEPTH: u32 = 64;

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while let Some(c) = self.b.get(self.i) {
            if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn value(&mut self, depth: u32) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("nesting too deep".into());
        }
        match self.b.get(self.i) {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'n') => self.lit("null", Json::Null),
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected '{}' at byte {}", *c as char, self.i)),
            None => Err("unexpected end of input".into()),
        }
    }

    fn lit(&mut self, want: &str, v: Json) -> Result<Json, String> {
        if self.b[self.i..].starts_with(want.as_bytes()) {
            self.i += want.len();
            Ok(v)
        } else {
            Err(format!("invalid literal at byte {}", self.i))
        }
    }

    fn object(&mut self, depth: u32) -> Result<Json, String> {
        self.i += 1; // '{'
        let mut pairs = Vec::new();
        let mut seen = BTreeMap::new();
        self.ws();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.ws();
            if self.b.get(self.i) != Some(&b'"') {
                return Err(format!("expected object key at byte {}", self.i));
            }
            let key = self.string()?;
            self.ws();
            if self.b.get(self.i) != Some(&b':') {
                return Err(format!("expected ':' at byte {}", self.i));
            }
            self.i += 1;
            self.ws();
            let val = self.value(depth + 1)?;
            // Last-writer-wins on duplicate keys (a defensive, total policy).
            if let Some(&idx) = seen.get(&key) {
                pairs[idx] = (key, val);
            } else {
                seen.insert(key.clone(), pairs.len());
                pairs.push((key, val));
            }
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Obj(pairs));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.i)),
            }
        }
    }

    fn array(&mut self, depth: u32) -> Result<Json, String> {
        self.i += 1; // '['
        let mut items = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.i)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // opening quote
        let mut out = String::new();
        loop {
            let c = *self.b.get(self.i).ok_or("unterminated string")?;
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or("bad escape")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0C}'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err("bad escape".into()),
                    }
                }
                // A raw multi-byte UTF-8 sequence: copy its bytes through.
                0x80..=0xFF => {
                    let start = self.i - 1;
                    while self.b.get(self.i).is_some_and(|b| *b >= 0x80) {
                        self.i += 1;
                    }
                    match std::str::from_utf8(&self.b[start..self.i]) {
                        Ok(s) => out.push_str(s),
                        Err(_) => return Err("invalid UTF-8 in string".into()),
                    }
                }
                c => out.push(c as char),
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let hex = self
            .b
            .get(self.i..self.i + 4)
            .ok_or("truncated \\u escape")?;
        let code = u32::from_str_radix(std::str::from_utf8(hex).map_err(|_| "bad \\u escape")?, 16)
            .map_err(|_| "bad \\u escape")?;
        self.i += 4;
        // Surrogates aren't reassembled (no MCP field needs them); map to the
        // replacement char rather than erroring, so parsing stays total.
        Ok(char::from_u32(code).unwrap_or('\u{FFFD}'))
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        while self
            .b
            .get(self.i)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            self.i += 1;
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).map_err(|_| "bad number")?;
        s.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("bad number '{s}'"))
    }
}

#[cfg(test)]
#[path = "json_tests.rs"]
mod tests;
