//! Layer C: the bgb tool windows (Debugger / VRAM viewer / I/O map). Each is a
//! pure content renderer composing the `ui` widgets over `slopgb_core::debug`
//! introspection, unit-tested headless; the event loop (B12b) feeds each one a
//! real softbuffer surface and routes its input.

pub mod debugger;
pub mod iomap;
pub mod mainwin;
pub mod options;
pub mod vram;

use std::rc::Rc;

use slopgb_core::{GameBoy, debug};

use crate::dbg::Breakpoints;
use crate::symbols::SymbolTable;
use crate::ui::canvas::Rect;
use crate::ui::dialog::InputDialog;
use crate::ui::font::GLYPH_W;
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::{checkbox, radio_group, scroll_content, vscrollbar};
use crate::ui::{Canvas, Theme, ToolWindow};
use debugger::DebuggerState;
use vram::{VramLayout, VramState, VramTab};

/// Per-window interactive state. The VRAM viewer and the debugger carry view
/// state (active tab / hover, or disasm cursor + open menu); the I/O map is
/// stateless.
#[derive(Clone, Debug)]
pub enum WinState {
    Stateless,
    Vram(VramState),
    // Boxed: `DebuggerState` is much larger than the other variants (disasm/menu/
    // dialog/bookmark state), and only ever a handful of `WinState`s exist (one
    // per open tool window), so the indirection costs nothing and keeps the enum
    // small. Deref coercion makes the box transparent at the match sites.
    Debugger(Box<DebuggerState>),
    Memory(MemoryView),
}

impl WinState {
    /// The initial state a freshly-opened window of `kind` owns.
    #[must_use]
    pub fn new(kind: ToolWindow) -> Self {
        match kind {
            ToolWindow::Vram => WinState::Vram(VramState::default()),
            ToolWindow::Debugger => WinState::Debugger(Box::default()),
            ToolWindow::IoMap => WinState::Stateless,
            ToolWindow::MemoryViewer => WinState::Memory(MemoryView::default()),
        }
    }
}

/// State for the standalone memory viewer window: the visible base address and
/// the loaded symbols (for the status bar). Navigated with the wheel / arrows /
/// PageUp-Down like the debugger's integrated pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryView {
    pub mem_base: u16,
    /// The bank browser's selection: `None` follows the live-mapped bank (the
    /// default); `Some(b)` pins to bank `b`, reinterpreted per region and folded
    /// to the region's bank count on use. `[`/`]` step it, a `BB:AAAA` Go-to sets
    /// it, and stepping back onto the live bank re-follows (`None`).
    pub bank: Option<u16>,
    pub symbols: Rc<SymbolTable>,
    /// Open `Go to…` prompt (Ctrl+G); `None` when idle. Mirrors the integrated
    /// debugger pane's modal, scoped to this standalone window.
    pub goto: Option<InputDialog>,
    /// The byte-edit cursor (bgb-style in-place editing): arrows move it, hex
    /// digits type over the byte at this address.
    pub cursor: u16,
    /// A pending high nibble mid-edit (`Some` after the first hex digit, before
    /// the second commits the byte); `None` when not typing.
    pub edit_hi: Option<u8>,
}

impl Default for MemoryView {
    fn default() -> Self {
        Self {
            mem_base: 0xFF00,
            bank: None,
            symbols: Rc::new(SymbolTable::default()),
            goto: None,
            cursor: 0xFF00,
            edit_hi: None,
        }
    }
}

impl MemoryView {
    /// Scroll the base by `rows` rows of 16 bytes (negative = lower addresses),
    /// wrapping the 64 KiB space — same model as the debugger memory pane.
    pub fn scroll(&mut self, rows: i32) {
        self.mem_base = self.mem_base.wrapping_add(rows.wrapping_mul(16) as u16);
    }

    /// Scrollbar `(frac, vis)` for a `visible`-row dump over the 64 KiB space:
    /// `frac` = base position, `vis` = visible fraction (thumb size).
    #[must_use]
    pub fn scroll_frac(&self, visible: usize) -> (f32, f32) {
        (self.mem_base as f32 / f32::from(u16::MAX), visible as f32 * 16.0 / 65536.0)
    }

    /// Jump the base to `frac` (0..1) of the 64 KiB space (row-aligned), from a
    /// scrollbar drag/click.
    pub fn set_scroll(&mut self, frac: f32) {
        self.mem_base = ((frac.clamp(0.0, 1.0) * f32::from(u16::MAX)) as u16) & !0x0F;
    }

    /// Apply a `Go to…` entry: a loaded symbol name resolves to its address,
    /// else a hex parse (accepting `$`/`0x` prefixes). Returns whether the entry
    /// resolved; an empty/garbage entry leaves the view unchanged. Positions the
    /// cursor at the target too. Mirrors the integrated pane's accept_dialog Goto.
    pub fn apply_goto(&mut self, text: &str) -> bool {
        let t = text.trim();
        // A loaded symbol name wins first (so a symbol that happens to contain a
        // colon isn't misread as a bank prefix). Then `BB:AAAA` — a bank-prefixed
        // address (the same form the MCP/debugger use), letting a human jump to an
        // arbitrary bank + address in one go. Then a bare hex address.
        if let Some(addr) = self.symbols.resolve(t) {
            self.mem_base = addr;
            self.cursor = addr;
            self.edit_hi = None;
            return true;
        }
        if let Some((b, a)) = t.split_once(':') {
            let addr = a.trim().trim_start_matches('$').trim_start_matches("0x");
            if let (Ok(bank), Ok(addr)) =
                (u16::from_str_radix(b.trim(), 16), u16::from_str_radix(addr, 16))
            {
                self.bank = Some(bank);
                self.mem_base = addr;
                self.cursor = addr;
                self.edit_hi = None;
                return true;
            }
        }
        let hex = t.trim_start_matches('$').trim_start_matches("0x");
        if let Ok(addr) = u16::from_str_radix(hex, 16) {
            self.mem_base = addr;
            self.cursor = addr;
            self.edit_hi = None;
            true
        } else {
            false
        }
    }

    /// Step the browsed bank by `delta` from the current selection (or `live`
    /// when following it), wrapping within the region's `count` and re-following
    /// on the live bank (see [`stepped_bank`]). Cancels a pending edit.
    pub fn step_bank(&mut self, delta: i32, live: u16, count: u16) {
        self.bank = stepped_bank(self.bank, delta, live, count);
        self.edit_hi = None;
    }

    /// Move the byte-edit cursor by `delta` bytes (wrapping 64 KiB), cancelling
    /// any pending edit, then scroll so it stays within `visible_rows` rows.
    pub fn move_cursor(&mut self, delta: i32, visible_rows: i32) {
        self.edit_hi = None;
        self.cursor = (i32::from(self.cursor) + delta).rem_euclid(0x1_0000) as u16;
        self.ensure_cursor_visible(visible_rows);
    }

    /// Scroll `mem_base` so the cursor stays within a window of `visible_rows`
    /// 16-byte rows — minimal one-edge scroll, wrapping the 64 KiB space.
    pub fn ensure_cursor_visible(&mut self, visible_rows: i32) {
        let vr = visible_rows.max(1);
        let cur = i32::from(self.cursor / 16); // cursor row, 0..4095
        let top = i32::from(self.mem_base / 16); // window top row
        let rel = (cur - top).rem_euclid(4096); // rows below top (wrapping)
        let new_top = if rel < vr {
            top // already visible
        } else if rel < 4096 - vr {
            cur - (vr - 1) // below the window: bring cursor to the last row
        } else {
            cur // above the window: bring cursor to the top row
        };
        self.mem_base = (new_top.rem_euclid(4096) as u16) * 16;
    }

    /// Feed a hex digit (0..=15) to the in-place editor. The first digit is held
    /// as the high nibble; the second completes the byte — returns `Some((addr,
    /// value))` for the caller to write, and advances the cursor. Otherwise `None`.
    pub fn edit_hex_digit(&mut self, d: u8) -> Option<(u16, u8)> {
        match self.edit_hi {
            None => {
                self.edit_hi = Some(d & 0x0F);
                None
            }
            Some(hi) => {
                let value = (hi << 4) | (d & 0x0F);
                let addr = self.cursor;
                self.edit_hi = None;
                self.cursor = self.cursor.wrapping_add(1);
                Some((addr, value))
            }
        }
    }

    /// Cancel a pending edit (Esc). Returns whether an edit was in progress (so
    /// the key is consumed only then).
    pub fn cancel_edit(&mut self) -> bool {
        self.edit_hi.take().is_some()
    }
}

/// Render a tool window's full content into `c` from the live machine and its
/// persistent UI `state` — the single entry point the event loop's redraw calls
/// (B12b). Pure (`&GameBoy`), so it tests headless against a real machine; the
/// winit layer only has to hand it a surface buffer + the window's state.
pub fn render(
    kind: ToolWindow,
    gb: &GameBoy,
    c: &mut Canvas,
    theme: &Theme,
    state: &WinState,
    bps: &Breakpoints,
) {
    let area = c.bounds();
    c.fill_rect(area, theme.bg);
    match kind {
        ToolWindow::Debugger => {
            let default = DebuggerState::default();
            let st = match state {
                WinState::Debugger(s) => s,
                _ => &default,
            };
            render_debugger(gb, c, area, theme, st, bps);
        }
        ToolWindow::Vram => {
            let default = VramState::default();
            let st = match state {
                WinState::Vram(s) => s,
                _ => &default,
            };
            render_vram(gb, c, area, theme, st);
        }
        ToolWindow::IoMap => render_iomap(gb, c, area, theme),
        ToolWindow::MemoryViewer => {
            let default = MemoryView::default();
            let st = match state {
                WinState::Memory(s) => s,
                _ => &default,
            };
            render_memory_window(gb, c, area, theme, st);
        }
    }
}

/// The live bank label for the banked region `base` sits in (ROM/VRAM/SRAM/
/// WRAM), or `None` in the fixed/unbanked regions — for the memory-viewer
/// status bar, so the CDL tint's bank is named. C000-CFFF is always WRAM
/// bank 0; D000-DFFF follows SVBK.
pub(crate) fn mem_bank_label(gb: &GameBoy, base: u16) -> Option<String> {
    match base {
        0x4000..=0x7FFF => Some(format!("ROM{:02X}", gb.rom_bank())),
        0x8000..=0x9FFF => Some(format!("VRM{}", gb.vram_bank())),
        0xA000..=0xBFFF => gb.ram_bank().map(|b| format!("SRM{b:02X}")),
        0xC000..=0xCFFF => Some("WRM0".to_string()),
        0xD000..=0xDFFF => Some(format!("WRM{}", gb.wram_bank())),
        _ => None,
    }
}

/// CDL: tint each visited byte's cell background (R/W/X) across `body`'s 16-col
/// hex grid from `base`, before the dump text draws over it. Off (all flags 0 /
/// log disabled) = no tint = unchanged view. Shared by the standalone memory
/// viewer and the debugger's memory pane so both give the same CDL feedback.
/// `Some(sel)` tints the browser's selected bank, folded to each address's own
/// region (`effective_bank`); `None` tints the live-mapped bank (the debugger
/// pane, unchanged).
fn cdl_tint(c: &mut Canvas, gb: &GameBoy, body: Rect, base: u16, bank: Option<u16>) {
    let lh = line_height();
    for row in 0..(body.h / lh).max(0) {
        for col in 0..16i32 {
            let addr = base.wrapping_add((row * 16 + col) as u16);
            let flag = match bank {
                Some(sel) => gb.cdl_flag_banked(effective_bank(sel, gb.region_bank_count(addr)), addr),
                None => gb.cdl_flag(addr),
            };
            if let Some(bg) = crate::cdl::cdl_color(flag) {
                let cch = 10 + 3 * col + i32::from(col >= 8);
                let (cx, cy) = (body.x + cch * GLYPH_W as i32, body.y + row * lh);
                c.fill_rect(Rect::new(cx, cy, 2 * GLYPH_W as i32, lh), bg);
            }
        }
    }
}

/// Fold a selected bank into the region containing an address: `sel % count`, so
/// an out-of-region selection (e.g. left over from another region) reads a valid
/// bank. `count` is `GameBoy::region_bank_count`; 0 (absent SRAM) pins to 0. The
/// one fold rule shared by the dump/tint/cursor render and the edit-write routing.
pub(crate) fn effective_bank(sel: u16, count: u16) -> u16 {
    sel % count.max(1)
}

/// The live-mapped bank of the banked region `base` sits in (0 for unbanked or an
/// absent/disabled RAM chip) — the "follow live" bank, and the stepper's start.
pub(crate) fn live_bank(gb: &GameBoy, base: u16) -> u16 {
    let b = match base {
        0x4000..=0x7FFF => gb.rom_bank(),
        0x8000..=0x9FFF => gb.vram_bank(),
        0xA000..=0xBFFF => gb.ram_bank().unwrap_or(0),
        0xD000..=0xDFFF => gb.wram_bank(),
        _ => 0,
    };
    b.min(usize::from(u16::MAX)) as u16
}

/// Resolve a browser selection to the concrete bank to read at `addr`: `None`
/// follows the live-mapped bank (the caller uses `debug_read`/`cdl_flag`);
/// `Some(sel)` pins to `sel` folded into `addr`'s region.
pub(crate) fn browse_bank(sel: Option<u16>, gb: &GameBoy, addr: u16) -> Option<u16> {
    sel.map(|b| effective_bank(b, gb.region_bank_count(addr)))
}

/// Read `addr` through the browser selection `sel`: the live-mapped bank when
/// following (`None`), else the pinned bank folded to `addr`'s region. The one
/// dump/cursor read shared by both memory panes.
pub(crate) fn banked_read(gb: &GameBoy, sel: Option<u16>, addr: u16) -> u8 {
    match browse_bank(sel, gb, addr) {
        Some(b) => gb.debug_read_banked(b, addr),
        None => gb.debug_read(addr),
    }
}

/// Write `val` to `addr` through the browser selection `sel` — the write mirror
/// of [`banked_read`], so an edit lands exactly where the dump shows it. Follow-
/// live (`None`) uses the RAMG-gated `debug_write` (matching the gated dump: a
/// write to disabled SRAM is the same no-op the CPU sees); a pinned bank uses the
/// raw `debug_write_banked`.
pub(crate) fn banked_write(gb: &mut GameBoy, sel: Option<u16>, addr: u16, val: u8) {
    match browse_bank(sel, gb, addr) {
        Some(b) => gb.debug_write_banked(b, addr, val),
        None => gb.debug_write(addr, val),
    }
}

/// Step the bank browser by `delta` from the current selection (or the live bank
/// when following it), wrapping within `count`. Landing back on the live bank
/// re-follows (`None`), so stepping always has a way home; a count of 0/1 keeps
/// the follow-live default.
pub(crate) fn stepped_bank(cur: Option<u16>, delta: i32, live: u16, count: u16) -> Option<u16> {
    let c = i32::from(count.max(1));
    let base = i32::from(cur.unwrap_or(live));
    let next = (base + delta).rem_euclid(c) as u16;
    (next != live).then_some(next)
}

/// The status-bar bank label for an **explicit** `bank` of the region `base`
/// sits in (the browser's selected bank), or `None` in the fixed/unbanked
/// regions and for absent cart RAM. Sibling of [`mem_bank_label`] (which names
/// the *live* bank); same format so the two compare cleanly.
pub(crate) fn sel_bank_label(gb: &GameBoy, base: u16, bank: u16) -> Option<String> {
    Some(match base {
        0x4000..=0x7FFF => format!("ROM{bank:02X}"),
        0x8000..=0x9FFF => format!("VRM{bank}"),
        0xA000..=0xBFFF if gb.region_bank_count(base) > 0 => format!("SRM{bank:02X}"),
        0xC000..=0xCFFF => "WRM0".to_string(),
        // WRAMX maps SVBK 0 → page 1 (there is no page-0 window here), so the
        // read/write/CDL paths fold bank 0 to 1; name the folded page so the label
        // matches the bytes shown (bank 0 aliases bank 1).
        0xD000..=0xDFFF => format!("WRM{}", bank.max(1)),
        _ => return None,
    })
}

/// The compact bank chip for the debugger memory pane (which has no status bar):
/// `Some("ROM05")` only when the browser is pinned to a bank; `None` while
/// following the live bank, so the default pane is drawn unchanged.
fn bank_chip_label(gb: &GameBoy, base: u16, sel: Option<u16>) -> Option<String> {
    let eff = effective_bank(sel?, gb.region_bank_count(base));
    sel_bank_label(gb, base, eff)
}

/// The memory-viewer bank status prefix for `loc` (the `AAAA  Name+off` part):
/// `None` follows the live bank → the classic `ROM05:loc` (live label, no
/// marker); `Some(raw)` is pinned → the selected label, plus `[live ROM02]` when
/// it has diverged from the live-mapped bank. Shared by the standalone status bar
/// and the debugger pane's bank chip.
pub(crate) fn mem_status_line(gb: &GameBoy, base: u16, sel: Option<u16>, loc: &str) -> String {
    let Some(raw) = sel else {
        return match mem_bank_label(gb, base) {
            Some(live) => format!("{live}:{loc}"),
            None => loc.to_string(),
        };
    };
    let eff = effective_bank(raw, gb.region_bank_count(base));
    match sel_bank_label(gb, base, eff) {
        Some(sel_l) => match mem_bank_label(gb, base) {
            Some(live) if live != sel_l => format!("{sel_l}:{loc}  [live {live}]"),
            _ => format!("{sel_l}:{loc}"),
        },
        None => loc.to_string(),
    }
}

/// Render the standalone memory viewer: the hex dump from the base address
/// filling the window above a one-line status bar showing the nearest preceding
/// symbol (`Name+offset`, or `----` with no symbols loaded).
fn render_memory_window(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme, st: &MemoryView) {
    let lh = line_height();
    let body = Rect::new(area.x, area.y, area.w, (area.h - lh).max(0));
    // The bank browser: read every dump byte / CDL flag / cursor byte through the
    // selection, folded to *each address's own region*, so a window straddling a
    // region boundary stays coherent per cell and matches the cursor + edit write.
    // `None` follows the live bank (unchanged classic view).
    cdl_tint(c, gb, body, st.mem_base, st.bank);
    debugger::render_memory(
        c,
        scroll_content(body),
        |a| banked_read(gb, st.bank, a),
        st.mem_base,
        theme,
        &st.symbols,
    );
    // Draggable scrollbar on the dump's right edge.
    let vis_rows = (body.h / lh).max(0);
    let (mf, mv) = st.scroll_frac(vis_rows.max(0) as usize);
    vscrollbar(c, body, mf, mv, theme);
    // In-place edit cursor: highlight the byte at `cursor` when it is visible in
    // the dump, overprinting its value (or the pending high nibble mid-edit).
    let off = st.cursor.wrapping_sub(st.mem_base);
    let (row, col) = (i32::from(off / 16), i32::from(off % 16));
    if row < (body.h / lh).max(0) {
        let gw = GLYPH_W as i32;
        // Fixed 9-char row label ("RRRR:AAAA"): byte `col`'s hex starts at char
        // 10 + 3*col, plus one for the extra gap before byte 8 (see `hex_row`).
        let cch = 10 + 3 * col + i32::from(col >= 8);
        let (cx, cy) = (body.x + cch * gw, body.y + row * lh);
        let accent = if st.edit_hi.is_some() {
            theme.current
        } else {
            theme.hilight
        };
        c.fill_rect(Rect::new(cx, cy, 2 * gw, lh), accent);
        let cur = banked_read(gb, st.bank, st.cursor);
        let text = match st.edit_hi {
            Some(hi) => format!("{hi:X}{:X}", cur & 0x0F),
            None => format!("{cur:02X}"),
        };
        draw_text(c, cx, cy, &text, theme.bg);
    }
    let bar_y = area.bottom() - lh;
    c.hline(area.x, bar_y, area.w, theme.border);
    let loc = match st.symbols.nearest_before(st.mem_base) {
        Some((name, base)) => format!("{:04X}  {name}+{:X}", st.mem_base, st.mem_base - base),
        None => format!("{:04X}  ----", st.mem_base),
    };
    // Following the live bank (`None`) shows the classic "ROM05:4000 …" status
    // (live label, no marker). When pinned to a bank, name it and append the live
    // bank it has diverged from: "…  [live ROM02]".
    let status = mem_status_line(gb, st.mem_base, st.bank, &loc);
    draw_text(c, area.x + 2, bar_y + 1, &status, theme.text);
    if let Some(dlg) = &st.goto {
        crate::ui::dialog::render(c, area, dlg, theme);
    }
}

fn regs_view(gb: &GameBoy, clock_base: u64) -> debugger::RegsView {
    let r = gb.cpu_regs();
    debugger::RegsView {
        af: r.af(),
        bc: r.bc(),
        de: r.de(),
        hl: r.hl(),
        sp: r.sp,
        pc: r.pc,
        ime: gb.ime(),
        ima: gb.ime_pending(),
        lcdc: gb.debug_read(0xFF40),
        stat: gb.debug_read(0xFF41),
        ly: gb.debug_read(0xFF44),
        ie: gb.debug_read(0xFFFF),
        iflag: gb.debug_read(0xFF0F),
        double_speed: gb.double_speed(),
        // Emulated cycles since the last user-clock reset (RM14); low 32 bits.
        cnt: gb.cycles().wrapping_sub(clock_base) as u32,
        rom_bank: gb.rom_bank(),
        ram_bank: gb.ram_bank(),
    }
}

fn render_debugger(
    gb: &GameBoy,
    c: &mut Canvas,
    area: Rect,
    theme: &Theme,
    st: &DebuggerState,
    bps: &Breakpoints,
) {
    let l = debugger::DebuggerLayout::for_size(area.w, area.h);
    let pc = gb.cpu_regs().pc;
    // The menu bar across the top, highlighting an open dropdown's parent label.
    debugger::render_menubar(c, l.menu, st.menu.as_ref().and_then(|m| m.bar), theme);
    // Disasm follows PC (or the pinned base); memory + stack from their bases.
    let start = st.disasm_start(pc);
    let rows = debugger::render_disasm(
        c,
        scroll_content(l.disasm),
        |a| gb.debug_read(a),
        start,
        pc,
        bps,
        &st.data_hints,
        st.disasm_fmt,
        &st.symbols,
        theme,
    );
    // Profiler: overlay per-line execution counts while logging (MB5).
    if gb.profiling() {
        debugger::render_profile_counts(c, scroll_content(l.disasm), &rows, |a| gb.profile_count(a), theme);
    }
    debugger::render_regs(c, l.regs, &regs_view(gb, st.clock_base), theme);
    let stack_rows = (l.stack.h / line_height()).max(0) as usize;
    debugger::render_stack(
        c,
        scroll_content(l.stack),
        &gb.stack(st.stack_off + stack_rows),
        st.stack_off,
        theme,
    );
    // The memory pane's bank browser (same model as the standalone viewer): reads
    // + CDL tint follow the live bank by default (`None`) and a pinned bank when
    // set. The pane has no status bar, so a pinned bank shows as a right-aligned
    // chip drawn over the top row.
    cdl_tint(c, gb, l.memory, st.mem_base, st.mem_bank);
    debugger::render_memory(
        c,
        scroll_content(l.memory),
        |a| banked_read(gb, st.mem_bank, a),
        st.mem_base,
        theme,
        &st.symbols,
    );
    if let Some(chip) = bank_chip_label(gb, st.mem_base, st.mem_bank) {
        let w = (chip.len() as i32) * GLYPH_W as i32;
        let cx = (l.memory.right() - w - 2).max(l.memory.x);
        c.fill_rect(Rect::new(cx - 1, l.memory.y, w + 2, line_height()), theme.current);
        draw_text(c, cx, l.memory.y, &chip, theme.bg);
    }
    // Draggable scrollbars on the three scrollable panes (right-edge strip).
    let lh = line_height();
    let (df, dv) = st.disasm_scroll((l.disasm.h / lh).max(0) as usize);
    vscrollbar(c, l.disasm, df, dv, theme);
    let (sf, sv) = st.stack_scroll(stack_rows);
    vscrollbar(c, l.stack, sf, sv, theme);
    let (mf, mv) = st.mem_scroll((l.memory.h / lh).max(0) as usize);
    vscrollbar(c, l.memory, mf, mv, theme);
    // The open context menu / modal draws last, on top of every pane.
    if let Some(om) = &st.menu {
        crate::ui::menu::render(c, om.origin, &om.items, om.hovered, theme);
    }
    if let Some(gd) = &st.dialog {
        crate::ui::dialog::render(c, area, &gd.input, theme);
    }
}

/// Per-tab VRAM geometry: the integer render scale fitted to the content area
/// (so content grows on resize), the grid cell pitch, the bounded drawn extent
/// (so the grid + frame hug the actual map, not the whole content rect — QA "bg
/// map should be bounded"), and whether the tab has a tile grid.
struct VramGeom {
    scale: i32,
    cell_w: i32,
    cell_h: i32,
    extent: Rect,
    grid: bool,
}

/// Compute [`VramGeom`] for `tab` inside the `content` area. Natural pixel sizes:
/// Tiles 16×24 tiles (128×192), BG map 32×32 (256×256), OAM an 8×5 grid of
/// 10-px cells (8-px tile + 2-px gap). Palettes has no grid.
fn vram_geom(tab: VramTab, content: Rect, tall: bool) -> VramGeom {
    let tiled = |cols: i32, rows: i32, cell_w: i32, cell_h: i32, scale: i32| VramGeom {
        scale,
        cell_w,
        cell_h,
        extent: Rect::new(content.x, content.y, cols * cell_w, rows * cell_h),
        grid: true,
    };
    match tab {
        VramTab::Tiles => {
            let s = vram::fit_scale(content.w, content.h, 16 * 8, 24 * 8);
            tiled(16, 24, 8 * s, 8 * s, s)
        }
        VramTab::BgMap => {
            let s = vram::fit_scale(content.w, content.h, 32 * 8, 32 * 8);
            tiled(32, 32, 8 * s, 8 * s, s)
        }
        VramTab::Oam => {
            // 8×16 mode needs a taller row pitch so the stacked tiles don't overlap.
            let (nw, nh) = (8 * vram::oam_cell(1), 5 * vram::oam_cell_h(1, tall));
            let s = vram::fit_scale(content.w, content.h, nw, nh);
            tiled(8, 5, vram::oam_cell(s), vram::oam_cell_h(s, tall), s)
        }
        VramTab::Palettes => VramGeom {
            scale: 1,
            cell_w: 0,
            cell_h: 0,
            extent: content,
            grid: false,
        },
    }
}

/// Two-column Tiles layout (CGB): bank 0 grid left, bank 1 grid right, each a
/// 16×24 tile grid fitted to half the `content` width with a small gutter
/// between. Returns `(left, right, scale)`. Shared by the render and the hover
/// hit-test so they can't drift.
fn tiles_two_col(content: Rect) -> (Rect, Rect, i32) {
    const GUTTER: i32 = 6;
    let half_w = (content.w - GUTTER).max(0) / 2;
    let s = vram::fit_scale(half_w, content.h, 16 * 8, 24 * 8);
    let (gw, gh) = (16 * 8 * s, 24 * 8 * s);
    let left = Rect::new(content.x, content.y, gw, gh);
    let right = Rect::new(content.x + half_w + GUTTER, content.y, gw, gh);
    (left, right, s)
}

/// Two-column BG-map layout: BG tilemap left, window tilemap right, each a 32×32
/// tile grid fitted to half the `content` width with a small gutter. Mirrors
/// [`tiles_two_col`]; shared by the render and the hover hit-test.
fn bgmap_two_col(content: Rect) -> (Rect, Rect, i32) {
    const GUTTER: i32 = 6;
    let half_w = (content.w - GUTTER).max(0) / 2;
    let s = vram::fit_scale(half_w, content.h, 32 * 8, 32 * 8);
    let g = 32 * 8 * s;
    let left = Rect::new(content.x, content.y, g, g);
    let right = Rect::new(content.x + half_w + GUTTER, content.y, g, g);
    (left, right, s)
}

fn render_vram(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme, state: &VramState) {
    let l = vram::layout(area);
    vram::render_tabs(c, area.x + 2, area.y + 2, state.tab, theme);
    let cgb = gb.model().is_cgb();
    let tall = gb.debug_read(0xFF40) & 0x04 != 0;
    let g = vram_geom(state.tab, l.content, tall);
    // CGB has two VRAM banks; the Tiles tab shows both side by side (bank 0 left,
    // bank 1 right), so its geometry differs from the single-grid vram_geom (each
    // grid fits half the content width). DMG has one bank → None.
    let tiles_two = (state.tab == VramTab::Tiles && cgb).then(|| tiles_two_col(l.content));
    // The BG-map tab shows the BG tilemap (left) and window tilemap (right) side
    // by side, like the two-bank Tiles view.
    let bgmap_two = (state.tab == VramTab::BgMap).then(|| bgmap_two_col(l.content));
    match state.tab {
        VramTab::Tiles => {
            // A raw tile has no inherent palette, so bgb renders the Tiles grid
            // in a neutral grey ramp rather than through one game palette. On CGB
            // both banks show at once (bank 0 left, bank 1 right); DMG has one.
            if let Some((left, right, s)) = tiles_two {
                vram::render_tiles(c, left, gb.vram(), 0, &vram::GREYS, s);
                vram::render_tiles(c, right, gb.vram(), 1, &vram::GREYS, s);
            } else {
                vram::render_tiles(c, l.content, gb.vram(), 0, &vram::GREYS, g.scale);
            }
        }
        VramTab::Oam => {
            let (pals, n) = obj_palettes(gb, state.show_paletted);
            vram::render_oam(
                c,
                l.content,
                gb.oam(),
                gb.vram(),
                &pals[..n],
                cgb,
                tall,
                g.scale,
            );
        }
        VramTab::BgMap => {
            let (bg_base, win_base, signed) = bgmap_bases(gb, state);
            let (pals, n) = bg_palettes(gb, state.show_paletted);
            let (left, right, s) = bgmap_two.expect("bgmap_two set on the BG map tab");
            // Left = BG tilemap with the screen viewport box; right = window
            // tilemap with the WX/WY region box (both gated by `scxy`).
            vram::render_bgmap(
                c, left, gb.vram(), bg_base, signed, &pals[..n], cgb, s,
                screen_overlay(gb, state.scxy), theme,
            );
            vram::render_bgmap(
                c, right, gb.vram(), win_base, signed, &pals[..n], cgb, s,
                window_overlay(gb, state.scxy), theme,
            );
        }
        VramTab::Palettes => {
            // On a monochrome model the CGB palette RAM is meaningless; show the
            // BGP/OBP0/OBP1 shade mappings instead (so rBGP/rOBP are inspectable).
            // CGB/AGB use the palette RAM path below.
            if !gb.model().is_cgb() {
                vram::render_palettes_dmg(
                    c,
                    l.content,
                    gb.debug_read(0xFF47),
                    gb.debug_read(0xFF48),
                    gb.debug_read(0xFF49),
                    theme,
                );
            } else {
                let (bg, obj) = gb.cgb_palette_ram();
                vram::render_palettes(c, l.content, bg, obj, theme);
            }
        }
    }
    // bgb frames the grid and the details column as separate panels. The grid
    // tabs frame the *bounded* extent (so the map doesn't bleed grid lines into
    // empty space); Palettes frames the whole content area. The two-grid Tiles /
    // BG-map views frame each grid separately.
    let two = tiles_two.or(bgmap_two);
    if let Some((left, right, s)) = two {
        let cell = 8 * s;
        if state.grid {
            draw_grid(c, left, cell, cell, theme);
            draw_grid(c, right, cell, cell, theme);
        }
        c.outline_rect(left, theme.border);
        c.outline_rect(right, theme.border);
    } else {
        if state.grid && g.grid {
            draw_grid(c, g.extent, g.cell_w, g.cell_h, theme);
        }
        c.outline_rect(if g.grid { g.extent } else { l.content }, theme.border);
    }
    c.outline_rect(l.details, theme.border);
    render_vram_controls(c, &l, state, cgb, theme);
    render_vram_details(gb, c, &l, state, g.scale, two, theme);
}

/// The BG-map tab's 8 BG palettes (CGB) or single BGP palette (DMG) as RGB888,
/// or a single neutral grey ramp when `show_paletted` is off.
/// Expand `cram`'s 8 CGB palettes (BG or OBJ) into `out` as RGB888.
fn cgb_palettes(cram: &[u8], out: &mut [[u32; 4]; 8]) {
    for (p, slot) in out.iter_mut().enumerate() {
        *slot = debug::cgb_palette_words(cram, p).map(xrgb);
    }
}

/// A DMG palette register (`BGP`/`OBP*`) as four RGB888 shades.
fn dmg_palette(gb: &GameBoy, reg: u16) -> [u32; 4] {
    debug::dmg_palette_shades(gb.debug_read(reg)).map(|s| vram::GREYS[s as usize])
}

fn bg_palettes(gb: &GameBoy, show_paletted: bool) -> ([[u32; 4]; 8], usize) {
    let mut out = [vram::GREYS; 8];
    if !show_paletted {
        (out, 1)
    } else if gb.model().is_cgb() {
        cgb_palettes(gb.cgb_palette_ram().0, &mut out);
        (out, 8)
    } else {
        out[0] = dmg_palette(gb, 0xFF47);
        (out, 1)
    }
}

/// The OAM tab's 8 OBJ palettes (CGB) or the OBP0/OBP1 pair (DMG) as RGB888, or
/// a single neutral grey ramp when `show_paletted` is off. Returns a fixed array
/// + the live count (no per-redraw allocation).
fn obj_palettes(gb: &GameBoy, show_paletted: bool) -> ([[u32; 4]; 8], usize) {
    let mut out = [vram::GREYS; 8];
    if !show_paletted {
        (out, 1)
    } else if gb.model().is_cgb() {
        cgb_palettes(gb.cgb_palette_ram().1, &mut out);
        (out, 8)
    } else {
        out[0] = dmg_palette(gb, 0xFF48);
        out[1] = dmg_palette(gb, 0xFF49);
        (out, 2)
    }
}

/// The BG grid's screen viewport (SCX/SCY) box when `on`, else no overlay.
fn screen_overlay(gb: &GameBoy, on: bool) -> vram::MapOverlay {
    if on {
        vram::MapOverlay::Screen {
            scx: gb.debug_read(0xFF43),
            scy: gb.debug_read(0xFF42),
        }
    } else {
        vram::MapOverlay::None
    }
}

/// The window grid's WX/WY region box when `on`, else no overlay.
fn window_overlay(gb: &GameBoy, on: bool) -> vram::MapOverlay {
    if on {
        vram::MapOverlay::Window {
            wx: gb.debug_read(0xFF4B),
            wy: gb.debug_read(0xFF4A),
        }
    } else {
        vram::MapOverlay::None
    }
}

/// A 15-bit BGR555 word as an XRGB8888 pixel.
fn xrgb(word: u16) -> u32 {
    let (r, g, b) = debug::rgb555_to_rgb888(word);
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Resolve the two BG-map bases (BG tilemap, window tilemap) + shared tile
/// addressing from the source radios, falling back to LCDC auto-detection: BG uses
/// the BG tilemap select (bit 3), window the window tilemap select (bit 6).
// ponytail: an explicit `Map` radio (9800/9C00) forces BOTH grids to that base
// (they then show the same region) — `Auto` is the useful default that shows the
// two distinct maps.
fn bgmap_bases(gb: &GameBoy, state: &VramState) -> (u16, u16, bool) {
    let lcdc = gb.debug_read(0xFF40);
    let base_of = |auto_9c00: bool| match state.map_src {
        1 => 0x9800,
        2 => 0x9C00,
        _ if auto_9c00 => 0x9C00,
        _ => 0x9800,
    };
    let signed = match state.tile_src {
        1 => true,
        2 => false,
        _ => lcdc & 0x10 == 0,
    };
    (base_of(lcdc & 0x08 != 0), base_of(lcdc & 0x40 != 0), signed)
}

/// Overlay grid lines at `cell_w`×`cell_h` pitch over the content area (the OAM
/// tab's cells are taller than wide in 8×16 mode).
fn draw_grid(c: &mut Canvas, content: Rect, cell_w: i32, cell_h: i32, theme: &Theme) {
    let saved = c.push_clip(content);
    let mut x = content.x;
    while x <= content.right() && cell_w > 0 {
        c.vline(x, content.y, content.h, theme.hilight);
        x += cell_w;
    }
    let mut y = content.y;
    while y <= content.bottom() && cell_h > 0 {
        c.hline(content.x, y, content.w, theme.hilight);
        y += cell_h;
    }
    c.set_clip(saved);
}

/// Draw the checkboxes/radios in the details column, reflecting `state`. `cgb`
/// gates the CGB-only Tiles bank toggle.
fn render_vram_controls(
    c: &mut Canvas,
    l: &VramLayout,
    state: &VramState,
    cgb: bool,
    theme: &Theme,
) {
    if state.tab == VramTab::Tiles && cgb {
        checkbox(
            c,
            l.tile_bank_box.x,
            l.tile_bank_box.y,
            state.tile_bank != 0,
            "VRAM bank 1",
            theme,
        );
    }
    if state.tab == VramTab::BgMap {
        radio_group(
            c,
            l.map_src[0].x,
            l.map_src[0].y,
            &vram::MAP_SRC,
            state.map_src as usize,
            theme,
        );
        radio_group(
            c,
            l.tile_src[0].x,
            l.tile_src[0].y,
            &vram::TILE_SRC,
            state.tile_src as usize,
            theme,
        );
        checkbox(c, l.scxy_box.x, l.scxy_box.y, state.scxy, "scxy", theme);
    }
    checkbox(
        c,
        l.paletted_box.x,
        l.paletted_box.y,
        state.show_paletted,
        "show paletted",
        theme,
    );
    if state.tab != VramTab::Palettes {
        checkbox(c, l.grid_box.x, l.grid_box.y, state.grid, "Grid", theme);
    }
}

/// Draw the hovered-cell field list (bgb's right panel) for the active tab.
/// `scale` is the tab's live render scale ([`vram_geom`]), so the hover hit-test
/// matches the drawn cell size at any window size.
fn render_vram_details(
    gb: &GameBoy,
    c: &mut Canvas,
    l: &VramLayout,
    state: &VramState,
    scale: i32,
    two: Option<(Rect, Rect, i32)>,
    theme: &Theme,
) {
    let Some((hx, hy)) = state.hover else {
        return;
    };
    let (lx, ly) = (hx - l.content.x, hy - l.content.y);
    if lx < 0 || ly < 0 {
        return;
    }
    let m8 = state.tile_hex_8bit;
    let lines = match state.tab {
        VramTab::Tiles => match two {
            Some((left, right, s)) => tile_details_two(lx, ly, left, right, s, m8),
            None => tile_details(lx, ly, scale, m8),
        },
        VramTab::Oam => oam_details(gb, lx, ly, scale, m8),
        VramTab::BgMap => match two {
            Some((left, right, s)) => bgmap_details_two(gb, state, lx, ly, left, right, s, m8),
            None => Vec::new(),
        },
        VramTab::Palettes => return,
    };
    let mut y = l.details.y;
    for line in lines {
        draw_text(c, l.details.x, y, &line, theme.text);
        y += line_height();
    }
}

/// A count shown decimal with its hex in parens, bgb-style: `10 ($0A)`,
/// `383 ($17F)`. Min two hex digits, widening as needed (tiles reach 383). When
/// `mask8` (Options → Debug "8-bit tile hex", matching tools that show the raw
/// tilemap byte) the hex wraps to the low 8 bits, so `383 ($7F)`.
fn dec_hex(n: u32, mask8: bool) -> String {
    let hex = if mask8 { n & 0xFF } else { n };
    format!("{n} (${hex:02X})")
}

/// Tiles-tab details: the tile under `(lx, ly)` in the 16-wide grid at `scale`.
/// The content area is wider than the grid, so an out-of-column hover has no tile.
fn tile_details(lx: i32, ly: i32, scale: i32, mask8: bool) -> Vec<String> {
    let col = lx / (8 * scale);
    let tile = (ly / (8 * scale)) * 16 + col;
    if col >= 16 || !(0..384).contains(&tile) {
        return Vec::new();
    }
    vec![
        format!("Tile No. {}", dec_hex(tile as u32, mask8)),
        format!("Tile Address 0:{:04X}", 0x8000 + tile * 16),
    ]
}

/// Two-bank Tiles hover (CGB): resolve content-relative `(lx, ly)` to a tile in
/// the left (bank 0) or right (bank 1) grid — geometry from [`tiles_two_col`] —
/// and print the real bank in the `bank:addr` label. A hover in the gutter or
/// off-grid yields no tile.
fn tile_details_two(lx: i32, ly: i32, left: Rect, right: Rect, scale: i32, mask8: bool) -> Vec<String> {
    let (bank, gx) = if lx < left.w {
        (0, lx)
    } else {
        let rx = lx - (right.x - left.x);
        if (0..right.w).contains(&rx) {
            (1, rx)
        } else {
            return Vec::new(); // gutter between the two grids
        }
    };
    let col = gx / (8 * scale);
    let tile = (ly / (8 * scale)) * 16 + col;
    if col >= 16 || !(0..384).contains(&tile) {
        return Vec::new();
    }
    vec![
        format!("Tile No. {}", dec_hex(tile as u32, mask8)),
        format!("Tile Address {bank}:{:04X}", 0x8000 + tile * 16),
    ]
}

/// OAM-tab details: the sprite under `(lx, ly)` in the 8-wide cell grid at `scale`.
fn oam_details(gb: &GameBoy, lx: i32, ly: i32, scale: i32, mask8: bool) -> Vec<String> {
    let tall = gb.debug_read(0xFF40) & 0x04 != 0;
    let (col, row) = (
        lx / vram::oam_cell(scale),
        ly / vram::oam_cell_h(scale, tall),
    );
    let idx = (row * 8 + col) as usize;
    if col >= 8 || idx >= 40 {
        return Vec::new();
    }
    let s = debug::oam_sprites(gb.oam())[idx];
    vec![
        format!("OAM addr FE{:02X}", idx * 4),
        format!("X-loc {}", s.x),
        format!("Y-loc {}", s.y),
        format!("Tile No {}", dec_hex(u32::from(s.tile), mask8)),
        format!("Attribute {:02X}", s.attr),
        format!("X-flip {}", u8::from(s.attr & 0x20 != 0)),
        format!("Y-flip {}", u8::from(s.attr & 0x40 != 0)),
        format!("Palette OBJ {}", s.attr & 0x07),
    ]
}

/// BG-map-tab details: resolve content-relative `(lx, ly)` to a cell in the left
/// (BG tilemap) or right (window tilemap) grid — geometry from [`bgmap_two_col`] —
/// and print which map it is + its address. A hover in the gutter or off-grid
/// yields no cell.
#[allow(clippy::too_many_arguments)]
fn bgmap_details_two(
    gb: &GameBoy,
    state: &VramState,
    lx: i32,
    ly: i32,
    left: Rect,
    right: Rect,
    scale: i32,
    mask8: bool,
) -> Vec<String> {
    let (is_window, gx) = if lx < left.w {
        (false, lx)
    } else {
        let rx = lx - (right.x - left.x);
        if (0..right.w).contains(&rx) {
            (true, rx)
        } else {
            return Vec::new(); // gutter between the two grids
        }
    };
    let (col, row) = (gx / (8 * scale), ly / (8 * scale));
    if col >= 32 || row >= 32 {
        return Vec::new();
    }
    let (bg_base, win_base, signed) = bgmap_bases(gb, state);
    let base = if is_window { win_base } else { bg_base };
    let idx = (row * 32 + col) as usize;
    let cell = debug::bg_map(gb.vram(), base)[idx];
    let tile = vram::tile_index(cell.tile, signed);
    vec![
        format!("{}  X {col}  Y {row}", if is_window { "Window" } else { "BG" }),
        format!("Tile No. {}", dec_hex(u32::from(cell.tile), mask8)),
        format!("Attribute {:02X}", cell.attr),
        format!("Map address {:04X}", base as usize + idx),
        format!("Tile address 0:{:04X}", 0x8000 + tile * 16),
        format!("X-flip {}", u8::from(cell.attr & 0x20 != 0)),
        format!("Y-flip {}", u8::from(cell.attr & 0x40 != 0)),
        format!("palette BG {}", cell.attr & 0x07),
    ]
}

fn render_iomap(gb: &GameBoy, c: &mut Canvas, area: Rect, theme: &Theme) {
    let read = |a: u16| gb.debug_read(a);
    let lh = line_height();
    let col_w = area.w / 4;
    let x = |i: i32| area.x + 2 + i * col_w;
    let y0 = area.y + 2;
    let label = |c: &mut Canvas, x: i32, y: i32, s: &str| draw_text(c, x, y, s, theme.text);

    // Col 0: LCD registers, then the LCDC bit breakdown.
    let after_lcd = iomap::render_group(c, x(0), y0, &read, iomap::LCD, theme);
    label(c, x(0), after_lcd + lh, "LCDC (FF40)");
    iomap::render_bits(
        c,
        x(0),
        after_lcd + 2 * lh,
        read(0xFF40),
        &iomap::LCDC_BITS,
        7,
        theme,
    );

    // Col 1: the "various" registers, then the STAT bit breakdown, then the
    // cartridge ROM/RAM bank indicator (distinct from VBK/SVBK above it).
    let after_var = iomap::render_group(c, x(1), y0, &read, iomap::VARIOUS, theme);
    label(c, x(1), after_var + lh, "STAT (FF41)");
    iomap::render_bits(
        c,
        x(1),
        after_var + 2 * lh,
        read(0xFF41),
        &iomap::STAT_BITS,
        6,
        theme,
    );
    label(
        c,
        x(1),
        after_var + (2 + iomap::STAT_BITS.len() as i32 + 1) * lh,
        &iomap::bank_line(gb.rom_bank(), gb.ram_bank()),
    );

    // Col 2: the sound channels + master control.
    iomap::render_group(c, x(2), y0, &read, iomap::SOUND, theme);

    // Col 3: GBC DMA + palette ports, then the IF/IE interrupt vectors.
    let after_dma = iomap::render_group(c, x(3), y0, &read, iomap::GBC_DMA, theme);
    let after_pal = iomap::render_group(c, x(3), after_dma + lh, &read, iomap::GBC_PAL, theme);
    label(c, x(3), after_pal + lh, "IF, IE");
    iomap::render_vectors(
        c,
        x(3),
        after_pal + 2 * lh,
        read(0xFF0F),
        read(0xFFFF),
        theme,
    );

    // Wave pattern (FF30–3F): full-width row along the bottom. Sourced from the
    // raw wave-RAM buffer (the gated FF3x read path is unreliable while ch3 plays).
    let wy = area.bottom() - lh - 2;
    label(c, x(0), wy, "wave (FF3x)");
    draw_text(
        c,
        x(0) + 11 * 8,
        wy,
        &iomap::wave_row(&gb.wave_ram()),
        theme.text,
    );
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
