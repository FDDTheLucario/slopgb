use super::*;
use crate::cheat::CheatList;
use crate::ui::canvas::{Canvas, Rect};

fn typed(d: &mut CheatDialog, comment: &str, code: &str) -> CheatEdit {
    for ch in comment.chars() {
        d.type_char(ch);
    }
    d.switch_field();
    for ch in code.chars() {
        d.type_char(ch);
    }
    d.accept().expect("accept")
}

#[test]
fn add_entry_captures_comment_and_code() {
    let mut d = CheatDialog::default();
    d.open_add();
    assert!(d.editor_open());
    let edit = typed(&mut d, "inf lives", "01FF0AC1");
    assert_eq!(edit.comment, "inf lives");
    assert_eq!(edit.code, "01FF0AC1");
    assert_eq!(edit.editing, None);
    assert!(!d.editor_open(), "editor closed on accept");
}

#[test]
fn backspace_edits_the_focused_field() {
    let mut d = CheatDialog::default();
    d.open_add();
    for ch in "abcx".chars() {
        d.type_char(ch);
    }
    d.backspace(); // removes 'x' from Comment
    d.switch_field();
    for ch in "01FF0AC1".chars() {
        d.type_char(ch);
    }
    let edit = d.accept().unwrap();
    assert_eq!(edit.comment, "abc");
    assert_eq!(edit.code, "01FF0AC1");
}

#[test]
fn edit_prefills_and_reports_the_row() {
    let mut d = CheatDialog::default();
    d.open_edit(2, "hp", "0163C1C1");
    let edit = d.accept().unwrap();
    assert_eq!(edit.comment, "hp");
    assert_eq!(edit.code, "0163C1C1");
    assert_eq!(edit.editing, Some(2));
}

#[test]
fn cancel_closes_the_editor_without_an_edit() {
    let mut d = CheatDialog::default();
    d.open_add();
    d.cancel_editor();
    assert!(!d.editor_open());
}

#[test]
fn advanced_toggle_defaults_off() {
    let d = CheatDialog::default();
    assert!(!d.advanced);
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
fn render_draws_the_panel_and_editor_without_panic() {
    let mut cheats = CheatList::default();
    cheats.add("infinite lives", "01FF0AC1");
    let mut d = CheatDialog { advanced: true, ..CheatDialog::default() };
    d.open_add();
    let (w, h) = (640usize, 480usize);
    let mut buf = vec![0u32; w * h];
    let mut c = Canvas::new(&mut buf, w, h);
    render(&mut c, &d, &cheats, &Theme::BGB);
    assert!(buf.iter().any(|&p| p != 0), "the dialog drew something");
}
