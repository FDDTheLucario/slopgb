//! A tiny std-only JSON reader for the SingleStepTests vector files (modeled on
//! the frontend's `mcp/json.rs`, but this copy lives in the CPU crate's test
//! code). Numbers are integral in these files, so they parse as `i64`; `null`
//! (a skipped cycle's data value) becomes `Null`.

/// A parsed JSON value. Objects keep insertion order; `get` is a linear scan
/// (test objects are small).
#[derive(Debug)]
pub enum J {
    Null,
    Int(i64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    /// The value at `key`, or `None` for a non-object / missing key.
    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The integer, if this is `Int`.
    pub fn int(&self) -> i64 {
        match self {
            J::Int(n) => *n,
            other => panic!("expected int, got {other:?}"),
        }
    }

    /// The integer, or `None` for `Null`.
    pub fn opt_int(&self) -> Option<i64> {
        match self {
            J::Null => None,
            J::Int(n) => Some(*n),
            other => panic!("expected int or null, got {other:?}"),
        }
    }

    /// The string slice, if this is `Str`.
    pub fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    /// The array slice, if this is `Arr`.
    pub fn arr(&self) -> &[J] {
        match self {
            J::Arr(items) => items,
            other => panic!("expected array, got {other:?}"),
        }
    }
}

/// Parse a whole JSON document; panics with a byte offset on malformed input
/// (test data, so a hard failure is the right signal).
pub fn parse(src: &str) -> J {
    let mut p = P {
        b: src.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value();
    p.ws();
    assert!(p.i == p.b.len(), "trailing data at byte {}", p.i);
    v
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn value(&mut self) -> J {
        match self.b.get(self.i) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => J::Str(self.string()),
            Some(b'n') => self.lit("null", J::Null),
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.number(),
            other => panic!("unexpected {other:?} at byte {}", self.i),
        }
    }

    fn lit(&mut self, want: &str, v: J) -> J {
        assert!(
            self.b[self.i..].starts_with(want.as_bytes()),
            "bad literal at byte {}",
            self.i
        );
        self.i += want.len();
        v
    }

    fn object(&mut self) -> J {
        self.i += 1; // '{'
        let mut pairs = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            return J::Obj(pairs);
        }
        loop {
            self.ws();
            let key = self.string();
            self.ws();
            assert!(
                self.b.get(self.i) == Some(&b':'),
                "expected ':' at {}",
                self.i
            );
            self.i += 1;
            self.ws();
            pairs.push((key, self.value()));
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return J::Obj(pairs);
                }
                _ => panic!("expected ',' or '}}' at byte {}", self.i),
            }
        }
    }

    fn array(&mut self) -> J {
        self.i += 1; // '['
        let mut items = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
            return J::Arr(items);
        }
        loop {
            self.ws();
            items.push(self.value());
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return J::Arr(items);
                }
                _ => panic!("expected ',' or ']' at byte {}", self.i),
            }
        }
    }

    fn string(&mut self) -> String {
        assert!(
            self.b.get(self.i) == Some(&b'"'),
            "expected string at {}",
            self.i
        );
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = *self.b.get(self.i).expect("unterminated string");
            self.i += 1;
            match c {
                b'"' => return out,
                b'\\' => {
                    let e = *self.b.get(self.i).expect("bad escape");
                    self.i += 1;
                    out.push(match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        _ => panic!("unsupported escape at byte {}", self.i),
                    });
                }
                c => out.push(c as char),
            }
        }
    }

    fn number(&mut self) -> J {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        while self.b.get(self.i).is_some_and(u8::is_ascii_digit) {
            self.i += 1;
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).unwrap();
        J::Int(s.parse().expect("integer vector field"))
    }
}
