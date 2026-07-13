use super::*;
use slopgb_core::{GameBoy, Model};

fn machine() -> GameBoy {
    GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).expect("zeroed rom loads")
}

#[test]
fn render_each_tool_window_fills_background_and_draws_content() {
    let theme = Theme::BGB;
    let gb = machine();
    for kind in [
        ToolWindow::Debugger,
        ToolWindow::Vram,
        ToolWindow::IoMap,
        ToolWindow::MemoryViewer,
    ] {
        let (w, h) = (640usize, 480usize);
        let mut buf = vec![0xDEAD_BEEF_u32; w * h];
        {
            let mut c = Canvas::new(&mut buf, w, h);
            render(
                kind,
                &gb,
                &mut c,
                &theme,
                &WinState::new(kind),
                &Breakpoints::default(),
            );
        }
        // The whole surface was painted (no leftover sentinel) and the window
        // background + some text ink are present.
        assert!(
            !buf.contains(&0xDEAD_BEEF),
            "{kind:?}: surface fully painted"
        );
        assert!(buf.contains(&theme.bg), "{kind:?}: background filled");
        assert!(buf.contains(&theme.text), "{kind:?}: content drawn");
    }
}

#[test]
fn memory_view_scroll_wraps_by_rows() {
    let mut m = MemoryView::default();
    assert_eq!(m.mem_base, 0xFF00);
    m.scroll(-1);
    assert_eq!(m.mem_base, 0xFEF0);
    m.scroll(2);
    assert_eq!(m.mem_base, 0xFF10);
    m.mem_base = 0xFFF0;
    m.scroll(1);
    assert_eq!(m.mem_base, 0x0000, "wraps past the top");
}

#[test]
fn memory_view_goto_resolves_hex_symbol_and_ignores_junk() {
    use crate::symbols::SymbolTable;
    use std::rc::Rc;
    let mut v = MemoryView::default();
    assert!(v.apply_goto("C000"));
    assert_eq!(v.mem_base, 0xC000);
    assert!(v.apply_goto("$8000"), "accepts $ prefix");
    assert_eq!(v.mem_base, 0x8000);
    assert!(!v.apply_goto("zzz"), "garbage rejected");
    assert_eq!(v.mem_base, 0x8000, "junk leaves base unchanged");
    // A loaded symbol name resolves to its address.
    v.symbols = Rc::new(SymbolTable::parse("00:1234 Foo"));
    assert!(v.apply_goto("Foo"));
    assert_eq!(v.mem_base, 0x1234);
}

#[test]
fn memory_view_goto_bank_prefixed_address_pins_bank_and_base() {
    let mut v = MemoryView::default();
    assert!(v.apply_goto("03:4000"), "BB:AAAA pins bank + base");
    assert_eq!((v.bank, v.mem_base, v.cursor), (Some(3), 0x4000, 0x4000));
    // The address half still accepts a $/0x prefix.
    assert!(v.apply_goto("2:$A100"));
    assert_eq!((v.bank, v.mem_base), (Some(2), 0xA100));
    // A colon-less address takes the plain symbol/hex path and leaves the bank.
    v.bank = Some(5);
    assert!(v.apply_goto("C000"));
    assert_eq!(
        (v.bank, v.mem_base),
        (Some(5), 0xC000),
        "plain goto leaves bank"
    );
    // A colon'd but non-hex bank fails both parses → whole input rejected, no move.
    assert!(!v.apply_goto("zz:1000"), "non-hex bank is not a valid goto");
    assert_eq!(
        (v.bank, v.mem_base),
        (Some(5), 0xC000),
        "rejected goto changes nothing"
    );
}

#[test]
fn memory_view_step_bank_wraps_and_refollows_the_live_bank() {
    // Live bank 2, a 4-bank region. Stepping off live pins; stepping back onto the
    // live bank re-follows (None).
    let mut v = MemoryView::default();
    assert_eq!(v.bank, None, "defaults to following the live bank");
    v.step_bank(1, 2, 4); // live 2 → pin 3
    assert_eq!(v.bank, Some(3));
    v.step_bank(1, 2, 4); // 3 → wrap to 0
    assert_eq!(v.bank, Some(0));
    v.step_bank(1, 2, 4); // 0 → 1
    assert_eq!(v.bank, Some(1));
    v.step_bank(1, 2, 4); // 1 → 2 == live → re-follow
    assert_eq!(v.bank, None, "landing on the live bank re-follows");
    // A fixed/unbanked region (count 0/1) keeps following live.
    v.step_bank(1, 0, 1);
    assert_eq!(v.bank, None);
    v.step_bank(1, 0, 0);
    assert_eq!(v.bank, None, "absent-RAM count 0 stays follow-live");
}

#[test]
fn memory_view_edit_two_nibbles_commit_a_byte_and_advance() {
    let mut v = MemoryView {
        cursor: 0xC000,
        ..Default::default()
    };
    // First nibble is held, no write yet.
    assert_eq!(v.edit_hex_digit(0xA), None);
    assert_eq!(v.edit_hi, Some(0xA));
    // Second nibble completes 0xA5, returns the write, advances the cursor.
    assert_eq!(v.edit_hex_digit(0x5), Some((0xC000, 0xA5)));
    assert_eq!(v.edit_hi, None);
    assert_eq!(v.cursor, 0xC001, "cursor advanced to the next byte");
}

#[test]
fn memory_view_cancel_edit_discards_pending_nibble() {
    let mut v = MemoryView::default();
    assert!(!v.cancel_edit(), "nothing to cancel when idle");
    v.edit_hex_digit(0xF);
    assert!(v.cancel_edit(), "a pending edit is cancelled");
    assert_eq!(v.edit_hi, None);
    assert!(!v.cancel_edit(), "already cancelled");
}

#[test]
fn memory_view_cursor_move_autoscrolls_and_cancels_edit() {
    let mut v = MemoryView::default(); // mem_base = cursor = 0xFF00
    v.edit_hex_digit(0xC); // start an edit
    v.move_cursor(-16, 8); // up one row cancels the edit and scrolls the view
    assert_eq!(v.edit_hi, None, "moving cancels a pending edit");
    assert_eq!(v.cursor, 0xFEF0);
    assert_eq!(
        v.mem_base, 0xFEF0,
        "scrolled up so the cursor stays visible"
    );
    // Moving within the visible window does not scroll.
    v.move_cursor(16, 8);
    assert_eq!(v.cursor, 0xFF00);
    assert_eq!(v.mem_base, 0xFEF0, "cursor still visible, no scroll");
}

#[test]
fn memory_window_tints_cdl_flagged_bytes() {
    let mut gb = machine();
    // ROM low area (bank 0): the physical CDL index equals the GB address, so
    // the fixture index is unambiguous under the bank-aware layout.
    let base = 0x0100u16;
    gb.set_cdl(true);
    let mut fixture = gb.cdl_flags().unwrap().to_vec();
    fixture[base as usize] = 4; // X → red at column 0
    assert!(gb.load_cdl(&fixture));
    // Cursor left at the default 0xFF00 (off-screen from base) so its overlay
    // can't cover the tinted cell.
    let st = WinState::Memory(MemoryView {
        mem_base: base,
        ..MemoryView::default()
    });
    let (w, h) = (430usize, 360usize);
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(
            ToolWindow::MemoryViewer,
            &gb,
            &mut c,
            &Theme::BGB,
            &st,
            &Breakpoints::default(),
        );
    }
    let gw = crate::ui::font::GLYPH_W;
    let lh = crate::ui::text::line_height() as usize;
    let want = crate::cdl::cdl_color(4).unwrap();
    let cell_has =
        |cx: usize, color: u32| (0..lh).any(|y| (cx..cx + 2 * gw).any(|x| buf[y * w + x] == color));
    // Column 0 hex starts at char 10; the flagged byte's cell is tinted.
    assert!(cell_has(10 * gw, want), "flagged byte cell tinted");
    // Column 1 (char 13) is unflagged → not tinted.
    assert!(!cell_has(13 * gw, want), "unflagged byte not tinted");
}

#[test]
fn mem_viewer_scrollbar_model_round_trips() {
    let mut v = MemoryView::default();
    v.set_scroll(0.5);
    assert!(
        (i32::from(v.mem_base) - 0x8000).abs() <= 0x10,
        "frac 0.5 -> mid of 64 KiB"
    );
    assert_eq!(v.mem_base & 0x0F, 0, "row-aligned");
    let (frac, vis) = v.scroll_frac(30);
    assert!((frac - 0.5).abs() < 0.01, "reported frac tracks the base");
    assert!(vis > 0.0 && vis < 1.0, "thumb smaller than the whole space");
}

#[test]
fn memory_window_draws_a_scrollbar_on_the_right_edge() {
    let gb = machine();
    let st = WinState::Memory(MemoryView::default());
    let (w, h) = (430usize, 360usize);
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(
            ToolWindow::MemoryViewer,
            &gb,
            &mut c,
            &Theme::BGB,
            &st,
            &Breakpoints::default(),
        );
    }
    // The track spans the dump body (window minus the one-line status bar); every
    // row there is the dim track or the bright thumb (never the 0 background).
    let body_h = h - crate::ui::text::line_height() as usize;
    let tx = w - crate::ui::widgets::SCROLLBAR_W as usize; // first track column
    let track_px: Vec<u32> = (0..body_h).map(|y| buf[y * w + tx]).collect();
    assert!(
        track_px
            .iter()
            .all(|&p| p == Theme::BGB.border || p == Theme::BGB.hilight),
        "right-edge strip is the scrollbar track/thumb"
    );
    assert!(track_px.contains(&Theme::BGB.hilight), "a thumb is drawn");
}

#[test]
fn mem_bank_label_names_the_live_banked_region() {
    let gb = machine(); // DMG, ROM-only (no MBC, no external RAM)
    assert_eq!(mem_bank_label(&gb, 0x0100), None, "fixed ROM bank 0");
    assert_eq!(
        mem_bank_label(&gb, 0x4000).as_deref(),
        Some("ROM01"),
        "None-mapper high bank"
    );
    assert_eq!(mem_bank_label(&gb, 0x8000).as_deref(), Some("VRM0"));
    assert_eq!(mem_bank_label(&gb, 0xA000), None, "no RAM chip");
    assert_eq!(mem_bank_label(&gb, 0xC000).as_deref(), Some("WRM0"));
    assert_eq!(
        mem_bank_label(&gb, 0xD000).as_deref(),
        Some("WRM1"),
        "DMG WRAM bank 1"
    );
    assert_eq!(mem_bank_label(&gb, 0xFF80), None, "HRAM unbanked");
}

#[test]
fn mem_bank_label_follows_cgb_wram_and_mbc_rom_banks() {
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x143] = 0x80; // CGB
    rom[0x147] = 0x19; // MBC5
    rom[0x148] = 0x03; // 8 banks
    let mut gb = GameBoy::new(Model::Cgb, rom).unwrap();
    gb.debug_write(0x2000, 5); // MBC5 ROMB0 = 5
    assert_eq!(mem_bank_label(&gb, 0x4000).as_deref(), Some("ROM05"));
    gb.debug_write(0xFF70, 3); // SVBK = 3
    assert_eq!(mem_bank_label(&gb, 0xD000).as_deref(), Some("WRM3"));
    assert_eq!(mem_bank_label(&gb, 0x8000).as_deref(), Some("VRM0"));
}

#[test]
fn effective_bank_folds_into_the_region_and_survives_count_zero() {
    assert_eq!(effective_bank(5, 4), 1, "5 % 4");
    assert_eq!(effective_bank(3, 4), 3);
    assert_eq!(effective_bank(0, 1), 0);
    assert_eq!(
        effective_bank(9, 0),
        0,
        "count 0 (absent RAM) never divides by zero"
    );
}

#[test]
fn stepped_bank_starts_from_live_and_refollows() {
    // Following live (None), stepping starts from the live bank.
    assert_eq!(stepped_bank(None, 1, 2, 4), Some(3), "live 2 + 1 → pin 3");
    assert_eq!(stepped_bank(None, -1, 2, 4), Some(1), "live 2 - 1 → pin 1");
    // From a pinned bank, wrap within the count.
    assert_eq!(stepped_bank(Some(3), 1, 2, 4), Some(0), "3 + 1 wraps to 0");
    assert_eq!(stepped_bank(Some(0), -1, 2, 4), Some(3), "0 - 1 wraps to 3");
    // Landing on the live bank re-follows.
    assert_eq!(stepped_bank(Some(1), 1, 2, 4), None, "onto live 2 → follow");
    assert_eq!(stepped_bank(None, 0, 2, 4), None, "delta 0 stays on live");
    // Fixed/unbanked (count 0/1) always follows live.
    assert_eq!(stepped_bank(Some(5), 1, 0, 1), None);
    assert_eq!(
        stepped_bank(None, 3, 0, 0),
        None,
        "count 0 never divides by zero"
    );
}

#[test]
fn banked_read_follows_live_or_reads_the_pinned_bank() {
    // MBC5, 8 ROM banks, distinct byte at each bank's 0x4000.
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x147] = 0x19; // MBC5
    rom[0x148] = 0x03; // 8 banks
    for b in 0..8usize {
        rom[b * 0x4000] = 0xB0 | b as u8;
    }
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    gb.debug_write(0x2000, 5); // map ROM bank 5 live at 0x4000
    // Following live reads the mapped bank; pinning reads the chosen bank.
    assert_eq!(
        banked_read(&gb, None, 0x4000),
        0xB5,
        "None follows live bank 5"
    );
    assert_eq!(
        banked_read(&gb, Some(2), 0x4000),
        0xB2,
        "Some(2) reads bank 2"
    );
    // Live_bank + bank_chip_label agree with the live mapping.
    assert_eq!(live_bank(&gb, 0x4000), 5);
    assert_eq!(
        bank_chip_label(&gb, 0x4000, None),
        None,
        "no chip while following live"
    );
    assert_eq!(
        bank_chip_label(&gb, 0x4000, Some(2)).as_deref(),
        Some("ROM02")
    );
    // live_bank across the other regions: VRAM 0, WRAM 1 (DMG), absent SRAM 0,
    // unbanked 0 — the "follow live" start for every region.
    assert_eq!(live_bank(&gb, 0x8000), 0, "DMG VRAM bank 0");
    assert_eq!(live_bank(&gb, 0xD000), 1, "DMG WRAM bank 1");
    assert_eq!(live_bank(&gb, 0xA000), 0, "no RAM chip → 0");
    assert_eq!(live_bank(&gb, 0xFF80), 0, "unbanked HRAM → 0");
    // The chip/status name a non-ROM banked region too (WRAMX bank 0 aliases 1).
    assert_eq!(
        bank_chip_label(&gb, 0xD000, Some(0)).as_deref(),
        Some("WRM1")
    );
}

#[test]
fn banked_write_matches_banked_read_gated_when_following_raw_when_pinned() {
    // MBC1 + 8 KiB RAM left DISABLED (RAMG off — the normal state outside a save).
    let mut rom = vec![0u8; 4 * 0x4000];
    rom[0x147] = 0x03; // MBC1+RAM+BATTERY
    rom[0x149] = 0x02; // 8 KiB RAM (1 bank)
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    // Following live (None): the dump is RAMG-gated open-bus, and a write is the
    // same gated no-op the CPU sees — so what's shown stays what's written.
    assert_eq!(
        banked_read(&gb, None, 0xA000),
        0xFF,
        "disabled SRAM reads FF"
    );
    banked_write(&mut gb, None, 0xA000, 0x42);
    assert_eq!(
        banked_read(&gb, None, 0xA000),
        0xFF,
        "gated write is a no-op"
    );
    // Pinned to bank 0 (a `00:A000` Go-to) browses the raw chip past RAMG, and a
    // write is visible on the next pinned read — coherent with that dump.
    banked_write(&mut gb, Some(0), 0xA000, 0x42);
    assert_eq!(
        banked_read(&gb, Some(0), 0xA000),
        0x42,
        "raw pinned write is visible"
    );
}

#[test]
fn mem_status_line_follows_live_then_marks_a_pinned_bank() {
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x147] = 0x19; // MBC5
    rom[0x148] = 0x03; // 8 banks
    let mut gb = GameBoy::new(Model::Dmg, rom).unwrap();
    gb.debug_write(0x2000, 5); // live ROM bank 5
    let loc = "4000  ----";
    // Following live: the classic label, no marker.
    assert_eq!(mem_status_line(&gb, 0x4000, None, loc), "ROM05:4000  ----");
    // Pinned to the live bank: named, no marker (not diverged).
    assert_eq!(
        mem_status_line(&gb, 0x4000, Some(5), loc),
        "ROM05:4000  ----"
    );
    // Pinned off the live bank: the selected bank + a [live ..] marker.
    assert_eq!(
        mem_status_line(&gb, 0x4000, Some(2), loc),
        "ROM02:4000  ----  [live ROM05]"
    );
}

#[test]
fn sel_bank_label_names_an_explicit_browsed_bank() {
    // MBC5 + 32 KiB RAM so SRAM has banks to name.
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x143] = 0x80; // CGB
    rom[0x147] = 0x1A; // MBC5+RAM
    rom[0x148] = 0x03; // 8 ROM banks
    rom[0x149] = 0x03; // 32 KiB RAM
    let gb = GameBoy::new(Model::Cgb, rom).unwrap();
    // The selected bank is named regardless of the live mapping.
    assert_eq!(sel_bank_label(&gb, 0x4000, 5).as_deref(), Some("ROM05"));
    assert_eq!(sel_bank_label(&gb, 0x8000, 1).as_deref(), Some("VRM1"));
    assert_eq!(sel_bank_label(&gb, 0xA000, 3).as_deref(), Some("SRM03"));
    assert_eq!(sel_bank_label(&gb, 0xD000, 7).as_deref(), Some("WRM7"));
    // WRAMX bank 0 aliases page 1 (SVBK 0 → 1), so the label names the folded page.
    assert_eq!(sel_bank_label(&gb, 0xD000, 0).as_deref(), Some("WRM1"));
    assert_eq!(sel_bank_label(&gb, 0x0100, 0), None, "fixed ROM0 unbanked");
    // A cart with no RAM chip names no SRAM bank.
    assert_eq!(sel_bank_label(&machine(), 0xA000, 0), None, "no RAM chip");
}

#[test]
fn memory_window_status_bar_shows_nearest_symbol() {
    use crate::symbols::SymbolTable;
    use std::rc::Rc;
    let gb = machine();
    let theme = Theme::BGB;
    let st = WinState::Memory(MemoryView {
        mem_base: 0x4008,
        bank: None,
        symbols: Rc::new(SymbolTable::parse("00:4000 Reset")),
        goto: None,
        cursor: 0x4008,
        edit_hi: None,
    });
    let (w, h) = (430usize, 360usize);
    let mut buf = vec![0u32; w * h];
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(
            ToolWindow::MemoryViewer,
            &gb,
            &mut c,
            &theme,
            &st,
            &Breakpoints::default(),
        );
    }
    // The status bar text is rendered (some ink in the bottom line).
    let lh = crate::ui::text::line_height() as usize;
    let bar_row = (h - lh) * w;
    assert!(
        buf[bar_row..].contains(&theme.text),
        "status bar drawn in the bottom row"
    );
}

#[test]
fn debugger_bank_chip_draws_only_while_pinned() {
    // MBC5 so ROMX has banks to pin. The chip is the pane's only source of the
    // accent colour, so counting accent pixels within the memory pane rect proves
    // the chip is drawn when pinned and absent while following the live bank.
    let mut rom = vec![0u8; 8 * 0x4000];
    rom[0x147] = 0x19; // MBC5
    rom[0x148] = 0x03; // 8 banks
    let gb = GameBoy::new(Model::Dmg, rom).unwrap();
    let (w, h) = (640usize, 480usize);
    let mem = debugger::DebuggerLayout::for_size(w as i32, h as i32).memory;
    let chip_pixels = |bank: Option<u16>| -> usize {
        let st = debugger::DebuggerState {
            mem_base: 0x4000,
            mem_bank: bank,
            ..Default::default()
        };
        let mut buf = vec![0u32; w * h];
        {
            let mut c = Canvas::new(&mut buf, w, h);
            render(
                ToolWindow::Debugger,
                &gb,
                &mut c,
                &Theme::BGB,
                &WinState::Debugger(Box::new(st)),
                &Breakpoints::default(),
            );
        }
        (mem.y..mem.bottom())
            .flat_map(|y| (mem.x..mem.right()).map(move |x| (y, x)))
            .filter(|&(y, x)| buf[y as usize * w + x as usize] == Theme::BGB.current)
            .count()
    };
    assert_eq!(
        chip_pixels(None),
        0,
        "no chip while following the live bank"
    );
    assert!(chip_pixels(Some(3)) > 0, "a chip is drawn when pinned");
}

#[test]
fn render_is_side_effect_free_on_the_machine() {
    // Rendering must not advance or mutate emulation (it takes &GameBoy).
    let gb = machine();
    let before = (gb.cycles(), gb.frame_count(), gb.cpu_regs().pc);
    let (w, h) = (320usize, 240usize);
    let mut buf = vec![0u32; w * h];
    let mut c = Canvas::new(&mut buf, w, h);
    render(
        ToolWindow::Debugger,
        &gb,
        &mut c,
        &Theme::BGB,
        &WinState::Stateless,
        &Breakpoints::default(),
    );
    assert_eq!((gb.cycles(), gb.frame_count(), gb.cpu_regs().pc), before);
}
