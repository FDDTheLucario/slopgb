use super::*;

#[test]
fn default_bindings_map_standard_layout() {
    let b = GamepadBindings::default();
    assert_eq!(b.gb_for(GpButton::South), Some(Button::A));
    assert_eq!(b.gb_for(GpButton::East), Some(Button::B));
    assert_eq!(b.gb_for(GpButton::Start), Some(Button::Start));
    assert_eq!(b.gb_for(GpButton::Select), Some(Button::Select));
    assert_eq!(b.gb_for(GpButton::DPadRight), Some(Button::Right));
    assert_eq!(b.gb_for(GpButton::DPadUp), Some(Button::Up));
    // North/West unbound by default.
    assert_eq!(b.gb_for(GpButton::North), None);
}

#[test]
fn bind_moves_a_button_and_never_double_maps() {
    let mut b = GamepadBindings::default();
    // Rebind A from South to North.
    b.bind(Button::A, GpButton::North);
    assert_eq!(b.gp_for(Button::A), Some(GpButton::North));
    assert_eq!(b.gb_for(GpButton::North), Some(Button::A));
    // South is now free (A moved off it).
    assert_eq!(b.gb_for(GpButton::South), None);
    // Binding an already-used control steals it from its old owner.
    b.bind(Button::B, GpButton::North);
    assert_eq!(b.gb_for(GpButton::North), Some(Button::B));
    assert_eq!(b.gp_for(Button::A), None, "A lost North to B");
}

#[test]
fn clear_unbinds_everything() {
    let mut b = GamepadBindings::default();
    b.clear();
    for gp in [GpButton::South, GpButton::DPadUp, GpButton::Start] {
        assert_eq!(b.gb_for(gp), None);
    }
}

#[test]
fn config_round_trips_and_default_string_is_stable() {
    let b = GamepadBindings::default();
    // The default seeds Settings::default — pin the exact string so they can't drift.
    assert_eq!(
        b.to_config(),
        "DPadRight,DPadLeft,DPadUp,DPadDown,South,East,Select,Start"
    );
    assert_eq!(default_map_config(), b.to_config());
    // Round-trip an edited map, including an unbound slot.
    let mut edited = b.clone();
    edited.bind(Button::A, GpButton::North);
    edited.unbind(Button::Start);
    let parsed = GamepadBindings::from_config(&edited.to_config());
    assert_eq!(parsed, edited);
    assert_eq!(parsed.gb_for(GpButton::North), Some(Button::A));
    assert_eq!(parsed.gp_for(Button::Start), None);
}

#[test]
fn from_config_defaults_unknown_and_short_input() {
    // Unknown token → that slot keeps its default; a truncated string leaves the
    // rest at default (can't wedge on a hand-edited config).
    let b = GamepadBindings::from_config("wat,DPadLeft");
    assert_eq!(b.gp_for(Button::Right), Some(GpButton::DPadRight)); // unknown → default
    assert_eq!(b.gp_for(Button::Left), Some(GpButton::DPadLeft));
    assert_eq!(b.gp_for(Button::A), Some(GpButton::South)); // untouched tail → default
}

#[test]
fn config_wizard_binds_each_step_and_finishes() {
    let mut w = GamepadConfigWizard::open(GamepadBindings::default());
    assert_eq!(w.current_button(), Some(Button::Right));
    assert!(w.finished().is_none());
    // Bind all 8 in order to face/shoulder buttons.
    for _ in 0..8 {
        w.bind(GpButton::North);
    }
    let done = w.finished().expect("wizard finished after 8 binds");
    // North was rebound repeatedly, stealing each time, so only the last (Start)
    // keeps it — proving bind() steals + advances.
    assert_eq!(done.gb_for(GpButton::North), Some(Button::Start));
}

#[test]
fn config_wizard_skip_clear_unbinds_current() {
    let mut w = GamepadConfigWizard::open(GamepadBindings::default());
    w.skip_clear(); // clears Right, advances
    let done_partial = {
        // finish the rest by keeping
        for _ in 0..7 {
            w.skip_keep();
        }
        w.finished().expect("finished")
    };
    assert_eq!(done_partial.gp_for(Button::Right), None, "Right cleared");
    assert_eq!(
        done_partial.gp_for(Button::Left),
        Some(GpButton::DPadLeft),
        "Left kept"
    );
}

#[test]
fn axis_dirs_respects_the_deadzone() {
    // Inside the deadzone → neither direction.
    assert_eq!(axis_dirs(0.4, 0.5), (false, false));
    assert_eq!(axis_dirs(-0.4, 0.5), (false, false));
    // Past the deadzone → the matching direction only.
    assert_eq!(axis_dirs(0.9, 0.5), (true, false));
    assert_eq!(axis_dirs(-0.9, 0.5), (false, true));
    // Exactly at the deadzone is not yet past it.
    assert_eq!(axis_dirs(0.5, 0.5), (false, false));
}

#[test]
fn stick_edges_fire_once_per_crossing() {
    let mut gp = Gamepads::new();
    let binds = GamepadBindings::default();
    // Push right past the deadzone → a single Right press edge.
    let mut ops = Vec::new();
    gp.stick_edge(&mut ops, 0.9, Button::Right, Button::Left);
    assert_eq!(ops, vec![(Button::Right, true)]);
    // Still held right → no new edge.
    let mut ops2 = Vec::new();
    gp.stick_edge(&mut ops2, 0.8, Button::Right, Button::Left);
    assert!(ops2.is_empty(), "held stick must not re-fire");
    // Return to centre → a Right release edge.
    let mut ops3 = Vec::new();
    gp.stick_edge(&mut ops3, 0.0, Button::Right, Button::Left);
    assert_eq!(ops3, vec![(Button::Right, false)]);
    let _ = binds; // the digital path is covered by the binding tests above
}
