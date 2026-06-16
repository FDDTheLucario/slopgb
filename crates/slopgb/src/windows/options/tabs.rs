//! Per-tab control descriptors for the Options dialog. Each tab builds a flat
//! list of placed [`Ctrl`]s from the current [`Settings`]; the same list drives
//! both rendering and click hit-testing, so the two can't diverge. Live controls
//! carry a [`Field`]; inert ones (`field: None`) render faithfully (greyed only
//! where bgb itself greys them) but do nothing on click.
//!
//! Layout coordinates are content-area-relative absolutes computed from the
//! `content` rect, so a click maps directly. Faithful to the bgb 1.6.4 captures
//! in `docs/bgb-reference/options/`.

// The tab builders construct their control list imperatively with a layout
// cursor (`Lay`), so `Vec::new()` + `push` is the natural idiom here, not a
// missing `vec![]` literal.
#![allow(clippy::vec_init_then_push)]

use super::{ModelChoice, SCHEMES, Settings, check, dropdown, radio, slider, slider_frac};
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::text::{draw_text, line_height, measure};
use crate::windows::options::OptionsTab;

/// A live setting a control drives. Inert controls have `field: None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Field {
    Stretch,
    Mono,
    LowercaseHex,
    ShowClocks,
    ShowFramerate,
    FreezeRecent,
    PauseOnFocusLoss,
    Model(ModelChoice),
    Volume,
    /// Fast-forward speed slider (maps the click fraction to 1..=`FF_SPEED_MAX`).
    FfSpeed,
    /// Framerate-limit slider (maps the click fraction to the `FRAMERATE_STEPS`).
    FramerateLimit,
    SchemeCycle,
}

/// Fast-forward speed slider range (1..=`FF_SPEED_MAX`).
const FF_SPEED_MAX: u32 = 20;
/// Discrete framerate-limit choices the Misc slider snaps to (0 = real speed).
const FRAMERATE_STEPS: [u32; 6] = [0, 30, 60, 120, 240, 300];

/// How a control draws + hit-tests.
#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Check { checked: bool, label: &'static str },
    Radio { selected: bool, label: &'static str },
    Dropdown { value: String, w: i32 },
    Slider { frac: f32, w: i32 },
    Button { label: &'static str, w: i32 },
    GroupBox { label: &'static str, w: i32, h: i32 },
    Label { text: String },
    Swatch { color: u32 },
}

/// A placed control: where it is, how it looks/acts, whether it is live, and
/// whether bgb itself greys it (visual only).
#[derive(Clone, Debug)]
pub(crate) struct Ctrl {
    pub rect: Rect,
    pub kind: Kind,
    pub field: Option<Field>,
    pub greyed: bool,
}

impl Ctrl {
    fn live(rect: Rect, kind: Kind, field: Field) -> Self {
        Self {
            rect,
            kind,
            field: Some(field),
            greyed: false,
        }
    }
    /// Inert but drawn black (bgb shows it normal, just unwired here).
    fn inert(rect: Rect, kind: Kind) -> Self {
        Self {
            rect,
            kind,
            field: None,
            greyed: false,
        }
    }
    /// Inert and greyed (bgb itself greys it out).
    fn grey(rect: Rect, kind: Kind) -> Self {
        Self {
            rect,
            kind,
            field: None,
            greyed: true,
        }
    }
}

/// A simple top-down layout cursor for placing controls in a content area.
struct Lay {
    x0: i32,
    x: i32,
    y: i32,
    step: i32,
}
impl Lay {
    fn new(content: Rect) -> Self {
        let step = line_height() + 3;
        Self {
            x0: content.x,
            x: content.x,
            y: content.y,
            step,
        }
    }
    /// Move to absolute column `cx` (content-relative) keeping the current row.
    fn col(&mut self, cx: i32) -> &mut Self {
        self.x = self.x0 + cx;
        self
    }
    /// Reset to the left column and advance one row.
    fn row(&mut self) -> &mut Self {
        self.x = self.x0;
        self.y += self.step;
        self
    }
    fn at(&self) -> (i32, i32) {
        (self.x, self.y)
    }
}

/// Build the active tab's control list.
pub(crate) fn controls(tab: OptionsTab, s: &Settings, content: Rect) -> Vec<Ctrl> {
    match tab {
        OptionsTab::Graphics => graphics(s, content),
        OptionsTab::System => system(s, content),
        OptionsTab::Debug => debug(s, content),
        OptionsTab::Exceptions => exceptions(s, content),
        OptionsTab::Sound => sound(s, content),
        OptionsTab::GbColors => gb_colors(s, content),
        OptionsTab::Joypad => joypad(s, content),
        OptionsTab::Misc => misc(s, content),
    }
}

/// Render the active tab.
pub(crate) fn render(tab: OptionsTab, s: &Settings, c: &mut Canvas, content: Rect, theme: &Theme) {
    for ct in controls(tab, s, content) {
        let enabled = !ct.greyed;
        let (x, y) = (ct.rect.x, ct.rect.y);
        match &ct.kind {
            Kind::Check { checked, label } => {
                check(c, x, y, *checked, label, enabled, theme);
            }
            Kind::Radio { selected, label } => {
                radio(c, x, y, *selected, label, enabled, theme);
            }
            Kind::Dropdown { value, w } => {
                dropdown(c, x, y, *w, value, enabled, theme);
            }
            Kind::Slider { frac, w } => {
                slider(c, x, y, *w, *frac, enabled, theme);
            }
            Kind::Button { label, w } => {
                let r = Rect::new(x, y, *w, line_height() + 4);
                c.outline_rect(r, super::fg(enabled, theme));
                let tx = x + (*w - measure(label)) / 2;
                draw_text(c, tx, y + 2, label, super::fg(enabled, theme));
            }
            Kind::GroupBox { label, w, h } => {
                // Caption sits just inside the top-left of the frame (drawing it
                // straddling the border would clip against the content area).
                c.outline_rect(Rect::new(x, y, *w, *h), theme.border);
                draw_text(c, x + 4, y + 1, label, theme.text);
            }
            Kind::Label { text } => {
                draw_text(c, x, y, text, super::fg(enabled, theme));
            }
            Kind::Swatch { color } => {
                c.fill_rect(ct.rect, *color);
                c.outline_rect(ct.rect, theme.text);
            }
        }
    }
}

/// Apply a content click: the first live control containing the point fires.
pub(crate) fn on_content_click(tab: OptionsTab, s: &mut Settings, px: i32, py: i32, content: Rect) {
    // Build against an immutable snapshot so the borrow ends before we mutate.
    let hit = controls(tab, s, content)
        .into_iter()
        .find(|ct| ct.field.is_some() && ct.rect.contains(px, py));
    let Some(ct) = hit else { return };
    let Some(field) = ct.field else { return };
    apply(field, s, &ct, px);
}

fn apply(field: Field, s: &mut Settings, ct: &Ctrl, px: i32) {
    match field {
        Field::Stretch => s.stretch = !s.stretch,
        Field::Mono => s.mono = !s.mono,
        Field::LowercaseHex => s.lowercase_hex = !s.lowercase_hex,
        Field::ShowClocks => s.show_clocks = !s.show_clocks,
        Field::ShowFramerate => s.show_framerate = !s.show_framerate,
        Field::FreezeRecent => s.freeze_recent = !s.freeze_recent,
        Field::PauseOnFocusLoss => s.pause_on_focus_loss = !s.pause_on_focus_loss,
        Field::Model(m) => s.model = m,
        Field::Volume => s.volume = slider_frac(ct.rect, px),
        // 1..=FF_SPEED_MAX mapped from the click fraction along the track.
        Field::FfSpeed => {
            let f = slider_frac(ct.rect, px);
            s.ff_speed = 1 + (f * (FF_SPEED_MAX - 1) as f32).round() as u32;
        }
        // Snap to the nearest discrete framerate step.
        Field::FramerateLimit => {
            let f = slider_frac(ct.rect, px);
            let idx = (f * (FRAMERATE_STEPS.len() - 1) as f32).round() as usize;
            s.framerate_limit = FRAMERATE_STEPS[idx.min(FRAMERATE_STEPS.len() - 1)];
        }
        Field::SchemeCycle => s.select_scheme((s.scheme + 1) % SCHEMES.len()),
    }
}

/// Reset only the active tab's live fields to their defaults.
pub(crate) fn reset_defaults(tab: OptionsTab, s: &mut Settings) {
    let d = Settings::default();
    match tab {
        OptionsTab::Graphics => s.stretch = d.stretch,
        OptionsTab::System => s.model = d.model,
        OptionsTab::Debug => {
            // Only the two live Debug fields; lowercase_disasm is inert (fixed).
            s.lowercase_hex = d.lowercase_hex;
            s.show_clocks = d.show_clocks;
        }
        // Exceptions has no live fields (all break conditions are inert stubs).
        OptionsTab::Exceptions => {}
        OptionsTab::Sound => {
            s.volume = d.volume;
            s.mono = d.mono;
        }
        OptionsTab::GbColors => s.select_scheme(d.scheme),
        OptionsTab::Joypad => {}
        OptionsTab::Misc => {
            s.ff_speed = d.ff_speed;
            s.framerate_limit = d.framerate_limit;
            s.show_framerate = d.show_framerate;
            s.freeze_recent = d.freeze_recent;
            s.pause_on_focus_loss = d.pause_on_focus_loss;
        }
    }
}

// --- Per-tab builders -------------------------------------------------------

fn graphics(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    // Left column: inert visual toggles + combos (bgb black).
    v.push(Ctrl::inert(
        rc(l.at(), "disable SGB colors"),
        chk("disable SGB colors", false),
    ));
    v.push(Ctrl::inert(
        rc(l.row().at(), "SGB border in screenshot"),
        chk("SGB border in screenshot", false),
    ));
    v.push(Ctrl::inert(
        rc(l.row().at(), "MGB auto border/colors"),
        chk("MGB auto border/colors", false),
    ));
    l.row();
    for (label, val) in [
        ("frame blend:", "off"),
        ("doubler:", "auto"),
        ("bpp:", "auto"),
        ("output:", "auto"),
        ("vsync:", "auto"),
        ("stretch:", "auto"),
    ] {
        draw_label_combo(&mut v, &mut l, label, val);
        l.row();
    }
    // The one live graphics control: stretch the LCD (fullscreen stretched).
    v.push(Ctrl::live(
        rc(l.at(), "stretch LCD to window"),
        chk("stretch LCD to window", s.stretch),
        Field::Stretch,
    ));
    v
}

fn system(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    let model = s.model;
    // Emulated-system radios: three live (Gameboy / Gameboy Color / automatic),
    // the SGB/auto-SGB variants inert (slopgb has no SGB system surface).
    let caption = "Emulated system (requires reset)";
    let radios: [(&str, Option<ModelChoice>); 8] = [
        ("Gameboy", Some(ModelChoice::Dmg)),
        ("Gameboy Color", Some(ModelChoice::Cgb)),
        ("Super Gameboy", None),
        ("automatic, prefer GBC", Some(ModelChoice::Auto)),
        ("automatic, prefer SGB", None),
        ("SGB + GBC", None),
        ("GBC + initial SGB border", None),
        ("Gameboy or GBC", None),
    ];
    // The groupbox encloses the caption row + all radios; width fits the widest
    // of the caption / radio labels so nothing renders past the border.
    let step = line_height() + 3;
    let dot = line_height() - 4;
    let widest = radios
        .iter()
        .map(|(lbl, _)| dot + 3 + measure(lbl))
        .chain(std::iter::once(measure(caption)))
        .max()
        .unwrap_or(0);
    // Radios are indented a few px so their dots sit inside the frame border.
    let indent = 6;
    let box_w = widest + indent + 10;
    let box_h = line_height() + radios.len() as i32 * step + 6;
    v.push(Ctrl::inert(
        Rect::new(l.x, l.y, box_w, box_h),
        Kind::GroupBox {
            label: caption,
            w: box_w,
            h: box_h,
        },
    ));
    let box_bottom = l.y + box_h;
    let rx = l.x0 + indent;
    let mut ry = l.y + line_height(); // first radio sits just below the caption
    for &(label, choice) in radios.iter() {
        let kind = Kind::Radio {
            selected: choice.is_some_and(|c| c == model),
            label,
        };
        match choice {
            Some(c) => v.push(Ctrl::live(rad((rx, ry), label), kind, Field::Model(c))),
            None => v.push(Ctrl::inert(rad((rx, ry), label), kind)),
        }
        ry += step;
    }
    // Inert system toggles (bgb black) — none wired in slopgb yet — below the box.
    l.x = l.x0;
    l.y = box_bottom + line_height() / 2;
    for label in [
        "automatic reset on system change",
        "Rewind enabled",
        "detect GB pocket / SGB2",
        "detect GBA",
        "GB Player",
        "Waitloop detection (fast)",
        "Save BGB legacy RTC files",
        "Save RTC in SAV file (VBA compatible)",
    ] {
        v.push(Ctrl::inert(rc(l.at(), label), chk(label, false)));
        l.row();
    }
    v
}

fn debug(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    // lowercase disassembler: slopgb's disasm is always no$gmb-lowercase, so this
    // reflects the fixed reality (inert, checked); lowercase-hex + show-clocks are live.
    v.push(Ctrl::inert(
        rc(l.at(), "lowercase disassembler"),
        chk("lowercase disassembler", s.lowercase_disasm),
    ));
    v.push(Ctrl::live(
        rc(l.row().at(), "lowercase hex"),
        chk("lowercase hex", s.lowercase_hex),
        Field::LowercaseHex,
    ));
    v.push(Ctrl::live(
        rc(l.row().at(), "show counted clocks"),
        chk("show counted clocks", s.show_clocks),
        Field::ShowClocks,
    ));
    l.row();
    // Inert / always-on debugger settings (bgb black; some checked by default).
    v.push(Ctrl::inert(
        rc(l.at(), "Registers can be edited"),
        chk("Registers can be edited", true),
    ));
    v.push(Ctrl::inert(
        rc(l.row().at(), "Live update memory viewer"),
        chk("Live update memory viewer", true),
    ));
    v.push(Ctrl::inert(
        rc(l.row().at(), "pressing Esc shows debugger"),
        chk("pressing Esc shows debugger", true),
    ));
    v.push(Ctrl::inert(
        rc(l.row().at(), "GB CPU usage meter"),
        chk("GB CPU usage meter", true),
    ));
    v.push(Ctrl::inert(
        rc(l.row().at(), "Start in debugger"),
        chk("Start in debugger", false),
    ));
    l.row();
    draw_label_combo(&mut v, &mut l, "Disasm syntax:", "no$gmb");
    v
}

fn exceptions(_s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    // All break-conditions are inert stubs (bgb black) — slopgb's core free-run
    // halts only on PC breakpoints / watchpoints, not on these opcode/access
    // conditions, so none is wired (faithful list from the capture). The bgb
    // capture shows "invalid opcode" checked by default, the rest unchecked.
    for (label, checked) in [
        ("break on OAM DMA bad accesses", false),
        ("break on 16 bits inc/dec FE00-FEFF", false),
        ("break on disabling LCD outside vblank", false),
        ("break on ram echo (E000-FDFF) access", false),
        ("break on SGB transfer start", false),
        ("break on ld b,b (40h)", false),
        ("break on invalid opcode", true),
    ] {
        v.push(Ctrl::inert(rc(l.at(), label), chk(label, checked)));
        l.row();
    }
    l.row();
    // Greyed sub-items bgb itself greys (accurate-emulation defaults locked).
    v.push(Ctrl::grey(
        rc(l.at(), "emulate locked ram (as in reality)"),
        chk("emulate locked ram (as in reality)", true),
    ));
    v.push(Ctrl::grey(
        rc(l.row().at(), "10 sprites per line limit (as in reality)"),
        chk("10 sprites per line limit (as in reality)", true),
    ));
    v
}

fn sound(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    draw_label_combo(&mut v, &mut l, "soundcard:", "auto");
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "8 bits output"),
        chk("8 bits output", false),
    ));
    v.push(Ctrl::live(
        rc(l.col(140).at(), "mono output"),
        chk("mono output", s.mono),
        Field::Mono,
    ));
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "High quality sound rendering"),
        chk("High quality sound rendering", true),
    ));
    l.row();
    l.row();
    // Samplerate radios (inert; device-driven in slopgb).
    let rates = ["Auto", "24000", "48000", "96000"];
    let mut cx = 0;
    for r in rates {
        v.push(Ctrl::inert(
            rad(l.col(cx).at(), r),
            Kind::Radio {
                selected: r == "Auto",
                label: r,
            },
        ));
        cx += measure(r) + 28;
    }
    l.row();
    l.row();
    // Live master volume slider.
    v.push(Ctrl::inert(
        rc(l.at(), "Volume:"),
        Kind::Label {
            text: "Volume:".into(),
        },
    ));
    v.push(Ctrl::live(
        Rect::new(l.x0 + 60, l.y, 180, line_height()),
        Kind::Slider {
            frac: s.volume,
            w: 180,
        },
        Field::Volume,
    ));
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "Latency:"),
        Kind::Label {
            text: "Latency:".into(),
        },
    ));
    v.push(Ctrl::inert(
        Rect::new(l.x0 + 60, l.y, 180, line_height()),
        Kind::Slider { frac: 0.5, w: 180 },
    ));
    v
}

fn gb_colors(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    // Four swatches of the live palette (lightest→darkest).
    for (i, c) in s.dmg_palette.iter().enumerate() {
        v.push(Ctrl::inert(
            Rect::new(l.x0 + i as i32 * 34, l.y, 30, 22),
            Kind::Swatch { color: *c },
        ));
    }
    l.y += 30;
    // Scheme dropdown — live, cycles through SCHEMES on click.
    v.push(Ctrl::inert(
        rc(l.at(), "Scheme:"),
        Kind::Label {
            text: "Scheme:".into(),
        },
    ));
    v.push(Ctrl::live(
        Rect::new(l.x0 + 60, l.y, 120, line_height() + 2),
        Kind::Dropdown {
            value: SCHEMES[s.scheme.min(SCHEMES.len() - 1)].name.to_string(),
            w: 120,
        },
        Field::SchemeCycle,
    ));
    l.row();
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "0-31 numbers"),
        chk("0-31 numbers", false),
    ));
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "DMG on GBC LCD colors"),
        chk("DMG on GBC LCD colors", false),
    ));
    l.row();
    // Contrast wheel (inert).
    v.push(Ctrl::inert(
        rc(l.at(), "Contrast wheel:"),
        Kind::Label {
            text: "Contrast wheel:".into(),
        },
    ));
    v.push(Ctrl::inert(
        Rect::new(l.x0 + 100, l.y, 140, line_height()),
        Kind::Slider { frac: 0.5, w: 140 },
    ));
    v
}

fn joypad(_s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    // Fully inert: faithful transcription of the capture (no slopgb backend).
    v.push(Ctrl::inert(
        Rect::new(l.x, l.y, 110, line_height() + 2),
        Kind::Dropdown {
            value: "joypad 0".into(),
            w: 110,
        },
    ));
    v.push(Ctrl::inert(
        rc(l.col(150).at(), "Screenshot button:"),
        Kind::Label {
            text: "Screenshot button: saves".into(),
        },
    ));
    l.row();
    l.row();
    for label in [
        "configure keyboard",
        "configure game controller",
        "clear game controller",
    ] {
        v.push(Ctrl::inert(
            Rect::new(l.x, l.y, 150, line_height() + 4),
            Kind::Button { label, w: 150 },
        ));
        l.row();
    }
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "configure extra buttons"),
        chk("configure extra buttons", false),
    ));
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "allow pressing L+R or U+D"),
        chk("allow pressing L+R or U+D", false),
    ));
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "Game controller works only if app has focus"),
        chk("Game controller works only if app has focus", true),
    ));
    l.row();
    v.push(Ctrl::inert(
        rc(l.at(), "Keyboard works only if app has focus"),
        chk("Keyboard works only if app has focus", true),
    ));
    v
}

fn misc(s: &Settings, content: Rect) -> Vec<Ctrl> {
    // Single column: slopgb's font is wide enough that bgb's two-column Misc
    // layout would overlap, so the checkboxes stack vertically (functional 1:1,
    // not pixel). "Load ROM dialog on startup" is inert — App settings are
    // in-memory only, so there is no persisted startup to honour.
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    let rows: [(&str, bool, Option<Field>); 7] = [
        ("Load ROM dialog on startup", false, None),
        (
            "freeze recent ROMs menu",
            s.freeze_recent,
            Some(Field::FreezeRecent),
        ),
        ("Show errors on ROM load", true, None),
        (
            "Show framerate",
            s.show_framerate,
            Some(Field::ShowFramerate),
        ),
        (
            "Pause if losing focus",
            s.pause_on_focus_loss,
            Some(Field::PauseOnFocusLoss),
        ),
        ("reduce CPU usage", true, None),
        ("Recovery save state", true, None),
    ];
    for (i, &(label, checked, field)) in rows.iter().enumerate() {
        if i > 0 {
            l.row();
        }
        let kind = chk(label, checked);
        match field {
            Some(f) => v.push(Ctrl::live(rc(l.at(), label), kind, f)),
            None => v.push(Ctrl::inert(rc(l.at(), label), kind)),
        }
    }
    // Live pacing sliders: a label on the left, the slider clear of it on the
    // right. NOTE: `framerate_limit` is consulted only by the timer-paced loop —
    // it has no effect while sound is on (audio-paced emulation must track the
    // native rate for correct pitch). `ff_speed` caps turbo frames-per-wake
    // (monotonic), not a true Nx wall-clock multiplier (turbo runs flat-out).
    let slider_x = l.x0 + 200;
    l.row();
    l.row();
    let fr_idx = FRAMERATE_STEPS
        .iter()
        .position(|&x| x == s.framerate_limit)
        .unwrap_or(0);
    let fr_frac = fr_idx as f32 / (FRAMERATE_STEPS.len() - 1) as f32;
    v.push(text_label(
        l.at(),
        format!("framerate (0 = real): {}", s.framerate_limit),
    ));
    v.push(Ctrl::live(
        Rect::new(slider_x, l.y, 110, line_height()),
        Kind::Slider {
            frac: fr_frac,
            w: 110,
        },
        Field::FramerateLimit,
    ));
    l.row();
    let ff_frac = s.ff_speed.saturating_sub(1) as f32 / (FF_SPEED_MAX - 1) as f32;
    v.push(text_label(
        l.at(),
        format!("fast forward speed: {}", s.ff_speed),
    ));
    v.push(Ctrl::live(
        Rect::new(slider_x, l.y, 110, line_height()),
        Kind::Slider {
            frac: ff_frac,
            w: 110,
        },
        Field::FfSpeed,
    ));
    v
}

// --- small builder helpers --------------------------------------------------

fn chk(label: &'static str, checked: bool) -> Kind {
    Kind::Check { checked, label }
}
/// checkbox hit-rect at a point.
fn rc((x, y): (i32, i32), label: &str) -> Rect {
    let box_sz = line_height() - 4;
    Rect::new(x, y, box_sz + 3 + measure(label), box_sz)
}
/// radio hit-rect at a point.
fn rad((x, y): (i32, i32), label: &str) -> Rect {
    let dot = line_height() - 4;
    Rect::new(x, y, dot + 3 + measure(label), dot)
}
/// An inert text label whose rect matches the rendered `text` width.
fn text_label((x, y): (i32, i32), text: String) -> Ctrl {
    let rect = Rect::new(x, y, measure(&text), line_height());
    Ctrl::inert(rect, Kind::Label { text })
}
/// Push a "label: [combo]" pair at the cursor (both inert).
fn draw_label_combo(v: &mut Vec<Ctrl>, l: &mut Lay, label: &'static str, val: &str) {
    let (x, y) = l.at();
    v.push(Ctrl::inert(
        Rect::new(x, y, measure(label), line_height()),
        Kind::Label {
            text: label.to_string(),
        },
    ));
    let cx = x + measure(label) + 6;
    v.push(Ctrl::inert(
        Rect::new(cx, y, 70, line_height() + 2),
        Kind::Dropdown {
            value: val.to_string(),
            w: 70,
        },
    ));
}
