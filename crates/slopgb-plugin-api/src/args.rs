//! Minimal argument reader for tool plugins. The host hands
//! [`ToolPlugin::call`](crate::ToolPlugin::call) the MCP `arguments` object
//! serialized as compact JSON (e.g. `{"from":"0100","to":"0103"}`); this pulls a
//! string field out of it without a full JSON dependency. Only string values are
//! recognised (every built-in tool argument is a string); other value shapes are
//! skipped. Total — malformed input yields `None`, never a panic.

/// The string value of top-level object field `key`, or `None`.
///
/// ```
/// assert_eq!(
///     slopgb_plugin_api::args::field(r#"{"from":"0100","to":"0103"}"#, "to").as_deref(),
///     Some("0103"),
/// );
/// ```
#[must_use]
pub fn field(json: &str, key: &str) -> Option<String> {
    let b = json.as_bytes();
    let mut i = skip_ws(b, 0);
    if b.get(i) != Some(&b'{') {
        return None;
    }
    i += 1;
    loop {
        i = skip_ws(b, i);
        match b.get(i) {
            Some(b'}') | None => return None,
            Some(b'"') => {}
            _ => return None, // not a well-formed key position
        }
        let (k, ni) = read_string(b, i)?;
        i = skip_ws(b, ni);
        if b.get(i) != Some(&b':') {
            return None;
        }
        i = skip_ws(b, i + 1);
        if b.get(i) == Some(&b'"') {
            let (v, ni) = read_string(b, i)?;
            if k == key {
                return Some(v);
            }
            i = ni;
        } else {
            i = skip_value(b, i);
        }
        i = skip_ws(b, i);
        match b.get(i) {
            Some(b',') => i += 1,
            _ => return None, // '}' or malformed: key not present
        }
    }
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while matches!(b.get(i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        i += 1;
    }
    i
}

/// Read a JSON string starting at the opening quote `b[i]`, returning its
/// unescaped value and the index just past the closing quote.
fn read_string(b: &[u8], mut i: usize) -> Option<(String, usize)> {
    i += 1; // opening quote
    let mut out = String::new();
    loop {
        let c = *b.get(i)?;
        i += 1;
        match c {
            b'"' => return Some((out, i)),
            b'\\' => {
                let e = *b.get(i)?;
                i += 1;
                match e {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'b' => out.push('\u{08}'),
                    b'f' => out.push('\u{0C}'),
                    b'u' => {
                        let hex = b.get(i..i + 4)?;
                        let code = u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        i += 4;
                    }
                    _ => return None,
                }
            }
            // Raw multi-byte UTF-8: copy the sequence through.
            0x80..=0xFF => {
                let start = i - 1;
                while b.get(i).is_some_and(|&x| x >= 0x80) {
                    i += 1;
                }
                out.push_str(std::str::from_utf8(&b[start..i]).ok()?);
            }
            c => out.push(c as char),
        }
    }
}

/// Skip a non-string JSON value (number/bool/null/array/object) at `b[i]`,
/// returning the index just past it. Best-effort and total.
fn skip_value(b: &[u8], mut i: usize) -> usize {
    let mut depth = 0i32;
    while let Some(&c) = b.get(i) {
        match c {
            b'[' | b'{' => depth += 1,
            b']' | b'}' if depth == 0 => break,
            b']' | b'}' => depth -= 1,
            b',' if depth == 0 => break,
            b'"' => {
                if let Some((_, ni)) = read_string(b, i) {
                    i = ni;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
