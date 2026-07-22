//! Layer C: the bgb tool windows (Debugger / VRAM viewer / I/O map). Each is a
//! pure content renderer composing the `ui` widgets over `slopgb_core::debug`
//! introspection, unit-tested headless; the event loop (B12b) feeds each one a
//! real softbuffer surface and routes its input.

pub mod debugger;
pub mod iomap;
pub mod mainwin;
pub mod options;
pub mod vram;
mod vram_render;

use std::rc::Rc;

use slopgb_core::{GameBoy, debug};

use crate::dbg::Breakpoints;
use crate::symbols::SymbolTable;
use crate::ui::canvas::Rect;
use crate::ui::dialog::InputDialog;
use crate::ui::font::GLYPH_W;
use crate::ui::text::{draw_text, line_height};
use crate::ui::widgets::{SCROLLBAR_W, checkbox, radio_group, scroll_content, vscrollbar};
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
        (
            self.mem_base as f32 / f32::from(u16::MAX),
            visible as f32 * 16.0 / 65536.0,
        )
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
        if let Some((bank, addr)) = self.symbols.resolve(t) {
            // Pin the browser to the symbol's own bank (so `01:6401 Foo` shows
            // bank 1's bytes). A bank-0 symbol lives in a fixed/unbanked region,
            // so keep following the live bank there (no needless pin chip).
            self.bank = (bank != 0).then_some(bank);
            self.mem_base = addr;
            self.cursor = addr;
            self.edit_hi = None;
            return true;
        }
        if let Some((b, a)) = t.split_once(':') {
            let addr = a.trim().trim_start_matches('$').trim_start_matches("0x");
            if let (Ok(bank), Ok(addr)) = (
                u16::from_str_radix(b.trim(), 16),
                u16::from_str_radix(addr, 16),
            ) {
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
            vram_render::render_vram(gb, c, area, theme, st);
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
        // Per-address: MBC6's window B (6000-7FFF) banks independently.
        0x4000..=0x7FFF => Some(format!("ROM{:02X}", gb.rom_bank_at(base))),
        0x8000..=0x9FFF => Some(format!("VRM{}", gb.vram_bank())),
        // Per-address: MBC6's RAM window B (B000-BFFF) banks independently.
        0xA000..=0xBFFF => gb.ram_bank_at(base).map(|b| format!("SRM{b:02X}")),
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
                Some(sel) => {
                    gb.cdl_flag_banked(effective_bank(sel, gb.region_bank_count(addr)), addr)
                }
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
        // Per-address: MBC6's window B (6000-7FFF) banks independently.
        0x4000..=0x7FFF => gb.rom_bank_at(base),
        0x8000..=0x9FFF => gb.vram_bank(),
        // Per-address: MBC6's RAM window B (B000-BFFF) banks independently.
        0xA000..=0xBFFF => gb.ram_bank_at(base).unwrap_or(0),
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

/// The concrete bank the dump shows for `addr`: the pinned selection folded to
/// `addr`'s region, or the live-mapped bank when following live. The bank a
/// symbol must match to be named at `addr` (bank-discriminated `name_at`).
pub(crate) fn shown_bank(gb: &GameBoy, sel: Option<u16>, addr: u16) -> u16 {
    browse_bank(sel, gb, addr).unwrap_or_else(|| live_bank(gb, addr))
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

/// Draw a right-aligned bank chip over the top row of a pane (the disasm/memory
/// panes have no status bar). `None` label → nothing drawn, so a live-following
/// pane is untouched.
fn draw_bank_chip(c: &mut Canvas, pane: Rect, label: Option<String>, theme: &Theme) {
    let Some(chip) = label else { return };
    let w = (chip.len() as i32) * GLYPH_W as i32;
    // Sit left of the pane's vertical scrollbar so the bar doesn't cover the chip.
    let cx = (pane.right() - SCROLLBAR_W - w - 2).max(pane.x);
    c.fill_rect(
        Rect::new(cx - 1, pane.y, w + 2, line_height()),
        theme.current,
    );
    draw_text(c, cx, pane.y, &chip, theme.bg);
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
/// filling the window above a one-line status bar showing the nearest symbol
/// preceding the cursor (`Name+offset`, or `----` with no symbols loaded).
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
        |a| shown_bank(gb, st.bank, a),
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
    let loc = match st
        .symbols
        .nearest_before(shown_bank(gb, st.bank, st.cursor), st.cursor)
    {
        Some((name, base)) => format!("{:04X}  {name}+{:X}", st.cursor, st.cursor - base),
        None => format!("{:04X}  ----", st.cursor),
    };
    // Following the live bank (`None`) shows the classic "ROM05:4000 …" status
    // (live label, no marker). When pinned to a bank, name it and append the live
    // bank it has diverged from: "…  [live ROM02]".
    let status = mem_status_line(gb, st.cursor, st.bank, &loc);
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
    let start = st.disasm_start();
    let rows = debugger::render_disasm(
        c,
        scroll_content(l.disasm),
        |a| banked_read(gb, st.disasm_bank, a),
        start,
        pc,
        bps,
        &st.data_hints,
        st.disasm_fmt,
        &st.symbols,
        |a| shown_bank(gb, st.disasm_bank, a),
        theme,
    );
    // Profiler: overlay per-line execution counts while logging (MB5).
    if gb.profiling() {
        debugger::render_profile_counts(
            c,
            scroll_content(l.disasm),
            &rows,
            |a| gb.profile_count(a),
            theme,
        );
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
        |a| shown_bank(gb, st.mem_bank, a),
    );
    draw_bank_chip(
        c,
        l.memory,
        bank_chip_label(gb, st.mem_base, st.mem_bank),
        theme,
    );
    // The disasm pane carries the same pinned-bank chip: while pinned to a bank
    // it names that bank (e.g. "ROM01"), so the "other bank view" reads as a
    // separate view even as the game maps other banks in.
    draw_bank_chip(
        c,
        l.disasm,
        bank_chip_label(gb, st.disasm_base, st.disasm_bank),
        theme,
    );
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
