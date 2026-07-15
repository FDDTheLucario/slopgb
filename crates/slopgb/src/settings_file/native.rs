//! slopgb's native settings format: a versioned, sectioned,
//! std-only text file (`slopgb.conf`) — the default store; bgb.ini is demoted to
//! import/export. Human-editable + git-diffable, forward/back compatible:
//! unknown keys/sections are preserved, missing keys default, `version` drives
//! future migrations.
//!
//! ```text
//! version = 1
//! [system]
//! model = auto        # auto | dmg | cgb
//! [sound]
//! volume = 1.0
//! [graphics]
//! palette = 0xE8FCCC, 0x90D4AC, 0x708C54, 0x382C14
//! [recent]
//! 0 = /roms/game.gbc
//! ```

use crate::ui::{CustomThemes, Theme, ThemeChoice};
use crate::windows::options::{
    AudioBackend, ModelChoice, PluginConfig, SCHEMES, ScreenshotFormat, Settings,
};

/// The current native-format version (bumped when a migration is needed).
pub const VERSION: u32 = 1;

/// One physical line: a `[section]` header, a `key = value` pair tagged with its
/// section, or a verbatim passthrough (comment / blank / unparseable).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Line {
    Section(String),
    Pair {
        section: String,
        key: String,
        val: String,
    },
    Raw(String),
}

/// A parsed native config, preserving line order + unknown keys/sections.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    lines: Vec<Line>,
}

impl Doc {
    /// Parse `text`. Top-level pairs (before any `[section]`) get section `""`.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut lines = Vec::new();
        let mut section = String::new();
        for raw in text.lines() {
            let t = raw.trim();
            if t.is_empty() || t.starts_with('#') {
                lines.push(Line::Raw(raw.to_string()));
            } else if let Some(name) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = name.trim().to_string();
                lines.push(Line::Section(section.clone()));
            } else if let Some((k, v)) = t.split_once('=') {
                lines.push(Line::Pair {
                    section: section.clone(),
                    key: k.trim().to_string(),
                    val: v.trim().to_string(),
                });
            } else {
                lines.push(Line::Raw(raw.to_string()));
            }
        }
        Self { lines }
    }

    /// Serialize back to text (LF-terminated lines).
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                Line::Section(s) => out.push_str(&format!("[{s}]")),
                Line::Pair { key, val, .. } => out.push_str(&format!("{key} = {val}")),
                Line::Raw(r) => out.push_str(r),
            }
            out.push('\n');
        }
        out
    }

    /// Value of `key` in `section` (`""` = top-level), or `None`.
    #[must_use]
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|l| match l {
            Line::Pair {
                section: s,
                key: k,
                val,
            } if s == section && k == key => Some(val.as_str()),
            _ => None,
        })
    }

    /// All `(key, value)` pairs in `section`, in order (for `[recent]`).
    #[must_use]
    pub fn section_pairs(&self, section: &str) -> Vec<(&str, &str)> {
        self.lines
            .iter()
            .filter_map(|l| match l {
                Line::Pair {
                    section: s,
                    key,
                    val,
                } if s == section => Some((key.as_str(), val.as_str())),
                _ => None,
            })
            .collect()
    }

    /// Every distinct `[section]` name that starts with `prefix`, in file
    /// order — e.g. `"theme."` finds every `[theme.NAME]` custom-theme
    /// section without the names being known ahead of time. Borrows from
    /// `self` (no cloning) like [`Self::section_pairs`].
    #[must_use]
    pub fn section_names_with_prefix(&self, prefix: &str) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for line in &self.lines {
            if let Line::Section(s) = line {
                if s.starts_with(prefix) && !out.contains(&s.as_str()) {
                    out.push(s.as_str());
                }
            }
        }
        out
    }

    /// Set `key` in `section`: overwrite in place, else append at the end of the
    /// section (creating the section header if absent). Preserves everything else.
    pub fn set(&mut self, section: &str, key: &str, val: &str) {
        for line in &mut self.lines {
            if let Line::Pair {
                section: s,
                key: k,
                val: v,
            } = line
            {
                if s == section && k == key {
                    *v = val.to_string();
                    return;
                }
            }
        }
        let pair = Line::Pair {
            section: section.to_string(),
            key: key.to_string(),
            val: val.to_string(),
        };
        // Insert after the section's last line (header or pair), else create it.
        let last = self.lines.iter().rposition(|l| match l {
            Line::Section(s) => s == section,
            Line::Pair { section: s, .. } => s == section,
            Line::Raw(_) => false,
        });
        match last {
            Some(i) => self.lines.insert(i + 1, pair),
            None => {
                if !section.is_empty() {
                    self.lines.push(Line::Section(section.to_string()));
                }
                self.lines.push(pair);
            }
        }
    }

    /// Replace the whole `[recent]` section with `paths` (numbered `0..`),
    /// dropping any prior recent entries.
    fn set_recent(&mut self, paths: &[String]) {
        self.lines
            .retain(|l| !matches!(l, Line::Pair { section, .. } if section == "recent"));
        for (i, p) in paths.iter().enumerate() {
            self.set("recent", &i.to_string(), p);
        }
    }
}

// --- typed codecs (native encodings: true/false, 0xRRGGBB) -----------------

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn fmt_hex(xrgb: u32) -> String {
    format!("0x{:06X}", xrgb & 0xFF_FFFF)
}

fn parse_hex(v: &str) -> Option<u32> {
    let h = v.trim().trim_start_matches("0x").trim_start_matches("0X");
    u32::from_str_radix(h, 16).ok()
}

// --- Settings <-> Doc ------------------------------------------------------

/// Read `Settings` from a parsed native doc; any absent/unparseable key takes its
/// `Settings::default()` value. Unknown `version`/keys are ignored (preserved on
/// the next save).
#[must_use]
pub fn from_doc(d: &Doc) -> (Settings, Vec<String>) {
    let def = Settings::default();
    let b = |sec: &str, k: &str, dv: bool| d.get(sec, k).and_then(parse_bool).unwrap_or(dv);
    let i = |sec: &str, k: &str, dv: i64| {
        d.get(sec, k)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(dv)
    };
    let s =
        |sec: &str, k: &str, dv: &str| d.get(sec, k).map_or_else(|| dv.to_string(), str::to_string);

    // palette: 4 comma-separated hex; fall back to default if malformed.
    let mut dmg_palette = def.dmg_palette;
    if let Some(list) = d.get("graphics", "palette") {
        let parsed: Vec<u32> = list.split(',').filter_map(parse_hex).collect();
        if parsed.len() == 4 {
            dmg_palette.copy_from_slice(&parsed);
        }
    }
    let scheme = SCHEMES
        .iter()
        .position(|s| s.colors == dmg_palette)
        .unwrap_or(def.scheme);

    let settings = Settings {
        model: match d.get("system", "model") {
            Some("dmg") => ModelChoice::Dmg,
            Some("cgb") => ModelChoice::Cgb,
            Some("sgb") => ModelChoice::Sgb,
            Some("sgb2") => ModelChoice::Sgb2,
            Some("auto-sgb") => ModelChoice::AutoSgb,
            Some("cgb-border") => ModelChoice::CgbBorder,
            Some("auto-nosgb") => ModelChoice::AutoNoSgb,
            _ => ModelChoice::Auto,
        },
        stretch: b("graphics", "stretch", def.stretch),
        frame_blend: b("graphics", "frame_blend", def.frame_blend),
        dmg_gbc_lcd: b("graphics", "dmg_gbc_lcd", def.dmg_gbc_lcd),
        contrast: d
            .get("graphics", "contrast")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(def.contrast),
        sgb_border_screenshot: b(
            "graphics",
            "sgb_border_screenshot",
            def.sgb_border_screenshot,
        ),
        screenshot_format: d
            .get("misc", "screenshot_format")
            .map_or(def.screenshot_format, ScreenshotFormat::from_key),
        screenshot_copies: b("misc", "screenshot_copies", def.screenshot_copies),
        volume: d
            .get("sound", "volume")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(def.volume),
        mono: b("sound", "mono", def.mono),
        audio_backend: d
            .get("sound", "audio_backend")
            .map_or(def.audio_backend, AudioBackend::from_key),
        lowercase_disasm: b("debug", "lowercase_disasm", def.lowercase_disasm),
        lowercase_hex: b("debug", "lowercase_hex", def.lowercase_hex),
        show_clocks: b("debug", "show_clocks", def.show_clocks),
        rgbds_disasm: b("debug", "rgbds_disasm", def.rgbds_disasm),
        tile_hex_8bit: b("debug", "tile_hex_8bit", def.tile_hex_8bit),
        memory_window: b("debug", "memory_window", def.memory_window),
        esc_shows_debugger: b("debug", "esc_shows_debugger", def.esc_shows_debugger),
        registers_editable: b("debug", "registers_editable", def.registers_editable),
        start_in_debugger: b("debug", "start_in_debugger", def.start_in_debugger),
        mem_live_update: b("debug", "mem_live_update", def.mem_live_update),
        cpu_usage_meter: b("debug", "cpu_usage_meter", def.cpu_usage_meter),
        ff_speed: i("misc", "ff_speed", i64::from(def.ff_speed)).clamp(1, 20) as u32,
        framerate_limit: i("misc", "framerate_limit", i64::from(def.framerate_limit)).max(0) as u32,
        show_framerate: b("misc", "show_framerate", def.show_framerate),
        freeze_recent: b("misc", "freeze_recent", def.freeze_recent),
        pause_on_focus_loss: b("misc", "pause_on_focus_loss", def.pause_on_focus_loss),
        show_errors_on_rom_load: b(
            "misc",
            "show_errors_on_rom_load",
            def.show_errors_on_rom_load,
        ),
        load_rom_dialog_on_startup: b(
            "misc",
            "load_rom_dialog_on_startup",
            def.load_rom_dialog_on_startup,
        ),
        reduce_cpu: b("misc", "reduce_cpu", def.reduce_cpu),
        scheme,
        dmg_palette,
        allow_opposing: b("misc", "allow_opposing", def.allow_opposing),
        uninited_wram: b("system", "uninited_wram", def.uninited_wram),
        break_ld_b_b: b("exceptions", "break_ld_b_b", def.break_ld_b_b),
        break_invalid_op: b("exceptions", "break_invalid_op", def.break_invalid_op),
        break_echo_ram: b("exceptions", "break_echo_ram", def.break_echo_ram),
        break_lcd_off_vblank: b(
            "exceptions",
            "break_lcd_off_vblank",
            def.break_lcd_off_vblank,
        ),
        bootroms_enabled: b("system", "bootroms_enabled", def.bootroms_enabled),
        bootrom_dmg: s("system", "bootrom_dmg", &def.bootrom_dmg),
        bootrom_gbc: s("system", "bootrom_gbc", &def.bootrom_gbc),
        bootrom_sgb: s("system", "bootrom_sgb", &def.bootrom_sgb),
        theme: d
            .get("ui", "theme")
            .map(ThemeChoice::from_key)
            .unwrap_or_default(),
        plugins: PluginConfig::from_persisted(
            s("plugins", "dir", ""),
            b("plugins", "allow_mutation", false),
            d.get("plugins", "disabled").unwrap_or(""),
        ),
    };
    let recent = d
        .section_pairs("recent")
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(_, v)| v.to_string())
        .collect();
    (settings, recent)
}

/// Update `d` in place from `settings` + `recent`: set the version + every mapped
/// key, preserving any unknown keys/sections already present.
pub fn to_doc(settings: &Settings, recent: &[String], d: &mut Doc) {
    let Settings {
        model: _,
        stretch: _,
        frame_blend: _,
        dmg_gbc_lcd: _,
        contrast: _,
        sgb_border_screenshot: _,
        screenshot_format: _,
        screenshot_copies: _,
        volume: _,
        mono: _,
        audio_backend: _,
        lowercase_disasm: _,
        lowercase_hex: _,
        show_clocks: _,
        rgbds_disasm: _,
        tile_hex_8bit: _,
        memory_window: _,
        esc_shows_debugger: _,
        registers_editable: _,
        start_in_debugger: _,
        mem_live_update: _,
        cpu_usage_meter: _,
        ff_speed: _,
        framerate_limit: _,
        show_framerate: _,
        freeze_recent: _,
        pause_on_focus_loss: _,
        show_errors_on_rom_load: _,
        load_rom_dialog_on_startup: _,
        reduce_cpu: _,
        scheme: _,
        dmg_palette: _,
        allow_opposing: _,
        uninited_wram: _,
        break_ld_b_b: _,
        break_invalid_op: _,
        break_echo_ram: _,
        break_lcd_off_vblank: _,
        bootroms_enabled: _,
        bootrom_dmg: _,
        bootrom_gbc: _,
        bootrom_sgb: _,
        theme: _,
        plugins: _,
    } = settings;
    let fb = |b: bool| if b { "true" } else { "false" };
    d.set("", "version", &VERSION.to_string());
    d.set(
        "system",
        "model",
        match settings.model {
            ModelChoice::Dmg => "dmg",
            ModelChoice::Cgb => "cgb",
            ModelChoice::Sgb => "sgb",
            ModelChoice::Sgb2 => "sgb2",
            ModelChoice::Auto => "auto",
            ModelChoice::AutoSgb => "auto-sgb",
            ModelChoice::CgbBorder => "cgb-border",
            ModelChoice::AutoNoSgb => "auto-nosgb",
        },
    );
    d.set("system", "bootroms_enabled", fb(settings.bootroms_enabled));
    d.set("system", "bootrom_dmg", &settings.bootrom_dmg);
    d.set("system", "bootrom_gbc", &settings.bootrom_gbc);
    d.set("system", "bootrom_sgb", &settings.bootrom_sgb);
    d.set("sound", "volume", &settings.volume.to_string());
    d.set("sound", "mono", fb(settings.mono));
    d.set("sound", "audio_backend", settings.audio_backend.to_key());
    d.set("graphics", "stretch", fb(settings.stretch));
    d.set("graphics", "frame_blend", fb(settings.frame_blend));
    d.set("graphics", "dmg_gbc_lcd", fb(settings.dmg_gbc_lcd));
    d.set("graphics", "contrast", &settings.contrast.to_string());
    d.set(
        "graphics",
        "sgb_border_screenshot",
        fb(settings.sgb_border_screenshot),
    );
    d.set(
        "misc",
        "screenshot_format",
        settings.screenshot_format.ext(),
    );
    d.set("misc", "screenshot_copies", fb(settings.screenshot_copies));
    let palette = settings
        .dmg_palette
        .iter()
        .map(|&c| fmt_hex(c))
        .collect::<Vec<_>>()
        .join(", ");
    d.set("graphics", "palette", &palette);
    d.set("debug", "lowercase_disasm", fb(settings.lowercase_disasm));
    d.set("debug", "lowercase_hex", fb(settings.lowercase_hex));
    d.set("debug", "show_clocks", fb(settings.show_clocks));
    d.set("debug", "rgbds_disasm", fb(settings.rgbds_disasm));
    d.set("debug", "tile_hex_8bit", fb(settings.tile_hex_8bit));
    d.set("debug", "memory_window", fb(settings.memory_window));
    d.set(
        "debug",
        "esc_shows_debugger",
        fb(settings.esc_shows_debugger),
    );
    d.set(
        "debug",
        "registers_editable",
        fb(settings.registers_editable),
    );
    d.set("debug", "start_in_debugger", fb(settings.start_in_debugger));
    d.set("debug", "mem_live_update", fb(settings.mem_live_update));
    d.set("debug", "cpu_usage_meter", fb(settings.cpu_usage_meter));
    d.set("misc", "ff_speed", &settings.ff_speed.to_string());
    d.set(
        "misc",
        "framerate_limit",
        &settings.framerate_limit.to_string(),
    );
    d.set("misc", "show_framerate", fb(settings.show_framerate));
    d.set("misc", "freeze_recent", fb(settings.freeze_recent));
    d.set(
        "misc",
        "pause_on_focus_loss",
        fb(settings.pause_on_focus_loss),
    );
    d.set(
        "misc",
        "show_errors_on_rom_load",
        fb(settings.show_errors_on_rom_load),
    );
    d.set(
        "misc",
        "load_rom_dialog_on_startup",
        fb(settings.load_rom_dialog_on_startup),
    );
    d.set("misc", "reduce_cpu", fb(settings.reduce_cpu));
    d.set("misc", "allow_opposing", fb(settings.allow_opposing));
    d.set("system", "uninited_wram", fb(settings.uninited_wram));
    d.set("exceptions", "break_ld_b_b", fb(settings.break_ld_b_b));
    d.set(
        "exceptions",
        "break_invalid_op",
        fb(settings.break_invalid_op),
    );
    d.set("exceptions", "break_echo_ram", fb(settings.break_echo_ram));
    d.set(
        "exceptions",
        "break_lcd_off_vblank",
        fb(settings.break_lcd_off_vblank),
    );
    d.set("ui", "theme", &settings.theme.to_key());
    d.set("plugins", "dir", &settings.plugins.dir);
    d.set(
        "plugins",
        "allow_mutation",
        fb(settings.plugins.allow_mutation),
    );
    d.set("plugins", "disabled", &settings.plugins.disabled_joined());
    d.set_recent(recent);
}

/// Every `[theme.NAME]` section as a registered custom theme (the theming
/// API): each section's `role = 0xRRGGBB` pairs feed [`Theme::from_pairs`]. A
/// section with an unknown role or a bad value is skipped (logged, not
/// fatal) so one bad custom theme can't break loading the rest.
#[must_use]
pub fn custom_themes(d: &Doc) -> CustomThemes {
    let mut out = CustomThemes::default();
    for section in d.section_names_with_prefix("theme.") {
        let Some(name) = section.strip_prefix("theme.").filter(|n| !n.is_empty()) else {
            continue;
        };
        match Theme::from_pairs(&d.section_pairs(section)) {
            Ok(theme) => out.insert(name, theme),
            Err(e) => eprintln!("slopgb: custom theme '{name}' in [{section}]: {e}"),
        }
    }
    out
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
