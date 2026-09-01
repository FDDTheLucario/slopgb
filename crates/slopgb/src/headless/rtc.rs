// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `gb:set_rtc(epoch)`. `slopgb-core`'s MBC3 RTC is already purely
//! cycle-driven and never reads the host clock — only *setting* it is new,
//! done frontend-side by rewriting the trailing RTC block of the save image
//! and reloading it, with no `slopgb-core` change.

use std::cell::RefCell;
use std::rc::Rc;

use slopscript::{Interp, Value};

use super::Machine;

pub(super) fn register(interp: &mut Interp<'_>, mc: &Rc<RefCell<Machine>>) {
    let m = Rc::clone(mc);
    interp.register_builtin("gb:set_rtc", move |args| set_rtc(&m, args));
}

/// The 16-byte RTC trailer `Cartridge::save_data` appends after cart RAM for
/// an MBC3+RTC cart (`crates/slopgb-core/src/cartridge/save.rs`): live
/// S,M,H,DL,DH; latched S,M,H,DL,DH; a little-endian sub-second T-cycle
/// counter (`u32`); the last latch-register write; one zero pad byte.
const RTC_TRAILER_LEN: usize = 16;

fn set_rtc(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let epoch = match args {
        [Value::Int(e)] if *e >= 0 => *e as u64,
        [Value::Int(e)] => return Err(format!("epoch must be non-negative, got {e}")),
        _ => return Err("expects (epoch)".to_string()),
    };

    let mut m = mc.borrow_mut();
    if m.session.gb.rtc_state().is_none() {
        return Err("cartridge has no RTC".to_string());
    }
    let Some(mut data) = m.session.gb.save_data() else {
        return Err("cartridge has no RTC".to_string());
    };
    if data.len() < RTC_TRAILER_LEN {
        return Err("cartridge save image is too short for an RTC trailer".to_string());
    }

    // Pan Docs "MBC3": S 0-59, M 0-59, H 0-23, a 9-bit day counter (DL + DH
    // bit 0), DH bit 7 the sticky day-counter carry, DH bit 6 halt (left
    // clear — the clock is never halted by this). A realistic Unix epoch
    // always overflows the 9-bit day counter, so the day field this writes
    // is `days mod 512` with the carry flag set — what the hardware
    // register can physically represent; seconds/minutes/hours stay exact.
    let s = (epoch % 60) as u8;
    let mi = (epoch / 60 % 60) as u8;
    let h = (epoch / 3600 % 24) as u8;
    let days = epoch / 86400;
    let dl = (days & 0xFF) as u8;
    let mut dh = ((days >> 8) & 1) as u8;
    if days > 511 {
        dh |= 0x80;
    }
    let regs = [s, mi, h, dl, dh];

    // Live and latched both get the same values; sub-second/latch_prev/pad
    // are zeroed, matching a fresh latch taken exactly at `epoch`.
    let start = data.len() - RTC_TRAILER_LEN;
    data[start..start + 5].copy_from_slice(&regs);
    data[start + 5..start + 10].copy_from_slice(&regs);
    data[start + 10..start + 14].copy_from_slice(&0u32.to_le_bytes());
    data[start + 14] = 0;
    data[start + 15] = 0;

    if !m.session.gb.load_save_data(&data) {
        return Err("failed to reload the patched RTC save image".to_string());
    }
    Ok(Value::Nil)
}
