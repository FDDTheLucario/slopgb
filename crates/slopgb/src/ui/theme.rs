//! The active colour palette for every bgb-style tool window: a flat table of
//! named colour roles (`u32` XRGB8888, `0x00RRGGBB`) with **no geometry** — so
//! swapping a [`Theme`] can only ever recolor pixels, never move or resize a
//! rect (the paramount look-only constraint of the theming feature: see
//! `docs/ui-state/theming.md`). Four built-in palettes
//! ([`Theme::BGB`]/[`Theme::CLASSIC`] identical to slopgb's original "Windows
//! 3" look, plus the contemporary [`Theme::LIGHT`] and [`Theme::DARK`]) plus a
//! tiny parser ([`Theme::from_pairs`]) for user-defined themes.
//!
//! [`ThemeChoice`] is the persisted *choice* (`Settings::theme`); resolving it
//! against the built-ins or a loaded [`CustomThemes`] registry yields the
//! concrete [`Theme`] the UI actually draws with.

use std::fmt;

/// One named colour role, XRGB8888 (`0x00RRGGBB`) — the entire visual
/// vocabulary every tool window draws with. Every field here is a colour;
/// nothing in this struct has an x/y/w/h, so a `Theme` swap can never move or
/// resize anything a widget draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Window / dialog background.
    pub bg: u32,
    /// Body text / ink.
    pub text: u32,
    /// The debugger's current-PC (or current-SP) row highlight.
    pub current: u32,
    /// A breakpoint marker.
    pub breakpoint: u32,
    /// General-purpose mid-tone highlight (bgb's grey `808080`).
    pub hilight: u32,
    /// Break/locked highlight — consumed when the debugger gains its break
    /// state; part of bgb's documented palette.
    pub freeze: u32,
    /// Frame / outline colour (dialog borders, box outlines).
    pub border: u32,
    /// Secondary background for a "recessed" content area (an input field) —
    /// distinct from the window `bg` in a modern theme.
    pub panel: u32,
    /// Button fill (unpressed).
    pub button_face: u32,
    /// The one tasteful "brand" accent — a checkbox/radio's checked mark.
    pub accent: u32,
    /// Hovered/selected menu-row background — distinct from [`Self::current`]
    /// (the debugger's current-PC-row highlight), so a custom theme can tell
    /// "a menu row is hovered" and "this is the current instruction" apart.
    pub selection_bg: u32,
    /// Text drawn over [`Self::selection_bg`].
    pub selection_fg: u32,
    /// Greyed-out / inert control text (bgb's "unavailable option" colour).
    pub disabled_text: u32,
    /// Scrollbar thumb colour.
    pub scrollbar: u32,
}

impl Theme {
    /// bgb's stock debugger palette, as XRGB8888 (`0x00RRGGBB`). The original
    /// 7 values are bgb's `bgb.ini` defaults converted from Windows
    /// `COLORREF` (`0x00BBGGRR`): bg white `FFFFFF`, text black, current-PC
    /// line blue, breakpoint red, hilight grey, freeze/locked yellow. Every
    /// new role is filled with the value the pre-theming draw code used at
    /// that call site, so pointing a call site at the new role instead of the
    /// old one is a no-op for this preset (pixel-identical to before the
    /// theming feature).
    pub const BGB: Theme = Theme {
        bg: 0x00FF_FFFF,
        text: 0x0000_0000,
        current: 0x0000_00FF,
        breakpoint: 0x00FF_0000,
        hilight: 0x0080_8080,
        freeze: 0x00FF_FF00,
        border: 0x0080_8080,
        panel: 0x00FF_FFFF,         // == bg
        button_face: 0x00FF_FFFF,   // == bg
        accent: 0x0000_0000,        // == text
        selection_bg: 0x0000_00FF,  // == current
        selection_fg: 0x00FF_FFFF,  // == bg
        disabled_text: 0x0080_8080, // == hilight
        scrollbar: 0x0080_8080,     // == hilight
    };

    /// The classic "Windows 3" bgb look, offered as a selectable
    /// [`ThemeChoice`] — identical to [`Self::BGB`].
    pub const CLASSIC: Theme = Theme::BGB;

    /// A contemporary flat light palette: soft neutral background, near-black
    /// text, one blue accent, flat (non-bevelled) borders.
    pub const LIGHT: Theme = Theme {
        bg: 0x00F5_F5F7,
        text: 0x0020_2124,
        current: 0x001A_73E8,
        breakpoint: 0x00D9_3025,
        hilight: 0x009A_A0A6,
        freeze: 0x00F9_AB00,
        border: 0x00DA_DCE0,
        panel: 0x00FF_FFFF,
        button_face: 0x00ED_EDF0,
        accent: 0x001A_73E8,
        selection_bg: 0x001A_73E8,
        selection_fg: 0x00FF_FFFF,
        disabled_text: 0x00BD_C1C6,
        scrollbar: 0x00C4_C7C5,
    };

    /// A contemporary flat dark palette: dark background, light text, the
    /// same blue accent family as [`Self::LIGHT`], flat borders.
    pub const DARK: Theme = Theme {
        bg: 0x0020_2124,
        text: 0x00E8_EAED,
        current: 0x008A_B4F8,
        breakpoint: 0x00F2_8B82,
        hilight: 0x009A_A0A6,
        freeze: 0x00FD_D663,
        border: 0x005F_6368,
        panel: 0x002D_2E31,
        button_face: 0x003C_4043,
        accent: 0x008A_B4F8,
        selection_bg: 0x008A_B4F8,
        selection_fg: 0x0020_2124,
        disabled_text: 0x005F_6368,
        scrollbar: 0x0080_868B,
    };

    /// Every role name [`Self::from_pairs`] recognizes.
    fn role_mut(&mut self, name: &str) -> Option<&mut u32> {
        Some(match name {
            "bg" => &mut self.bg,
            "text" => &mut self.text,
            "current" => &mut self.current,
            "breakpoint" => &mut self.breakpoint,
            "hilight" => &mut self.hilight,
            "freeze" => &mut self.freeze,
            "border" => &mut self.border,
            "panel" => &mut self.panel,
            "button_face" => &mut self.button_face,
            "accent" => &mut self.accent,
            "selection_bg" => &mut self.selection_bg,
            "selection_fg" => &mut self.selection_fg,
            "disabled_text" => &mut self.disabled_text,
            "scrollbar" => &mut self.scrollbar,
            _ => return None,
        })
    }

    /// Parse `0xRRGGBB` / `0XRRGGBB` (also a bare `RRGGBB`); `None` if it's
    /// not exactly 6 hex digits.
    fn parse_hex(v: &str) -> Option<u32> {
        let t = v.trim();
        let h = t
            .strip_prefix("0x")
            .or_else(|| t.strip_prefix("0X"))
            .unwrap_or(t);
        if h.len() != 6 {
            return None;
        }
        u32::from_str_radix(h, 16).ok()
    }

    /// Build a [`Theme`] from `(role, "0xRRGGBB")` pairs — the theming API's
    /// config format (a custom theme's `[theme.NAME]` section). A role absent
    /// from `pairs` keeps [`Theme::LIGHT`]'s value (a sane base default); an
    /// unrecognized role name or an unparseable value is a
    /// [`ThemeParseError`] — never a panic.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Result<Theme, ThemeParseError> {
        let mut t = Theme::LIGHT;
        for &(role, value) in pairs {
            let hex = Self::parse_hex(value).ok_or_else(|| ThemeParseError::BadValue {
                role: role.to_string(),
                value: value.to_string(),
            })?;
            match t.role_mut(role) {
                Some(slot) => *slot = hex,
                None => return Err(ThemeParseError::UnknownRole(role.to_string())),
            }
        }
        Ok(t)
    }
}

/// A [`Theme::from_pairs`] failure: an unrecognized role name, or a role
/// whose value isn't `0x` + 6 hex digits. Always returned, never panics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeParseError {
    /// A key that isn't a role [`Theme::from_pairs`] recognizes.
    UnknownRole(String),
    /// A role whose value didn't parse as `0xRRGGBB`.
    BadValue { role: String, value: String },
}

impl fmt::Display for ThemeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeParseError::UnknownRole(k) => write!(f, "unknown theme role '{k}'"),
            ThemeParseError::BadValue { role, value } => write!(
                f,
                "theme role '{role}' has a bad colour value '{value}' (want 0xRRGGBB)"
            ),
        }
    }
}

/// Which palette is active — the persisted `Settings::theme` choice.
/// `Custom` carries a name resolved against a loaded [`CustomThemes`]
/// registry. Selected via the Options Theme tab (the three built-ins), the
/// config file, or the Light↔Dark hotkey (`T`); a `Custom` theme is config-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    Light,
    Dark,
    Classic,
    Custom(String),
}

impl Default for ThemeChoice {
    /// The modern flat look, not bgb's classic grey/white — a fresh slopgb
    /// (no config yet) now looks contemporary out of the box.
    fn default() -> Self {
        ThemeChoice::Light
    }
}

impl ThemeChoice {
    /// Resolve to a concrete [`Theme`]: the three built-ins are immediate;
    /// `Custom(name)` looks `name` up in `custom` — an unregistered name logs
    /// and falls back to [`Theme::LIGHT`] (never a panic, never a stuck UI).
    #[must_use]
    pub fn resolve(&self, custom: &CustomThemes) -> Theme {
        match self {
            ThemeChoice::Light => Theme::LIGHT,
            ThemeChoice::Dark => Theme::DARK,
            ThemeChoice::Classic => Theme::CLASSIC,
            ThemeChoice::Custom(name) => custom.get(name).copied().unwrap_or_else(|| {
                eprintln!("slopgb: unknown custom theme '{name}' — falling back to Light");
                Theme::LIGHT
            }),
        }
    }

    /// Encode for persistence (the native config's `theme` key, and the
    /// bgb-ini `SlopgbTheme` extra): `light` / `dark` / `classic` /
    /// `custom:NAME`. An empty-named `Custom` (never produced by
    /// [`Self::from_key`], but not `unsafe`-illegal to construct directly)
    /// encodes as [`Self::default`]'s key, so this always agrees with
    /// [`Self::from_key`] — never a lossy `"custom:"` round-trip.
    #[must_use]
    pub fn to_key(&self) -> String {
        match self {
            ThemeChoice::Light => "light".to_string(),
            ThemeChoice::Dark => "dark".to_string(),
            ThemeChoice::Classic => "classic".to_string(),
            ThemeChoice::Custom(name) if name.is_empty() => Self::default().to_key(),
            ThemeChoice::Custom(name) => format!("custom:{name}"),
        }
    }

    /// Decode [`Self::to_key`]; anything unrecognized (including empty, or a
    /// bare `custom:` with no name) falls back to [`Self::default`] —
    /// non-fatal, so a hand-edited config can't wedge startup.
    #[must_use]
    pub fn from_key(v: &str) -> Self {
        match v {
            "light" => ThemeChoice::Light,
            "dark" => ThemeChoice::Dark,
            "classic" => ThemeChoice::Classic,
            _ => v
                .strip_prefix("custom:")
                .filter(|n| !n.is_empty())
                .map_or_else(Self::default, |n| ThemeChoice::Custom(n.to_string())),
        }
    }
}

/// Named custom themes loaded from the settings file's `[theme.NAME]`
/// sections — the theming API's registry. Empty until populated by
/// `settings_file::load_custom_themes` (or [`Self::insert`] directly, e.g. in
/// a test).
#[derive(Clone, Debug, Default)]
pub struct CustomThemes(Vec<(String, Theme)>);

impl CustomThemes {
    /// Register (or replace) a named theme.
    pub fn insert(&mut self, name: impl Into<String>, theme: Theme) {
        let name = name.into();
        match self.0.iter_mut().find(|(n, _)| *n == name) {
            Some((_, t)) => *t = theme,
            None => self.0.push((name, theme)),
        }
    }

    /// Look up a registered theme by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Theme> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
