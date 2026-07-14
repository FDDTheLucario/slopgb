//! Per-tab control builders for the Options dialog. Each `pub(super) fn`
//! builds the flat [`Ctrl`] list for one tab from the current [`Settings`];
//! the parent's `controls()` dispatches to them and `render()`/`apply()` read
//! the same list. Split out of `tabs.rs` to keep both files under the
//! 1000-line cap.

// The builders construct their control list imperatively with a layout cursor
// (`Lay`), so `Vec::new()` + `push` is the natural idiom here, not a missing
// `vec![]` literal.
#![allow(clippy::vec_init_then_push)]

use super::*;
use crate::windows::options::BootromSlot;

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

// --- Per-tab builders -------------------------------------------------------

pub(super) fn graphics(s: &Settings, content: Rect) -> Vec<Ctrl> {
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
    // "frame blend" is live: a label + a dropdown that cycles off ↔ on.
    let fb_label = "frame blend:";
    let (fx, fy) = l.at();
    v.push(text_label((fx, fy), fb_label.to_owned()));
    let fb_cx = fx + measure(fb_label) + 6;
    v.push(Ctrl::live(
        Rect::new(fb_cx, fy, 70, line_height() + 2),
        Kind::Dropdown {
            value: if s.frame_blend { "on" } else { "off" }.to_string(),
            w: 70,
        },
        Field::FrameBlend,
    ));
    l.row();
    for (label, val) in [
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

pub(super) fn system(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    let model = s.model;
    // Emulated-system radios — all live: slopgb has a full SGB system (border,
    // palettes, SPC700/S-DSP audio), so the SGB/auto-SGB variants each resolve
    // to a real model (see `ModelChoice::resolve`).
    let caption = "Emulated system (requires reset)";
    let radios: [(&str, ModelChoice); 8] = [
        ("Gameboy", ModelChoice::Dmg),
        ("Gameboy Color", ModelChoice::Cgb),
        ("Super Gameboy", ModelChoice::Sgb),
        ("automatic, prefer GBC", ModelChoice::Auto),
        ("automatic, prefer SGB", ModelChoice::AutoSgb),
        ("SGB + GBC", ModelChoice::Sgb2),
        ("GBC + initial SGB border", ModelChoice::CgbBorder),
        ("Gameboy or GBC", ModelChoice::AutoNoSgb),
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
            selected: choice == model,
            label,
        };
        v.push(Ctrl::live(rad((rx, ry), label), kind, Field::Model(choice)));
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
    // Live: bgb's `UninitedWRAM` (an ini-only key in bgb; surfaced here as a
    // checkbox). Power on with seeded-random garbage RAM; applied on next reset.
    let uw = "uninitialized RAM at power-on";
    v.push(Ctrl::live(
        rc(l.at(), uw),
        chk(uw, s.uninited_wram),
        Field::UninitedWram,
    ));
    l.row();
    // --- Boot ROM paths (right column; bgb's System tab, options-system.png) ---
    // Three labeled path fields each with a "..." browse button, then a live
    // "bootroms enabled" checkbox. The path box shows the configured path; the
    // "..." button opens the path modal (routes Field::PickBootrom out).
    let rx0 = l.x0 + box_w + 12; // just right of the emulated-system box
    // Fit the path box + 22px "..." button + a small margin within the content.
    let path_w = (content.x + content.w - rx0 - 30).clamp(40, 150);
    let mut by = content.y;
    for (slot, label) in [
        (BootromSlot::Dmg, "DMG bootrom:"),
        (BootromSlot::Gbc, "GBC bootrom:"),
        (BootromSlot::Sgb, "SGB bootrom:"),
    ] {
        v.push(text_label((rx0, by), label.to_owned()));
        by += line_height() + 1;
        v.push(Ctrl::inert(
            Rect::new(rx0, by, path_w, line_height() + 2),
            Kind::Dropdown {
                value: slot.path(s).to_owned(),
                w: path_w,
            },
        ));
        v.push(Ctrl::live(
            Rect::new(rx0 + path_w + 4, by, 22, line_height() + 2),
            Kind::Button {
                label: "...",
                w: 22,
            },
            Field::PickBootrom(slot),
        ));
        by += step + 4;
    }
    v.push(Ctrl::live(
        rc((rx0, by), "bootroms enabled"),
        chk("bootroms enabled", s.bootroms_enabled),
        Field::BootromsEnabled,
    ));
    v
}

pub(super) fn debug(s: &Settings, content: Rect) -> Vec<Ctrl> {
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
    v.push(Ctrl::live(
        rc(l.row().at(), "8-bit tile hex ($7F not $17F)"),
        chk("8-bit tile hex ($7F not $17F)", s.tile_hex_8bit),
        Field::TileHex8bit,
    ));
    v.push(Ctrl::live(
        rc(l.row().at(), "memory viewer in own window"),
        chk("memory viewer in own window", s.memory_window),
        Field::MemoryWindow,
    ));
    // "Pure bgb mode": one toggle that flips every slopgb-departure setting to its
    // bgb-faithful value (checked when already there).
    v.push(Ctrl::live(
        rc(l.row().at(), "pure bgb mode (no slopgb extras)"),
        chk("pure bgb mode (no slopgb extras)", pure_bgb(s)),
        Field::PureBgb,
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
    v.push(Ctrl::live(
        rc(l.row().at(), "pressing Esc shows debugger"),
        chk("pressing Esc shows debugger", s.esc_shows_debugger),
        Field::EscShowsDebugger,
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
    // The disassembler dialect: a live dropdown (click cycles rgbds ↔ no$gmb).
    // The single control for the syntax — there is no separate checkbox.
    let (x, y) = l.at();
    let label = "Disasm syntax:";
    v.push(Ctrl::inert(
        Rect::new(x, y, measure(label), line_height()),
        Kind::Label {
            text: label.to_string(),
        },
    ));
    let cx = x + measure(label) + 6;
    v.push(Ctrl::live(
        Rect::new(cx, y, 70, line_height() + 2),
        Kind::Dropdown {
            value: if s.rgbds_disasm { "rgbds" } else { "no$gmb" }.to_string(),
            w: 70,
        },
        Field::RgbdsDisasm,
    ));
    v
}

pub(super) fn exceptions(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    // Four break conditions are wired to the core exception-break mask (the
    // free run halts when armed + the debugger is open); the rest stay
    // faithfully inert (bgb black) — no clean golden-safe detector / no backend.
    // Each entry: (label, live field, checked-from-settings).
    let rows: [(&str, Option<Field>, bool); 7] = [
        ("break on OAM DMA bad accesses", None, false),
        ("break on 16 bits inc/dec FE00-FEFF", None, false),
        (
            "break on disabling LCD outside vblank",
            Some(Field::BreakLcdOffVblank),
            s.break_lcd_off_vblank,
        ),
        (
            "break on ram echo (E000-FDFF) access",
            Some(Field::BreakEchoRam),
            s.break_echo_ram,
        ),
        ("break on SGB transfer start", None, false),
        (
            "break on ld b,b (40h)",
            Some(Field::BreakLdBB),
            s.break_ld_b_b,
        ),
        (
            "break on invalid opcode",
            Some(Field::BreakInvalidOp),
            s.break_invalid_op,
        ),
    ];
    for (label, field, checked) in rows {
        let ctrl = match field {
            Some(f) => Ctrl::live(rc(l.at(), label), chk(label, checked), f),
            None => Ctrl::inert(rc(l.at(), label), chk(label, checked)),
        };
        v.push(ctrl);
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

pub(super) fn sound(s: &Settings, content: Rect) -> Vec<Ctrl> {
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
    l.row();
    l.row();
    // SGB audio backend (a slopgb extra) — dropdown, cycles Built-in ↔ SGB
    // coprocessor on click. Drives the same seam as `--sgb-coprocessor`.
    v.push(Ctrl::inert(
        rc(l.at(), "SGB audio:"),
        Kind::Label {
            text: "SGB audio:".into(),
        },
    ));
    v.push(Ctrl::live(
        Rect::new(l.x0 + 90, l.y, 150, line_height() + 2),
        Kind::Dropdown {
            value: s.audio_backend.label().to_string(),
            w: 150,
        },
        Field::AudioBackend,
    ));
    v
}

pub(super) fn gb_colors(s: &Settings, content: Rect) -> Vec<Ctrl> {
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
    v.push(Ctrl::live(
        rc(l.at(), "DMG on GBC LCD colors"),
        chk("DMG on GBC LCD colors", s.dmg_gbc_lcd),
        Field::DmgGbcLcd,
    ));
    l.row();
    // Contrast wheel — live slider over the present-side contrast filter.
    v.push(Ctrl::inert(
        rc(l.at(), "Contrast wheel:"),
        Kind::Label {
            text: "Contrast wheel:".into(),
        },
    ));
    v.push(Ctrl::live(
        Rect::new(l.x0 + 100, l.y, 140, line_height()),
        Kind::Slider {
            frac: s.contrast,
            w: 140,
        },
        Field::Contrast,
    ));
    v
}

pub(super) fn joypad(s: &Settings, content: Rect) -> Vec<Ctrl> {
    // Single column: like the Misc tab, slopgb's font is wide enough that bgb's
    // two-column Joypad layout (`options-joypad.png`) would overlap (the long
    // "configure game controller" / focus-check labels span the whole width), so
    // the controls stack vertically — functional 1:1, not pixel. The two live
    // controls are "configure keyboard" (the key-rebind wizard) and "allow
    // pressing L+R or U+D" (SOCD toggle); the rest is inert (no gamepad /
    // WAV-AVI recording / joystick backend under the winit/softbuffer/cpal rule).
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    let btn = |label: &'static str, w: i32| Kind::Button { label, w };

    // The joypad selector.
    v.push(Ctrl::inert(
        Rect::new(l.x, l.y, 110, line_height() + 2),
        Kind::Dropdown {
            value: "joypad 0".into(),
            w: 110,
        },
    ));
    l.row();
    // "configure keyboard" is live — it opens the key-rebind wizard.
    v.push(Ctrl::live(
        Rect::new(l.x, l.y, 150, line_height() + 4),
        btn("configure keyboard", 150),
        Field::ConfigureKeyboard,
    ));
    l.row();
    for label in ["configure game controller", "clear game controller"] {
        v.push(Ctrl::inert(
            Rect::new(l.x, l.y, 180, line_height() + 4),
            btn(label, 180),
        ));
        l.row();
    }
    v.push(Ctrl::inert(
        rc(l.at(), "configure extra buttons"),
        chk("configure extra buttons", false),
    ));
    l.row();

    // The inert recording/screenshot/rapid-speed combos, each on its own row.
    draw_label_combo(&mut v, &mut l, "Screenshot button:", "saves");
    l.row();
    draw_label_combo(&mut v, &mut l, "Screenshots:", "bmp");
    l.row();
    draw_label_combo(&mut v, &mut l, "Rapid speed:", "2 2");
    l.row();

    // Mappable button records groupbox (Audio / Video / Audio channels).
    let group_h = 2 * line_height() + 14;
    let (gx, gy) = (l.x, l.y);
    v.push(Ctrl::inert(
        Rect::new(gx, gy, 200, group_h),
        Kind::GroupBox {
            label: "Mappable button records",
            w: 200,
            h: group_h,
        },
    ));
    let grow = gy + line_height() + 2;
    v.push(Ctrl::inert(rc((gx + 8, grow), "Audio"), chk("Audio", true)));
    v.push(Ctrl::inert(
        rc((gx + 80, grow), "Video"),
        chk("Video", true),
    ));
    v.push(Ctrl::inert(
        rc((gx + 8, grow + line_height()), "Audio channels"),
        chk("Audio channels", false),
    ));
    l.y += group_h + 2;

    // The MBC7 joystick-ID field (parenthetical game name dropped to fit slopgb's
    // wider font — the control's function is unchanged, still inert).
    v.push(Ctrl::inert(
        Rect::new(l.x, l.y, 24, line_height() + 2),
        Kind::Button { label: "0", w: 24 },
    ));
    v.push(text_label(
        (l.x + 32, l.y + 1),
        "use joystick (ID) for MBC7".to_string(),
    ));
    l.row();

    // "allow pressing L+R or U+D" is live — the SOCD filter toggle.
    v.push(Ctrl::live(
        rc(l.at(), "allow pressing L+R or U+D"),
        chk("allow pressing L+R or U+D", s.allow_opposing),
        Field::AllowOpposing,
    ));
    l.row();
    // The focus checkboxes (inert — winit only delivers keys to the focused
    // window, so these are always effectively checked).
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

pub(super) fn misc(s: &Settings, content: Rect) -> Vec<Ctrl> {
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

pub(super) fn theme_tab(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    v.push(text_label(l.at(), "UI colour theme:".to_owned()));
    // The three built-in themes as radios; a `Custom` theme (config-only) selects
    // none of them and is noted below instead. ponytail: built-ins only —
    // enumerate CustomThemes here if a named custom theme ever needs its own radio.
    for (label, choice) in [
        ("Light", ThemeRadio::Light),
        ("Dark", ThemeRadio::Dark),
        ("Classic", ThemeRadio::Classic),
    ] {
        l.row();
        let selected = match choice {
            ThemeRadio::Light => matches!(s.theme, ThemeChoice::Light),
            ThemeRadio::Dark => matches!(s.theme, ThemeChoice::Dark),
            ThemeRadio::Classic => matches!(s.theme, ThemeChoice::Classic),
        };
        let kind = Kind::Radio { selected, label };
        v.push(Ctrl::live(rad(l.at(), label), kind, Field::Theme(choice)));
    }
    if let ThemeChoice::Custom(name) = &s.theme {
        l.row();
        v.push(text_label(l.at(), format!("(custom theme active: {name})")));
    }
    v
}

pub(super) fn plugins(s: &Settings, content: Rect) -> Vec<Ctrl> {
    let mut l = Lay::new(content);
    let mut v = Vec::new();
    // The scanned plugins directory (read-only; set via --plugins / the config
    // file — there is no in-dialog browse yet).
    let dir = if s.plugins.dir.is_empty() {
        "(none)".to_owned()
    } else {
        s.plugins.dir.clone()
    };
    v.push(text_label(l.at(), format!("Plugins dir: {dir}")));
    l.row();
    // Live: allow mutation-capable plugins (default off, golden-safe).
    v.push(Ctrl::live(
        rc(l.at(), "allow plugin mutation"),
        chk("allow plugin mutation", s.plugins.allow_mutation),
        Field::PluginAllowMutation,
    ));
    l.row();
    l.row();
    // One live enable checkbox per discovered plugin. The plugin's "name [caps]"
    // is a dynamic string, so it is drawn as a separate inert label beside a
    // static-empty checkbox (the shared `Kind::Check` label is `&'static str`);
    // the checkbox hit-rect still spans the whole row so a click on the name
    // toggles it. An empty list shows an inert note.
    if s.plugins.entries.is_empty() {
        v.push(text_label(l.at(), "(no plugins discovered)".to_owned()));
    } else {
        let box_sz = line_height() - 4;
        for (i, e) in s.plugins.entries.iter().enumerate() {
            let (x, y) = l.at();
            let label = format!("{} [{}]", e.name, e.capabilities);
            let w = box_sz + 3 + measure(&label);
            v.push(Ctrl::live(
                Rect::new(x, y, w, box_sz),
                Kind::Check {
                    checked: e.enabled,
                    label: "",
                },
                Field::PluginEnable(i),
            ));
            v.push(text_label((x + box_sz + 3, y), label));
            l.row();
        }
    }
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
/// Push a `label: combo` pair at the cursor (both inert).
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
