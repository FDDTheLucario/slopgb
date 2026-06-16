//! The debugger menu bar + its six dropdowns (MB1+): File / Search / Run /
//! Debug / Window / Execution profiler, transcribed item-for-item from
//! `docs/bgb-reference/menus/menubar-*.png`, plus the breakpoint/watchpoint
//! manager list popup (RM15). Pure layout + builders; the parent
//! [`super::DebuggerState`] owns the open-menu state.

use crate::dbg::DebugAction;
use crate::input::Action;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::font::GLYPH_H;
use crate::ui::menu::MenuItem;
use crate::ui::text::{draw_text, measure};
use crate::ui::{Theme, ToolWindow};

use super::{DebuggerState, MenuChoice, OpenMenu, ProfilerView, disabled, region_label};

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
        _ => profiler_menu(st.prof),
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

/// An enabled dropdown item carrying a shortcut label + its choice.
fn en_sc(label: &str, sc: &str, choice: MenuChoice) -> (MenuItem, MenuChoice) {
    (MenuItem::new(label).shortcut(sc), choice)
}

/// A dropdown item running a frontend [`Action`] (shared with the keyboard map).
fn cmd(label: &str, sc: &str, action: Action) -> (MenuItem, MenuChoice) {
    en_sc(label, sc, MenuChoice::Command(action))
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
        (
            MenuItem::new("save screenshot"),
            MenuChoice::Command(Action::SaveScreenshot),
        ),
        (
            MenuItem::new("save memory_dump..."),
            MenuChoice::Command(Action::DbgSaveMemDump),
        ),
        disabled("save asm..."),
        dis_sc("Undo", "Ctrl+Z"),
        dis_sc("Redo", "Ctrl+Alt+Z"),
        disabled("Fix area with erase value"),
    ]
}

fn search_menu() -> Vec<(MenuItem, MenuChoice)> {
    vec![
        cmd("Search string (eg. 'ld a,')", "Ctrl+F", Action::DbgSearch),
        cmd("Continue search", "Ctrl+C", Action::DbgContinueSearch),
        cmd("go to next bookmark", "Ctrl+N", Action::DbgNextBookmark),
        cmd("go to previous bookmark", "Ctrl+B", Action::DbgPrevBookmark),
        cmd("go to PC", "Ctrl+A", Action::DbgGoToPc),
    ]
}

fn run_menu(cursor: u16) -> Vec<(MenuItem, MenuChoice)> {
    vec![
        // "Run" is the F9 action verbatim (its shortcut), so the menu item and
        // the key stay in lockstep: from a break it resumes; while already
        // running it toggles a break, exactly as pressing F9 would.
        cmd("Run", "F9", Action::DbgBreak),
        dis_sc("Run no break", "Shift+F9"),
        dis_sc("Run not this break", "Ctrl+F9"),
        cmd("Reset (numpad *)", "Ctrl+R", Action::Reset),
        cmd("Trace", "F7", Action::DbgStep),
        dis_sc("Trace reverse", "Shift+F7"),
        cmd("Step Over", "F3", Action::DbgStepOver),
        dis_sc("Step Over reverse", "Shift+F3"),
        disabled("Animate (Alt+A)"),
        en_sc(
            "Run to Cursor",
            "F4",
            MenuChoice::Act(DebugAction::RunToCursor(cursor)),
        ),
        dis_sc("Run cursor no break", "Shift+F4"),
        dis_sc("Run cursor reverse", "Ctrl+F4"),
        cmd("Jump to cursor", "F6", Action::DbgJumpToCursor),
        (
            MenuItem::new("Call cursor"),
            MenuChoice::Act(DebugAction::Call(cursor)),
        ),
        cmd("Step out", "F8", Action::DbgStepOut),
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
        cmd("Breakpoints", "Ctrl+H", Action::DbgManageBreakpoints),
        cmd("Watchpoints", "Ctrl+J", Action::DbgManageWatchpoints),
    ]
}

/// Build the breakpoint/watchpoint **manager** popup (RM15) listing `addrs`,
/// one per row; selecting a row toggles (clears) that entry through the normal
/// menu-click path. bgb's manager is a persistent window — this is the
/// functional list, reusing the context-menu widget. `watch` picks the clear
/// action; an empty list shows a single greyed `(none)`.
#[must_use]
pub fn address_list_menu(addrs: &[u16], watch: bool, origin: (i32, i32)) -> OpenMenu {
    let entries: Vec<(MenuItem, MenuChoice)> = if addrs.is_empty() {
        vec![disabled("(none)")]
    } else {
        addrs
            .iter()
            .map(|&a| {
                // An idempotent *clear* (not a toggle) so a row from a stale
                // snapshot can never re-arm an entry the user cleared elsewhere.
                let choice = if watch {
                    MenuChoice::Act(DebugAction::ClearWatchpoint(a))
                } else {
                    MenuChoice::Act(DebugAction::ClearBreakpoint(a))
                };
                (
                    MenuItem::new(format!("{}:{a:04X}", region_label(a))),
                    choice,
                )
            })
            .collect()
    };
    let (items, choices) = entries.into_iter().unzip();
    OpenMenu {
        origin,
        items,
        choices,
        hovered: None,
        bar: None,
    }
}

fn window_menu() -> Vec<(MenuItem, MenuChoice)> {
    vec![
        cmd("VRAM viewer", "F5", Action::ToggleTool(ToolWindow::Vram)),
        disabled("SGB packets"),
        disabled("log link transfers (to SGB window)"),
        dis_sc("Options", "F11"),
        disabled("cheats"),
        disabled("cheat searcher"),
        cmd("IO map", "F10", Action::ToggleTool(ToolWindow::IoMap)),
        disabled("screen"),
        dis_sc("joypads", "Ctrl+K"),
        disabled("debug messages"),
    ]
}

fn profiler_menu(prof: ProfilerView) -> Vec<(MenuItem, MenuChoice)> {
    // The three modes are a radio group; the active one is check-marked (bgb's
    // • marker). "stop" is the default (profiling off).
    vec![
        (
            MenuItem::new("logging mode").checked(prof.logging && !prof.brk),
            MenuChoice::Command(Action::ProfilerLogging),
        ),
        (
            MenuItem::new("break mode").checked(prof.logging && prof.brk),
            MenuChoice::Command(Action::ProfilerBreak),
        ),
        (
            MenuItem::new("stop").checked(!prof.logging),
            MenuChoice::Command(Action::ProfilerStop),
        ),
        (
            MenuItem::new("clear buffer"),
            MenuChoice::Command(Action::ProfilerClear),
        ),
        disabled(&format!("{} addresses seen", prof.seen)),
    ]
}
