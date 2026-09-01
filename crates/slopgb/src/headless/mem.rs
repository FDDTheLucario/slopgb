// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `gb:read` / `gb:poke`: symbol- or bank/addr-addressed memory access via
//! [`slopgb_core::GameBoy::debug_read_banked`] /
//! [`slopgb_core::GameBoy::debug_write_banked`] (`&self`/gated `&mut self`
//! debug introspection — golden-safe, see `docs/headless-plan.md`
//! "Golden-safe"). A stale `.sym` corrupts a symbol-addressed read exactly
//! as badly as a write, so both shapes get the same bank/address resolution
//! and range checks.

use std::cell::RefCell;
use std::rc::Rc;

use slopscript::{Interp, Value};

use crate::mcp::addr::Region;

use super::Machine;

pub(super) fn register(interp: &mut Interp<'_>, mc: &Rc<RefCell<Machine>>) {
    let m = Rc::clone(mc);
    interp.register_builtin("gb:read", move |args| read(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:poke", move |args| poke(&m, args));
}

fn resolve_symbol(mc: &Rc<RefCell<Machine>>, name: &str) -> Result<(u16, u16), String> {
    mc.borrow()
        .syms
        .resolve(name)
        .ok_or_else(|| format!("unknown symbol '{name}'"))
}

fn resolve_bank_addr(bank: i64, addr: i64) -> Result<(u16, u16), String> {
    let bank = u16::try_from(bank).map_err(|_| format!("bank {bank} out of range 0..=65535"))?;
    let addr = addr_in_range(addr)?;
    Ok((bank, addr))
}

fn addr_in_range(addr: i64) -> Result<u16, String> {
    u16::try_from(addr).map_err(|_| format!("address {addr:#x} out of range 0x0000..=0xFFFF"))
}

/// Validate a multi-byte run starting at `addr`: `n` must be non-negative,
/// `addr + n` must not wrap past `0xFFFF`, and the run must not leave the
/// region `addr` started in (reusing `mcp::addr::Region`, the same
/// region table the `peek`/`disassemble` MCP tools enforce a range against)
/// — a run that would is a script bug, not a silent read/write into whatever
/// memory happens to sit next door (e.g. `gb:read(1, 0x7FFE, 8)` starting in
/// ROMX and running on into VRAM).
fn run_len(addr: u16, n: i64) -> Result<u16, String> {
    if n < 0 {
        return Err(format!("byte count {n} must be non-negative"));
    }
    if n == 0 {
        return Ok(0);
    }
    let end = u32::from(addr) + n as u32;
    if end > 0x1_0000 {
        return Err(format!(
            "address {addr:#06X} + {n} bytes would wrap past 0xFFFF"
        ));
    }
    let last = (end - 1) as u16;
    let (start_region, end_region) = (Region::of(addr), Region::of(last));
    if start_region != end_region {
        return Err(format!(
            "address {addr:#06X} + {n} bytes ({last:#06X}) leaves the {start_region:?} region \
             it started in (now {end_region:?}) — split the read/poke at the region boundary"
        ));
    }
    Ok(n as u16)
}

fn read_byte(mc: &Rc<RefCell<Machine>>, bank: u16, addr: u16) -> u8 {
    mc.borrow().session.gb.debug_read_banked(bank, addr)
}

fn read_run(mc: &Rc<RefCell<Machine>>, bank: u16, addr: u16, n: i64) -> Result<Value, String> {
    let len = run_len(addr, n)?;
    let m = mc.borrow();
    let bytes = (0..len)
        .map(|i| m.session.gb.debug_read_banked(bank, addr + i))
        .collect();
    Ok(Value::Bytes(bytes))
}

fn read(mc: &Rc<RefCell<Machine>>, a: &[Value]) -> Result<Value, String> {
    match a {
        [Value::Str(name)] => {
            let (bank, addr) = resolve_symbol(mc, name)?;
            Ok(Value::Int(i64::from(read_byte(mc, bank, addr))))
        }
        [Value::Str(name), Value::Int(n)] => {
            let (bank, addr) = resolve_symbol(mc, name)?;
            read_run(mc, bank, addr, *n)
        }
        [Value::Int(bank), Value::Int(addr)] => {
            let (bank, addr) = resolve_bank_addr(*bank, *addr)?;
            Ok(Value::Int(i64::from(read_byte(mc, bank, addr))))
        }
        [Value::Int(bank), Value::Int(addr), Value::Int(n)] => {
            let (bank, addr) = resolve_bank_addr(*bank, *addr)?;
            read_run(mc, bank, addr, *n)
        }
        _ => Err("expects (symbol), (symbol, n), (bank, addr) or (bank, addr, n)".to_string()),
    }
}

fn poke_byte(mc: &Rc<RefCell<Machine>>, bank: u16, addr: u16, v: i64) -> Result<Value, String> {
    if !(0..=255).contains(&v) {
        return Err(format!("poke value {v} out of range 0..=255"));
    }
    mc.borrow_mut()
        .session
        .gb
        .debug_write_banked(bank, addr, v as u8);
    Ok(Value::Nil)
}

fn poke_run(
    mc: &Rc<RefCell<Machine>>,
    bank: u16,
    addr: u16,
    bytes: &[u8],
) -> Result<Value, String> {
    // `bytes.len()` is exactly the run length already (a `Bytes` value, not
    // a separate count) — `run_len` here is only the wrap/region check, not
    // a truncation; a wrap or region-crossing write is an error, never
    // silently short-written.
    run_len(addr, bytes.len() as i64)?;
    let mut m = mc.borrow_mut();
    for (i, b) in bytes.iter().enumerate() {
        m.session.gb.debug_write_banked(bank, addr + i as u16, *b);
    }
    Ok(Value::Nil)
}

fn poke(mc: &Rc<RefCell<Machine>>, a: &[Value]) -> Result<Value, String> {
    match a {
        [Value::Str(name), Value::Int(v)] => {
            let (bank, addr) = resolve_symbol(mc, name)?;
            poke_byte(mc, bank, addr, *v)
        }
        [Value::Str(name), Value::Bytes(bytes)] => {
            let (bank, addr) = resolve_symbol(mc, name)?;
            poke_run(mc, bank, addr, bytes)
        }
        [Value::Int(bank), Value::Int(addr), Value::Int(v)] => {
            let (bank, addr) = resolve_bank_addr(*bank, *addr)?;
            poke_byte(mc, bank, addr, *v)
        }
        [Value::Int(bank), Value::Int(addr), Value::Bytes(bytes)] => {
            let (bank, addr) = resolve_bank_addr(*bank, *addr)?;
            poke_run(mc, bank, addr, bytes)
        }
        _ => Err(
            "expects (symbol, v), (bank, addr, v) — v an Int 0..=255 or a Bytes run".to_string(),
        ),
    }
}
