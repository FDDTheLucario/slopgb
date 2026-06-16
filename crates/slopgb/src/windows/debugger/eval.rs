//! A small expression evaluator for the debugger's "Evaluate expression" (RM14).
//! bgb-style: numbers are hex by default; registers take precedence over hex
//! (so `bc` is the register pair, `ff` is `0x00FF`); `[x]` reads one byte.
//!
//! ```text
//!   expr   := term (('+' | '-') term)*
//!   term   := factor ('*' factor)*
//!   factor := number | register | '[' expr ']' | '(' expr ')'
//! ```
//!
//! All arithmetic wraps in `u16`. The evaluator is **total**: every malformed
//! input returns an `Err(String)` (shown to the user) — it never panics.

use slopgb_core::Registers;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tok<'a> {
    Word(&'a str),
    Plus,
    Minus,
    Star,
    LParen,
    RParen,
    LBrack,
    RBrack,
}

fn tokenize(s: &str) -> Result<Vec<Tok<'_>>, String> {
    let mut toks = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' => i += 1,
            b'+' => push(&mut toks, Tok::Plus, &mut i),
            b'-' => push(&mut toks, Tok::Minus, &mut i),
            b'*' => push(&mut toks, Tok::Star, &mut i),
            b'(' => push(&mut toks, Tok::LParen, &mut i),
            b')' => push(&mut toks, Tok::RParen, &mut i),
            b'[' => push(&mut toks, Tok::LBrack, &mut i),
            b']' => push(&mut toks, Tok::RBrack, &mut i),
            _ if c.is_ascii_alphanumeric() => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                toks.push(Tok::Word(&s[start..i]));
            }
            _ => return Err(format!("unexpected character '{}'", c as char)),
        }
    }
    Ok(toks)
}

fn push<'a>(toks: &mut Vec<Tok<'a>>, t: Tok<'a>, i: &mut usize) {
    toks.push(t);
    *i += 1;
}

struct Parser<'a, F> {
    toks: Vec<Tok<'a>>,
    pos: usize,
    regs: &'a Registers,
    read: F,
}

impl<'a, F: Fn(u16) -> u8> Parser<'a, F> {
    fn peek(&self) -> Option<Tok<'a>> {
        self.toks.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<Tok<'a>> {
        let t = self.peek();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expr(&mut self) -> Result<u16, String> {
        let mut v = self.term()?;
        while let Some(t) = self.peek() {
            match t {
                Tok::Plus => {
                    self.pos += 1;
                    v = v.wrapping_add(self.term()?);
                }
                Tok::Minus => {
                    self.pos += 1;
                    v = v.wrapping_sub(self.term()?);
                }
                _ => break,
            }
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<u16, String> {
        let mut v = self.factor()?;
        while let Some(Tok::Star) = self.peek() {
            self.pos += 1;
            v = v.wrapping_mul(self.factor()?);
        }
        Ok(v)
    }

    fn factor(&mut self) -> Result<u16, String> {
        match self.bump() {
            Some(Tok::Word(w)) => self.word_value(w),
            Some(Tok::LParen) => {
                let v = self.expr()?;
                self.expect(Tok::RParen, ")")?;
                Ok(v)
            }
            Some(Tok::LBrack) => {
                let addr = self.expr()?;
                self.expect(Tok::RBrack, "]")?;
                Ok(u16::from((self.read)(addr)))
            }
            Some(t) => Err(format!("unexpected token {t:?}")),
            None => Err("unexpected end of expression".into()),
        }
    }

    fn expect(&mut self, want: Tok, label: &str) -> Result<(), String> {
        if self.bump() == Some(want) {
            Ok(())
        } else {
            Err(format!("expected '{label}'"))
        }
    }

    /// A word is a register name (case-insensitive, taking precedence) or a hex
    /// literal.
    fn word_value(&self, w: &str) -> Result<u16, String> {
        let r = self.regs;
        Ok(match w.to_ascii_lowercase().as_str() {
            "af" => r.af(),
            "bc" => r.bc(),
            "de" => r.de(),
            "hl" => r.hl(),
            "sp" => r.sp,
            "pc" => r.pc,
            "a" => r.af() >> 8,
            "f" => r.af() & 0xFF,
            "b" => r.bc() >> 8,
            "c" => r.bc() & 0xFF,
            "d" => r.de() >> 8,
            "e" => r.de() & 0xFF,
            "h" => r.hl() >> 8,
            "l" => r.hl() & 0xFF,
            _ => u16::from_str_radix(w, 16).map_err(|_| format!("unknown token '{w}'"))?,
        })
    }
}

/// Evaluate `s` against the live registers + memory (`read`), returning the
/// 16-bit value or a human-readable error.
pub fn eval_expr(s: &str, regs: &Registers, read: impl Fn(u16) -> u8) -> Result<u16, String> {
    let toks = tokenize(s)?;
    if toks.is_empty() {
        return Err("empty expression".into());
    }
    let mut p = Parser {
        toks,
        pos: 0,
        regs,
        read,
    };
    let v = p.expr()?;
    if p.pos != p.toks.len() {
        return Err("trailing tokens".into());
    }
    Ok(v)
}

#[cfg(test)]
#[path = "eval_tests.rs"]
mod tests;
