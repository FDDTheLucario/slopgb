//! The bgb debugger window (Layer C): composes the `ui` widgets over
//! `slopgb_core::debug` introspection. This module is the window *content* —
//! pure rendering into a [`Canvas`], unit-tested headless; the winit surface
//! wiring (B12b) feeds it a real buffer later.

use std::collections::BTreeSet;

use slopgb_core::Registers;

use crate::dbg::{DebugAction, RegField};
use crate::input::Action;
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::dialog::{self, DialogKey, DialogResult, InputDialog};
use crate::ui::menu::{self, MenuItem};
use crate::ui::text::{draw_text, hex_row, line_height};
use crate::ui::widgets::scroll_list;

/// The four panes of the debugger body, partitioned from the window size to
/// match bgb's layout (see `docs/bgb-reference/02-debugger.png`): a thin menu
/// bar, the disassembly pane filling the upper-left, the registers panel
/// top-right with the stack list below it, and the memory hex dump across the
/// bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebuggerLayout {
    pub menu: Rect,
    pub disasm: Rect,
    pub regs: Rect,
    pub stack: Rect,
    pub memory: Rect,
}

impl DebuggerLayout {
    /// Partition a `w × h` window. Proportions mirror bgb's ~1172×786 layout:
    /// menu bar fixed-height, memory pane ~38 % of the body at the bottom,
    /// registers/stack a right-hand column ~⅓ wide, registers the top ~30 % of
    /// that column.
    #[must_use]
    pub fn for_size(w: i32, h: i32) -> Self {
        let menu_h = 18.min(h);
        let body_top = menu_h;
        let mem_h = ((h - menu_h) * 38 / 100).max(0);
        let body_bot = h - mem_h;
        let right_w = (w * 33 / 100).clamp(0, w);
        let left_w = w - right_w;
        let body_h = body_bot - body_top;
        let regs_h = (body_h * 30 / 100).max(0);
        Self {
            menu: Rect::new(0, 0, w, menu_h),
            disasm: Rect::new(0, body_top, left_w, body_h),
            regs: Rect::new(left_w, body_top, right_w, regs_h),
            stack: Rect::new(left_w, body_top + regs_h, right_w, body_h - regs_h),
            memory: Rect::new(0, body_bot, w, mem_h),
        }
    }
}

/// A coarse bank label from the address region. Precise ROM/VRAM/WRAM bank
/// numbers are a deferred accessor (see handoff); this gives bgb's `ROM0:` for
/// the fixed bank and a best-effort tag elsewhere.
fn region_label(addr: u16) -> &'static str {
    match addr {
        0x0000..=0x3FFF => "ROM0",
        0x4000..=0x7FFF => "ROMX",
        0x8000..=0x9FFF => "VRAM",
        0xA000..=0xBFFF => "SRAM",
        0xC000..=0xCFFF => "WRA0",
        0xD000..=0xDFFF => "WRAX",
        0xE000..=0xFDFF => "ECHO",
        0xFE00..=0xFE9F => "OAM ",
        0xFEA0..=0xFEFF => "??? ",
        0xFF00..=0xFF7F => "I/O ",
        0xFF80..=0xFFFE => "HRAM",
        0xFFFF => "IE  ",
    }
}

/// The values the registers panel shows, gathered from the machine
/// (`cpu_regs` + `ime`/`ime_pending`/`double_speed` + `debug_read` of the PPU /
/// interrupt registers). Built by the window layer so the renderer stays pure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegsView {
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub sp: u16,
    pub pc: u16,
    pub ime: bool,
    pub ima: bool,
    pub lcdc: u8,
    pub stat: u8,
    pub ly: u8,
    pub ie: u8,
    pub iflag: u8,
    pub double_speed: bool,
    /// User-clock counter (RM14): emulated cycles since the last "Set user clocks
    /// counter" reset, shown on the `hl` line's right column like bgb.
    pub cnt: u32,
    /// Cartridge ROM bank currently mapped at 0x4000-0x7FFF
    /// ([`slopgb_core::GameBoy::rom_bank`]).
    pub rom_bank: usize,
    /// External-RAM bank visible at 0xA000, or `None` when RAM is disabled/absent
    /// ([`slopgb_core::GameBoy::ram_bank`]).
    pub ram_bank: Option<usize>,
}

/// The two-column register lines bgb shows (`af= …  lcdc=…`, …). The `hl` line's
/// right column carries the user-clock counter (RM14); the `ima` line's carries
/// the cartridge ROM/RAM bank indicator (distinct from the VRAM/WRAM banks).
#[must_use]
pub fn regs_lines(r: &RegsView) -> Vec<String> {
    let flag = |b: bool| if b { '1' } else { '.' };
    let ram = r
        .ram_bank
        .map_or_else(|| "--".to_string(), |b| format!("{b:02X}"));
    vec![
        format!("af= {:04X}   lcdc={:02X}", r.af, r.lcdc),
        format!("bc= {:04X}   stat={:02X}", r.bc, r.stat),
        format!("de= {:04X}   ly= {:02X}", r.de, r.ly),
        format!("hl= {:04X}   cnt= {}", r.hl, r.cnt),
        format!("sp= {:04X}   ie= {:02X}", r.sp, r.ie),
        format!("pc= {:04X}   if= {:02X}", r.pc, r.iflag),
        format!("ime={}   spd= {}", flag(r.ime), u8::from(r.double_speed)),
        // ROM bank is 3 hex digits: MBC5 banks reach 0x1FF (9-bit).
        format!("ima={}   rom {:03X} ram {ram}", flag(r.ima), r.rom_bank),
    ]
}

/// Draw the registers panel into `rect`.
pub fn render_regs(c: &mut Canvas, rect: Rect, r: &RegsView, theme: &Theme) {
    let saved = c.push_clip(rect);
    for (i, line) in regs_lines(r).iter().enumerate() {
        draw_text(
            c,
            rect.x + 1,
            rect.y + i as i32 * line_height(),
            line,
            theme.text,
        );
    }
    c.set_clip(saved);
}

/// Stack-pane lines from [`slopgb_core::GameBoy::stack`] output: `LABEL:ADDR WORD`,
/// descending from SP.
#[must_use]
pub fn stack_lines(stack: &[(u16, u16)]) -> Vec<String> {
    stack
        .iter()
        .map(|&(a, w)| format!("{}:{a:04X} {w:04X}", region_label(a)))
        .collect()
}

/// Draw the stack pane; the top (SP) row gets the highlight bar, as in bgb.
pub fn render_stack(c: &mut Canvas, rect: Rect, stack: &[(u16, u16)], theme: &Theme) {
    let lines = stack_lines(stack);
    let texts: Vec<&str> = lines.iter().map(String::as_str).collect();
    let highlight = (!texts.is_empty()).then_some(0);
    scroll_list(c, rect, &texts, 0, highlight, theme);
}

/// Memory-pane rows: `count` hex-dump lines of 16 bytes each from `start`,
/// via [`hex_row`] over `read` (use `GameBoy::debug_read`).
#[must_use]
pub fn memory_rows(read: impl Fn(u16) -> u8, start: u16, count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            let base = start.wrapping_add((i * 16) as u16);
            let bytes: Vec<u8> = (0..16).map(|j| read(base.wrapping_add(j))).collect();
            hex_row(&format!("{}:{base:04X}", region_label(base)), &bytes)
        })
        .collect()
}

/// Draw the memory hex-dump pane.
pub fn render_memory(
    c: &mut Canvas,
    rect: Rect,
    read: impl Fn(u16) -> u8,
    start: u16,
    theme: &Theme,
) {
    let count = (rect.h / line_height()).max(0) as usize + 1;
    let rows = memory_rows(read, start, count);
    let texts: Vec<&str> = rows.iter().map(String::as_str).collect();
    scroll_list(c, rect, &texts, 0, None, theme);
}

// ---------------------------------------------------------------------------
// Interaction (RM4): per-window view state, click resolution, context menus.

/// Per-window debugger view state (mirrors `vram::VramState`): which addresses
/// each pane shows, the selected cursor, the follow-PC toggle, and an open
/// context menu. Owned by `WinState::Debugger`, mutated by the click/hover
/// hit-tests and read by the renderer. The breakpoint *set* is **not** here — it
/// lives in the App-owned `dbg::Debugger` (both the key handler and the run loop
/// consult it; see `docs/bgb-menu-design.md` RA1).
/// Live execution-profiler state for the Execution-profiler dropdown (MB5):
/// which radio mode is active and the distinct-addresses-seen count. Cached on
/// [`DebuggerState`] (refreshed from the machine when the menu opens) so the
/// pure menu builder needs no `&GameBoy`. `logging=false` ⇒ "stop";
/// `logging && !brk` ⇒ "logging mode"; `logging && brk` ⇒ "break mode".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfilerView {
    pub logging: bool,
    pub brk: bool,
    pub seen: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebuggerState {
    /// Disasm view base when [`pinned`](Self::pinned) (else the pane follows PC).
    pub disasm_base: u16,
    /// Memory-dump view base.
    pub mem_base: u16,
    /// Last-clicked address (the menu's cursor).
    pub cursor: Option<u16>,
    /// "Stay on bank and address": the disasm view stays put instead of
    /// following PC (RM12).
    pub pinned: bool,
    /// An open right-click context menu, if any.
    pub menu: Option<OpenMenu>,
    /// An open modal prompt (Go to… / edit register), if any.
    pub dialog: Option<ModalDialog>,
    /// Addresses forced to render as `db XX` data instead of decoded code
    /// (RM9 — "Data go here" / "force code view" / "Modify code/data").
    pub data_hints: BTreeSet<u16>,
    /// Cached profiler state for the Execution-profiler dropdown (MB5), refreshed
    /// from the machine when the menu opens.
    pub prof: ProfilerView,
    /// Last Search-string query (MB3), reused by "Continue search".
    pub search_query: String,
    /// Address of the last search hit; "Continue search" resumes just after it.
    pub search_hit: Option<u16>,
    /// Numbered bookmark slots 0-9 (bgb Ctrl+Shift+digit set / Ctrl+digit goto;
    /// Ctrl+N/Ctrl+B walk these plus the breakpoints).
    pub bookmarks: [Option<u16>; 10],
    /// Pending Evaluate-expression text (RM14), stored on accept for the scan.
    pub eval_input: String,
    /// Baseline for the regs-pane `cnt` user-clock counter (RM14): `cnt` is
    /// `gb.cycles() - clock_base`; "Set user clocks counter" zeroes it.
    pub clock_base: u64,
    /// Disasm display options (Options → Debug: lowercase hex / show clocks),
    /// pushed from `App::apply_settings`.
    pub disasm_fmt: DisasmFmt,
}

impl Default for DebuggerState {
    fn default() -> Self {
        Self {
            disasm_base: 0x0100,
            mem_base: 0xFF00,
            cursor: None,
            pinned: false,
            menu: None,
            dialog: None,
            data_hints: BTreeSet::new(),
            prof: ProfilerView::default(),
            search_query: String::new(),
            search_hit: None,
            bookmarks: [None; 10],
            eval_input: String::new(),
            clock_base: 0,
            disasm_fmt: DisasmFmt::default(),
        }
    }
}

impl DebuggerState {
    /// The address the disasm pane starts at: the pinned base, or PC when
    /// following.
    #[must_use]
    pub fn disasm_start(&self, pc: u16) -> u16 {
        if self.pinned { self.disasm_base } else { pc }
    }
}

/// Which pane a `Go to…` repositions (RM5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GotoTarget {
    Disasm,
    Memory,
}

/// What an open modal does on accept: reposition a pane (`Go to…`, RM5) or
/// write a register pair (`edit register`, RM11). The two share one hex
/// [`InputDialog`] + the same key/click plumbing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Goto(GotoTarget),
    EditReg(RegField),
    /// Search-string prompt (MB3): a non-hex text field whose accept stores the
    /// query and triggers a scan (run where the machine is reachable).
    SearchString,
    /// Evaluate-expression prompt (RM14): accept stores the expression and
    /// triggers an evaluation (run where the machine is reachable).
    EvalExpr,
    /// Evaluate-expression *result* box (RM14): display-only; accept/cancel close.
    EvalResult,
}

/// An open modal: the hex-input box plus what accepting it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModalDialog {
    pub input: InputDialog,
    pub kind: DialogKind,
}

/// Which pane a click landed in, resolved to its address where meaningful — the
/// pure result of [`target_at`], reused so a menu action and the rendering can
/// never disagree about the clicked address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickTarget {
    Disasm(u16),
    Memory(u16),
    Stack(u16),
    /// An editable register-pair row (af/bc/de/hl/sp/pc → "edit register").
    Reg(RegField),
    /// A non-editable registers-pane row (ime/spd/ima) — the menu still shows
    /// `edit register`, greyed.
    Registers,
    None,
}

/// What selecting a menu item does. Execution effects (`Act`) and frontend
/// commands (`Command`, reusing the keyboard `Action` dispatch) are returned to
/// `main` as a [`MenuOutcome`]; view effects (`TogglePin`, `OpenGoto`) mutate
/// the window's own `DebuggerState`; `None` is a separator / disabled /
/// not-yet-wired stub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuChoice {
    Act(DebugAction),
    /// A frontend action shared with the keyboard map (Run/Trace/Step Over/Step
    /// out/Reset, VRAM/IO-map window toggles) — `main` runs it through the same
    /// `run_action` the keys use, so a menu item and its hotkey never diverge.
    Command(Action),
    TogglePin,
    OpenGoto(GotoTarget),
    /// Flip the code/data hint at the address ("Modify code/data" / "Modify data").
    ToggleDataHint(u16),
    /// Force the address to decode as code ("force code view" / "Code go here").
    MarkCode(u16),
    /// Force the address to render as data ("Data go here").
    MarkData(u16),
    /// Open the "edit register" hex prompt for a register pair (RM11).
    OpenEditReg(RegField),
    None,
}

/// What a clicked menu item asks `main` to do against the live machine: either a
/// debugger [`DebugAction`] (applied via `dbg::Debugger::apply`) or a frontend
/// [`Action`] (run through `main`'s shared `run_action`, same as the keyboard).
/// View-only effects never reach here — they mutate `DebuggerState` in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuOutcome {
    Act(DebugAction),
    Command(Action),
}

/// An open context menu: its origin, the rendered items, the parallel choice for
/// each, the hovered row, and — for a menu-bar dropdown — which bar label it
/// hangs from (so the bar highlights it; `None` for a pane right-click menu).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenMenu {
    pub origin: (i32, i32),
    pub items: Vec<MenuItem>,
    pub choices: Vec<MenuChoice>,
    pub hovered: Option<usize>,
    pub bar: Option<usize>,
}

impl OpenMenu {
    /// Whether `(px, py)` is inside the menu box (a click here dismisses the menu
    /// but is otherwise swallowed — only a click *outside* falls through to the
    /// menu bar / panes).
    #[must_use]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        menu::menu_bounds(self.origin, &self.items).contains(px, py)
    }

    /// The choice under `(px, py)`, if it lands on an enabled item.
    #[must_use]
    pub fn choice_at(&self, px: i32, py: i32) -> Option<MenuChoice> {
        menu::item_at(self.origin, &self.items, px, py).map(|i| self.choices[i])
    }

    /// Update the hovered row; returns whether it changed (so the loop only
    /// redraws on a real change).
    pub fn hover_at(&mut self, px: i32, py: i32) -> bool {
        let new = menu::item_at(self.origin, &self.items, px, py);
        let changed = self.hovered != new;
        self.hovered = new;
        changed
    }
}

/// Resolve a click at `(px, py)` to the pane + address under it. Re-runs
/// `disasm_rows` from the same view-base the renderer used (variable-length
/// instructions ⇒ fixed pixel math can't work), so hit-test and render agree.
#[must_use]
pub fn target_at(
    read: impl Fn(u16) -> u8,
    area: Rect,
    st: &DebuggerState,
    pc: u16,
    sp: u16,
    px: i32,
    py: i32,
) -> ClickTarget {
    let l = DebuggerLayout::for_size(area.w, area.h);
    let lh = line_height().max(1);
    if l.disasm.contains(px, py) {
        let row = ((py - l.disasm.y) / lh) as usize;
        // Addresses are format-independent, so the default fmt is fine here.
        let rows = disasm_rows(
            read,
            st.disasm_start(pc),
            row + 1,
            &st.data_hints,
            DisasmFmt::default(),
        );
        return rows
            .get(row)
            .map_or(ClickTarget::None, |r| ClickTarget::Disasm(r.addr));
    }
    if l.memory.contains(px, py) {
        let row = ((py - l.memory.y) / lh) as u16;
        return ClickTarget::Memory(st.mem_base.wrapping_add(row.wrapping_mul(16)));
    }
    if l.stack.contains(px, py) {
        let row = ((py - l.stack.y) / lh) as u16;
        return ClickTarget::Stack(sp.wrapping_sub(row.wrapping_mul(2)));
    }
    if l.regs.contains(px, py) {
        // Rows match `regs_lines`: 0 af, 1 bc, 2 de, 3 hl, 4 sp, 5 pc are the
        // editable pairs; 6 ime/spd, 7 ima are not (left as `Registers`).
        let row = ((py - l.regs.y) / lh) as usize;
        return match row {
            0 => ClickTarget::Reg(RegField::Af),
            1 => ClickTarget::Reg(RegField::Bc),
            2 => ClickTarget::Reg(RegField::De),
            3 => ClickTarget::Reg(RegField::Hl),
            4 => ClickTarget::Reg(RegField::Sp),
            5 => ClickTarget::Reg(RegField::Pc),
            _ => ClickTarget::Registers,
        };
    }
    ClickTarget::None
}

/// Build the context menu for a right-click `target`, item-for-item as bgb's
/// captures (`docs/bgb-reference/menus/rc-*.png`). Items whose action isn't
/// wired yet (Go to / copy / modify / watchpoints / register edit — later
/// milestones) render **disabled** (greyed) so the menu structure matches bgb
/// while only the working subset is selectable. `None` for a pane with no menu.
#[must_use]
pub fn menu_for(target: ClickTarget, st: &DebuggerState, origin: (i32, i32)) -> Option<OpenMenu> {
    let entries: Vec<(MenuItem, MenuChoice)> = match target {
        ClickTarget::Disasm(addr) => disasm_entries(addr, st, true),
        ClickTarget::Memory(addr) => disasm_entries(addr, st, false),
        ClickTarget::Stack(addr) => stack_entries(addr),
        ClickTarget::Reg(field) => vec![(
            MenuItem::new("edit register"),
            MenuChoice::OpenEditReg(field),
        )],
        ClickTarget::Registers => vec![disabled("edit register")],
        ClickTarget::None => return None,
    };
    let (items, choices) = entries.into_iter().unzip();
    Some(OpenMenu {
        origin,
        items,
        choices,
        hovered: None,
        bar: None,
    })
}

/// A greyed, not-yet-wired item.
fn disabled(label: &str) -> (MenuItem, MenuChoice) {
    (MenuItem::new(label).disabled(), MenuChoice::None)
}

/// The disasm (rc-disasm.png) / memory (rc-memory.png) right-click menu — the
/// memory variant drops `force code view`. `addr` is the clicked cursor.
fn disasm_entries(addr: u16, st: &DebuggerState, is_disasm: bool) -> Vec<(MenuItem, MenuChoice)> {
    let goto = if is_disasm {
        GotoTarget::Disasm
    } else {
        GotoTarget::Memory
    };
    let mut v = vec![
        (
            MenuItem::new("Go to...").shortcut("Ctrl+G"),
            MenuChoice::OpenGoto(goto),
        ),
        (
            MenuItem::new("Modify code/data"),
            MenuChoice::ToggleDataHint(addr),
        ),
        (
            MenuItem::new("Copy data"),
            MenuChoice::Command(Action::DbgCopyData(addr)),
        ),
        (
            MenuItem::new("Copy code"),
            MenuChoice::Command(Action::DbgCopyCode(addr)),
        ),
        disabled("Insert size"),
    ];
    if is_disasm {
        v.push((MenuItem::new("force code view"), MenuChoice::MarkCode(addr)));
    }
    v.push((
        MenuItem::new("Stay on bank and address").checked(st.pinned),
        MenuChoice::TogglePin,
    ));
    v.push((
        MenuItem::new("Run to cursor"),
        MenuChoice::Act(DebugAction::RunToCursor(addr)),
    ));
    v.push((
        MenuItem::new("Jump to cursor"),
        MenuChoice::Act(DebugAction::SetPc(addr)),
    ));
    v.push((
        MenuItem::new("Call cursor"),
        MenuChoice::Act(DebugAction::Call(addr)),
    ));
    v.push((
        MenuItem::new("Set watchpoint..."),
        MenuChoice::Act(DebugAction::ToggleWatchpoint(addr)),
    ));
    v.push((
        MenuItem::new("Set break/condition..."),
        MenuChoice::Act(DebugAction::ToggleBreakpoint(addr)),
    ));
    v
}

/// The stack pane (rc-stack.png) right-click menu. `addr` is the clicked stack
/// slot; its Go-to / code-data items act on that address.
fn stack_entries(addr: u16) -> Vec<(MenuItem, MenuChoice)> {
    vec![
        (
            MenuItem::new("Go to...").shortcut("Ctrl+G"),
            MenuChoice::OpenGoto(GotoTarget::Memory),
        ),
        (
            MenuItem::new("Modify data"),
            MenuChoice::ToggleDataHint(addr),
        ),
        (MenuItem::new("Code go here"), MenuChoice::MarkCode(addr)),
        (MenuItem::new("Data go here"), MenuChoice::MarkData(addr)),
    ]
}

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
    st.dialog = Some(ModalDialog {
        input: InputDialog::new("Go to address", true),
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
fn accept_dialog(st: &mut DebuggerState, kind: DialogKind, text: &str) -> Option<MenuOutcome> {
    let parsed = u16::from_str_radix(text.trim(), 16).ok();
    match kind {
        DialogKind::Goto(target) => {
            if let Some(addr) = parsed {
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

// --- menu bar + dropdowns (MB1): see the `menubar` submodule -----------------

pub mod disasm;
// `DisasmRow` is reachable as `debugger::disasm::DisasmRow`; not re-exported here
// (no non-test caller names it, and a `pub use` would be an unused-import warning).
pub use disasm::{DisasmFmt, disasm_rows, render_disasm, render_profile_counts};
mod menubar;
pub use menubar::{address_list_menu, menubar_at, menubar_menu, render_menubar};
mod search;
pub use search::{find_match, next_mark};
mod eval;
pub use eval::eval_expr;
// `menubar_rects` is exercised only by the debugger tests; the rest of the crate
// reaches the bar via menubar_at/menubar_menu/render_menubar.
#[cfg(test)]
pub use menubar::menubar_rects;

#[cfg(test)]
#[path = "debugger_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "debugger_profiler_tests.rs"]
mod profiler_tests;

#[cfg(test)]
#[path = "debugger_search_tests.rs"]
mod search_tests;

#[cfg(test)]
#[path = "debugger_eval_tests.rs"]
mod eval_tests;
