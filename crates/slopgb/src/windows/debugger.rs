//! The bgb debugger window (Layer C): composes the `ui` widgets over
//! `slopgb_core::debug` introspection. This module is the window *content* —
//! pure rendering into a [`Canvas`], unit-tested headless; the winit surface
//! wiring (B12b) feeds it a real buffer later.

use std::collections::BTreeSet;

use slopgb_core::debug;

use crate::dbg::{Breakpoints, DebugAction};
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::dialog::{self, DialogKey, DialogResult, InputDialog};
use crate::ui::font::GLYPH_H;
use crate::ui::menu::{self, MenuItem};
use crate::ui::text::{draw_text, hex_row, line_height, measure};
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

/// One decoded disassembly line: its address and the formatted bgb text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisasmRow {
    pub addr: u16,
    pub text: String,
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

/// Disassemble `count` instructions from `start`, each formatted as a bgb
/// disasm line `LABEL:ADDR  bytes  mnemonic  ;m-cycles`. `read(addr)` yields the
/// byte at `addr` (use `GameBoy::debug_read`). Exact column widths are tuned in
/// the C8 visual diff; the content (addr/bytes/mnemonic/cycles) is final.
pub fn disasm_rows(
    read: impl Fn(u16) -> u8,
    start: u16,
    count: usize,
    data_hints: &BTreeSet<u16>,
) -> Vec<DisasmRow> {
    let mut rows = Vec::with_capacity(count);
    let mut addr = start;
    for _ in 0..count {
        // An address marked data renders as a single `db XX` byte (RM9), so the
        // disassembler doesn't mis-decode an embedded data table as code.
        if data_hints.contains(&addr) {
            let b = read(addr);
            let text = format!(
                "{}:{addr:04X} {:<9}{:<20};",
                region_label(addr),
                format!("{b:02X}"),
                format!("db {b:02X}")
            );
            rows.push(DisasmRow { addr, text });
            addr = addr.wrapping_add(1);
            continue;
        }
        let bytes = [
            read(addr),
            read(addr.wrapping_add(1)),
            read(addr.wrapping_add(2)),
        ];
        let insn = debug::decode(&bytes, addr);
        let hex: String = bytes[..insn.len as usize]
            .iter()
            .map(|b| format!("{b:02X} "))
            .collect();
        let text = format!(
            "{}:{addr:04X} {:<9}{:<20};{}",
            region_label(addr),
            hex.trim_end(),
            insn.text,
            insn.cycles
        );
        rows.push(DisasmRow { addr, text });
        addr = addr.wrapping_add(u16::from(insn.len.max(1)));
    }
    rows
}

/// Width of the disasm pane's left gutter — holds the red breakpoint dot, and
/// the current-PC highlight bar extends across it.
pub const DISASM_GUTTER: i32 = 7;

/// Render the disasm pane: decode from `start` to fill `rect`, draw the rows
/// past a left gutter with the row at `pc` highlighted (the blue current-PC
/// bar), and a red dot in the gutter on every row carrying a breakpoint.
/// Returns the rows so the window can hit-test clicks.
#[allow(clippy::too_many_arguments)]
pub fn render_disasm(
    c: &mut Canvas,
    rect: Rect,
    read: impl Fn(u16) -> u8,
    start: u16,
    pc: u16,
    bps: &Breakpoints,
    data_hints: &BTreeSet<u16>,
    theme: &Theme,
) -> Vec<DisasmRow> {
    let lh = line_height();
    let count = (rect.h / lh).max(0) as usize + 1;
    let rows = disasm_rows(read, start, count, data_hints);
    let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
    let highlight = rows.iter().position(|r| r.addr == pc);
    let body = Rect::new(
        rect.x + DISASM_GUTTER,
        rect.y,
        (rect.w - DISASM_GUTTER).max(0),
        rect.h,
    );
    let drawn = scroll_list(c, body, &texts, 0, highlight, theme);
    // Extend the PC bar across the gutter and stamp breakpoint dots in it.
    for (i, row) in rows.iter().enumerate().take(drawn) {
        let y = rect.y + i as i32 * lh;
        if Some(i) == highlight {
            c.fill_rect(Rect::new(rect.x, y, DISASM_GUTTER, lh), theme.current);
        }
        if bps.contains(row.addr) {
            let cy = y + lh / 2;
            c.fill_rect(Rect::new(rect.x + 1, cy - 2, 4, 4), theme.breakpoint);
        }
    }
    rows
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
}

/// The two-column register lines bgb shows (`af= …  lcdc=…`, …). `cnt` and the
/// ROM-bank field are omitted pending their deferred accessors.
#[must_use]
pub fn regs_lines(r: &RegsView) -> Vec<String> {
    let flag = |b: bool| if b { '1' } else { '.' };
    vec![
        format!("af= {:04X}   lcdc={:02X}", r.af, r.lcdc),
        format!("bc= {:04X}   stat={:02X}", r.bc, r.stat),
        format!("de= {:04X}   ly= {:02X}", r.de, r.ly),
        format!("hl= {:04X}", r.hl),
        format!("sp= {:04X}   ie= {:02X}", r.sp, r.ie),
        format!("pc= {:04X}   if= {:02X}", r.pc, r.iflag),
        format!("ime={}   spd= {}", flag(r.ime), u8::from(r.double_speed)),
        format!("ima={}", flag(r.ima)),
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
    /// An open modal prompt (Go to…), if any.
    pub dialog: Option<GotoDialog>,
    /// Addresses forced to render as `db XX` data instead of decoded code
    /// (RM9 — "Data go here" / "force code view" / "Modify code/data").
    pub data_hints: BTreeSet<u16>,
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

/// An open `Go to…` modal: the hex-input box plus which pane it moves on accept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GotoDialog {
    pub input: InputDialog,
    pub target: GotoTarget,
}

/// Which pane a click landed in, resolved to its address where meaningful — the
/// pure result of [`target_at`], reused so a menu action and the rendering can
/// never disagree about the clicked address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickTarget {
    Disasm(u16),
    Memory(u16),
    Stack(u16),
    Registers,
    None,
}

/// What selecting a menu item does. Execution effects (`Act`) are returned to
/// `main` to apply against the machine; view effects (`TogglePin`, `OpenGoto`)
/// mutate the window's own `DebuggerState`; `None` is a separator / disabled /
/// not-yet-wired stub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuChoice {
    Act(DebugAction),
    TogglePin,
    OpenGoto(GotoTarget),
    /// Flip the code/data hint at the address ("Modify code/data" / "Modify data").
    ToggleDataHint(u16),
    /// Force the address to decode as code ("force code view" / "Code go here").
    MarkCode(u16),
    /// Force the address to render as data ("Data go here").
    MarkData(u16),
    None,
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
        let rows = disasm_rows(read, st.disasm_start(pc), row + 1, &st.data_hints);
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
        return ClickTarget::Registers;
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
        disabled("Copy data"),
        disabled("Copy code"),
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
    v.push(disabled("Jump to cursor"));
    v.push(disabled("Call cursor"));
    v.push(disabled("Set watchpoint..."));
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
/// item performs its [`MenuChoice`] (execution actions return a [`DebugAction`]
/// for `main`; `TogglePin` is a view effect handled here). With no menu open, a
/// left-click selects the clicked line (sets the cursor). Pure over `read` + the
/// register snapshot, so it tests headless.
pub fn on_left_click(
    read: impl Fn(u16) -> u8,
    area: Rect,
    st: &mut DebuggerState,
    pc: u16,
    sp: u16,
    px: i32,
    py: i32,
) -> Option<DebugAction> {
    let l = DebuggerLayout::for_size(area.w, area.h);
    // An open menu eats the click: an enabled item acts; a click anywhere else
    // inside the box just dismisses (disabled item / separator); a click outside
    // dismisses *and* falls through, so clicking the bar can open another menu.
    if let Some(om) = st.menu.take() {
        if let Some(choice) = om.choice_at(px, py) {
            return apply_choice(st, choice, pc);
        }
        if om.contains(px, py) {
            return None;
        }
    }
    // Menu-bar label → open its dropdown below the bar.
    if l.menu.contains(px, py) {
        if let Some(idx) = menubar_at(l.menu, px, py) {
            st.menu = Some(menubar_menu(idx, l.menu, st, pc));
        }
        return None;
    }
    // Otherwise select the clicked pane line (sets the cursor).
    if let ClickTarget::Disasm(a) | ClickTarget::Memory(a) | ClickTarget::Stack(a) =
        target_at(read, area, st, pc, sp, px, py)
    {
        st.cursor = Some(a);
    }
    None
}

/// Apply a selected menu choice: execution effects return a [`DebugAction`] for
/// `main`; view effects mutate `st` in place.
fn apply_choice(st: &mut DebuggerState, choice: MenuChoice, pc: u16) -> Option<DebugAction> {
    match choice {
        MenuChoice::Act(action) => Some(action),
        MenuChoice::TogglePin => {
            // Freeze the disasm view where it currently sits when pinning on.
            if !st.pinned {
                st.disasm_base = pc;
            }
            st.pinned = !st.pinned;
            None
        }
        MenuChoice::OpenGoto(target) => {
            open_goto(st, target);
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

// --- Go to… modal (RM5) ----------------------------------------------------

/// Open the `Go to…` hex prompt for `target` (closing any open menu).
pub fn open_goto(st: &mut DebuggerState, target: GotoTarget) {
    st.menu = None;
    st.dialog = Some(GotoDialog {
        input: InputDialog::new("Go to address", true),
        target,
    });
}

/// Apply an accepted `Go to…` address: reposition the target pane (the disasm
/// pane pins to the entered base so it stops following PC).
fn apply_goto(st: &mut DebuggerState, target: GotoTarget, text: &str) {
    let Ok(addr) = u16::from_str_radix(text.trim(), 16) else {
        return; // empty / unparseable: leave the view unchanged
    };
    match target {
        GotoTarget::Disasm => {
            st.disasm_base = addr;
            st.pinned = true;
        }
        GotoTarget::Memory => st.mem_base = addr,
    }
}

/// Feed one key to the open `Go to…` dialog: accept repositions + closes,
/// cancel closes, anything else keeps editing. Returns whether a dialog was open
/// to consume the key.
pub fn feed_goto(st: &mut DebuggerState, key: DialogKey) -> bool {
    let Some(gd) = &mut st.dialog else {
        return false;
    };
    match gd.input.on_key(key) {
        DialogResult::Accept(text) => {
            let target = gd.target;
            apply_goto(st, target, &text);
            st.dialog = None;
        }
        DialogResult::Cancel => st.dialog = None,
        DialogResult::Continue => {}
    }
    true
}

/// Handle a left-click while the `Go to…` dialog is open: OK accepts, Cancel
/// dismisses. Returns whether the dialog consumed the click.
pub fn goto_click(st: &mut DebuggerState, area: Rect, px: i32, py: i32) -> bool {
    let Some(gd) = &st.dialog else {
        return false;
    };
    match dialog::click(&gd.input, area, px, py) {
        DialogResult::Accept(text) => {
            let target = gd.target;
            apply_goto(st, target, &text);
            st.dialog = None;
        }
        DialogResult::Cancel => st.dialog = None,
        DialogResult::Continue => {}
    }
    true
}

// --- menu bar + dropdowns (MB1) --------------------------------------------

/// The debugger menu-bar labels, left to right (menubar-*.png).
pub const MENUBAR: [&str; 6] = [
    "File",
    "Search",
    "Run",
    "Debug",
    "Window",
    "Execution profiler",
];

/// Padding each side of a menu-bar label.
const BAR_PAD: i32 = 5;

/// Hit-rect of each menu-bar label within the `bar` rect (the layout's `menu`).
#[must_use]
pub fn menubar_rects(bar: Rect) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(MENUBAR.len());
    let mut x = bar.x;
    for label in MENUBAR {
        let w = measure(label) + 2 * BAR_PAD;
        rects.push(Rect::new(x, bar.y, w, bar.h));
        x += w;
    }
    rects
}

/// The menu-bar label index under `(px, py)`, if any.
#[must_use]
pub fn menubar_at(bar: Rect, px: i32, py: i32) -> Option<usize> {
    menubar_rects(bar).iter().position(|r| r.contains(px, py))
}

/// Draw the menu bar; the dropdown's parent label (if a bar menu is open) is
/// highlighted.
pub fn render_menubar(c: &mut Canvas, bar: Rect, open: Option<usize>, theme: &Theme) {
    c.fill_rect(bar, theme.bg);
    c.hline(bar.x, bar.bottom() - 1, bar.w, theme.border);
    let ty = bar.y + (bar.h - GLYPH_H as i32) / 2;
    for (i, (label, r)) in MENUBAR.iter().zip(menubar_rects(bar)).enumerate() {
        let fg = if open == Some(i) {
            c.fill_rect(r, theme.current);
            theme.bg
        } else {
            theme.text
        };
        draw_text(c, r.x + BAR_PAD, ty, label, fg);
    }
}

/// Build the dropdown for menu-bar label `idx`, hung under its label. Items are
/// transcribed from menubar-{file,search,run,debug,window,profiler}.png; the few
/// already-supported ones (Debug → Toggle breakpoint, Run → Run to Cursor) are
/// enabled, the rest greyed pending MB2–MB5. The cursor address (or PC) is what
/// the enabled execution items act on.
#[must_use]
pub fn menubar_menu(idx: usize, bar: Rect, st: &DebuggerState, pc: u16) -> OpenMenu {
    let cursor = st.cursor.unwrap_or(pc);
    let entries = match idx {
        0 => file_menu(),
        1 => search_menu(),
        2 => run_menu(cursor),
        3 => debug_menu(cursor),
        4 => window_menu(),
        _ => profiler_menu(),
    };
    let origin = (
        menubar_rects(bar).get(idx).map_or(bar.x, |r| r.x),
        bar.bottom(),
    );
    let (items, choices) = entries.into_iter().unzip();
    OpenMenu {
        origin,
        items,
        choices,
        hovered: None,
        bar: Some(idx),
    }
}

/// A greyed dropdown item carrying a shortcut label.
fn dis_sc(label: &str, sc: &str) -> (MenuItem, MenuChoice) {
    (
        MenuItem::new(label).shortcut(sc).disabled(),
        MenuChoice::None,
    )
}

fn file_menu() -> Vec<(MenuItem, MenuChoice)> {
    vec![
        dis_sc("Load ROM...", "F12"),
        disabled("Load ROM without reset..."),
        dis_sc("Save ROM as...", "Ctrl+S"),
        disabled("Reload ROM"),
        disabled("Reload SRAM"),
        disabled("Load SRAM..."),
        disabled("reload SYM file"),
        dis_sc("Load state...", "Ctrl+L"),
        dis_sc("Save state...", "Ctrl+W"),
        disabled("Fix checksums"),
        disabled("save screenshot"),
        disabled("save memory_dump..."),
        disabled("save asm..."),
        dis_sc("Undo", "Ctrl+Z"),
        dis_sc("Redo", "Ctrl+Alt+Z"),
        disabled("Fix area with erase value"),
    ]
}

fn search_menu() -> Vec<(MenuItem, MenuChoice)> {
    vec![
        dis_sc("Search string (eg. 'ld a,')", "Ctrl+F"),
        dis_sc("Continue search", "Ctrl+C"),
        dis_sc("go to next bookmark", "Ctrl+N"),
        dis_sc("go to previous bookmark", "Ctrl+B"),
        dis_sc("go to PC", "Ctrl+A"),
    ]
}

fn run_menu(cursor: u16) -> Vec<(MenuItem, MenuChoice)> {
    vec![
        dis_sc("Run", "F9"),
        dis_sc("Run no break", "Shift+F9"),
        dis_sc("Run not this break", "Ctrl+F9"),
        dis_sc("Reset (numpad *)", "Ctrl+R"),
        dis_sc("Trace", "F7"),
        dis_sc("Trace reverse", "Shift+F7"),
        dis_sc("Step Over", "F3"),
        dis_sc("Step Over reverse", "Shift+F3"),
        disabled("Animate (Alt+A)"),
        (
            MenuItem::new("Run to Cursor").shortcut("F4"),
            MenuChoice::Act(DebugAction::RunToCursor(cursor)),
        ),
        dis_sc("Run cursor no break", "Shift+F4"),
        dis_sc("Run cursor reverse", "Ctrl+F4"),
        dis_sc("Jump to cursor", "F6"),
        disabled("Call cursor"),
        dis_sc("Step out", "F8"),
        dis_sc("Step out reverse", "Shift+F8"),
        disabled("jump (SP); SP=SP+2"),
        dis_sc("Rewind cycles...", "Ctrl+E"),
    ]
}

fn debug_menu(cursor: u16) -> Vec<(MenuItem, MenuChoice)> {
    vec![
        (
            MenuItem::new("Toggle breakpoint").shortcut("F2"),
            MenuChoice::Act(DebugAction::ToggleBreakpoint(cursor)),
        ),
        disabled("Evaluate expression"),
        disabled("Set user clocks counter"),
        dis_sc("Breakpoints", "Ctrl+H"),
        dis_sc("Watchpoints", "Ctrl+J"),
    ]
}

fn window_menu() -> Vec<(MenuItem, MenuChoice)> {
    vec![
        dis_sc("VRAM viewer", "F5"),
        disabled("SGB packets"),
        disabled("log link transfers (to SGB window)"),
        dis_sc("Options", "F11"),
        disabled("cheats"),
        disabled("cheat searcher"),
        dis_sc("IO map", "F10"),
        disabled("screen"),
        dis_sc("joypads", "Ctrl+K"),
        disabled("debug messages"),
    ]
}

fn profiler_menu() -> Vec<(MenuItem, MenuChoice)> {
    vec![
        disabled("logging mode"),
        disabled("break mode"),
        disabled("stop (*)"),
        disabled("clear buffer"),
        disabled("0 addresses seen"),
    ]
}

#[cfg(test)]
#[path = "debugger_tests.rs"]
mod tests;
