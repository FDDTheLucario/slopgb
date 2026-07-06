//! Debugger interaction (RM4/RM5/RM7/RM11): left/right/double-click
//! resolution, menu-choice application, and the modal prompt (Go to /
//! Search / Evaluate / edit register) open + accept plumbing. Pure over
//! `read` + register snapshots so it unit-tests headless; the parent
//! [`super::DebuggerState`] owns the view state these mutate.

use super::*;

/// Handle a left-click. With a menu open, any click closes it and an enabled
/// item performs its [`MenuChoice`] (execution / command effects return a
/// [`MenuOutcome`] for `main`; `TogglePin` is a view effect handled here). With
/// no menu open, a left-click selects the clicked line (sets the cursor). Pure
/// over `read` + the register snapshot, so it tests headless.
pub fn on_left_click(
    read: impl Fn(u16) -> u8,
    area: Rect,
    st: &mut DebuggerState,
    regs: Registers,
    px: i32,
    py: i32,
) -> Option<MenuOutcome> {
    let l = DebuggerLayout::for_size(area.w, area.h);
    // An open menu eats the click: an enabled item acts; a click anywhere else
    // inside the box just dismisses (disabled item / separator); a click outside
    // dismisses *and* falls through, so clicking the bar can open another menu.
    if let Some(om) = st.menu.take() {
        if let Some(choice) = om.choice_at(px, py) {
            return apply_choice(st, choice, regs);
        }
        if om.contains(px, py) {
            return None;
        }
    }
    // Menu-bar label → open its dropdown below the bar.
    if l.menu.contains(px, py) {
        if let Some(idx) = menubar_at(l.menu, px, py) {
            st.menu = Some(menubar_menu(idx, l.menu, st, regs.pc));
        }
        return None;
    }
    // Otherwise select the clicked pane line (sets the cursor).
    if let ClickTarget::Disasm(a) | ClickTarget::Memory(a) | ClickTarget::Stack(a) =
        target_at(read, area, st, regs.pc, regs.sp, px, py)
    {
        st.cursor = Some(a);
    }
    None
}

/// Handle a left **double**-click: bgb toggles a breakpoint on the double-clicked
/// disassembly line. Returns the toggle outcome, or `None` when a menu is open or
/// the click isn't on a disasm line (the paired single-click already moved the
/// cursor). Pure over `read`, so it tests headless.
#[must_use]
pub fn on_double_click(
    read: impl Fn(u16) -> u8,
    area: Rect,
    st: &DebuggerState,
    pc: u16,
    sp: u16,
    px: i32,
    py: i32,
) -> Option<MenuOutcome> {
    if st.menu.is_some() {
        return None;
    }
    match target_at(read, area, st, pc, sp, px, py) {
        ClickTarget::Disasm(a) => Some(MenuOutcome::Act(DebugAction::ToggleBreakpoint(a))),
        _ => None,
    }
}

/// The current value of a register pair, for seeding the "edit register" prompt.
fn reg_value(r: &Registers, f: RegField) -> u16 {
    match f {
        RegField::Af => r.af(),
        RegField::Bc => r.bc(),
        RegField::De => r.de(),
        RegField::Hl => r.hl(),
        RegField::Sp => r.sp,
        RegField::Pc => r.pc,
    }
}

/// Apply a selected menu choice: execution / command effects return a
/// [`MenuOutcome`] for `main`; view effects mutate `st` in place.
fn apply_choice(
    st: &mut DebuggerState,
    choice: MenuChoice,
    regs: Registers,
) -> Option<MenuOutcome> {
    match choice {
        MenuChoice::Act(action) => Some(MenuOutcome::Act(action)),
        MenuChoice::Command(action) => Some(MenuOutcome::Command(action)),
        MenuChoice::TogglePin => {
            // Freeze the disasm view where it currently sits when pinning on.
            if !st.pinned {
                st.disasm_base = regs.pc;
            }
            st.pinned = !st.pinned;
            None
        }
        MenuChoice::OpenGoto(target) => {
            open_goto(st, target);
            None
        }
        MenuChoice::OpenEditReg(field) => {
            open_edit_reg(st, field, reg_value(&regs, field));
            None
        }
        MenuChoice::ToggleDataHint(a) => {
            if !st.data_hints.remove(&a) {
                st.data_hints.insert(a);
            }
            None
        }
        MenuChoice::MarkCode(a) => {
            st.data_hints.remove(&a);
            None
        }
        MenuChoice::MarkData(a) => {
            st.data_hints.insert(a);
            None
        }
        MenuChoice::None => None,
    }
}

/// Handle a right-click: open the clicked pane's context menu at the cursor (and
/// select that line), or dismiss an already-open menu. Pure over `read`.
pub fn on_right_click(
    read: impl Fn(u16) -> u8,
    area: Rect,
    st: &mut DebuggerState,
    pc: u16,
    sp: u16,
    px: i32,
    py: i32,
) {
    if st.menu.is_some() {
        st.menu = None;
        return;
    }
    let target = target_at(read, area, st, pc, sp, px, py);
    if let ClickTarget::Disasm(a) | ClickTarget::Memory(a) | ClickTarget::Stack(a) = target {
        st.cursor = Some(a);
    }
    st.menu = menu_for(target, st, (px, py));
}

// --- modal prompts: Go to… (RM5) + edit register (RM11) --------------------

/// Open the `Go to…` hex prompt for `target` (closing any open menu).
pub fn open_goto(st: &mut DebuggerState, target: GotoTarget) {
    st.menu = None;
    // Free text (not hex-only) so a symbol name can be typed; the accept resolves
    // a name to its address, falling back to a hex parse.
    st.dialog = Some(ModalDialog {
        input: InputDialog::new("Go to address or symbol", false),
        kind: DialogKind::Goto(target),
    });
}

/// Open the `Search string` text prompt (MB3), closing any open menu. Non-hex
/// (it accepts mnemonics like `ld a,`); accept stores the query + runs the scan.
pub fn open_search(st: &mut DebuggerState) {
    st.menu = None;
    st.dialog = Some(ModalDialog {
        input: InputDialog::new("Search string (eg. 'ld a,')", false),
        kind: DialogKind::SearchString,
    });
}

/// Open the `Evaluate expression` text prompt (RM14), closing any open menu.
pub fn open_eval(st: &mut DebuggerState) {
    st.menu = None;
    st.dialog = Some(ModalDialog {
        input: InputDialog::new("Evaluate expression", false),
        kind: DialogKind::EvalExpr,
    });
}

/// Show an Evaluate-expression result (RM14) in a display-only box seeded with
/// `text` (closing any open menu); any accept/cancel dismisses it.
pub fn show_eval_result(st: &mut DebuggerState, text: String) {
    st.menu = None;
    st.dialog = Some(ModalDialog {
        input: InputDialog::new("Result", false).with_initial(text),
        kind: DialogKind::EvalResult,
    });
}

/// Open the `edit register` hex prompt for `field`, seeded with its current
/// `value` (closing any open menu).
pub fn open_edit_reg(st: &mut DebuggerState, field: RegField, value: u16) {
    st.menu = None;
    st.dialog = Some(ModalDialog {
        input: InputDialog::new("edit register", true).with_initial(format!("{value:04X}")),
        kind: DialogKind::EditReg(field),
    });
}

/// Apply an accepted `Go to…` address: reposition the target pane (the disasm
/// pane pins to the entered base so it stops following PC).
fn apply_goto(st: &mut DebuggerState, target: GotoTarget, addr: u16) {
    match target {
        GotoTarget::Disasm => {
            st.disasm_base = addr;
            st.pinned = true;
        }
        GotoTarget::Memory => st.mem_base = addr,
    }
}

/// Apply an accepted modal: `Go to…` repositions a pane (view effect, no
/// outcome); `edit register` returns the register write for `main` to apply.
/// An empty / unparseable entry leaves everything unchanged.
pub(crate) fn accept_dialog(
    st: &mut DebuggerState,
    kind: DialogKind,
    text: &str,
) -> Option<MenuOutcome> {
    // Accept the rendered hex forms too: a leading `$` (RGBDS) or `0x`.
    let hex = text.trim().trim_start_matches('$').trim_start_matches("0x");
    let parsed = u16::from_str_radix(hex, 16).ok();
    match kind {
        DialogKind::Goto(target) => {
            // A loaded symbol name resolves to its address; else the hex parse.
            if let Some(addr) = st.symbols.resolve(text.trim()).or(parsed) {
                apply_goto(st, target, addr);
            }
            None
        }
        DialogKind::EditReg(field) => {
            parsed.map(|v| MenuOutcome::Act(DebugAction::SetReg(field, v)))
        }
        // Store the query + reset the cursor, then signal `main` to run the scan
        // (it needs the machine's memory, which this layer doesn't hold).
        DialogKind::SearchString => {
            st.search_query = text.trim().to_owned();
            st.search_hit = None;
            (!st.search_query.is_empty()).then_some(MenuOutcome::Command(Action::DbgContinueSearch))
        }
        // Stash the expression, then signal `main` to evaluate it (needs the
        // machine). The result is shown via a follow-up EvalResult box.
        DialogKind::EvalExpr => {
            st.eval_input = text.trim().to_owned();
            (!st.eval_input.is_empty()).then_some(MenuOutcome::Command(Action::DbgEvalRun))
        }
        // The result box is display-only; accepting/cancelling just closes it.
        DialogKind::EvalResult => None,
    }
}

/// Feed one key to the open modal: accept applies + closes, cancel closes,
/// anything else keeps editing. Returns `(was a dialog open to consume the key,
/// outcome for `main`)`.
pub fn feed_dialog(st: &mut DebuggerState, key: DialogKey) -> (bool, Option<MenuOutcome>) {
    let Some(md) = &mut st.dialog else {
        return (false, None);
    };
    let kind = md.kind;
    let result = md.input.on_key(key);
    let outcome = resolve_dialog(st, kind, result);
    (true, outcome)
}

/// Handle a left-click while a modal is open: OK accepts, Cancel dismisses.
/// Returns `(did the dialog consume the click, outcome for `main`)`.
pub fn dialog_click(
    st: &mut DebuggerState,
    area: Rect,
    px: i32,
    py: i32,
) -> (bool, Option<MenuOutcome>) {
    let Some(md) = &st.dialog else {
        return (false, None);
    };
    let kind = md.kind;
    let result = dialog::click(&md.input, area, px, py);
    let outcome = resolve_dialog(st, kind, result);
    (true, outcome)
}

/// Resolve a [`DialogResult`] from key or click: accept/cancel close the modal
/// (accept may yield a [`MenuOutcome`]), continue leaves it open.
fn resolve_dialog(
    st: &mut DebuggerState,
    kind: DialogKind,
    result: DialogResult,
) -> Option<MenuOutcome> {
    match result {
        DialogResult::Accept(text) => {
            st.dialog = None;
            accept_dialog(st, kind, &text)
        }
        DialogResult::Cancel => {
            st.dialog = None;
            None
        }
        DialogResult::Continue => None,
    }
}
