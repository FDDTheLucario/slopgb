//! The bgb "Options" control panel (functional clone of bgb 1.6.4's Options
//! property sheet — captures in `docs/bgb-reference/options/`). A modal overlay
//! over the LCD (like [`super::mainwin::InfoBox`]): an 8-tab dialog laid out in
//! bgb's two-row Windows tab control (row 1 Graphics/System/Debug/Exceptions,
//! row 2 Sound/GB Colors/Joypad/Misc), the active tab's group sitting in the
//! bottom row touching the content, with OK/Cancel/Apply/Defaults buttons.
//!
//! Settings backed by a real slopgb capability are **live**; bgb-only controls
//! (SGB borders, game-controller config, WAV/AVI recording, bootroms, link, …)
//! are rendered faithfully but inert/greyed, exactly as bgb greys unavailable
//! controls. Goal is functional 1:1, not pixel or code parity.
//!
//! `main` owns the `Option<OptionsState>` and routes keys/clicks to it, then
//! applies an [`OptionsOutcome`] to `App`/`Session`/`GameBoy`. The per-tab
//! control descriptors live in [`tabs`].

use slopgb_core::Model;

use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height, measure};

pub mod tabs;

// --- Settings ---------------------------------------------------------------

/// Which emulated system the System tab selects. Maps to a [`Model`] override
/// (or auto-detect) applied on the next ROM (re)load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelChoice {
    /// "automatic, prefer GBC" — let `GameBoy::auto_model` decide (no override).
    Auto,
    /// "Gameboy" — force DMG.
    Dmg,
    /// "Gameboy Color" — force CGB.
    Cgb,
}

impl ModelChoice {
    /// The `Model` override this choice applies on reload (`None` = auto-detect).
    #[must_use]
    pub fn as_override(self) -> Option<Model> {
        match self {
            ModelChoice::Auto => None,
            ModelChoice::Dmg => Some(Model::Dmg),
            ModelChoice::Cgb => Some(Model::Cgb),
        }
    }

    /// Recover the closest choice from a concrete live model.
    #[must_use]
    pub fn from_model(m: Model) -> Self {
        if m.is_cgb() {
            ModelChoice::Cgb
        } else {
            ModelChoice::Dmg
        }
    }

    /// Seed from the persistent model *preference* (`--model`, the value reused
    /// for every ROM load): `None` → `Auto` (bgb's default; auto-detects each
    /// ROM and so never desyncs across loads / never force-switches on Apply).
    #[must_use]
    pub fn from_option(m: Option<Model>) -> Self {
        m.map_or(ModelChoice::Auto, ModelChoice::from_model)
    }
}

/// A named DMG palette preset (GB Colors tab "Scheme"). `colors[0]` is the
/// lightest shade (GB color 0), `colors[3]` the darkest — the order
/// `GameBoy::set_dmg_palette` expects.
#[derive(Clone, Copy, Debug)]
pub struct Scheme {
    pub name: &'static str,
    pub colors: [u32; 4],
}

/// Built-in DMG palette schemes. Index 0 ("BGB 0.3") is slopgb's default — bgb's
/// own default pale-green LCD palette, decoded straight from `bgb.ini`'s
/// `Color0..3` (stored BGR: `CCFCE8 90D4AC 708C54 382C14` → RGB below). So a
/// fresh slopgb (and its no-ROM blank screen) matches bgb out of the box; the
/// core power-on default stays grayscale, available here as a selectable scheme.
pub const SCHEMES: [Scheme; 4] = [
    Scheme {
        name: "BGB 0.3",
        colors: [0x00E8_FCCC, 0x00AC_D490, 0x0054_8C70, 0x0014_2C38],
    },
    Scheme {
        name: "Grayscale",
        colors: [0x00FF_FFFF, 0x00AA_AAAA, 0x0055_5555, 0x0000_0000],
    },
    Scheme {
        name: "DMG green",
        colors: [0x009B_BC0F, 0x008B_AC0F, 0x0030_6230, 0x000F_380F],
    },
    Scheme {
        name: "Pocket",
        colors: [0x00E3_E6C9, 0x00B5_B69E, 0x007B_7563, 0x002E_2D26],
    },
];

/// Every Options setting. Live fields drive a real slopgb capability; the dialog
/// also carries the (mostly inert) bgb controls implicitly via [`tabs`]. Cloned
/// into [`OptionsState`] as the working + baseline copies. (Not `Copy` — it
/// holds the bootrom path strings.)
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// System → Emulated system (applied on reload).
    pub model: ModelChoice,
    /// Graphics → stretch the LCD to fill (fullscreen stretched).
    pub stretch: bool,
    /// Sound → master volume, 0.0..=1.0.
    pub volume: f32,
    /// Sound → mono output (downmix L/R).
    pub mono: bool,
    /// Debug → lowercase disassembler mnemonics.
    pub lowercase_disasm: bool,
    /// Debug → lowercase hex digits in the disasm/memory panes.
    pub lowercase_hex: bool,
    /// Debug → show the counted-clocks column in the disasm pane.
    pub show_clocks: bool,
    /// Debug → disassemble in RGBDS syntax (vs bgb / no$gmb). Default on.
    pub rgbds_disasm: bool,
    /// Debug → pop the memory viewer out into its own window (a slopgb extra).
    pub memory_window: bool,
    /// Misc → fast-forward speed multiplier (turbo), 1..=20.
    pub ff_speed: u32,
    /// Misc → framerate limit (0 = real speed / 60 fps).
    pub framerate_limit: u32,
    /// Misc → show the framerate in the title bar.
    pub show_framerate: bool,
    /// Misc → freeze the Recent ROMs menu (stop auto-updating it).
    pub freeze_recent: bool,
    /// Misc → pause emulation when the window loses focus.
    pub pause_on_focus_loss: bool,
    /// GB Colors → selected scheme index into [`SCHEMES`].
    pub scheme: usize,
    /// GB Colors → the live DMG palette (lightest→darkest).
    pub dmg_palette: [u32; 4],
    /// Joypad → "allow pressing L+R or U+D". `false` (bgb default) filters
    /// opposing directions so the joypad never reports both at once.
    pub allow_opposing: bool,
    /// Exceptions → "break on ld b,b (40h)".
    pub break_ld_b_b: bool,
    /// Exceptions → "break on invalid opcode" (bgb checks this by default).
    pub break_invalid_op: bool,
    /// Exceptions → "break on ram echo (E000-FDFF) access".
    pub break_echo_ram: bool,
    /// Exceptions → "break on disabling LCD outside vblank".
    pub break_lcd_off_vblank: bool,
    /// System → "bootroms enabled": execute the configured boot ROM on ROM load
    /// (bgb's checkbox). Off by default — slopgb then boots post-boot.
    pub bootroms_enabled: bool,
    /// System → DMG/MGB/SGB bootrom path (empty = none). Used when
    /// [`Self::bootroms_enabled`] and the loaded model is DMG-class.
    pub bootrom_dmg: String,
    /// System → GBC bootrom path (empty = none). Used for CGB/AGB models.
    pub bootrom_gbc: String,
    /// System → SGB bootrom path (empty = none). Faithful field; slopgb maps an
    /// SGB model to the DMG-class 256 B boot ROM, so this feeds the SGB models.
    pub bootrom_sgb: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: ModelChoice::Auto,
            stretch: false,
            volume: 1.0,
            mono: false,
            lowercase_disasm: true,
            lowercase_hex: false,
            show_clocks: true,
            rgbds_disasm: true,
            memory_window: false,
            ff_speed: 10,
            framerate_limit: 0,
            show_framerate: false,
            freeze_recent: false,
            pause_on_focus_loss: false,
            scheme: 0,
            dmg_palette: SCHEMES[0].colors,
            allow_opposing: false,
            break_ld_b_b: false,
            // bgb ships with "break on invalid opcode" checked.
            break_invalid_op: true,
            break_echo_ram: false,
            break_lcd_off_vblank: false,
            bootroms_enabled: false,
            bootrom_dmg: String::new(),
            bootrom_gbc: String::new(),
            bootrom_sgb: String::new(),
        }
    }
}

impl Settings {
    /// Apply scheme `i`: select it and copy its palette into [`Self::dmg_palette`].
    pub fn select_scheme(&mut self, i: usize) {
        if let Some(s) = SCHEMES.get(i) {
            self.scheme = i;
            self.dmg_palette = s.colors;
        }
    }

    /// The core exception-break mask (`EXC_*` bits) for the armed conditions —
    /// pushed to the machine by `App::apply_exceptions`.
    #[must_use]
    pub fn exception_mask(&self) -> u16 {
        use slopgb_core::{EXC_ECHO_RAM, EXC_INVALID_OPCODE, EXC_LCD_OFF_VBLANK, EXC_LD_B_B};
        let mut m = 0;
        if self.break_ld_b_b {
            m |= EXC_LD_B_B;
        }
        if self.break_invalid_op {
            m |= EXC_INVALID_OPCODE;
        }
        if self.break_echo_ram {
            m |= EXC_ECHO_RAM;
        }
        if self.break_lcd_off_vblank {
            m |= EXC_LCD_OFF_VBLANK;
        }
        m
    }
}

// --- Tabs -------------------------------------------------------------------

/// The eight Options tabs, in bgb's order (row 1 then row 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsTab {
    Graphics,
    System,
    Debug,
    Exceptions,
    Sound,
    GbColors,
    Joypad,
    Misc,
}

impl OptionsTab {
    /// Tabs in bgb's top group (row 1 when one of them is active).
    pub const GROUP_A: [OptionsTab; 4] = [
        OptionsTab::Graphics,
        OptionsTab::System,
        OptionsTab::Debug,
        OptionsTab::Exceptions,
    ];
    /// Tabs in bgb's bottom group (row 2 when one of them is active).
    pub const GROUP_B: [OptionsTab; 4] = [
        OptionsTab::Sound,
        OptionsTab::GbColors,
        OptionsTab::Joypad,
        OptionsTab::Misc,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            OptionsTab::Graphics => "Graphics",
            OptionsTab::System => "System",
            OptionsTab::Debug => "Debug",
            OptionsTab::Exceptions => "Exceptions",
            OptionsTab::Sound => "Sound",
            OptionsTab::GbColors => "GB Colors",
            OptionsTab::Joypad => "Joypad",
            OptionsTab::Misc => "Misc",
        }
    }

    /// 0 for [`Self::GROUP_A`], 1 for [`Self::GROUP_B`].
    #[must_use]
    pub fn group(self) -> u8 {
        if OptionsTab::GROUP_A.contains(&self) {
            0
        } else {
            1
        }
    }
}

// --- Dialog geometry --------------------------------------------------------

/// bgb's Options dialog is 345×361. slopgb's fixed 7px font is wider than bgb's
/// proportional one, so the System tab's caption-sized box + the right-column
/// bootrom path fields need a little more width to fit the same layout; 420 keeps
/// every control visible at the default scale (centred, clipped if smaller).
pub const DIALOG_W: i32 = 420;
pub const DIALOG_H: i32 = 361;
/// Height of one tab row.
const TAB_ROW_H: i32 = 19;
/// Tab-label horizontal padding (matches the strip in [`OptionsState::tab_hitboxes`]).
const TAB_PAD: i32 = 6;
/// Button-row height + button size.
const BTN_H: i32 = 22;
const BTN_W: i32 = 70;

/// The four bottom buttons, left-to-right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsButton {
    Ok,
    Cancel,
    Apply,
    Defaults,
}

impl OptionsButton {
    pub const ALL: [OptionsButton; 4] = [
        OptionsButton::Ok,
        OptionsButton::Cancel,
        OptionsButton::Apply,
        OptionsButton::Defaults,
    ];
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            OptionsButton::Ok => "OK",
            OptionsButton::Cancel => "Cancel",
            OptionsButton::Apply => "Apply",
            OptionsButton::Defaults => "Defaults",
        }
    }
}

/// What `main` does after a button press. `*Apply` variants push `working` to the
/// live state; `Close`/`StayReset` do not (matching bgb: **Defaults only changes
/// the controls** — nothing goes live until OK/Apply; Cancel reverts and closes
/// without re-applying, since the live state already equals the baseline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsOutcome {
    /// OK: apply `working` to the live machine, then close.
    CloseApply,
    /// Cancel: `working` reverted to baseline; close without applying.
    Close,
    /// Apply: apply `working` to the live machine, stay open.
    StayApply,
    /// Defaults: `working` reset to the tab's defaults; stay open, do NOT apply.
    StayReset,
    /// Joypad → "configure keyboard": open the key-rebind wizard. Neither
    /// applies nor closes the dialog (the wizard floats above it).
    ConfigureKeyboard,
    /// System → a `...` bootrom-path button: open the shared path modal over the
    /// dialog to edit that slot's path. Neither applies nor closes.
    PickBootrom(BootromSlot),
}

/// Which bootrom-path field (bgb's System tab: DMG / GBC / SGB bootrom). The
/// loaded model selects the slot (DMG/MGB → Dmg, CGB/AGB → Gbc, SGB/SGB2 → Sgb).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootromSlot {
    Dmg,
    Gbc,
    Sgb,
}

impl BootromSlot {
    /// This slot's path in `settings` (mutable, for the modal to write into).
    pub(crate) fn path_mut(self, s: &mut Settings) -> &mut String {
        match self {
            BootromSlot::Dmg => &mut s.bootrom_dmg,
            BootromSlot::Gbc => &mut s.bootrom_gbc,
            BootromSlot::Sgb => &mut s.bootrom_sgb,
        }
    }

    /// This slot's path in `settings` (read-only, for rendering).
    #[must_use]
    pub(crate) fn path(self, s: &Settings) -> &str {
        match self {
            BootromSlot::Dmg => &s.bootrom_dmg,
            BootromSlot::Gbc => &s.bootrom_gbc,
            BootromSlot::Sgb => &s.bootrom_sgb,
        }
    }
}

impl OptionsOutcome {
    /// Whether `main` should close the dialog after this outcome.
    #[must_use]
    pub fn closes(self) -> bool {
        matches!(self, OptionsOutcome::CloseApply | OptionsOutcome::Close)
    }
    /// Whether `main` should push `working` to the live machine.
    #[must_use]
    pub fn applies(self) -> bool {
        matches!(self, OptionsOutcome::CloseApply | OptionsOutcome::StayApply)
    }
}

// --- State ------------------------------------------------------------------

/// Live interactive state for the Options dialog. `working` holds the edits in
/// progress; `baseline` is the last-applied snapshot used to revert on Cancel.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionsState {
    pub active: OptionsTab,
    pub working: Settings,
    pub baseline: Settings,
}

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
        let (top, bottom) = if active_group == 0 {
            (OptionsTab::GROUP_B, OptionsTab::GROUP_A)
        } else {
            (OptionsTab::GROUP_A, OptionsTab::GROUP_B)
        };
        let mut out = Vec::with_capacity(8);
        for (row, tabs) in [top, bottom].into_iter().enumerate() {
            let y = dialog.y + row as i32 * TAB_ROW_H;
            let mut cx = dialog.x + 4;
            for t in tabs {
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

// --- Rendering --------------------------------------------------------------

/// Draw the whole Options dialog centred in `c`.
pub fn render(c: &mut Canvas, st: &OptionsState, theme: &Theme) {
    let dialog = OptionsState::dialog_rect(c.bounds());
    c.fill_rect(dialog, theme.bg);
    c.outline_rect(dialog, theme.border);
    // Tab strip (two rows; active tab outlined, others just labelled).
    for (t, r) in st.tab_hitboxes(dialog) {
        if t == st.active {
            c.fill_rect(r, theme.bg);
            c.outline_rect(r, theme.text);
        } else {
            c.outline_rect(r, theme.border);
        }
        draw_text(c, r.x + TAB_PAD, r.y + 3, t.label(), theme.text);
    }
    // Active tab content.
    let content = OptionsState::content_rect(dialog);
    let saved = c.push_clip(content);
    tabs::render(st.active, &st.working, c, content, theme);
    c.set_clip(saved);
    // Button row.
    for (b, r) in OptionsState::button_rects(dialog) {
        c.fill_rect(r, theme.bg);
        c.outline_rect(r, theme.text);
        let tx = r.x + (r.w - measure(b.label())) / 2;
        draw_text(
            c,
            tx,
            r.y + (r.h - line_height()) / 2,
            b.label(),
            theme.text,
        );
    }
}

// --- Shared control-drawing helpers (enabled-aware; greyed when inert) ------

/// Foreground colour for a control: black when live, grey when inert (matches
/// bgb greying out unavailable options).
fn fg(enabled: bool, theme: &Theme) -> u32 {
    if enabled { theme.text } else { theme.hilight }
}

/// Draw a checkbox honouring `enabled` (greyed if inert). The clickable hit-rect
/// is owned by the [`tabs::Ctrl`] list, so this only draws.
pub(crate) fn check(
    c: &mut Canvas,
    x: i32,
    y: i32,
    checked: bool,
    label: &str,
    enabled: bool,
    theme: &Theme,
) {
    let color = fg(enabled, theme);
    let box_sz = line_height() - 4;
    c.outline_rect(Rect::new(x, y, box_sz, box_sz), color);
    if checked {
        c.fill_rect(Rect::new(x + 2, y + 2, box_sz - 4, box_sz - 4), color);
    }
    draw_text(c, x + box_sz + 3, y, label, color);
}

/// Draw a single radio option honouring `enabled`. Hit-rect owned by the caller.
pub(crate) fn radio(
    c: &mut Canvas,
    x: i32,
    y: i32,
    selected: bool,
    label: &str,
    enabled: bool,
    theme: &Theme,
) {
    let color = fg(enabled, theme);
    let dot = line_height() - 4;
    c.outline_rect(Rect::new(x, y, dot, dot), color);
    if selected {
        c.fill_rect(Rect::new(x + 2, y + 2, dot - 4, dot - 4), color);
    }
    draw_text(c, x + dot + 3, y, label, color);
}

/// Draw an inert "dropdown" (combo box): a bordered box showing `value` with a
/// `▼`-ish arrow. Used for both live (scheme) and inert combos.
pub(crate) fn dropdown(
    c: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    value: &str,
    enabled: bool,
    theme: &Theme,
) {
    let color = fg(enabled, theme);
    let h = line_height() + 2;
    let r = Rect::new(x, y, w, h);
    c.outline_rect(r, color);
    draw_text(c, x + 3, y + 1, value, color);
    // arrow box on the right
    c.vline(r.right() - h, y, h, color);
    draw_text(c, r.right() - h + 2, y + 1, "v", color);
}

/// Draw a horizontal slider with `frac` (0..1) thumb. The clickable track rect is
/// owned by the [`tabs::Ctrl`] list (and read back by [`slider_frac`]).
pub(crate) fn slider(
    c: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    frac: f32,
    enabled: bool,
    theme: &Theme,
) {
    let color = fg(enabled, theme);
    let mid = y + line_height() / 2;
    c.hline(x, mid, w, color);
    let f = frac.clamp(0.0, 1.0);
    let tx = x + (f * (w - 6) as f32) as i32;
    c.fill_rect(Rect::new(tx, y, 6, line_height()), color);
}

/// The fraction (0..1) a click at `px` maps to within a slider `track`.
#[must_use]
pub(crate) fn slider_frac(track: Rect, px: i32) -> f32 {
    if track.w <= 0 {
        return 0.0;
    }
    ((px - track.x) as f32 / track.w as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
