use super::*;

const T: Theme = Theme::BGB;

fn canvas(w: usize, h: usize) -> Vec<u32> {
    vec![0x00AA_AAAA; w * h] // distinct from any theme colour
}

#[test]
fn checkbox_check_state_changes_the_box_interior() {
    let (w, h) = (60, GLYPH_H);
    let mut on = canvas(w, h);
    let mut off = canvas(w, h);
    let hit;
    {
        let mut c = Canvas::new(&mut on, w, h);
        hit = checkbox(&mut c, 1, 0, true, "grid", &T);
    }
    {
        let mut c = Canvas::new(&mut off, w, h);
        checkbox(&mut c, 1, 0, false, "grid", &T);
    }
    // The box interior (around its centre) is filled black only when checked.
    let box_sz = GLYPH_H - 2;
    let cx = 1 + box_sz / 2;
    let cy = box_sz / 2;
    assert_eq!(on[cy * w + cx], T.text, "checked: interior filled");
    assert_eq!(off[cy * w + cx], T.bg, "unchecked: interior bg");
    // Hit rect spans the box plus the label.
    assert!(hit.w > box_sz as i32, "hit rect includes the label");
    assert!(hit.contains(1, 0));
}

#[test]
fn button_draws_border_and_pressed_inverts() {
    let (w, h) = (40, 16);
    let mut up = canvas(w, h);
    let mut down = canvas(w, h);
    let r = Rect::new(2, 2, 30, 12);
    {
        let mut c = Canvas::new(&mut up, w, h);
        let hit = button(&mut c, r, "OK", false, &T);
        assert_eq!(hit, r);
    }
    {
        let mut c = Canvas::new(&mut down, w, h);
        button(&mut c, r, "OK", true, &T);
    }
    // Top-left border pixel is the text colour in both states.
    assert_eq!(up[2 * w + 2], T.text);
    // Fill differs: a non-border interior pixel is bg when up, text when down.
    let ix = (r.y as usize + 1) * w + (r.x as usize + 1);
    assert_eq!(up[ix], T.bg, "unpressed interior is bg");
    assert_eq!(down[ix], T.text, "pressed interior inverts");
}

#[test]
fn swatch_fills_colour_with_a_border() {
    let (w, h) = (12, 12);
    let mut buf = canvas(w, h);
    let color = 0x0012_3456;
    {
        let mut c = Canvas::new(&mut buf, w, h);
        swatch(&mut c, Rect::new(1, 1, 8, 8), color, &T);
    }
    // Centre is the colour; the border ring is theme.text.
    assert_eq!(buf[4 * w + 4], color, "interior filled with colour");
    assert_eq!(buf[w + 1], T.text, "top-left border");
    assert_eq!(buf[0], 0x00AA_AAAA, "outside untouched");
}
