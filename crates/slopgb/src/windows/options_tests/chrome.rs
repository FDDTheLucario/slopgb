//! Dialog-chrome tests (tab strip, button row, render, scratch/Defaults
//! semantics), split out of `options_tests.rs` to keep it under the
//! 1000-line cap.

use super::*;
use crate::ui::canvas::Canvas;

// --- Task 2: tab labels + groups --------------------------------------------

#[test]
fn options_tab_labels_and_groups() {
    let all: Vec<OptionsTab> = OptionsTab::GROUP_A
        .iter()
        .chain(&OptionsTab::GROUP_B)
        .copied()
        .collect();
    assert_eq!(all.len(), 10);
    assert_eq!(
        all.iter().map(|t| t.label()).collect::<Vec<_>>(),
        [
            "Graphics",
            "System",
            "Debug",
            "Exceptions",
            "Sound",
            "GB Colors",
            "Joypad",
            "Misc",
            "Theme",
            "Plugins"
        ]
    );
    // The slopgb Theme tab lives in the bottom group.
    assert!(OptionsTab::GROUP_B.contains(&OptionsTab::Theme));
    for t in OptionsTab::GROUP_A {
        assert_eq!(t.group(), 0);
    }
    for t in OptionsTab::GROUP_B {
        assert_eq!(t.group(), 1);
    }
}

// --- Task 3: tab switching + two-row swap ------------------------------------

#[test]
fn tab_click_switches_active() {
    let mut st = OptionsState::new(Settings::default());
    let boxes = st.tab_hitboxes(dialog());
    let (tab, r) = boxes
        .iter()
        .find(|(t, _)| *t == OptionsTab::Sound)
        .cloned()
        .unwrap();
    assert_eq!(tab, OptionsTab::Sound);
    st.on_click(r.x + 2, r.y + 2, BOUNDS);
    assert_eq!(st.active, OptionsTab::Sound);
}

#[test]
fn active_group_sits_on_bottom_row() {
    // System (group A) active → group A is the bottom row (larger y).
    let st = OptionsState::new(Settings::default()); // active = System
    let boxes = st.tab_hitboxes(dialog());
    let row_y = |want: OptionsTab| boxes.iter().find(|(t, _)| *t == want).unwrap().1.y;
    assert!(
        row_y(OptionsTab::System) > row_y(OptionsTab::Sound),
        "active group A must be the bottom row"
    );

    // Switch to a group-B tab → group B drops to the bottom.
    let mut st2 = OptionsState::new(Settings::default());
    st2.active = OptionsTab::GbColors;
    let b2 = st2.tab_hitboxes(dialog());
    let y2 = |want: OptionsTab| b2.iter().find(|(t, _)| *t == want).unwrap().1.y;
    assert!(
        y2(OptionsTab::GbColors) > y2(OptionsTab::Graphics),
        "active group B must be the bottom row"
    );
}

// --- Task 4: chrome layout ---------------------------------------------------

#[test]
fn chrome_button_order() {
    let rects = OptionsState::button_rects(dialog());
    assert_eq!(
        rects.iter().map(|(b, _)| *b).collect::<Vec<_>>(),
        OptionsButton::ALL.to_vec()
    );
    // left-to-right
    for w in rects.windows(2) {
        assert!(w[0].1.x < w[1].1.x);
    }
}

#[test]
fn render_does_not_panic_and_draws() {
    let mut buf = vec![0u32; (BOUNDS.w * BOUNDS.h) as usize];
    let mut c = Canvas::new(&mut buf, BOUNDS.w as usize, BOUNDS.h as usize);
    let st = OptionsState::new(Settings::default());
    render(&mut c, &st, &T);
    let d = dialog();
    // The dialog bg (white) was written.
    let idx = ((d.y + 1) * BOUNDS.w + d.x + 1) as usize;
    assert_eq!(buf[idx], T.bg);
    // The button row drew ink (the OK button's border) somewhere on its row.
    let (_, ok) = OptionsState::button_rects(d)[0];
    let row_has_ink = (ok.x..ok.right()).any(|x| {
        let i = ((ok.y + ok.h / 2) * BOUNDS.w + x) as usize;
        buf[i] == T.text
    });
    assert!(row_has_ink, "button row should draw the OK button border");
}

// --- Task 5: scratch / button semantics --------------------------------------

#[test]
fn scratch_semantics_cancel_reverts() {
    let mut st = OptionsState::new(Settings::default());
    st.working.volume = 0.25;
    let out = st.press(OptionsButton::Cancel);
    assert_eq!(out, OptionsOutcome::Close);
    assert_eq!(st.working.volume, 1.0, "Cancel reverts to baseline");
}

#[test]
fn scratch_semantics_apply_commits_stays_open() {
    let mut st = OptionsState::new(Settings::default());
    st.working.volume = 0.25;
    let out = st.press(OptionsButton::Apply);
    assert_eq!(out, OptionsOutcome::StayApply);
    assert!(out.applies() && !out.closes(), "Apply applies + stays open");
    assert_eq!(st.baseline.volume, 0.25, "Apply commits baseline");
    // a subsequent Cancel keeps the committed value
    st.working.volume = 0.9;
    st.press(OptionsButton::Cancel);
    assert_eq!(st.working.volume, 0.25);
}

#[test]
fn scratch_semantics_ok_applies_and_closes() {
    let mut st = OptionsState::new(Settings::default());
    st.working.mono = true;
    let out = st.press(OptionsButton::Ok);
    assert_eq!(out, OptionsOutcome::CloseApply);
    assert!(out.applies() && out.closes(), "OK applies + closes");
    assert!(st.baseline.mono);
}

#[test]
fn defaults_button_stays_open_without_applying() {
    // bgb's Defaults only edits the controls — it does not push live until OK/Apply.
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Sound;
    st.working.volume = 0.3;
    let out = st.press(OptionsButton::Defaults);
    assert_eq!(out, OptionsOutcome::StayReset);
    assert!(!out.applies(), "Defaults does not apply live");
    assert!(!out.closes(), "Defaults stays open");
    assert_eq!(st.working.volume, 1.0, "Defaults reset the working control");
}

#[test]
fn defaults_resets_only_active_tab() {
    let mut st = OptionsState::new(Settings::default());
    st.active = OptionsTab::Sound;
    st.working.volume = 0.1;
    st.working.lowercase_hex = true; // a Debug-tab field, must survive
    st.press(OptionsButton::Defaults);
    assert_eq!(st.working.volume, 1.0, "Sound Defaults resets volume");
    assert!(
        st.working.lowercase_hex,
        "Debug field untouched by Sound Defaults"
    );
}

#[test]
fn defaults_resets_every_tab_only_its_own_fields() {
    // For each tab, mutate one of its live fields away from default, press
    // Defaults on that tab, and assert it reset (and an out-of-tab field is
    // untouched). Covers every reset_defaults branch, not just Sound.
    type Case = (OptionsTab, fn(&mut Settings), fn(&Settings) -> bool);
    let cases: &[Case] = &[
        (OptionsTab::Graphics, |s| s.stretch = true, |s| !s.stretch),
        (
            OptionsTab::System,
            |s| s.model = ModelChoice::Cgb,
            |s| s.model == ModelChoice::Auto,
        ),
        (
            OptionsTab::Debug,
            |s| s.lowercase_hex = true,
            |s| !s.lowercase_hex,
        ),
        (OptionsTab::Sound, |s| s.volume = 0.1, |s| s.volume == 1.0),
        (OptionsTab::GbColors, |s| s.scheme = 2, |s| s.scheme == 0),
        (OptionsTab::Misc, |s| s.ff_speed = 3, |s| s.ff_speed == 10),
        (
            OptionsTab::Theme,
            |s| s.theme = ThemeChoice::Dark,
            |s| s.theme == ThemeChoice::Light,
        ),
        (
            // invalid-opcode defaults checked, so flip it off to test the reset.
            OptionsTab::Exceptions,
            |s| s.break_invalid_op = false,
            |s| s.break_invalid_op,
        ),
    ];
    for (tab, mutate, is_default) in cases {
        let mut st = OptionsState::new(Settings::default());
        st.active = *tab;
        mutate(&mut st.working);
        st.working.mono = true; // an out-of-tab field (Sound) — survives unless tab==Sound
        st.press(OptionsButton::Defaults);
        assert!(
            is_default(&st.working),
            "{tab:?} Defaults did not reset its field"
        );
        if *tab != OptionsTab::Sound {
            assert!(
                st.working.mono,
                "{tab:?} Defaults clobbered an out-of-tab field"
            );
        }
    }
}
