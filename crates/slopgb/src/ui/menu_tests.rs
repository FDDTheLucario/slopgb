use super::*;

const T: Theme = Theme::BGB;
const BG_FILL: u32 = 0x00AA_AAAA; // canvas init, distinct from any theme colour

fn canvas(w: usize, h: usize) -> Vec<u32> {
    vec![BG_FILL; w * h]
}

/// Count pixels of `color` inside `r` of a `w`-wide buffer.
fn count(buf: &[u32], w: usize, r: Rect, color: u32) -> usize {
    let mut n = 0;
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            if buf[y as usize * w + x as usize] == color {
                n += 1;
            }
        }
    }
    n
}

fn sample_menu() -> Vec<MenuItem> {
    vec![
        MenuItem::new("Go to…").shortcut("Ctrl+G"),
        MenuItem::new("Copy code"),
        MenuItem::new("force code view").disabled(),
    ]
}

#[test]
fn menu_rects_are_three_stacked_non_overlapping_rows() {
    let items = sample_menu();
    let rects = menu_rects((10, 20), &items);
    assert_eq!(rects.len(), 3);
    // Same width, stacked top-to-bottom with no gaps or overlaps.
    for w in rects.windows(2) {
        assert_eq!(w[0].w, w[1].w, "all rows equal width");
        assert_eq!(w[0].bottom(), w[1].y, "rows abut without overlap");
    }
    // All rows fit inside the menu bounds.
    let bounds = menu_bounds((10, 20), &items);
    for r in &rects {
        assert_eq!(r.x, bounds.x);
        assert!(r.right() <= bounds.right());
        assert!(r.bottom() <= bounds.bottom());
    }
}

#[test]
fn render_draws_label_and_right_aligned_shortcut() {
    let items = sample_menu();
    let (w, h) = (
        menu_width(&items) as usize + 4,
        menu_height(&items) as usize + 4,
    );
    let mut buf = canvas(w, h);
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, (0, 0), &items, None, &T);
    }
    let row0 = menu_rects((0, 0), &items)[0];
    // Label ink (theme.text) on the left half; shortcut ink on the right half.
    let half = Rect::new(row0.x, row0.y, row0.w / 2, row0.h);
    let right = Rect::new(row0.x + row0.w / 2, row0.y, row0.w / 2, row0.h);
    assert!(count(&buf, w, half, T.text) > 0, "label drawn on the left");
    assert!(
        count(&buf, w, right, T.text) > 0,
        "shortcut drawn on the right"
    );
}

#[test]
fn disabled_item_draws_greyed_not_black() {
    let items = sample_menu();
    let (w, h) = (
        menu_width(&items) as usize + 4,
        menu_height(&items) as usize + 4,
    );
    let mut buf = canvas(w, h);
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, (0, 0), &items, None, &T);
    }
    let disabled = menu_rects((0, 0), &items)[2]; // "force code view"
    assert!(
        count(&buf, w, disabled, T.hilight) > 0,
        "disabled label drawn in the grey colour"
    );
    assert_eq!(
        count(&buf, w, disabled, T.text),
        0,
        "disabled row has no black text"
    );
}

#[test]
fn item_at_resolves_only_enabled_non_separator_rows() {
    let items = vec![
        MenuItem::new("Code go here"),
        MenuItem::separator(),
        MenuItem::new("Data go here"),
        MenuItem::new("locked").disabled(),
    ];
    let origin = (5, 5);
    let rects = menu_rects(origin, &items);
    let mid = |r: &Rect| (r.x + r.w / 2, r.y + r.h / 2);
    let (x0, y0) = mid(&rects[0]);
    assert_eq!(item_at(origin, &items, x0, y0), Some(0), "enabled row 0");
    let (xs, ys) = mid(&rects[1]);
    assert_eq!(item_at(origin, &items, xs, ys), None, "separator is dead");
    let (x2, y2) = mid(&rects[2]);
    assert_eq!(item_at(origin, &items, x2, y2), Some(2), "enabled row 2");
    let (x3, y3) = mid(&rects[3]);
    assert_eq!(
        item_at(origin, &items, x3, y3),
        None,
        "disabled row is dead"
    );
    // A point outside every row is None (click-away).
    assert_eq!(item_at(origin, &items, x0, rects[0].y - 50), None);
}

#[test]
fn render_highlights_the_hovered_row() {
    let items = sample_menu();
    let (w, h) = (
        menu_width(&items) as usize + 4,
        menu_height(&items) as usize + 4,
    );
    let mut buf = canvas(w, h);
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, (0, 0), &items, Some(1), &T);
    }
    let rects = menu_rects((0, 0), &items);
    // Hovered row 1 is flooded with the highlight colour.
    assert!(
        count(&buf, w, rects[1], T.current) > rects[1].w as usize,
        "hovered row filled with the highlight bar"
    );
    // A non-hovered row is not.
    assert_eq!(
        count(&buf, w, rects[0], T.current),
        0,
        "row 0 not highlighted"
    );
}

#[test]
fn hovering_a_disabled_row_does_not_highlight_it() {
    let items = sample_menu();
    let (w, h) = (
        menu_width(&items) as usize + 4,
        menu_height(&items) as usize + 4,
    );
    let mut buf = canvas(w, h);
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, (0, 0), &items, Some(2), &T); // row 2 is disabled
    }
    let disabled = menu_rects((0, 0), &items)[2];
    assert_eq!(
        count(&buf, w, disabled, T.current),
        0,
        "disabled row never gets the highlight bar"
    );
}

#[test]
fn checked_item_draws_a_mark_unchecked_does_not() {
    let on = vec![MenuItem::new("Enable sound").checked(true)];
    let off = vec![MenuItem::new("Enable sound").checked(false)];
    let (w, h) = (menu_width(&on) as usize, menu_height(&on) as usize);
    let mut a = canvas(w, h);
    let mut b = canvas(w, h);
    {
        let mut c = Canvas::new(&mut a, w, h);
        render(&mut c, (0, 0), &on, None, &T);
    }
    {
        let mut c = Canvas::new(&mut b, w, h);
        render(&mut c, (0, 0), &off, None, &T);
    }
    // The mark column (left of the label) carries ink only when checked.
    let mark = Rect::new(BORDER, 0, MARK_W, h as i32);
    assert!(count(&a, w, mark, T.text) > 0, "check-mark drawn when on");
    assert_eq!(count(&b, w, mark, T.text), 0, "no mark when off");
}

#[test]
fn submenu_arrow_widens_the_menu_and_draws_on_the_right() {
    let items = vec![MenuItem::new("State").submenu()];
    let (w, h) = (menu_width(&items) as usize, menu_height(&items) as usize);
    let mut buf = canvas(w, h);
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, (0, 0), &items, None, &T);
    }
    // Arrow ink lands in the right column, past the label.
    let right = Rect::new(
        w as i32 - PAD_R - ARROW_W - 1,
        0,
        PAD_R + ARROW_W + 1,
        h as i32,
    );
    assert!(
        count(&buf, w, right, T.text) > 0,
        "submenu arrow drawn right"
    );
}

#[test]
fn separator_is_thin_and_draws_a_divider_line() {
    let items = vec![
        MenuItem::new("a"),
        MenuItem::separator(),
        MenuItem::new("b"),
    ];
    let rects = menu_rects((0, 0), &items);
    assert!(
        rects[1].h < rects[0].h,
        "separator row is thinner than a text row"
    );
    let (w, h) = (menu_width(&items) as usize, menu_height(&items) as usize);
    let mut buf = canvas(w, h);
    {
        let mut c = Canvas::new(&mut buf, w, h);
        render(&mut c, (0, 0), &items, None, &T);
    }
    assert!(
        count(&buf, w, rects[1], T.border) > 0,
        "separator draws a divider line"
    );
}
