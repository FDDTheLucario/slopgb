//! `Settings` ↔ bgb.ini mapping. Reads the recognized keys into `Settings`
//! (defaulting anything absent/unparseable) and writes them back, touching only
//! the mapped keys + our `Slopgb*` extras — every other line the ini holds is
//! preserved by the [`Ini`] model. Key map: `docs/settings-persistence-plan.md`.
//!
//! `model` maps to bgb `SystemMode` (radio index from `options-system.png`);
//! `scheme` follows `dmg_palette`, persisted via `Color0..3`.

use super::ini::{self, Ini};
use crate::windows::options::{ModelChoice, SCHEMES, Settings};

/// Read a `Settings` from a parsed bgb.ini; any key absent or unparseable takes
/// its `Settings::default()` value.
#[must_use]
pub fn from_ini(f: &Ini) -> Settings {
    let d = Settings::default();
    let boolean = |k: &str, def: bool| f.get(k).map_or(def, ini::parse_bool);
    let int = |k: &str, def: i64| f.get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(def);
    let text = |k: &str, def: &str| f.get(k).map_or_else(|| def.to_string(), str::to_string);

    // DMG palette from Color0..3 (BGR hex); scheme = the SCHEMES index it matches.
    let mut dmg_palette = d.dmg_palette;
    for (i, key) in ["Color0", "Color1", "Color2", "Color3"].iter().enumerate() {
        if let Some(c) = f.get(key).and_then(ini::parse_color_hex) {
            dmg_palette[i] = c;
        }
    }
    let scheme = SCHEMES
        .iter()
        .position(|s| s.colors == dmg_palette)
        .unwrap_or(d.scheme);

    Settings {
        // bgb SystemMode radio index (options-system.png): 0=Gameboy(DMG),
        // 1=Gameboy Color(CGB), 3=automatic prefer GBC; 2 + 4..7 are SGB/auto
        // variants slopgb doesn't distinguish, so they collapse to Auto.
        model: match f.get("SystemMode") {
            Some("0") => ModelChoice::Dmg,
            Some("1") => ModelChoice::Cgb,
            _ => ModelChoice::Auto,
        },
        // slopgb's fullscreen-stretch has no bgb equivalent (bgb's `stretch` is a
        // video-scaling dropdown, not a mode), so it's a `Slopgb` extra.
        stretch: boolean("SlopgbStretch", d.stretch),
        volume: (int("Volume", 100) as f32 / 100.0).clamp(0.0, 1.0),
        mono: boolean("SoundMono", d.mono),
        lowercase_disasm: boolean("DebugLowercase", d.lowercase_disasm),
        lowercase_hex: boolean("DebugHexLower", d.lowercase_hex),
        show_clocks: boolean("DebugCountedClocks", d.show_clocks),
        rgbds_disasm: f.get("DisasmSyntax").map_or(d.rgbds_disasm, |v| v == "rgbds"),
        tile_hex_8bit: boolean("SlopgbTileHex8bit", d.tile_hex_8bit),
        memory_window: boolean("SlopgbMemoryWindow", d.memory_window),
        esc_shows_debugger: boolean("DebugEsc", d.esc_shows_debugger),
        ff_speed: int("UndelayedSpeed", i64::from(d.ff_speed)).clamp(1, 20) as u32,
        framerate_limit: int("FrameRate", i64::from(d.framerate_limit)).max(0) as u32,
        show_framerate: boolean("SlopgbShowFramerate", d.show_framerate),
        freeze_recent: boolean("RecentFrozen", d.freeze_recent),
        pause_on_focus_loss: boolean("PauseOnDefocus", d.pause_on_focus_loss),
        scheme,
        dmg_palette,
        allow_opposing: boolean("JoyOpposite", d.allow_opposing),
        break_ld_b_b: boolean("SlopgbBreakLdBB", d.break_ld_b_b),
        break_invalid_op: boolean("InvalidOpBreak", d.break_invalid_op),
        break_echo_ram: boolean("SlopgbBreakEchoRam", d.break_echo_ram),
        break_lcd_off_vblank: boolean("DebugDisableLCD", d.break_lcd_off_vblank),
        bootroms_enabled: boolean("BootromEnabled", d.bootroms_enabled),
        bootrom_dmg: text("DmgBootRom", &d.bootrom_dmg),
        bootrom_gbc: text("CgbBootRom", &d.bootrom_gbc),
        bootrom_sgb: text("SgbBootRom", &d.bootrom_sgb),
    }
}

/// Update `f` in place to reflect `s`: overwrite the mapped bgb keys + our
/// `Slopgb*` extras (bgb ignores unknown keys), preserving every other line.
/// `model`/`SystemMode` is left untouched (see module doc).
pub fn to_ini(s: &Settings, f: &mut Ini) {
    f.set(
        "SystemMode",
        match s.model {
            ModelChoice::Dmg => "0",
            ModelChoice::Cgb => "1",
            ModelChoice::Auto => "3",
        },
    );
    f.set("Volume", &((s.volume * 100.0).round() as i64).to_string());
    f.set("SoundMono", ini::fmt_bool(s.mono));
    f.set("DebugLowercase", ini::fmt_bool(s.lowercase_disasm));
    f.set("DebugHexLower", ini::fmt_bool(s.lowercase_hex));
    f.set("DebugCountedClocks", ini::fmt_bool(s.show_clocks));
    f.set("DisasmSyntax", if s.rgbds_disasm { "rgbds" } else { "no$gmb" });
    f.set("DebugEsc", ini::fmt_bool(s.esc_shows_debugger));
    f.set("UndelayedSpeed", &s.ff_speed.to_string());
    f.set("FrameRate", &s.framerate_limit.to_string());
    f.set("RecentFrozen", ini::fmt_bool(s.freeze_recent));
    f.set("PauseOnDefocus", ini::fmt_bool(s.pause_on_focus_loss));
    f.set("JoyOpposite", ini::fmt_bool(s.allow_opposing));
    f.set("InvalidOpBreak", ini::fmt_bool(s.break_invalid_op));
    f.set("DebugDisableLCD", ini::fmt_bool(s.break_lcd_off_vblank));
    f.set("BootromEnabled", ini::fmt_bool(s.bootroms_enabled));
    f.set("DmgBootRom", &s.bootrom_dmg);
    f.set("CgbBootRom", &s.bootrom_gbc);
    f.set("SgbBootRom", &s.bootrom_sgb);
    for (i, key) in ["Color0", "Color1", "Color2", "Color3"].iter().enumerate() {
        f.set(key, &ini::fmt_color_hex(s.dmg_palette[i]));
    }
    // slopgb-only fields — no bgb key, stored under a `Slopgb` prefix bgb ignores.
    f.set("SlopgbStretch", ini::fmt_bool(s.stretch));
    f.set("SlopgbTileHex8bit", ini::fmt_bool(s.tile_hex_8bit));
    f.set("SlopgbMemoryWindow", ini::fmt_bool(s.memory_window));
    f.set("SlopgbShowFramerate", ini::fmt_bool(s.show_framerate));
    f.set("SlopgbBreakLdBB", ini::fmt_bool(s.break_ld_b_b));
    f.set("SlopgbBreakEchoRam", ini::fmt_bool(s.break_echo_ram));
}

#[cfg(test)]
#[path = "bgb_tests.rs"]
mod tests;
