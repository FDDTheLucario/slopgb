// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! Tagged constants: `BTN_*` (`0x100 + n`), CPU registers (`0x200 + n`) and
//! models (`0x300 + n`) — `docs/headless-plan.md`, "Constants to register".
//! Each is a tagged [`Value::Int`] so a bare number can never be mistaken
//! for one and vice versa (a `gb:tap(A, ...)` that silently meant the
//! accumulator would read perfectly and do the wrong thing); every decode
//! function here validates the tag and names what it expected.

use slopgb_core::Button;
use slopscript::{Interp, Value};

use crate::windows::options::ModelChoice;

const BTN_TAG: i64 = 0x100;
const REG_TAG: i64 = 0x200;
const MODEL_TAG: i64 = 0x300;

const BUTTONS: [(&str, Button); 8] = [
    ("BTN_A", Button::A),
    ("BTN_B", Button::B),
    ("BTN_START", Button::Start),
    ("BTN_SELECT", Button::Select),
    ("BTN_UP", Button::Up),
    ("BTN_DOWN", Button::Down),
    ("BTN_LEFT", Button::Left),
    ("BTN_RIGHT", Button::Right),
];

/// A CPU register the script can name: the 8-bit halves plus the six pairs
/// [`slopgb_core::DebugReg`] can write directly (`docs/headless-plan.md`:
/// "A F B C D E H L AF BC DE HL SP PC").
#[derive(Clone, Copy)]
pub(super) enum RegSlot {
    A,
    F,
    B,
    C,
    D,
    E,
    H,
    L,
    Af,
    Bc,
    De,
    Hl,
    Sp,
    Pc,
}

const REGS: [(&str, RegSlot); 14] = [
    ("A", RegSlot::A),
    ("F", RegSlot::F),
    ("B", RegSlot::B),
    ("C", RegSlot::C),
    ("D", RegSlot::D),
    ("E", RegSlot::E),
    ("H", RegSlot::H),
    ("L", RegSlot::L),
    ("AF", RegSlot::Af),
    ("BC", RegSlot::Bc),
    ("DE", RegSlot::De),
    ("HL", RegSlot::Hl),
    ("SP", RegSlot::Sp),
    ("PC", RegSlot::Pc),
];

/// The five model names that map exactly onto a [`ModelChoice`]:
/// `ModelChoice::from_model` folds `Mgb`/`Dmg0` to `Dmg` and `Agb` to `Cgb`
/// (`crates/slopgb/src/windows/options.rs`), so `MGB`/`DMG0`/`AGB` constants
/// could not do what their name says — only these five are registered. An
/// omitted name is already a parse-time "unbound identifier" error, which is
/// the honest failure.
const MODELS: [(&str, ModelChoice); 5] = [
    ("AUTO", ModelChoice::Auto),
    ("DMG", ModelChoice::Dmg),
    ("CGB", ModelChoice::Cgb),
    ("SGB", ModelChoice::Sgb),
    ("SGB2", ModelChoice::Sgb2),
];

pub(super) fn register(interp: &mut Interp<'_>) {
    for (i, (name, _)) in BUTTONS.iter().enumerate() {
        interp.register_const(name, Value::Int(BTN_TAG + i as i64));
    }
    for (i, (name, _)) in REGS.iter().enumerate() {
        interp.register_const(name, Value::Int(REG_TAG + i as i64));
    }
    for (i, (name, _)) in MODELS.iter().enumerate() {
        interp.register_const(name, Value::Int(MODEL_TAG + i as i64));
    }
}

/// `v`'s offset into a `base`-tagged table of `len` entries, or `None` when
/// `v` isn't an `Int`, or is one outside the tagged range (including a bare
/// untagged number, which is exactly the mistake the tag scheme exists to
/// catch).
fn tag_index(v: &Value, base: i64, len: usize) -> Option<usize> {
    let Value::Int(n) = v else { return None };
    let i = n - base;
    (0..len as i64).contains(&i).then_some(i as usize)
}

pub(super) fn decode_button(v: &Value) -> Result<Button, String> {
    tag_index(v, BTN_TAG, BUTTONS.len())
        .map(|i| BUTTONS[i].1)
        .ok_or_else(|| {
            format!(
                "expects a button constant (BTN_A, BTN_B, BTN_START, BTN_SELECT, BTN_UP, \
                 BTN_DOWN, BTN_LEFT, BTN_RIGHT), got {}",
                v.kind()
            )
        })
}

pub(super) fn decode_reg(v: &Value) -> Result<RegSlot, String> {
    tag_index(v, REG_TAG, REGS.len())
        .map(|i| REGS[i].1)
        .ok_or_else(|| {
            format!(
                "expects a register constant (A F B C D E H L AF BC DE HL SP PC), got {}",
                v.kind()
            )
        })
}

pub(super) fn decode_model(v: &Value) -> Result<ModelChoice, String> {
    tag_index(v, MODEL_TAG, MODELS.len())
        .map(|i| MODELS[i].1)
        .ok_or_else(|| {
            format!(
                "expects a model constant (AUTO DMG CGB SGB SGB2), got {}",
                v.kind()
            )
        })
}
