//! `Settings` ↔ bgb.ini mapping. Reads the recognized keys into `Settings`
//! (defaulting anything absent/unparseable) and writes them back, touching only
//! the mapped keys + our `Slopgb*` extras — every other line the ini holds is
//! preserved by the [`Ini`] model.
//!
//! `model` maps to bgb `SystemMode` (radio index from `options-system.png`);
//! `scheme` follows `dmg_palette`, persisted via `Color0..3`.

use super::ini::{self, Ini};
use crate::ui::ThemeChoice;
use crate::windows::options::{
    AudioBackend, ModelChoice, PluginConfig, SCHEMES, ScreenshotFormat, Settings,
};

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
        // bgb SystemMode radio index (options-system.png), full 0..7 fidelity:
        // 0=Gameboy(DMG), 1=Gameboy Color(CGB), 2=Super Gameboy, 3=auto prefer
        // GBC, 4=auto prefer SGB, 5=SGB+GBC(SGB2), 6=GBC+initial SGB border,
        // 7=Gameboy or GBC. "3" and any unknown value fall back to Auto.
        model: match f.get("SystemMode") {
            Some("0") => ModelChoice::Dmg,
            Some("1") => ModelChoice::Cgb,
            Some("2") => ModelChoice::Sgb,
            Some("4") => ModelChoice::AutoSgb,
            Some("5") => ModelChoice::Sgb2,
            Some("6") => ModelChoice::CgbBorder,
            Some("7") => ModelChoice::AutoNoSgb,
            _ => ModelChoice::Auto,
        },
        // slopgb's fullscreen-stretch has no bgb equivalent (bgb's `stretch` is a
        // video-scaling dropdown, not a mode), so it's a `Slopgb` extra.
        stretch: boolean("SlopgbStretch", d.stretch),
        disable_sgb_colors: boolean("SlopgbDisableSgbColors", d.disable_sgb_colors),
        // No faithful bgb key mapped — stored as `Slopgb` extras.
        frame_blend: boolean("SlopgbFrameBlend", d.frame_blend),
        doubler: boolean("SlopgbDoubler", d.doubler),
        dmg_gbc_lcd: boolean("SlopgbDmgGbcLcd", d.dmg_gbc_lcd),
        contrast: f
            .get("SlopgbContrast")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(d.contrast),
        sgb_border_screenshot: boolean("SlopgbSgbBorderScreenshot", d.sgb_border_screenshot),
        screenshot_format: f
            .get("SlopgbScreenshotFormat")
            .map_or(d.screenshot_format, ScreenshotFormat::from_key),
        screenshot_copies: boolean("SlopgbScreenshotCopies", d.screenshot_copies),
        volume: (int("Volume", (d.volume * 100.0) as i64) as f32 / 100.0).clamp(0.0, 1.0),
        mono: boolean("SoundMono", d.mono),
        // No bgb equivalent (a `Slopgb` extra) — the SGB audio backend.
        audio_backend: f
            .get("SlopgbAudioBackend")
            .map_or(d.audio_backend, AudioBackend::from_key),
        audio_device: text("SlopgbAudioDevice", &d.audio_device),
        audio_sample_rate: int("SlopgbAudioSampleRate", i64::from(d.audio_sample_rate)).max(0)
            as u32,
        audio_latency: f
            .get("SlopgbAudioLatency")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(d.audio_latency),
        audio_8bit: boolean("SlopgbAudio8Bit", d.audio_8bit),
        audio_hq: boolean("SlopgbAudioHq", d.audio_hq),
        lowercase_disasm: boolean("DebugLowercase", d.lowercase_disasm),
        lowercase_hex: boolean("DebugHexLower", d.lowercase_hex),
        show_clocks: boolean("DebugCountedClocks", d.show_clocks),
        rgbds_disasm: f
            .get("DisasmSyntax")
            .map_or(d.rgbds_disasm, |v| v == "rgbds"),
        tile_hex_8bit: boolean("SlopgbTileHex8bit", d.tile_hex_8bit),
        memory_window: boolean("SlopgbMemoryWindow", d.memory_window),
        esc_shows_debugger: boolean("DebugEsc", d.esc_shows_debugger),
        registers_editable: boolean("SlopgbRegistersEditable", d.registers_editable),
        start_in_debugger: boolean("SlopgbStartInDebugger", d.start_in_debugger),
        mem_live_update: boolean("SlopgbMemLiveUpdate", d.mem_live_update),
        cpu_usage_meter: boolean("SlopgbCpuUsageMeter", d.cpu_usage_meter),
        ff_speed: int("UndelayedSpeed", i64::from(d.ff_speed)).clamp(1, 20) as u32,
        framerate_limit: int("FrameRate", i64::from(d.framerate_limit)).max(0) as u32,
        show_framerate: boolean("SlopgbShowFramerate", d.show_framerate),
        freeze_recent: boolean("RecentFrozen", d.freeze_recent),
        pause_on_focus_loss: boolean("PauseOnDefocus", d.pause_on_focus_loss),
        show_errors_on_rom_load: boolean("SlopgbShowRomErrors", d.show_errors_on_rom_load),
        load_rom_dialog_on_startup: boolean("SlopgbLoadRomOnStartup", d.load_rom_dialog_on_startup),
        reduce_cpu: boolean("SlopgbReduceCpu", d.reduce_cpu),
        recovery_save_state: boolean("SlopgbRecoverySaveState", d.recovery_save_state),
        scheme,
        dmg_palette,
        palette_edit_shade: int("SlopgbPaletteShade", d.palette_edit_shade as i64).clamp(0, 3)
            as usize,
        palette_0_31: boolean("Slopgb031Numbers", d.palette_0_31),
        allow_opposing: boolean("JoyOpposite", d.allow_opposing),
        rapid_speed: int("SlopgbRapidSpeed", i64::from(d.rapid_speed)).clamp(1, 4) as u32,
        record_audio: boolean("SlopgbRecordAudio", d.record_audio),
        record_video: boolean("SlopgbRecordVideo", d.record_video),
        record_audio_channels: boolean("SlopgbRecordAudioChannels", d.record_audio_channels),
        rtc_vba_sav: boolean("SlopgbRtcVbaSav", d.rtc_vba_sav),
        rtc_bgb_legacy: boolean("SlopgbRtcBgbLegacy", d.rtc_bgb_legacy),
        uninited_wram: boolean("UninitedWRAM", d.uninited_wram),
        auto_reset_on_system_change: boolean(
            "SlopgbAutoResetOnSystemChange",
            d.auto_reset_on_system_change,
        ),
        rewind_enabled: boolean("SlopgbRewindEnabled", d.rewind_enabled),
        break_ld_b_b: boolean("SlopgbBreakLdBB", d.break_ld_b_b),
        break_invalid_op: boolean("InvalidOpBreak", d.break_invalid_op),
        break_echo_ram: boolean("SlopgbBreakEchoRam", d.break_echo_ram),
        break_lcd_off_vblank: boolean("DebugDisableLCD", d.break_lcd_off_vblank),
        break_oam_dma_bad: boolean("SlopgbBreakOamDmaBad", d.break_oam_dma_bad),
        break_incdec_fexx: boolean("SlopgbBreakIncDecFexx", d.break_incdec_fexx),
        break_sgb_transfer: boolean("SlopgbBreakSgbTransfer", d.break_sgb_transfer),
        bootroms_enabled: boolean("BootromEnabled", d.bootroms_enabled),
        bootrom_dmg: text("DmgBootRom", &d.bootrom_dmg),
        bootrom_gbc: text("CgbBootRom", &d.bootrom_gbc),
        bootrom_sgb: text("SgbBootRom", &d.bootrom_sgb),
        // No bgb equivalent (a `Slopgb` extra, like the other slopgb-only
        // fields below).
        theme: f
            .get("SlopgbTheme")
            .map(ThemeChoice::from_key)
            .unwrap_or_default(),
        plugins: PluginConfig::from_persisted(
            text("SlopgbPluginsDir", ""),
            boolean("SlopgbPluginsAllowMutation", false),
            f.get("SlopgbPluginsDisabled").unwrap_or(""),
        ),
    }
}

/// Update `f` in place to reflect `s`: overwrite the mapped bgb keys + our
/// `Slopgb*` extras (bgb ignores unknown keys), preserving every other line.
/// `model` is written to bgb's `SystemMode` (see module doc).
pub fn to_ini(s: &Settings, f: &mut Ini) {
    let Settings {
        model: _,
        stretch: _,
        disable_sgb_colors: _,
        frame_blend: _,
        doubler: _,
        dmg_gbc_lcd: _,
        contrast: _,
        sgb_border_screenshot: _,
        screenshot_format: _,
        screenshot_copies: _,
        volume: _,
        mono: _,
        audio_backend: _,
        audio_device: _,
        audio_sample_rate: _,
        audio_latency: _,
        audio_8bit: _,
        audio_hq: _,
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
        recovery_save_state: _,
        scheme: _,
        dmg_palette: _,
        palette_edit_shade: _,
        palette_0_31: _,
        allow_opposing: _,
        rapid_speed: _,
        record_audio: _,
        record_video: _,
        record_audio_channels: _,
        rtc_vba_sav: _,
        rtc_bgb_legacy: _,
        uninited_wram: _,
        auto_reset_on_system_change: _,
        rewind_enabled: _,
        break_ld_b_b: _,
        break_invalid_op: _,
        break_echo_ram: _,
        break_lcd_off_vblank: _,
        break_oam_dma_bad: _,
        break_incdec_fexx: _,
        break_sgb_transfer: _,
        bootroms_enabled: _,
        bootrom_dmg: _,
        bootrom_gbc: _,
        bootrom_sgb: _,
        theme: _,
        plugins: _,
    } = s;
    f.set(
        "SystemMode",
        match s.model {
            ModelChoice::Dmg => "0",
            ModelChoice::Cgb => "1",
            ModelChoice::Sgb => "2",
            ModelChoice::Auto => "3",
            ModelChoice::AutoSgb => "4",
            ModelChoice::Sgb2 => "5",
            ModelChoice::CgbBorder => "6",
            ModelChoice::AutoNoSgb => "7",
        },
    );
    f.set("Volume", &((s.volume * 100.0).round() as i64).to_string());
    f.set("SoundMono", ini::fmt_bool(s.mono));
    f.set("DebugLowercase", ini::fmt_bool(s.lowercase_disasm));
    f.set("DebugHexLower", ini::fmt_bool(s.lowercase_hex));
    f.set("DebugCountedClocks", ini::fmt_bool(s.show_clocks));
    f.set(
        "DisasmSyntax",
        if s.rgbds_disasm { "rgbds" } else { "no$gmb" },
    );
    f.set("DebugEsc", ini::fmt_bool(s.esc_shows_debugger));
    f.set(
        "SlopgbRegistersEditable",
        ini::fmt_bool(s.registers_editable),
    );
    f.set("SlopgbStartInDebugger", ini::fmt_bool(s.start_in_debugger));
    f.set("SlopgbMemLiveUpdate", ini::fmt_bool(s.mem_live_update));
    f.set("SlopgbCpuUsageMeter", ini::fmt_bool(s.cpu_usage_meter));
    f.set("UndelayedSpeed", &s.ff_speed.to_string());
    f.set("FrameRate", &s.framerate_limit.to_string());
    f.set("RecentFrozen", ini::fmt_bool(s.freeze_recent));
    f.set("PauseOnDefocus", ini::fmt_bool(s.pause_on_focus_loss));
    f.set(
        "SlopgbShowRomErrors",
        ini::fmt_bool(s.show_errors_on_rom_load),
    );
    f.set(
        "SlopgbLoadRomOnStartup",
        ini::fmt_bool(s.load_rom_dialog_on_startup),
    );
    f.set("SlopgbReduceCpu", ini::fmt_bool(s.reduce_cpu));
    f.set(
        "SlopgbRecoverySaveState",
        ini::fmt_bool(s.recovery_save_state),
    );
    f.set("JoyOpposite", ini::fmt_bool(s.allow_opposing));
    f.set("SlopgbRapidSpeed", &s.rapid_speed.to_string());
    f.set("SlopgbRecordAudio", ini::fmt_bool(s.record_audio));
    f.set("SlopgbRecordVideo", ini::fmt_bool(s.record_video));
    f.set(
        "SlopgbRecordAudioChannels",
        ini::fmt_bool(s.record_audio_channels),
    );
    f.set("SlopgbRtcVbaSav", ini::fmt_bool(s.rtc_vba_sav));
    f.set("SlopgbRtcBgbLegacy", ini::fmt_bool(s.rtc_bgb_legacy));
    f.set("UninitedWRAM", ini::fmt_bool(s.uninited_wram));
    f.set(
        "SlopgbAutoResetOnSystemChange",
        ini::fmt_bool(s.auto_reset_on_system_change),
    );
    f.set("SlopgbRewindEnabled", ini::fmt_bool(s.rewind_enabled));
    f.set("InvalidOpBreak", ini::fmt_bool(s.break_invalid_op));
    f.set("DebugDisableLCD", ini::fmt_bool(s.break_lcd_off_vblank));
    f.set("SlopgbBreakOamDmaBad", ini::fmt_bool(s.break_oam_dma_bad));
    f.set("SlopgbBreakIncDecFexx", ini::fmt_bool(s.break_incdec_fexx));
    f.set(
        "SlopgbBreakSgbTransfer",
        ini::fmt_bool(s.break_sgb_transfer),
    );
    f.set("BootromEnabled", ini::fmt_bool(s.bootroms_enabled));
    f.set("DmgBootRom", &s.bootrom_dmg);
    f.set("CgbBootRom", &s.bootrom_gbc);
    f.set("SgbBootRom", &s.bootrom_sgb);
    for (i, key) in ["Color0", "Color1", "Color2", "Color3"].iter().enumerate() {
        f.set(key, &ini::fmt_color_hex(s.dmg_palette[i]));
    }
    // slopgb-only fields — no bgb key, stored under a `Slopgb` prefix bgb ignores.
    f.set("SlopgbPaletteShade", &s.palette_edit_shade.to_string());
    f.set("Slopgb031Numbers", ini::fmt_bool(s.palette_0_31));
    f.set("SlopgbStretch", ini::fmt_bool(s.stretch));
    f.set(
        "SlopgbDisableSgbColors",
        ini::fmt_bool(s.disable_sgb_colors),
    );
    f.set("SlopgbFrameBlend", ini::fmt_bool(s.frame_blend));
    f.set("SlopgbDoubler", ini::fmt_bool(s.doubler));
    f.set("SlopgbDmgGbcLcd", ini::fmt_bool(s.dmg_gbc_lcd));
    f.set("SlopgbContrast", &s.contrast.to_string());
    f.set(
        "SlopgbSgbBorderScreenshot",
        ini::fmt_bool(s.sgb_border_screenshot),
    );
    f.set("SlopgbScreenshotFormat", s.screenshot_format.ext());
    f.set("SlopgbScreenshotCopies", ini::fmt_bool(s.screenshot_copies));
    f.set("SlopgbTileHex8bit", ini::fmt_bool(s.tile_hex_8bit));
    f.set("SlopgbMemoryWindow", ini::fmt_bool(s.memory_window));
    f.set("SlopgbShowFramerate", ini::fmt_bool(s.show_framerate));
    f.set("SlopgbBreakLdBB", ini::fmt_bool(s.break_ld_b_b));
    f.set("SlopgbBreakEchoRam", ini::fmt_bool(s.break_echo_ram));
    f.set("SlopgbTheme", &s.theme.to_key());
    f.set("SlopgbAudioBackend", s.audio_backend.to_key());
    f.set("SlopgbAudioDevice", &s.audio_device);
    f.set("SlopgbAudioSampleRate", &s.audio_sample_rate.to_string());
    f.set("SlopgbAudioLatency", &s.audio_latency.to_string());
    f.set("SlopgbAudio8Bit", ini::fmt_bool(s.audio_8bit));
    f.set("SlopgbAudioHq", ini::fmt_bool(s.audio_hq));
    f.set("SlopgbPluginsDir", &s.plugins.dir);
    f.set(
        "SlopgbPluginsAllowMutation",
        ini::fmt_bool(s.plugins.allow_mutation),
    );
    f.set("SlopgbPluginsDisabled", &s.plugins.disabled_joined());
}

#[cfg(test)]
#[path = "bgb_tests.rs"]
mod tests;
