# No-ROM startup + opt-in boot-ROM execution

## No-ROM startup (bgb-style)

The ROM CLI arg is optional (`cli::Options.rom: Option<PathBuf>`). Launched without a
ROM, slopgb opens to a blank LCD frozen at power-on (`Session::blank`,
`App.rom_loaded=false` → `should_idle` gates emulation off) showing a solid
lightest-shade frame (`App.blank_frame`). A ROM loads later via drag-drop / Load
ROM... / Recent ROMs, which flips `rom_loaded` + `App::apply_palette`.

The default DMG palette is bgb's pale-green LCD (`SCHEMES[0]` "BGB 0.3" =
`E8FCCC ACD490 548C70 142C38`, decoded from `bgb.ini` BGR; `App::apply_palette`
pushes it at startup + each (re)load) so a fresh slopgb matches bgb; the core
power-on default stays grayscale. Title is bare `"slopgb"` with no ROM
(`window_title`). Plan: [`../bgb-noload-startup-plan.md`](../bgb-noload-startup-plan.md).

## Opt-in boot-ROM execution

Core foundation done. `GameBoy::new_with_boot(model, rom, boot_rom)` runs a real boot
ROM (Nintendo logo scroll + chime) from power-on:

- `Registers::power_on`/`Cpu::power_on` (PC=0, regs 0); the boot ROM mapped over the
  low cart region (`interconnect/boot_rom.rs` `attach_boot_rom`/`boot_rom_byte`:
  256 B DMG-class → 0000-00FF; 2304 B CGB-class → 0000-00FF + 0200-08FF,
  0100-01FF=cart); FF50-bit0 write hands off (unmaps); no `apply_post_boot_state`.
- A CGB/AGB machine enters true power-on **CGB mode** while booting
  (`attach_boot_rom`→`set_cgb_mode(true)`) regardless of cart, so a DMG cart's
  boot-ROM compat-palette/OPRI writes land; the boot ROM's KEY0/FF4C DMG-lock (bit 2,
  gated on `boot_active`) re-locks DMG-compat before hand-off (converges to `new`'s
  precomputed `cgb_mode`).

**Golden-safe by opt-in** — `new` is untouched (its post-boot body is the shared
`GameBoy::post_boot`); `boot_active` defaults false so the boot-region read +
FF50-disable + FF4C + `set_cgb_mode` branches are never taken (gbtr golden
byte-identical + mooneye green). Boot ROM not serialized (mid-boot save unsupported).

### Wiring

`--boot <path>` / `SLOPGB_BOOT` (`resolve_boot_rom` → `Session::build_gb`/
`boot_size_ok`: 256 B DMG-class / 2304 B CGB-class; wrong size falls back to
post-boot, logged). Core `new_with_boot` *also* self-validates the size and falls
back to `post_boot`, so a direct caller can't build a half-mapped machine. Applied on
every ROM load via `App.boot_rom`. `Session::reset`/`set_model` **re-run** the boot
ROM (re-resolve the per-model `OwnedBootSpec` through `build_gb`), so a power-cycle /
model switch replays the logo+chime like bgb.

### Options System-tab bootrom UI (1:1 with `options-system.png`)

DMG/GBC/SGB path fields + `...` browse buttons (`Field::PickBootrom`→
`OptionsOutcome::PickBootrom`→a path modal over the dialog, writing into
`options.working`) + a live "bootroms enabled" checkbox. Resolved per model on ROM
load (`Session::BootSpec`, Options paths over `--boot`). SGB maps to the DMG-class
256 B boot ROM.

### Validation

`tests/bootrom.rs` runs the **real `dmg_boot.bin`** from power-on and converges to
the direct-init post-boot regs at FF50 hand-off (skipped if absent, or a hard failure
under `SLOPGB_REQUIRE_ROMS=1`). `bootroms/` is gitignored — copyrighted boot ROMs
never vendored. Plan: [`../bgb-bootrom-plan.md`](../bgb-bootrom-plan.md).
