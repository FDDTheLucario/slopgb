// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Value rendering for `print()`/`hex()`. `hex()`'s byte format — uppercase,
//! space-joined, no `0x`, minimum two digits per byte — matches
//! `crates/slopgb/src/mcp/tools.rs`'s `peek_range`/`dump_rows`, so a script
//! and the MCP `peek` tool read the same bytes the same way.

use crate::Value;

pub(super) fn format_value(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Bytes(b) => hex_bytes(b),
        Value::Nil => "nil".to_string(),
    }
}

pub(super) fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn hex_int(n: i64) -> String {
    format!("{n:02X}")
}
