# Options control panel

**Options...** (F11) opens a bgb-style tabbed control panel (`windows::options`,
captures in [`../bgb-reference/options/`](../bgb-reference/)). An 8-tab two-row
property sheet (Graphics/System/Debug/Exceptions · Sound/GB Colors/Joypad/Misc; the
active tab's group sits in the bottom row, bgb's multi-row behaviour) drawn as a
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
| Sound | volume + mono (`AudioPipe::set_volume` gain/downmix) |
| Graphics | stretch (→ fullscreen-stretched window size) |
| Debug | lowercase-hex + show-clocks (→ `DisasmFmt` via `tools.set_disasm_fmt`); "pressing Esc shows debugger" (`Settings.esc_shows_debugger`, default on → `handle_key` opens the debugger on Esc instead of quitting); RGBDS syntax; "memory viewer in own window" |
| Misc | fast-forward-speed + framerate-limit sliders (→ `app_pacing` `turbo_max_frames`/`frame_interval`); show-framerate (title); freeze-recent-ROMs (`push_recent` gate); pause-if-losing-focus (auto-pause on focus loss, auto-resume on refocus unless manually paused via `App.paused_by_focus`) |

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

Armed only while the debugger window is open (`dbg_armed`). Plan:
[`../bgb-exceptions-break-plan.md`](../bgb-exceptions-break-plan.md). The tab's other
conditions (OAM-DMA bad access, 16-bit inc/dec FE00-FEFF, SGB transfer, MBC,
inaccessible VRAM, halt+ints bug, uninitialized RAM) render but are **inert** — no
clean golden-safe detector/backend.

## Joypad (`keymap`)

Plan: [`../bgb-joypad-functional-plan.md`](../bgb-joypad-functional-plan.md).

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
- The rest (game-controller config/clear, Mappable-button-records,
  Screenshots/Rapid-speed/Screenshot-button combos, joystick-ID) is faithful-but-inert.

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

## Inert

SGB, game-controller config, WAV/AVI, rewind, RTC, Load-ROM-on-startup render
faithfully but inert — slopgb has no backend, and `App.settings` is in-memory only.
