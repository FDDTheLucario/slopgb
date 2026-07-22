//! Pins the builder↔`reset_defaults` single source. `reset_defaults` derives the
//! fields it resets from what each tab's builder renders ([`controls`]), so a
//! control and its reset can't drift apart — this proves it end to end: perturb
//! every live control a tab renders, reset the tab, and every control must render
//! its default state again. A field the builder shows but no reset covers fails.

use super::*;
use crate::ui::canvas::Rect;
use crate::windows::options::{OptionsTab, Settings};

/// A content rect large enough for every tab to lay out all its controls.
const R: Rect = Rect::new(0, 0, 760, 560);

const TABS: [OptionsTab; 10] = [
    OptionsTab::Graphics,
    OptionsTab::System,
    OptionsTab::Debug,
    OptionsTab::Exceptions,
    OptionsTab::Sound,
    OptionsTab::GbColors,
    OptionsTab::Joypad,
    OptionsTab::Misc,
    OptionsTab::Theme,
    OptionsTab::Plugins,
];

#[test]
fn reset_defaults_restores_every_control_each_tab_renders() {
    let d = Settings::default();
    for tab in TABS {
        // Perturb every live control the tab renders away from its default (a
        // left-click at x=0 — toggles flip, sliders go to the low end, radios /
        // dropdowns cycle). `PureBgb` is skipped: it's a compound action that
        // flips several other toggles rather than a stored field of its own.
        let mut s = d.clone();
        for ct in controls(tab, &s, R) {
            match ct.field {
                Some(Field::PureBgb) | None => {}
                Some(f) => apply(f, &mut s, &ct, 0),
            }
        }
        reset_defaults(tab, &mut s);
        // Every rendered control now matches what the default settings render.
        for (now, def) in controls(tab, &s, R).iter().zip(controls(tab, &d, R).iter()) {
            let field = now.field;
            match (&now.kind, &def.kind) {
                (Kind::Check { checked: a, .. }, Kind::Check { checked: b, .. })
                | (Kind::Radio { selected: a, .. }, Kind::Radio { selected: b, .. }) => {
                    assert_eq!(a, b, "{tab:?}: {field:?} rendered but not reset to default");
                }
                (Kind::Slider { frac: a, .. }, Kind::Slider { frac: b, .. }) => {
                    assert!(
                        (a - b).abs() < 1e-6,
                        "{tab:?}: {field:?} slider not reset ({a} vs {b})"
                    );
                }
                (Kind::Dropdown { value: a, .. }, Kind::Dropdown { value: b, .. }) => {
                    assert_eq!(a, b, "{tab:?}: {field:?} dropdown not reset to default");
                }
                _ => {}
            }
        }
    }
}
