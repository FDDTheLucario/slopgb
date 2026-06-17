//! Headless tests for the popup-window focus-dismiss latch. The winit glue
//! (borderless window creation, surface, event routing) is verified live; this
//! pins the WM-quirk handling that nearly closed the menu on open.

use super::*;

#[test]
fn spurious_on_map_focus_loss_does_not_dismiss() {
    // Some WMs send `Focused(false)` right after mapping a borderless window,
    // before it is ever focused — must NOT dismiss the freshly-opened menu.
    let mut focused_once = false;
    assert!(
        !focus_dismiss(&mut focused_once, false),
        "on-map focus-out ignored"
    );
    assert!(!focused_once, "latch stays unset until a real focus gain");
}

#[test]
fn focus_loss_after_a_gain_dismisses() {
    let mut focused_once = false;
    assert!(
        !focus_dismiss(&mut focused_once, true),
        "gaining focus never dismisses"
    );
    assert!(focused_once, "gain arms the latch");
    assert!(
        focus_dismiss(&mut focused_once, false),
        "later focus loss dismisses"
    );
}
