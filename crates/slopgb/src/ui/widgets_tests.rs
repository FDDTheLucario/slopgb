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
fn checkbox_rect_equals_the_drawn_hit_rect() {
    let pure = checkbox_rect(1, 0, "grid");
    let (w, h) = (60, GLYPH_H);
    let mut buf = canvas(w, h);
    let mut c = Canvas::new(&mut buf, w, h);
    let drawn = checkbox(&mut c, 1, 0, true, "grid", &T);
    assert_eq!(pure, drawn);
}

#[test]
fn radio_rects_equal_the_drawn_hit_rects() {
    let opts = ["Auto", "9800", "9C00"];
    let pure = radio_rects(1, 0, &opts);
    let (w, h) = (140, GLYPH_H);
    let mut buf = canvas(w, h);
    let mut c = Canvas::new(&mut buf, w, h);
    let drawn = radio_group(&mut c, 1, 0, &opts, 1, &T);
    assert_eq!(pure, drawn);
}

#[test]
fn radio_group_marks_only_the_selected_option() {
    let (w, h) = (140, GLYPH_H);
    let mut buf = canvas(w, h);
    let rects;
    {
        let mut c = Canvas::new(&mut buf, w, h);
        rects = radio_group(&mut c, 1, 0, &["Auto", "9800", "9C00"], 1, &T);
    }
    assert_eq!(rects.len(), 3);
    assert!(
        rects[0].x < rects[1].x && rects[1].x < rects[2].x,
        "left-to-right"
    );
    // The dot's filled interior centre: rect.x+4, y+4 (dot = GLYPH_H-4).
    let centre = |r: &Rect| ((r.x as usize) + 4) + 4 * w;
    assert_eq!(buf[centre(&rects[1])], T.text, "selected dot filled");
    assert_eq!(buf[centre(&rects[0])], T.bg, "unselected dot empty");
    assert_eq!(buf[centre(&rects[2])], T.bg);
}

#[test]
fn tab_rects_match_what_tab_strip_returns() {
    // The pure geometry a window hit-tests against must equal the rects
    // tab_strip draws and returns, so a click maps to the right tab.
    let labels = ["BG map", "Tiles", "OAM", "Palettes"];
    let pure = tab_rects(5, 3, &labels);
    let (w, h) = (220, GLYPH_H + 4);
    let mut buf = canvas(w, h);
    let mut c = Canvas::new(&mut buf, w, h);
    let drawn = tab_strip(&mut c, 5, 3, &labels, 1, &T);
    assert_eq!(pure, drawn, "pure rects equal the drawn rects");
    // Each width is measure(label)+2*PAD(=4); tabs advance left-to-right by w+2.
    assert_eq!(pure[0].w, measure("BG map") + 8);
    assert_eq!(pure[1].x, pure[0].right() + 2);
}

#[test]
fn tab_strip_outlines_the_active_tab() {
    let (w, h) = (180, GLYPH_H + 4);
    let mut buf = canvas(w, h);
    let rects;
    {
        let mut c = Canvas::new(&mut buf, w, h);
        rects = tab_strip(&mut c, 0, 0, &["BG map", "Tiles", "OAM", "Palettes"], 2, &T);
    }
    assert_eq!(rects.len(), 4);
    // Active tab (index 2 = OAM) has an outline: its top-left pixel is text.
    let active = rects[2];
    assert_eq!(buf[(active.y as usize) * w + active.x as usize], T.text);
    // An inactive tab's top-left was not outlined (stays canvas background).
    let inactive = rects[0];
    assert_eq!(
        buf[(inactive.y as usize) * w + inactive.x as usize],
        0x00AA_AAAA
    );
}

#[test]
fn scroll_list_windows_rows_and_highlights() {
    use crate::ui::text::line_height;
    let lh = line_height() as usize;
    let (w, h) = (80, lh * 3); // viewport shows exactly 3 rows
    let mut buf = canvas(w, h);
    let rows = ["r0", "r1", "r2", "r3", "r4", "r5"];
    let drawn;
    {
        let mut c = Canvas::new(&mut buf, w, h);
        // Offset 2 -> rows 2,3,4 visible; highlight row 3 (the middle one).
        drawn = scroll_list(
            &mut c,
            Rect::new(0, 0, w as i32, h as i32),
            &rows,
            2,
            Some(3),
            &T,
        );
    }
    assert_eq!(drawn, 3, "viewport holds 3 rows");
    // Row index 1 of the viewport (global row 3) has the blue highlight bar.
    let bar_y = lh + 1; // inside the 2nd visible row
    assert_eq!(buf[bar_y * w], T.current, "highlighted row bar");
    // The first visible row (global row 2) is not highlighted (bg untouched
    // where there's no glyph ink): far-right pixel of that row.
    assert_eq!(buf[2 * w - 1], 0x00AA_AAAA, "non-highlight row bg");
}

#[test]
fn scroll_list_past_the_end_draws_only_remaining_rows() {
    use crate::ui::text::line_height;
    let lh = line_height() as usize;
    let (w, h) = (40, lh * 4);
    let mut buf = canvas(w, h);
    let rows = ["a", "b", "c"];
    let mut c = Canvas::new(&mut buf, w, h);
    // Offset 2 with a 4-row viewport: only "c" remains.
    let drawn = scroll_list(
        &mut c,
        Rect::new(0, 0, w as i32, h as i32),
        &rows,
        2,
        None,
        &T,
    );
    assert_eq!(drawn, 1);
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

#[test]
fn vscroll_track_is_the_panes_right_edge_strip() {
    let pane = Rect::new(10, 20, 100, 200);
    let t = vscroll_track(pane);
    assert_eq!(t.x, pane.right() - SCROLLBAR_W);
    assert_eq!((t.y, t.w, t.h), (20, SCROLLBAR_W, 200));
    // scroll_content is the pane minus that strip.
    assert_eq!(scroll_content(pane).w, 100 - SCROLLBAR_W);
}

#[test]
fn vscroll_frac_maps_cursor_top_to_0_bottom_to_1_middle_to_half() {
    let track = Rect::new(0, 0, SCROLLBAR_W, 100);
    let vis = 0.2; // 20%-tall thumb
    assert_eq!(
        vscroll_frac(track, -50, vis),
        0.0,
        "above the track clamps to 0"
    );
    assert_eq!(vscroll_frac(track, 999, vis), 1.0, "below clamps to 1");
    let mid = vscroll_frac(track, 50, vis);
    assert!(
        (mid - 0.5).abs() < 0.05,
        "cursor at track center ~ frac 0.5, got {mid}"
    );
}

#[test]
fn vscrollbar_draws_a_thumb_over_a_dim_track() {
    let (w, h) = (40usize, 100usize);
    let mut buf = canvas(w, h);
    let pane = Rect::new(0, 0, w as i32, h as i32);
    {
        let mut c = Canvas::new(&mut buf, w, h);
        vscrollbar(&mut c, pane, 0.0, 0.3, &T); // thumb at top
    }
    let tx = (pane.right() - SCROLLBAR_W) as usize; // a track column
    // Top of the track is the thumb (hilight); far bottom is the dim track.
    assert_eq!(buf[tx], T.hilight, "thumb at the top");
    assert_eq!(buf[(h - 1) * w + tx], T.border, "dim track below the thumb");
    // Left of the track is untouched content area.
    assert_eq!(buf[0], 0x00AA_AAAA, "content area untouched");
}
