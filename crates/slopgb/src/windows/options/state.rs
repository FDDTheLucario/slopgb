//! [`OptionsState`]'s dialog geometry + click-routing methods, a second
//! `impl OptionsState` block split out of `options.rs` to keep it under the
//! 1000-line cap.

use super::*;

impl OptionsState {
    /// Open the dialog seeded from the current live `settings`.
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        Self {
            active: OptionsTab::System,
            working: settings.clone(),
            baseline: settings,
        }
    }

    /// The centred dialog rect within `bounds`.
    #[must_use]
    pub fn dialog_rect(bounds: Rect) -> Rect {
        let x = bounds.x + (bounds.w - DIALOG_W) / 2;
        let y = bounds.y + (bounds.h - DIALOG_H) / 2;
        Rect::new(x, y, DIALOG_W, DIALOG_H)
    }

    /// The content area below the two tab rows and above the button row.
    #[must_use]
    pub fn content_rect(dialog: Rect) -> Rect {
        let top = dialog.y + 2 * TAB_ROW_H + 4;
        let bottom = dialog.bottom() - BTN_H - 8;
        Rect::new(dialog.x + 6, top, dialog.w - 12, bottom - top)
    }

    /// Each tab's hit-rect, with the active tab's group on the bottom row (bgb's
    /// multi-row tab control behaviour). Returns `(tab, rect)` in draw order
    /// (top row first, then bottom row).
    #[must_use]
    pub fn tab_hitboxes(&self, dialog: Rect) -> Vec<(OptionsTab, Rect)> {
        let active_group = self.active.group();
        // Slices: the two groups now differ in length (Theme extends GROUP_B), so
        // the swapped branches can't be equal-length arrays.
        let (top, bottom): (&[OptionsTab], &[OptionsTab]) = if active_group == 0 {
            (&OptionsTab::GROUP_B, &OptionsTab::GROUP_A)
        } else {
            (&OptionsTab::GROUP_A, &OptionsTab::GROUP_B)
        };
        let mut out = Vec::with_capacity(9);
        for (row, tabs) in [top, bottom].into_iter().enumerate() {
            let y = dialog.y + row as i32 * TAB_ROW_H;
            let mut cx = dialog.x + 4;
            for &t in tabs {
                let w = measure(t.label()) + TAB_PAD * 2;
                out.push((t, Rect::new(cx, y, w, TAB_ROW_H)));
                cx += w + 2;
            }
        }
        out
    }

    /// The four button hit-rects, in [`OptionsButton::ALL`] order.
    #[must_use]
    pub fn button_rects(dialog: Rect) -> Vec<(OptionsButton, Rect)> {
        let y = dialog.bottom() - BTN_H - 4;
        let gap = 8;
        // OK left-aligned; Cancel/Apply/Defaults follow; Defaults right-aligned.
        let mut out = Vec::with_capacity(4);
        let mut x = dialog.x + 8;
        for b in OptionsButton::ALL {
            out.push((b, Rect::new(x, y, BTN_W, BTN_H)));
            x += BTN_W + gap;
        }
        out
    }

    /// Route a left-click at `(px, py)` (window pixels). Tabs switch the active
    /// tab; buttons return their [`OptionsOutcome`]; content clicks mutate
    /// `working` (and a few — e.g. "configure keyboard" — return their own
    /// outcome). Returns `Some(outcome)` for a button press or such a control.
    pub fn on_click(&mut self, px: i32, py: i32, bounds: Rect) -> Option<OptionsOutcome> {
        let dialog = Self::dialog_rect(bounds);
        for (t, r) in self.tab_hitboxes(dialog) {
            if r.contains(px, py) {
                self.active = t;
                return None;
            }
        }
        for (b, r) in Self::button_rects(dialog) {
            if r.contains(px, py) {
                return Some(self.press(b));
            }
        }
        let content = Self::content_rect(dialog);
        tabs::on_content_click(self.active, &mut self.working, px, py, content)
    }

    /// Apply a button's semantics. OK applies + closes; Cancel reverts + closes;
    /// Apply commits the baseline + stays open; Defaults resets the active tab.
    pub fn press(&mut self, b: OptionsButton) -> OptionsOutcome {
        match b {
            OptionsButton::Ok => {
                self.baseline = self.working.clone();
                OptionsOutcome::CloseApply
            }
            OptionsButton::Cancel => {
                self.working = self.baseline.clone();
                OptionsOutcome::Close
            }
            OptionsButton::Apply => {
                self.baseline = self.working.clone();
                OptionsOutcome::StayApply
            }
            OptionsButton::Defaults => {
                // bgb's Defaults only resets the controls; nothing goes live until
                // the user presses OK/Apply, so this does NOT apply.
                tabs::reset_defaults(self.active, &mut self.working);
                OptionsOutcome::StayReset
            }
        }
    }
}
