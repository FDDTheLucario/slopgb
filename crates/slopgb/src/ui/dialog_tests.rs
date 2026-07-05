use super::*;

const T: Theme = Theme::BGB;
const BG_FILL: u32 = 0x00AA_AAAA;

/// Feed a string of characters through `on_key`, returning the last result.
fn type_str(dlg: &mut InputDialog, s: &str) -> DialogResult {
    let mut r = DialogResult::Continue;
    for ch in s.chars() {
        r = dlg.on_key(DialogKey::Char(ch));
    }
    r
}

#[test]
fn hex_field_collects_typed_digits_then_accepts_on_enter() {
    let mut dlg = InputDialog::new("Go to…", true);
    type_str(&mut dlg, "1A2B");
    assert_eq!(dlg.buffer, "1A2B");
    assert_eq!(
        dlg.on_key(DialogKey::Enter),
        DialogResult::Accept("1A2B".into())
    );
}

#[test]
fn backspace_removes_the_last_character() {
    let mut dlg = InputDialog::new("Go to…", true);
    type_str(&mut dlg, "1A2B");
    dlg.on_key(DialogKey::Backspace);
    assert_eq!(dlg.buffer, "1A2");
}

#[test]
fn escape_cancels() {
    let mut dlg = InputDialog::new("Go to…", true);
    type_str(&mut dlg, "12");
    assert_eq!(dlg.on_key(DialogKey::Escape), DialogResult::Cancel);
}

#[test]
fn hex_field_uppercases_and_rejects_non_hex() {
    let mut dlg = InputDialog::new("addr", true);
    type_str(&mut dlg, "0xfz9g"); // 'x','z','g' rejected; 'f','9' kept, uppercased
    assert_eq!(dlg.buffer, "0F9");
}

#[test]
fn hex_field_caps_at_four_digits() {
    let mut dlg = InputDialog::new("addr", true);
    type_str(&mut dlg, "DEADBEEF");
    assert_eq!(
        dlg.buffer, "DEAD",
        "a u16 address is at most four hex digits"
    );
}

#[test]
fn text_field_accepts_arbitrary_printable_input() {
    let mut dlg = InputDialog::new("Set break/condition…", false);
    type_str(&mut dlg, "a==FF44");
    assert_eq!(
        dlg.buffer, "a==FF44",
        "non-hex field keeps the literal text"
    );
    // Enter trims surrounding whitespace.
    let mut dlg2 = InputDialog::new("expr", false);
    type_str(&mut dlg2, "  ld a  ");
    assert_eq!(
        dlg2.on_key(DialogKey::Enter),
        DialogResult::Accept("ld a".into())
    );
}

#[test]
fn text_field_admits_long_paths_but_still_caps() {
    // A real path easily exceeds the old 40-char cap.
    let mut dlg = InputDialog::new("Load ROM (path)", false);
    let path = "/home/user/roms/game-boy-test-roms-v7.0/dmg-acid2/dmg-acid2.gb";
    type_str(&mut dlg, path);
    assert_eq!(dlg.buffer, path, "long paths are no longer truncated");
    // Still bounded so the buffer can't grow without limit.
    let mut big = InputDialog::new("p", false);
    type_str(&mut big, &"x".repeat(2000));
    assert_eq!(big.buffer.chars().count(), 1024, "text cap is 1024");
}

#[test]
fn field_scroll_keeps_caret_visible() {
    // Fits: nothing scrolled off.
    assert_eq!(field_scroll(5, 31), 0);
    // Exactly fills the visible width: one char scrolled off to leave a caret cell.
    assert_eq!(field_scroll(31, 31), 1);
    // Overflow: show the trailing (visible-1) chars; caret column stays < visible.
    assert_eq!(field_scroll(50, 31), 20);
    assert!(50 - field_scroll(50, 31) < 31, "caret within field");
    // Degenerate widths never panic.
    assert_eq!(field_scroll(10, 1), 10);
    assert_eq!(field_scroll(10, 0), 10);
    assert_eq!(field_scroll(0, 31), 0);
}

#[test]
fn render_scrolls_long_buffer_inside_the_field_no_overflow() {
    let area = Rect::new(0, 0, 400, 300);
    let (w, h) = (400usize, 300usize);
    let mut buf = vec![BG_FILL; w * h];
    let mut dlg = InputDialog::new("Load ROM (path)", false);
    type_str(&mut dlg, &"W".repeat(60)); // far wider than the field
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, area, &dlg, &T);
    }
    let l = layout(area);
    // No field ink lands at or past the field's right border (clip holds).
    let spill = (l.field.y..l.field.bottom())
        .flat_map(|y| (l.field.right()..l.boxr.right() + 8).map(move |x| (x, y)))
        .filter(|&(x, y)| buf[y as usize * w + x as usize] == T.text)
        .count();
    assert_eq!(spill, 0, "text must not spill past the field/box");
    // The scrolled window fits the field width in pixels.
    let visible = ((l.field.w - 4) / GLYPH_W as i32).max(0) as usize;
    let skip = field_scroll(dlg.buffer.chars().count(), visible);
    let shown = dlg.buffer.chars().count() - skip;
    assert!(
        shown as i32 * GLYPH_W as i32 <= l.field.w,
        "drawn substring fits inside the field"
    );
}

#[test]
fn with_initial_prefills_the_buffer() {
    let dlg = InputDialog::new("edit register", true).with_initial("1234");
    assert_eq!(dlg.buffer, "1234");
}

#[test]
fn clicking_ok_accepts_and_cancel_dismisses() {
    let mut dlg = InputDialog::new("Go to…", true);
    type_str(&mut dlg, "0150");
    let area = Rect::new(0, 0, 400, 300);
    let l = layout(area);
    let mid = |r: Rect| (r.x + r.w / 2, r.y + r.h / 2);
    let (ox, oy) = mid(l.ok);
    assert_eq!(
        click(&dlg, area, ox, oy),
        DialogResult::Accept("0150".into())
    );
    let (cx, cy) = mid(l.cancel);
    assert_eq!(click(&dlg, area, cx, cy), DialogResult::Cancel);
    // A click on neither button keeps the dialog open.
    assert_eq!(
        click(&dlg, area, l.boxr.x + 1, l.boxr.y + 1),
        DialogResult::Continue
    );
}

#[test]
fn render_draws_the_title_and_buffer_centred_in_the_window() {
    let area = Rect::new(0, 0, 400, 300);
    let (w, h) = (400usize, 300usize);
    let mut buf = vec![BG_FILL; w * h];
    let mut dlg = InputDialog::new("Go to…", true);
    type_str(&mut dlg, "0150");
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, area, &dlg, &T);
    }
    let l = layout(area);
    // The modal box is filled (its interior is the theme bg, not the canvas fill).
    let inside = (l.boxr.y as usize + 2) * w + (l.boxr.x as usize + 2);
    assert_eq!(buf[inside], T.bg, "modal box painted over the window");
    // The field carries buffer/caret ink.
    let count_text: usize = (l.field.y..l.field.bottom())
        .flat_map(|y| (l.field.x..l.field.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| buf[y as usize * w + x as usize] == T.text)
        .count();
    assert!(count_text > 0, "buffer text + caret drawn in the field");
}
