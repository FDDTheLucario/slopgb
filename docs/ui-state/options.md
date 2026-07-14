# Options control panel

**Options...** (F11) opens a bgb-style tabbed control panel (`windows::options`,
captures in [`../bgb-reference/options/`](../bgb-reference/)). A 10-tab two-row
property sheet (Graphics/System/Debug/Exceptions · Sound/GB Colors/Joypad/Misc, plus
slopgb Theme + Plugins tabs; the active tab's group sits in the bottom row, bgb's
multi-row behaviour) drawn as a
modal LCD overlay (`App.options: Option<OptionsState>`, click/key captured like the
info box, Esc=Cancel).

## Buttons + control model

OK/Cancel/Apply/Defaults over a working+baseline scratch: OK applies+closes, Cancel
reverts, Apply commits+stays-open, Defaults resets the active tab's live fields. Each
control is a `tabs::Ctrl` — one list drives both render and click hit-test, so they
can't drift. Live controls carry a `Field`; inert ones render faithfully (greyed only
where bgb itself greys them).

## Live settings (`App::apply_settings` on OK/Apply)

| Tab | Setting → effect |
|---|---|
| System | Emulated system (Gameboy/Gameboy Color/automatic → `ModelChoice` → `Session::set_model` rebuilds the machine on change; palette re-applied after) |
| GB Colors | scheme (`SCHEMES` presets → `GameBoy::set_dmg_palette`) |
| Sound | volume + mono (`AudioPipe::set_volume` gain/downmix); **SGB audio backend** dropdown (Built-in HLE APU / SGB coprocessor → `Settings.audio_backend` → `Session::set_sgb_coprocessor`, the same seam `--sgb-coprocessor` drives; the CLI flag/env still wins the launch, else the persisted choice is honored at startup. Default Built-in → byte-identical. A no-op off SGB) |
| Graphics | stretch (→ fullscreen-stretched window size); **frame blend** (`postfx` present filter — averages the frame with the previous one); **SGB border in screenshot** (`save_screenshot` uses the 256×224 composite when a border is loaded) |
| GB Colors | (above) plus **DMG on GBC LCD colors** + **contrast** wheel — `postfx` per-pixel present filters (frontend-only, golden-safe) |
| Debug | lowercase-hex + show-clocks (→ `DisasmFmt` via `tools.set_disasm_fmt`); "pressing Esc shows debugger" (`Settings.esc_shows_debugger`, default on → `handle_key` opens the debugger on Esc instead of quitting); RGBDS syntax; "memory viewer in own window"; **Registers can be edited** (→ `DebuggerState.registers_editable` via `tools.set_registers_editable`; off greys the register-edit menu); **Start in debugger** (opens the debugger window at launch); **Live update memory viewer** (`tools.request_redraw_live` skips the standalone memory window's per-frame redraw when off — it then repaints only on interaction, bgb's non-continuous refresh) |
| Misc | fast-forward-speed + framerate-limit sliders (→ `app_pacing` `turbo_max_frames`/`frame_interval`); show-framerate (title); freeze-recent-ROMs (`push_recent` gate); pause-if-losing-focus (auto-pause on focus loss, auto-resume on refocus unless manually paused via `App.paused_by_focus`); **Show errors on ROM load** (a failed load pops an info box, default on); **Load ROM dialog on startup** (opens the picker at launch when no CLI ROM) |
| Joypad | **Screenshot button** saves↔copies (copies puts the frame on the clipboard as PNG via `clipboard::copy_image_png`); **Screenshots** format bmp↔png (`ScreenshotFormat` → `screenshot::to_bmp` / `mcp::png::encode`) |
| Theme | Light/Dark/Classic radios → `Settings.theme` (`ThemeChoice`; the render path recolors from it each redraw — see [theming.md](theming.md)). Custom themes stay config-only. |
| Plugins | Per-plugin **enable** checkbox (`Field::PluginEnable(i)` → `PluginConfig.entries[i].enabled` → `PluginHost::set_enabled`, skipping a disabled plugin's `on_frame`), the read-only plugins-dir display, and an **allow-mutation** toggle (`Field::PluginAllowMutation`, default off). No bgb equivalent — see [plugin-api.md](plugin-api.md#managing-plugins-from-the-ui). |

**Pure bgb mode** (Debug tab): one toggle flips every slopgb-departure setting
(rgbds→off, memory-window→off) to bgb-faithful.

## Exceptions (golden-safe core break mask)

Four live break conditions feed an `exc_mask: u16` on `Interconnect` (inert/`0` ⇒
fingerprint byte-identical; `GameBoy::set_exceptions`/`exceptions`, the `EXC_*` bits;
`App::apply_exceptions` pushes `Settings::exception_mask` at startup/load/OK-Apply),
halting the debugger free-run via `exc_hit` in `run_frame_until_breakpoint`:

- break on ld b,b (`0x40`) / invalid opcode (the 11 undefined ops, **default-checked
  like bgb**) — checked in `Bus::check_exec` at instruction-execute.
- break on echo-RAM (E000-FDFF) access (`check_access`) / disabling the LCD outside
  vblank (FF40 bit7→0 while on + PPU mode≠1, `check_exc_lcd`) — checked in the ticked
  bus path.

Armed only while the debugger window is open (`dbg_armed`). The tab's other
conditions (OAM-DMA bad access, 16-bit inc/dec FE00-FEFF, SGB transfer, MBC,
inaccessible VRAM, halt+ints bug, uninitialized RAM) render but are **inert** — no
clean golden-safe detector/backend.

## Joypad (`keymap`)

- **Configure keyboard** opens bgb's sequential key-rebind wizard
  (`keymap::KeyConfigWizard`, `docs/bgb-reference/options/joypad-keyconfig.png`):
  8 buttons right→start, each a GB illustration + "press and hold the button for X" +
  Cancel/Skip-clear/Skip-keep. A keypress binds the current button and advances; the
  App captures every game-window key while it floats over the LCD; Esc cancels,
  finishing commits. Over the rebindable `App.bindings` (`keymap::KeyBindings`,
  default Z=A/X=B/Enter=Start/RShift=Select/arrows). `handle_key` resolves a held
  button via `bindings.button_for` **before** `input::map` (which no longer carries
  `Action::Button`).
- **Allow pressing L+R or U+D** toggles the SOCD filter (`Settings.allow_opposing`,
  default off = bgb). `keymap::socd_suppress` in `App::set_button` releases the
  opposite direction on a new press and **resurrects** a still-held one on release
  (last-input priority); verified via the golden-safe `&self` read
  `GameBoy::debug_button`→`Joypad::pressed` (tests only).
- **Screenshot button** (saves↔copies) and **Screenshots** (bmp↔png) combos are
  live (see the Live-settings table). The rest (game-controller config/clear,
  Mappable-button-records, Rapid-speed combo, joystick-ID) is faithful-but-inert.

## Live input timing (`app_input` + `input::apply_input`)

A key press is *deferred* from the winit event to the next emulated frame and applied
at a **wall-clock-derived sub-frame T-cycle offset** (`queue_input`→
`apply_pending_input`, stepping the core to the offset before `gb.press`), so the
joypad interrupt fires on a *varied* LCD line — real-hardware input entropy (passes
`little-things-gb/tellinglys`, "Pass! Joypad interrupt timing is realistic"; a
frame-boundary-only press always hit the same line).

- **Do** drop queued presses while frozen (paused/no-ROM/broken) — a press on a
  frozen machine shouldn't register.
- **Do** still honor releases while frozen (`flush_idle_input`) so a button released
  while paused can't stick held on resume.

## System tab — bootrom UI

See [startup-and-boot.md](startup-and-boot.md). DMG/GBC/SGB path fields + `...` browse
buttons (`Field::PickBootrom`→`OptionsOutcome::PickBootrom`→a path modal over the
dialog) + a live "bootroms enabled" checkbox. `Settings` is no longer `Copy` (holds
the path strings); `DIALOG_W` 345→420 for slopgb's wider fixed font.

## Persistence (`crates/slopgb/src/settings_file/`)

`App.settings` + recent ROMs persist to disk. Loaded at startup (seeds
`settings`/`recent`; CLI `--model` still wins the session), saved on Options
Apply/OK, ROM load, and Quit. Atomic write (temp+rename) in the config dir
(`$XDG_CONFIG_HOME/slopgb/` else `~/.config/slopgb/`; `%APPDATA%\slopgb\` on
Windows).

**Native format (phase 2, default):** `slopgb.conf` — a versioned sectioned
text file (`native.rs`): `version = 1`, `[system]`/`[sound]`/`[graphics]`/
`[debug]`/`[misc]`/`[exceptions]`/`[recent]`, `true`/`false`, `0xRRGGBB` colors,
comma-list palette, numbered `[recent]` POSIX paths. Unknown keys/sections +
comments preserved; missing keys default.

**bgb.ini (phase 1, import/export):** the `ini` module keeps an ordered-line
model that round-trips a real bgb.ini byte-identically (bgb's ~250 unmodelled
keys survive verbatim); `bgb.rs` maps ~19 keys (`DisasmSyntax`, `DebugHexLower`,
`Volume`, `SystemMode`↔model, `Color0..3` BGR-hex, `Recent0..9` with wine
`Z:\`↔POSIX). slopgb-only fields go under a `Slopgb` prefix bgb ignores.
**Precedence:** native wins; else a bgb.ini is migrated into the native store
once; else defaults. Game menu → Other → **Import/Export bgb.ini...** for
explicit interop. `model` maps to bgb `SystemMode` (System-tab radio index:
Auto↔3, Dmg↔0, Cgb↔1); an explicit CLI `--model` still wins the session.
**Recent ROMs** persist via `Recent0..9` (wine `Z:\`↔POSIX path translation),
saved on ROM load + Quit. bgb's window-geometry / open-on-start keys have no
slopgb equivalent — preserved verbatim, not acted on. Phase 1 complete; phase 2
(a modern native format) is planned.

## Inert

Still faithful-but-inert — each blocked on real work, not a wiring gap:

- **GB CPU usage meter** — needs a core halt-cycle counter (golden-safe read) +
  a metric window + an uncaptured bgb meter widget.
- **0-31 numbers** — the exact bgb palette-number display is uncaptured (would
  violate the never-invent-bgb-UI rule); capture it first.
- **Rapid speed** — blocked on the missing rapid-fire button binding ("configure
  extra buttons" is itself inert).
- **Recovery save state** — the periodic write reuses `save_state_to`, but
  restore-on-relaunch is an unmade UX decision (auto-load vs prompt).
- **reduce CPU usage** — already effectively on: the non-turbo loop parks with
  `ControlFlow::WaitUntil`, so it doesn't busy-spin.
- Faithful chrome with no dep-free/golden-safe backend: game-controller config,
  WAV/AVI recording, rewind, RTC files, doubler/bpp/output/vsync scalers,
  soundcard/samplerate/latency, lowercase-disassembler toggle, "disable SGB
  colors".
