# Theming: Light/Dark/Classic + a custom-theme API

Contemporary UI look, replacing the bare "Windows 3" bgb chrome as the default —
**colour only**: no rect/control was moved, resized, or added. `Theme` (`ui/theme.rs`)
carries only `u32` XRGB8888 fields, so it structurally cannot encode geometry; every
tool window already took `&Theme` (bgb-clone-plan Layer B/C), so swapping the active
palette recolors the whole UI with zero call-site churn beyond the handful of
role-repoints below.

## Role set (13, additive over the original 7)

`bg`/`text`/`current`/`breakpoint`/`hilight`/`freeze`/`border` are the original bgb
roles, unchanged. New: `panel` (recessed field bg), `button_face` (unpressed button
fill), `accent` (checkbox/radio checked-mark), `selection_bg`/`selection_fg` (hovered
menu row — split from `current`, the debugger's current-PC-row color, so a custom
theme can tell "hovered" and "current instruction" apart), `disabled_text`
(greyed/inert control text), `scrollbar` (thumb colour).

`Theme::BGB` fills every new role with the value the pre-theming draw code used at
that call site (`panel=button_face=bg`, `accent=text`, `selection_bg=current`,
`selection_fg=bg`, `disabled_text=scrollbar=hilight`) — so repointing a call site at
the new role (`ui/widgets.rs` checkbox/radio-dot fill → `accent`, button fill →
`button_face`, scrollbar thumb → `scrollbar`; `ui/menu.rs` hover row →
`selection_bg`/`selection_fg`, disabled row → `disabled_text`; `ui/dialog.rs`
input-field fill → `panel`; `windows/options.rs` `fg()` disabled colour →
`disabled_text`, button-row fill → `button_face`) is pixel-identical for `BGB`.
`Theme::CLASSIC == Theme::BGB` (offered as a selectable choice, not just the default).

A prior revision also carried `bevel_light`/`bevel_dark` (raised-bevel edges) and a
`to_pairs`/`role`/`ROLE_NAMES` export API; both were dead (no draw call read the bevel
roles, no export UI called `to_pairs`) and were deleted (YAGNI) — `from_pairs` (the
import half, live via `settings_file::load_custom_themes`) stands alone on `role_mut`.
A theme file setting `bevel_light`/`bevel_dark` now honestly gets `UnknownRole`.

## Palettes (hex, XRGB8888)

| Role | LIGHT | DARK |
|---|---|---|
| bg | `F5F5F7` | `202124` |
| text | `202124` | `E8EAED` |
| current / accent / selection_bg | `1A73E8` | `8AB4F8` |
| breakpoint | `D93025` | `F28B82` |
| hilight | `9AA0A6` | `9AA0A6` |
| freeze | `F9AB00` | `FDD663` |
| border | `DADCE0` | `5F6368` |
| panel | `FFFFFF` | `2D2E31` |
| button_face | `EDEDF0` | `3C4043` |
| selection_fg | `FFFFFF` | `202124` |
| disabled_text | `BDC1C6` | `5F6368` |
| scrollbar | `C4C7C5` | `80868B` |

One accent family (blue) shared by `current`/`accent`/`selection_bg` in both; flat
borders (no raised/sunken 3-D bevel effect — drawing one wasn't needed to hit
"contemporary" and would only ever be a pixel recolor if added later, never a
geometry change).

## `ThemeChoice` + resolution

`enum ThemeChoice { Light, Dark, Classic, Custom(String) }`, `Default = Light` (a
fresh install now looks modern, not bgb-grey). `Settings.theme: ThemeChoice`
(`windows/options.rs`) is the persisted choice; `ThemeChoice::resolve(&CustomThemes)
-> Theme` is called once per `redraw()` in `main.rs` (`self.settings.theme.resolve
(&self.custom_themes)`), replacing the old hardcoded `ui::Theme::BGB` — the **only**
line in the whole render path that changed to make every window themeable. No new
`&Theme` plumbing anywhere: the same parameter that always flowed through now just
carries a different value.

**Options → Theme tab** (a slopgb extra, no bgb equivalent): Light/Dark/Classic radios
select the three built-in themes; the pick applies + persists through the normal
Options OK/Apply flow (the render path already recolors from `settings.theme` every
redraw, so the tab's click handler only has to set `s.theme` — no new plumbing). A
named `Custom` theme has no radio and stays config-only — when one is active the tab
shows an inert `(custom theme active: NAME)` line instead of a lit radio. The config
file and the Light↔Dark hotkey (`T`, below) still select the theme too.

## Hotkey

Bare `T`, global (any focus, like `P`/`R`/`F9`) → `Action::ToggleTheme`
(`input.rs::map`) → `App::toggle_theme` (`app_run.rs`): flips Light↔Dark and persists
immediately (`settings_file::save`), so the choice survives a crash/kill, not just a
clean Quit. Classic/Custom aren't in the toggle cycle — pressing `T` from either lands
on Dark (a defined, non-stuck outcome), since they're config/CLI-only selections.
`toggle_theme_no_persist` is the disk-free half tests drive (mirrors
`apply_settings`/`apply_settings_no_persist` in `app_menu.rs`).

## Persistence

Native `slopgb.conf`: `[ui]` section, `theme = light|dark|classic|custom:NAME`. bgb.ini:
`SlopgbTheme` (no bgb equivalent — a `Slopgb*` extra, like `SlopgbStretch`). Both sides
decode via `ThemeChoice::from_key`/`to_key`; an unrecognized value (including a bare
`custom:` with no name) falls back to `ThemeChoice::default()` non-fatally — a
hand-edited config can't wedge startup.

## Custom-theme API

`Theme::from_pairs(&[(role, "0xRRGGBB"), ...]) -> Result<Theme, ThemeParseError>`:
missing roles fall back to `Theme::LIGHT`'s value; an unknown role name or unparseable
value is a `ThemeParseError` (`UnknownRole`/`BadValue`), never a panic. Import-only —
there's no export direction (no export UI to feed).

`ThemeChoice::Custom(name)` resolves against `CustomThemes`, loaded once at startup by
`settings_file::load_custom_themes()`: every `[theme.NAME]` section in `slopgb.conf`
becomes a registered theme (`name`'s section-pairs fed straight to `from_pairs`); a
malformed section is skipped + logged (one bad custom theme can't break the rest). An
unregistered `Custom` name at resolve time also logs and falls back to `Theme::LIGHT`.
Example:

```ini
[theme.solarized]
bg = 0x002B36
text = 0x93A1A1
current = 0x268BD2
```

```ini
[ui]
theme = custom:solarized
```

bgb.ini has no sectioned format, so custom themes are native-store-only (bgb.ini
import/export round-trips the *choice* string, not a `Custom` theme's own palette).
`[theme.NAME]` sections are unknown lines to `native::to_doc`, so they already survive
a settings save via the existing "preserve unknown sections" machinery — no special
case needed.

## Layout-invariance guard

`ui/theme_tests.rs::theme_swap_only_recolors_a_whole_window_never_moves_it`: renders
the real Options dialog (tabs/checkboxes/radios/dropdown/slider/buttons — nearly every
shared widget) three times, once per LIGHT/DARK/CLASSIC, into a `Canvas::new_recording`
that logs every `put`/`fill_rect` call's rect; asserts the three recorded geometry
lists are identical and the three pixel buffers differ. This is the test that would
fail first if any repoint above had accidentally also touched a position/size argument
instead of only a colour argument.
