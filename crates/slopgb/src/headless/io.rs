// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! `gb:load_rom` / `gb:load_battery` / `gb:save_battery` / `gb:save_state` /
//! `gb:load_state` / `gb:load_symbols` / `gb:screenshot` — every builtin that touches a file.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use slopgb_core::{SCREEN_H, SCREEN_W};
use slopgb_plugin_host::PluginRegistry;
use slopscript::{Interp, Value};

use crate::app_boot::effective_plugin_flags;
use crate::session::{self, Session};
use crate::symbols::SymbolTable;
use crate::windows::options::ModelChoice;

use super::{Machine, consts};

pub(super) fn register<'a>(
    interp: &mut Interp<'a>,
    mc: &Rc<RefCell<Machine>>,
    registry: &'a PluginRegistry,
) {
    let m = Rc::clone(mc);
    interp.register_builtin("gb:load_rom", move |args| load_rom(&m, registry, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:load_battery", move |args| load_battery(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:save_battery", move |args| save_battery(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:save_state", move |args| save_state(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:load_state", move |args| load_state(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:load_symbols", move |args| load_symbols(&m, args));
    let m = Rc::clone(mc);
    interp.register_builtin("gb:screenshot", move |args| screenshot(&m, args));
}

fn expect_path(args: &[Value]) -> Result<&str, String> {
    match args {
        [Value::Str(p)] => Ok(p.as_str()),
        _ => Err("expects (path)".to_string()),
    }
}

/// `Session::load_rom`, never `Session::load`: a headless run must not
/// auto-restore `<rom>.sav`, or a control run would silently inherit the
/// previous run's output (`docs/headless-plan.md`). Re-applies the boot
/// ROM / SGB BIOS / plugins dir `main` already resolved, plus the plugin
/// registry's already-resolved flag values, to the freshly loaded machine —
/// the same treatment an interactive ROM (re)load gives it.
fn load_rom(
    mc: &Rc<RefCell<Machine>>,
    registry: &PluginRegistry,
    args: &[Value],
) -> Result<Value, String> {
    let (path, model) = match args {
        [Value::Str(p)] => (p.as_str(), ModelChoice::Auto),
        [Value::Str(p), m] => (p.as_str(), consts::decode_model(m)?),
        _ => return Err("expects (path) or (path, MODEL)".to_string()),
    };
    let mut m = mc.borrow_mut();
    let boot = session::BootSpec::cli(m.boot_rom.as_deref());
    let mut new_session = Session::load_rom(Path::new(path), model, &boot, m.ram_init)?;
    new_session.set_sgb_bios(m.sgb_bios.clone());
    new_session.set_plugins_dir(m.plugins_dir.clone());
    new_session.set_plugin_flags(effective_plugin_flags(registry));
    m.session = new_session;
    Ok(Value::Nil)
}

/// Explicit, never inferred: a `false` return (wrong size, or the cartridge
/// has no battery) is a runtime error, not a silent skip.
fn load_battery(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let path = expect_path(args)?;
    let data = fs::read(path).map_err(|e| format!("cannot read '{path}': {e}"))?;
    let mut m = mc.borrow_mut();
    if !m.session.gb.load_save_data(&data) {
        return Err(format!(
            "'{path}' rejected (wrong size, or the cartridge has no battery)"
        ));
    }
    Ok(Value::Nil)
}

/// The canonical timestamp-free `save_data` image, deliberately **not** the
/// VBA-footer variant `Session::save_image` writes for the interactive path:
/// that footer stamps the host wall clock, which would make a headless
/// run's output non-reproducible.
fn save_battery(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let path = expect_path(args)?;
    let data = {
        let m = mc.borrow();
        m.session
            .gb
            .save_data()
            .ok_or_else(|| "cartridge has no battery RAM".to_string())?
    };
    session::write_atomic(Path::new(path), &data)
        .map_err(|e| format!("cannot write '{path}': {e}"))?;
    Ok(Value::Nil)
}

fn save_state(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let path = expect_path(args)?;
    mc.borrow().session.save_state_to(Path::new(path))?;
    Ok(Value::Nil)
}

fn load_state(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let path = expect_path(args)?;
    mc.borrow_mut().session.load_state_from(Path::new(path))?;
    Ok(Value::Nil)
}

fn load_symbols(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let path = expect_path(args)?;
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read '{path}': {e}"))?;
    mc.borrow_mut().syms = SymbolTable::parse(&text);
    Ok(Value::Nil)
}

/// The bare 160×144 LCD (no SGB border), encoded by the path's extension:
/// `.png` or `.bmp`, the same encoders as the interactive screenshot.
fn screenshot(mc: &Rc<RefCell<Machine>>, args: &[Value]) -> Result<Value, String> {
    let path = expect_path(args)?;
    let data = {
        let m = mc.borrow();
        let frame = &m.session.gb.frame()[..];
        match Path::new(path).extension().and_then(|e| e.to_str()) {
            Some("png") => crate::mcp::png::encode(frame, SCREEN_W, SCREEN_H),
            Some("bmp") => crate::screenshot::to_bmp(frame, SCREEN_W, SCREEN_H),
            _ => return Err(format!("'{path}' must end in .png or .bmp")),
        }
    };
    session::write_atomic(Path::new(path), &data)
        .map_err(|e| format!("cannot write '{path}': {e}"))?;
    Ok(Value::Nil)
}
