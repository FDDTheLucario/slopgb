use super::*;
use crate::cheat::CheatList;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::dialog::DialogKey;

fn typed(d: &mut CheatDialog, text: &str) -> CheatEdit {
    for ch in text.chars() {
        assert!(d.input_key(DialogKey::Char(ch)).is_none());
    }
    d.input_key(DialogKey::Enter).expect("accept")
}

#[test]
fn add_entry_parses_comment_and_code() {
    let mut d = CheatDialog::default();
    d.open_add();
    assert!(d.input_open());
    let edit = typed(&mut d, "inf lives = 01FF0AC1");
    assert_eq!(edit.comment, "inf lives");
    assert_eq!(edit.code, "01FF0AC1");
    assert_eq!(edit.editing, None);
    assert!(!d.input_open(), "entry closed on accept");
}

#[test]
fn code_without_equals_is_all_code() {
    let mut d = CheatDialog::default();
    d.open_add();
    let edit = typed(&mut d, "01FF0AC1");
    assert_eq!(edit.comment, "");
    assert_eq!(edit.code, "01FF0AC1");
}

#[test]
fn edit_prefills_and_reports_the_row() {
    let mut d = CheatDialog::default();
    d.open_edit(2, "hp", "0163 C1C1");
    let edit = d.input_key(DialogKey::Enter).unwrap();
    assert_eq!(edit.comment, "hp");
    assert_eq!(edit.code, "0163 C1C1");
    assert_eq!(edit.editing, Some(2));
}

#[test]
fn escape_closes_the_entry_without_an_edit() {
    let mut d = CheatDialog::default();
    d.open_add();
    assert!(d.input_key(DialogKey::Escape).is_none());
    assert!(!d.input_open());
}

#[test]
fn hit_resolves_buttons_and_rows() {
    let mut cheats = CheatList::default();
    cheats.add("a", "01FF0AC1");
    cheats.add("b", "0142 20C0");
    let area = Rect::new(0, 0, 640, 480);
    let (mut saw_button, mut saw_row) = (false, false);
    for py in 0..480 {
        for px in (0..640).step_by(5) {
            match hit(area, &cheats, px, py) {
                Some(CheatHit::Button(_)) => saw_button = true,
                Some(CheatHit::Row(_)) => saw_row = true,
                None => {}
            }
        }
    }
    assert!(saw_button, "a point resolves to a button");
    assert!(saw_row, "a point resolves to a cheat row");
}

#[test]
fn render_draws_the_panel_without_panic() {
    let mut cheats = CheatList::default();
    cheats.add("infinite lives", "01FF0AC1");
    let d = CheatDialog::default();
    let (w, h) = (640usize, 480usize);
    let mut buf = vec![0u32; w * h];
    let mut c = Canvas::new(&mut buf, w, h);
    render(&mut c, &d, &cheats, &Theme::BGB);
    assert!(buf.iter().any(|&p| p != 0), "the dialog drew something");
}
