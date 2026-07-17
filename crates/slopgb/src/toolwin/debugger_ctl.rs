//! Debugger-window control surface: the `ToolWindows` methods that drive the
//! debugger view (view lookup, navigation, modals, bookmarks, search, disasm
//! copy/export, eval). Split out of `toolwin.rs` to keep each file under the
//! 1000-line cap; the struct, fields, and event-routing methods stay in the
//! parent. A child module can reach the parent's private `ToolView`,
//! `WinState`, and struct fields, so these move verbatim.

use super::*;

impl ToolWindows {
    /// The (single) open debugger window's view, by kind.
    fn debugger_view_mut(&mut self) -> Option<&mut ToolView> {
        let id = self.reg.id_of(ToolWindow::Debugger)?;
        self.views.get_mut(&id)
    }

    fn debugger_view(&self) -> Option<&ToolView> {
        let id = self.reg.id_of(ToolWindow::Debugger)?;
        self.views.get(&id)
    }

    /// The debugger's selected cursor address (keyboard breakpoint / run-to-cursor).
    #[must_use]
    pub fn debugger_cursor(&self) -> Option<u16> {
        match &self.debugger_view()?.state {
            WinState::Debugger(s) => s.cursor,
            _ => None,
        }
    }

    /// The ROM-bank qualifier for a breakpoint toggled at `addr` from the
    /// debugger's disasm view (the pinned bank on a switchable-ROM line, else
    /// `None`). `None` when no debugger window is open.
    #[must_use]
    pub fn debugger_disasm_bp_bank(&self, addr: u16) -> Option<u16> {
        match &self.debugger_view()?.state {
            WinState::Debugger(s) => s.disasm_bp_bank(addr),
            _ => None,
        }
    }

    /// Whether the debugger window has an open modal, so `main` routes keys to it
    /// instead of the focus keymap.
    #[must_use]
    pub fn debugger_modal_active(&self) -> bool {
        if let Some(WinState::Debugger(s)) = self.debugger_view().map(|v| &v.state) {
            s.dialog.is_some()
        } else {
            false
        }
    }

    /// Feed one key to the debugger's open modal (Go to… / edit register);
    /// redraw if it consumed the key. Returns the modal's outcome (an `edit
    /// register` accept yields a register write) for `main` to apply.
    pub fn feed_debugger_dialog(&mut self, key: DialogKey) -> Option<MenuOutcome> {
        let view = self.debugger_view_mut()?;
        if let WinState::Debugger(s) = &mut view.state {
            let (consumed, outcome) = debugger::feed_dialog(s, key);
            if consumed {
                view.window.request_redraw();
            }
            return outcome;
        }
        None
    }

    /// Open a prebuilt context menu on the debugger window — the bp/wp manager
    /// list (RM15), which `main` builds from the App-owned breakpoint/watchpoint
    /// state and hands here. Redraws.
    pub fn set_debugger_menu(&mut self, menu: debugger::OpenMenu) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            s.menu = Some(menu);
            view.window.request_redraw();
        }
    }

    /// Center the disasm pane on PC and unpin, so it follows execution again —
    /// Search → "go to PC" / Ctrl+A, and after every trace step / PC jump. A
    /// manual scroll pins the view; this re-attaches it. Redraws.
    pub fn center_debugger_on_pc(&mut self, gb: &GameBoy) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        let area = view.area();
        if let WinState::Debugger(s) = &mut view.state {
            let l = debugger::DebuggerLayout::for_size(area.w, area.h);
            let visible = (l.disasm.h / line_height()).max(0) as usize;
            s.center_disasm_on_pc(gb.cpu_regs().pc, |a| gb.debug_read(a), visible);
            view.window.request_redraw();
        }
    }

    /// Scroll the debugger window's memory pane by `rows` rows of 16 bytes
    /// (arrow keys; negative scrolls toward lower addresses). Redraws.
    pub fn scroll_debugger_memory(&mut self, rows: i32) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            s.scroll_memory(rows);
            view.window.request_redraw();
        }
    }

    /// Page the debugger memory pane by one visible page in direction `dir` (±1)
    /// (PageUp/PageDown); the page is the pane's visible row count. Redraws.
    pub fn page_debugger_memory(&mut self, dir: i32) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        let area = view.area();
        if let WinState::Debugger(s) = &mut view.state {
            let l = debugger::DebuggerLayout::for_size(area.w, area.h);
            let rows = (l.memory.h / line_height()).max(1);
            s.scroll_memory(dir.signum() * rows);
            view.window.request_redraw();
        }
    }

    /// Step the debugger memory pane's browsed bank by `delta` (`[` / `]`),
    /// starting from the live-mapped bank when following it and re-following on
    /// the live bank (see `windows::stepped_bank`). Redraws.
    pub fn step_debugger_bank(&mut self, delta: i32, gb: &GameBoy) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            let live = crate::windows::live_bank(gb, s.mem_base);
            let count = gb.region_bank_count(s.mem_base);
            s.mem_bank = crate::windows::stepped_bank(s.mem_bank, delta, live, count);
            view.window.request_redraw();
        }
    }

    /// Open the debugger's `Go to…` modal on the disasm pane (Ctrl+G).
    pub fn open_debugger_goto(&mut self) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            debugger::open_goto(s, debugger::GotoTarget::Disasm);
            view.window.request_redraw();
        }
    }

    /// Open the debugger's `Search string` modal (MB3, Ctrl+F).
    pub fn open_debugger_search(&mut self) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            debugger::open_search(s);
            view.window.request_redraw();
        }
    }

    /// Run the stored Search-string query over the machine (MB3): from just after
    /// the last hit, or the current disasm base for a fresh search. Pins the
    /// disasm to a match and remembers it for "Continue search"; a miss leaves
    /// the view unchanged.
    pub fn debugger_search(&mut self, gb: &GameBoy) {
        let pc = gb.cpu_regs().pc;
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            if s.search_query.is_empty() {
                return;
            }
            let from = s
                .search_hit
                .map_or_else(|| s.disasm_start(pc), |h| h.wrapping_add(1));
            if let Some(addr) = debugger::find_match(|a| gb.debug_read(a), from, &s.search_query) {
                s.disasm_base = addr;
                s.pinned = true;
                s.search_hit = Some(addr);
                view.window.request_redraw();
            }
        }
    }

    /// Set numbered bookmark `slot` (0-9) to `addr` (bgb Ctrl+Shift+digit).
    pub fn set_debugger_bookmark(&mut self, slot: u8, addr: u16) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            if let Some(b) = s.bookmarks.get_mut(slot as usize) {
                *b = Some(addr);
                view.window.request_redraw();
            }
        }
    }

    /// Jump the disasm to numbered bookmark `slot` if set (bgb Ctrl+digit).
    pub fn goto_debugger_bookmark(&mut self, slot: u8) {
        let addr = match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => s.bookmarks.get(slot as usize).copied().flatten(),
            _ => None,
        };
        if let Some(addr) = addr {
            self.debugger_goto_addr(addr);
        }
    }

    /// Pin the disasm to `addr` (go-to-bookmark + the next/previous-mark walk).
    pub fn debugger_goto_addr(&mut self, addr: u16) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            s.disasm_base = addr;
            s.pinned = true;
            view.window.request_redraw();
        }
    }

    /// The set bookmark addresses (input to the next/previous-mark walk).
    #[must_use]
    pub fn debugger_bookmarks(&self) -> Vec<u16> {
        match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => s.bookmarks.iter().flatten().copied().collect(),
            _ => Vec::new(),
        }
    }

    /// Where the next/previous-mark walk starts: the current disasm view base.
    #[must_use]
    pub fn debugger_disasm_ref(&self, pc: u16) -> u16 {
        match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => s.disasm_start(pc),
            _ => pc,
        }
    }

    /// Disassemble a fixed region from the current disasm base for the File →
    /// "save asm..." export (MB2): the formatted lines joined by newlines,
    /// honouring the pane's code/data hints.
    #[must_use]
    pub fn debugger_disasm_dump(&self, gb: &GameBoy) -> String {
        const COUNT: usize = 4096;
        let pc = gb.cpu_regs().pc;
        let (start, hints, fmt) = match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => (s.disasm_start(pc), s.data_hints.clone(), s.disasm_fmt),
            _ => (
                pc,
                std::collections::BTreeSet::new(),
                debugger::DisasmFmt::default(),
            ),
        };
        let rows = debugger::disasm_rows(|a| gb.debug_read(a), start, COUNT, &hints, fmt, &|a| {
            crate::windows::live_bank(gb, a)
        });
        rows.iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clipboard text for the debugger "Copy code" / "Copy data" items (RM10):
    /// 16 disassembled rows from `addr` (`code`) or 16 hex bytes from `addr`
    /// (data), honouring the pane's code/data hints + hex-case format.
    #[must_use]
    pub fn debugger_copy_text(&self, gb: &GameBoy, addr: u16, code: bool) -> String {
        let (hints, fmt) = match self.debugger_view().map(|v| &v.state) {
            Some(WinState::Debugger(s)) => (s.data_hints.clone(), s.disasm_fmt),
            _ => (
                std::collections::BTreeSet::new(),
                debugger::DisasmFmt::default(),
            ),
        };
        if code {
            let rows = debugger::disasm_rows(|a| gb.debug_read(a), addr, 16, &hints, fmt, &|a| {
                crate::windows::live_bank(gb, a)
            });
            rows.iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            (0..16u16)
                .map(|i| {
                    let b = gb.debug_read(addr.wrapping_add(i));
                    if fmt.lowercase_hex {
                        format!("{b:02x}")
                    } else {
                        format!("{b:02X}")
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    /// Push the disasm display options (Options → Debug) to the debugger view,
    /// repainting it if open. Inert when the debugger window isn't built yet.
    pub fn set_disasm_fmt(&mut self, fmt: debugger::DisasmFmt) {
        if let Some(view) = self.debugger_view_mut() {
            if let WinState::Debugger(s) = &mut view.state {
                if s.disasm_fmt != fmt {
                    s.disasm_fmt = fmt;
                    view.window.request_redraw();
                }
            }
        }
    }

    /// Open the debugger's `Evaluate expression` modal (RM14).
    pub fn open_debugger_eval(&mut self) {
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            debugger::open_eval(s);
            view.window.request_redraw();
        }
    }

    /// Evaluate the stored expression against the live machine (RM14) and show
    /// the result (or the error) in a display-only box.
    pub fn debugger_eval(&mut self, gb: &GameBoy) {
        let regs = gb.cpu_regs();
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            let expr = s.eval_input.clone();
            let text = match debugger::eval_expr(&expr, &regs, |a| gb.debug_read(a)) {
                Ok(v) => format!("{expr} = {v:04X} ({v})"),
                Err(e) => format!("{expr}: {e}"),
            };
            debugger::show_eval_result(s, text);
            view.window.request_redraw();
        }
    }

    /// Zero the regs-pane `cnt` user-clock counter (RM14, "Set user clocks
    /// counter"): re-baseline it to the current cycle count.
    pub fn reset_debugger_clocks(&mut self, gb: &GameBoy) {
        let now = gb.cycles();
        let Some(view) = self.debugger_view_mut() else {
            return;
        };
        if let WinState::Debugger(s) = &mut view.state {
            s.clock_base = now;
            view.window.request_redraw();
        }
    }
}
