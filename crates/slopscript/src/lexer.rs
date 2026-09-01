// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Tokenizer for slopscript source. Comments (`-- to end of line`),
//! whitespace and newlines carry no meaning past this stage; every surviving
//! token records the 1-based source line it started on, since both parse
//! errors and a `within` expiry report need it.

/// A lexical token plus the 1-based line it started on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokenKind {
    Int(i64),
    Str(String),
    Ident(String),

    Let,
    Proc,
    If,
    Else,
    While,
    Repeat,
    Until,
    For,
    Within,
    And,
    Or,
    Not,
    True,
    False,

    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Assign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    DotDot,

    Eof,
}

fn keyword(word: &str) -> Option<TokenKind> {
    Some(match word {
        "let" => TokenKind::Let,
        "proc" => TokenKind::Proc,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "repeat" => TokenKind::Repeat,
        "until" => TokenKind::Until,
        "for" => TokenKind::For,
        "within" => TokenKind::Within,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        _ => return None,
    })
}

/// Tokenize `src` in full. Errors carry the 1-based line they occurred on.
pub(crate) fn lex(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut out = Vec::new();

    while i < chars.len() {
        let c = chars[i];

        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        let start_line = line;

        if c.is_ascii_digit() || (c == '$' && chars.get(i + 1).is_some_and(char::is_ascii_hexdigit))
        {
            let (kind, next) = lex_number(&chars, i, start_line)?;
            out.push(Token {
                kind,
                line: start_line,
            });
            i = next;
            continue;
        }

        if c == '"' {
            let (s, next) = lex_string(&chars, i, start_line)?;
            out.push(Token {
                kind: TokenKind::Str(s),
                line: start_line,
            });
            i = next;
            continue;
        }

        if c.is_ascii_alphabetic() || c == '_' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let kind = keyword(&word).unwrap_or(TokenKind::Ident(word));
            out.push(Token {
                kind,
                line: start_line,
            });
            i = j;
            continue;
        }

        let (kind, next) = lex_punct(&chars, i, start_line)?;
        out.push(Token {
            kind,
            line: start_line,
        });
        i = next;
    }

    out.push(Token {
        kind: TokenKind::Eof,
        line,
    });
    Ok(out)
}

/// Lex a decimal (`123`), `0x`-hex, or `$`-hex integer literal starting at
/// `i`. Returns the token and the index just past it.
fn lex_number(chars: &[char], i: usize, line: u32) -> Result<(TokenKind, usize), String> {
    let (digits_start, radix) = if chars[i] == '$' {
        (i + 1, 16)
    } else if chars[i] == '0' && matches!(chars.get(i + 1), Some('x') | Some('X')) {
        (i + 2, 16)
    } else {
        (i, 10)
    };
    let is_digit = |c: char| {
        if radix == 16 {
            c.is_ascii_hexdigit()
        } else {
            c.is_ascii_digit()
        }
    };
    let mut j = digits_start;
    while j < chars.len() && is_digit(chars[j]) {
        j += 1;
    }
    if j == digits_start {
        return Err(format!("line {line}: malformed number literal"));
    }
    let text: String = chars[digits_start..j].iter().collect();
    let v = i64::from_str_radix(&text, radix)
        .map_err(|_| format!("line {line}: integer literal out of range"))?;
    Ok((TokenKind::Int(v), j))
}

/// Lex a double-quoted string starting at `i` (the opening `"`). Handles
/// `\n \t \\ \"`; any other escape or an unterminated string is an error.
fn lex_string(chars: &[char], i: usize, line: u32) -> Result<(String, usize), String> {
    let mut j = i + 1;
    let mut s = String::new();
    loop {
        match chars.get(j) {
            None | Some('\n') => return Err(format!("line {line}: unterminated string literal")),
            Some('"') => {
                j += 1;
                break;
            }
            Some('\\') => {
                let esc = chars
                    .get(j + 1)
                    .ok_or_else(|| format!("line {line}: unterminated string literal"))?;
                s.push(match esc {
                    'n' => '\n',
                    't' => '\t',
                    '\\' => '\\',
                    '"' => '"',
                    other => return Err(format!("line {line}: unknown string escape '\\{other}'")),
                });
                j += 2;
            }
            Some(c) => {
                s.push(*c);
                j += 1;
            }
        }
    }
    Ok((s, j))
}

/// Lex one punctuation/operator token starting at `i`, preferring the
/// two-character spellings (`== != <= >= ..`) over their single-character
/// prefixes.
fn lex_punct(chars: &[char], i: usize, line: u32) -> Result<(TokenKind, usize), String> {
    let c = chars[i];
    let two = chars.get(i + 1).copied();
    let (kind, len) = match (c, two) {
        ('=', Some('=')) => (TokenKind::Eq, 2),
        ('!', Some('=')) => (TokenKind::Ne, 2),
        ('<', Some('=')) => (TokenKind::Le, 2),
        ('>', Some('=')) => (TokenKind::Ge, 2),
        ('.', Some('.')) => (TokenKind::DotDot, 2),
        ('{', _) => (TokenKind::LBrace, 1),
        ('}', _) => (TokenKind::RBrace, 1),
        ('(', _) => (TokenKind::LParen, 1),
        (')', _) => (TokenKind::RParen, 1),
        ('[', _) => (TokenKind::LBracket, 1),
        (']', _) => (TokenKind::RBracket, 1),
        (',', _) => (TokenKind::Comma, 1),
        (';', _) => (TokenKind::Semicolon, 1),
        (':', _) => (TokenKind::Colon, 1),
        ('=', _) => (TokenKind::Assign, 1),
        ('<', _) => (TokenKind::Lt, 1),
        ('>', _) => (TokenKind::Gt, 1),
        ('+', _) => (TokenKind::Plus, 1),
        ('-', _) => (TokenKind::Minus, 1),
        ('*', _) => (TokenKind::Star, 1),
        ('/', _) => (TokenKind::Slash, 1),
        ('%', _) => (TokenKind::Percent, 1),
        _ => return Err(format!("line {line}: unexpected character '{c}'")),
    };
    Ok((kind, i + len))
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod tests;
