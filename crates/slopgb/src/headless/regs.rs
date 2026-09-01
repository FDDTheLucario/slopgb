// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `gb:reg` / `gb:set_reg`. [`slopgb_core::DebugReg`] only writes the six
//! 16-bit pairs, so an 8-bit half is written by reading its pair, splicing
//! the byte into the high or low half, and writing the pair back
//! (`docs/headless-plan.md`, "Builtins to register").

use std::cell::RefCell;
use std::rc::Rc;

use slopgb_core::DebugReg;
use slopscript::{Interp, Value};

use super::consts::RegSlot;
use super::{Machine, consts};

pub(super) fn register(interp: &mut Interp<'_>, mc: &Rc<RefCell<Machine>>) {
    let m = Rc::clone(mc);
    interp.register_builtin("gb:reg", move |args| reg(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:set_reg", move |args| set_reg(&m, args));
}

fn reg(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let [r] = args else {
        return Err(format!("expects (R), got {} argument(s)", args.len()));
    };
    let slot = consts::decode_reg(r)?;
    let regs = mc.borrow().session.gb.cpu_regs();
    Ok(Value::Int(match slot {
        RegSlot::A => i64::from(regs.a),
        RegSlot::F => i64::from(regs.f()),
        RegSlot::B => i64::from(regs.b),
        RegSlot::C => i64::from(regs.c),
        RegSlot::D => i64::from(regs.d),
        RegSlot::E => i64::from(regs.e),
        RegSlot::H => i64::from(regs.h),
        RegSlot::L => i64::from(regs.l),
        RegSlot::Af => i64::from(regs.af()),
        RegSlot::Bc => i64::from(regs.bc()),
        RegSlot::De => i64::from(regs.de()),
        RegSlot::Hl => i64::from(regs.hl()),
        RegSlot::Sp => i64::from(regs.sp),
        RegSlot::Pc => i64::from(regs.pc),
    }))
}

fn set_reg(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let [r, v] = args else {
        return Err(format!("expects (R, v), got {} argument(s)", args.len()));
    };
    let slot = consts::decode_reg(r)?;
    let Value::Int(v) = *v else {
        return Err(format!("register value must be an Int, got {}", v.kind()));
    };
    let mut m = mc.borrow_mut();
    let regs = m.session.gb.cpu_regs();
    let (pair, value) = match slot {
        RegSlot::A => (DebugReg::Af, splice_hi(regs.af(), v)?),
        RegSlot::F => (DebugReg::Af, splice_lo(regs.af(), v)?),
        RegSlot::B => (DebugReg::Bc, splice_hi(regs.bc(), v)?),
        RegSlot::C => (DebugReg::Bc, splice_lo(regs.bc(), v)?),
        RegSlot::D => (DebugReg::De, splice_hi(regs.de(), v)?),
        RegSlot::E => (DebugReg::De, splice_lo(regs.de(), v)?),
        RegSlot::H => (DebugReg::Hl, splice_hi(regs.hl(), v)?),
        RegSlot::L => (DebugReg::Hl, splice_lo(regs.hl(), v)?),
        RegSlot::Af => (DebugReg::Af, int_to_u16(v, 0xFFFF)?),
        RegSlot::Bc => (DebugReg::Bc, int_to_u16(v, 0xFFFF)?),
        RegSlot::De => (DebugReg::De, int_to_u16(v, 0xFFFF)?),
        RegSlot::Hl => (DebugReg::Hl, int_to_u16(v, 0xFFFF)?),
        RegSlot::Sp => (DebugReg::Sp, int_to_u16(v, 0xFFFF)?),
        RegSlot::Pc => (DebugReg::Pc, int_to_u16(v, 0xFFFF)?),
    };
    m.session.gb.debug_set_reg(pair, value);
    Ok(Value::Nil)
}

fn splice_hi(pair: u16, byte: i64) -> Result<u16, String> {
    let b = int_to_u16(byte, 0xFF)?;
    Ok((b << 8) | (pair & 0x00FF))
}

fn splice_lo(pair: u16, byte: i64) -> Result<u16, String> {
    let b = int_to_u16(byte, 0xFF)?;
    Ok((pair & 0xFF00) | b)
}

fn int_to_u16(v: i64, max: u16) -> Result<u16, String> {
    if v < 0 || v > i64::from(max) {
        return Err(format!("value {v} out of range 0..={max}"));
    }
    Ok(v as u16)
}
