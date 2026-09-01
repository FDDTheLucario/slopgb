// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

use super::*;

fn kinds(src: &str) -> Vec<TokenKind> {
    lex(src).unwrap().into_iter().map(|t| t.kind).collect()
}

#[test]
fn comments_are_stripped() {
    assert_eq!(
        kinds("1 -- a comment\n2"),
        vec![TokenKind::Int(1), TokenKind::Int(2), TokenKind::Eof]
    );
    // a comment with nothing after it on the last line
    assert_eq!(kinds("-- only a comment"), vec![TokenKind::Eof]);
}

#[test]
fn hex_prefixes_agree() {
    assert_eq!(kinds("0x1F"), vec![TokenKind::Int(31), TokenKind::Eof]);
    assert_eq!(kinds("$1F"), vec![TokenKind::Int(31), TokenKind::Eof]);
    assert_eq!(kinds("$1f"), vec![TokenKind::Int(31), TokenKind::Eof]);
}

#[test]
fn decimal_ints() {
    assert_eq!(kinds("0"), vec![TokenKind::Int(0), TokenKind::Eof]);
    assert_eq!(kinds("70224"), vec![TokenKind::Int(70224), TokenKind::Eof]);
}

#[test]
fn string_escapes() {
    let toks = lex(r#""a\nb\tc\\d\"e""#).unwrap();
    assert_eq!(toks[0].kind, TokenKind::Str("a\nb\tc\\d\"e".to_string()));
}

#[test]
fn unterminated_string_names_line() {
    let err = lex("let s = \"abc").unwrap_err();
    assert!(err.contains("line 1"), "{err}");
}

#[test]
fn line_numbers_advance_across_newlines_and_comments() {
    let toks = lex("1\n-- comment\n2\n\n3").unwrap();
    let lines: Vec<u32> = toks.iter().map(|t| t.line).collect();
    // 1 (line 1), 2 (line 3), 3 (line 5), Eof (line 5)
    assert_eq!(lines, vec![1, 3, 5, 5]);
}

#[test]
fn reserved_words_lex_as_keywords_not_identifiers() {
    let words = [
        "let", "proc", "if", "else", "while", "repeat", "until", "for", "within", "and", "or",
        "not", "true", "false",
    ];
    for w in words {
        let toks = lex(w).unwrap();
        assert_ne!(
            toks[0].kind,
            TokenKind::Ident(w.to_string()),
            "{w} lexed as an identifier"
        );
    }
    // a non-reserved word does lex as an identifier
    assert_eq!(
        kinds("gameState"),
        vec![TokenKind::Ident("gameState".to_string()), TokenKind::Eof]
    );
}

#[test]
fn punctuation_and_operators() {
    assert_eq!(
        kinds("{ } ( ) [ ] , ; : = == != < <= > >= + - * / % .."),
        vec![
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBracket,
            TokenKind::RBracket,
            TokenKind::Comma,
            TokenKind::Semicolon,
            TokenKind::Colon,
            TokenKind::Assign,
            TokenKind::Eq,
            TokenKind::Ne,
            TokenKind::Lt,
            TokenKind::Le,
            TokenKind::Gt,
            TokenKind::Ge,
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::DotDot,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn namespaced_call_tokens() {
    assert_eq!(
        kinds("gb:read(1)"),
        vec![
            TokenKind::Ident("gb".to_string()),
            TokenKind::Colon,
            TokenKind::Ident("read".to_string()),
            TokenKind::LParen,
            TokenKind::Int(1),
            TokenKind::RParen,
            TokenKind::Eof,
        ]
    );
}
