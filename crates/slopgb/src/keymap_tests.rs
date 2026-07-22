//! Tests for [`KeyBindings`] (JP1) and [`KeyConfigWizard`] (JP3).

use super::*;

#[test]
fn defaults_reproduce_the_historical_bindings() {
    let b = KeyBindings::default();
    assert_eq!(b.key_for(Button::A), Some(KeyCode::KeyZ));
    assert_eq!(b.key_for(Button::B), Some(KeyCode::KeyX));
    assert_eq!(b.key_for(Button::Start), Some(KeyCode::Enter));
    assert_eq!(b.key_for(Button::Select), Some(KeyCode::ShiftRight));
    assert_eq!(b.key_for(Button::Up), Some(KeyCode::ArrowUp));
    assert_eq!(b.key_for(Button::Down), Some(KeyCode::ArrowDown));
    assert_eq!(b.key_for(Button::Left), Some(KeyCode::ArrowLeft));
    assert_eq!(b.key_for(Button::Right), Some(KeyCode::ArrowRight));
}

#[test]
fn button_for_reverse_maps_and_misses_unbound() {
    let b = KeyBindings::default();
    assert_eq!(b.button_for(KeyCode::KeyZ), Some(Button::A));
    assert_eq!(b.button_for(KeyCode::ArrowRight), Some(Button::Right));
    assert_eq!(b.button_for(KeyCode::KeyQ), None);
}

#[test]
fn set_rebinds_and_frees_the_old_key() {
    let mut b = KeyBindings::default();
    b.set(Button::A, KeyCode::KeyQ);
    assert_eq!(b.key_for(Button::A), Some(KeyCode::KeyQ));
    // The old A key (Z) is now unbound.
    assert_eq!(b.button_for(KeyCode::KeyZ), None);
    assert_eq!(b.button_for(KeyCode::KeyQ), Some(Button::A));
}

#[test]
fn set_to_a_used_key_unbinds_the_other_button() {
    let mut b = KeyBindings::default();
    // Assign B's existing key (X) to A: B must lose it (one button per key).
    b.set(Button::A, KeyCode::KeyX);
    assert_eq!(b.button_for(KeyCode::KeyX), Some(Button::A));
    assert_eq!(b.key_for(Button::B), None);
}

#[test]
fn clear_unbinds_a_button() {
    let mut b = KeyBindings::default();
    b.clear(Button::Start);
    assert_eq!(b.key_for(Button::Start), None);
    assert_eq!(b.button_for(KeyCode::Enter), None);
}

#[test]
fn key_name_labels_common_keys() {
    assert_eq!(key_name(KeyCode::ArrowUp), "Up");
    assert_eq!(key_name(KeyCode::KeyZ), "Z");
    assert_eq!(key_name(KeyCode::KeyS), "S");
    assert_eq!(key_name(KeyCode::Enter), "Enter");
    assert_eq!(key_name(KeyCode::ShiftRight), "RShift");
    assert_eq!(key_name(KeyCode::Digit3), "3");
    assert_eq!(key_name(KeyCode::F13), "?");
}

// --- KeyConfigWizard (JP3) --------------------------------------------------

#[test]
fn wizard_walks_buttons_in_the_bgb_order() {
    let w = KeyConfigWizard::open(KeyBindings::default());
    assert_eq!(w.current_button(), Some(Button::Right), "starts at right");
    assert_eq!(w.prompt_name(), "right");
    // The order is exactly bgb's captured sequence.
    let mut w = KeyConfigWizard::open(KeyBindings::default());
    let order: Vec<_> = (0..8)
        .map(|_| {
            let b = w.current_button().unwrap();
            w.skip_keep();
            b
        })
        .collect();
    assert_eq!(order, WIZARD_ORDER.to_vec());
}

#[test]
fn bind_key_sets_the_current_button_and_advances() {
    let mut w = KeyConfigWizard::open(KeyBindings::default());
    w.bind_key(KeyCode::KeyW); // right := W
    assert_eq!(w.current_button(), Some(Button::Left), "advanced");
    // The working copy reflects the new binding when finished.
    for _ in 0..7 {
        w.skip_keep();
    }
    let done = w.finished().expect("ran to the end");
    assert_eq!(done.key_for(Button::Right), Some(KeyCode::KeyW));
}

#[test]
fn skip_keep_preserves_skip_clear_unbinds() {
    let mut w = KeyConfigWizard::open(KeyBindings::default());
    w.skip_keep(); // right kept (default ArrowRight)
    w.skip_clear(); // left unbound
    for _ in 0..6 {
        w.skip_keep();
    }
    let done = w.finished().unwrap();
    assert_eq!(done.key_for(Button::Right), Some(KeyCode::ArrowRight));
    assert_eq!(done.key_for(Button::Left), None);
}

#[test]
fn finished_is_none_until_every_step_is_past() {
    let mut w = KeyConfigWizard::open(KeyBindings::default());
    for _ in 0..7 {
        assert!(
            w.finished().is_none(),
            "not done mid-wizard (cancel = drop)"
        );
        w.skip_keep();
    }
    assert!(w.finished().is_none(), "still on the last step");
    w.skip_keep();
    assert!(w.finished().is_some(), "committed after the 8th advance");
}

#[test]
fn current_key_shows_the_live_mapping() {
    let w = KeyConfigWizard::open(KeyBindings::default());
    // First step is Right → default ArrowRight.
    assert_eq!(w.current_key(), Some(KeyCode::ArrowRight));
}

// --- wizard render + hit-test (JP4) -----------------------------------------

#[test]
fn wizard_buttons_hit_test_at_their_centres() {
    let w = KeyConfigWizard::open(KeyBindings::default());
    let bounds = Rect::new(0, 0, 480, 432);
    for (kind, r) in button_rects(bounds) {
        let hit = w.button_at(bounds, r.x + r.w / 2, r.y + r.h / 2);
        assert_eq!(hit, Some(kind));
    }
    // A click well outside the box hits nothing.
    assert_eq!(w.button_at(bounds, 2, 2), None);
}

#[test]
fn wizard_render_draws_box_text_and_red_current_button() {
    let mut buf = vec![0u32; 480 * 432];
    let mut c = Canvas::new(&mut buf, 480, 432);
    let w = KeyConfigWizard::open(KeyBindings::default());
    w.render(&mut c, &Theme::BGB);
    assert!(buf.contains(&Theme::BGB.text), "drew ink");
    assert!(
        buf.contains(&Theme::BGB.breakpoint),
        "the current button (right) is highlighted red"
    );
}

// --- SOCD filter (JP7) ------------------------------------------------------

#[test]
fn opposite_pairs_the_axes_and_ignores_actions() {
    assert_eq!(opposite(Button::Left), Some(Button::Right));
    assert_eq!(opposite(Button::Right), Some(Button::Left));
    assert_eq!(opposite(Button::Up), Some(Button::Down));
    assert_eq!(opposite(Button::Down), Some(Button::Up));
    for b in [Button::A, Button::B, Button::Select, Button::Start] {
        assert_eq!(opposite(b), None);
    }
}

#[test]
fn socd_suppress_only_when_filter_is_on() {
    // Filter on (allow_opposing == false, bgb default): a direction suppresses
    // its opposite.
    assert_eq!(socd_suppress(Button::Left, false), Some(Button::Right));
    assert_eq!(socd_suppress(Button::Down, false), Some(Button::Up));
    // Action buttons never suppress anything.
    assert_eq!(socd_suppress(Button::A, false), None);
    // Filter off (allow_opposing == true): both directions may be held.
    assert_eq!(socd_suppress(Button::Left, true), None);
    assert_eq!(socd_suppress(Button::Up, true), None);
}

#[test]
fn key_map_round_trips_through_config() {
    let mut b = KeyBindings::default();
    b.set(Button::A, KeyCode::KeyQ);
    b.clear(Button::Select);
    let cfg = b.to_config();
    assert_eq!(cfg, "Right,Left,Up,Down,Q,X,-,Enter");
    assert_eq!(KeyBindings::from_config(&cfg), b);
    // A truncated or garbled config falls back to the slot default, never wedges.
    let partial = KeyBindings::from_config("Right,Left,bogus");
    assert_eq!(partial.key_for(Button::Up), Some(KeyCode::ArrowUp));
    assert_eq!(partial.key_for(Button::Start), Some(KeyCode::Enter));
}
