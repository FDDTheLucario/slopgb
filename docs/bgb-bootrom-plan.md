# Boot-ROM support — TDD plan + golden-safety contract

bgb plays the boot ROM (Nintendo logo scroll + "po-ling" chime + header/logo check) when a bootrom
path is configured. slopgb currently runs **no** boot ROM — `GameBoy::new` installs the calibrated
post-boot state directly (`interconnect/boot.rs apply_post_boot_state`) and FF50 is a no-op. The user
vendored a full boot-ROM set in `bootroms/` (DMG 256 B, CGB 2304 B, MGB/SGB/…). This adds **opt-in**
boot-ROM execution.

## The golden-safety invariant (non-negotiable)

Power-on / the memory map / boot regs are the most-calibrated core (mooneye `boot_regs`/`boot_hwio`/
`boot_div`, gambatte div timing). **With NO boot ROM attached (the default + every gbtr/mooneye path)
behavior must stay byte-identical:**

- `GameBoy::new` is unchanged — `Registers::post_boot` + `apply_post_boot_state`. The boot-ROM path is
  a *separate* constructor `new_with_boot`.
- The interconnect's `boot_rom: Option<Vec<u8>>` defaults `None` and `boot_active` defaults `false`; the
  boot-region read branch and the FF50-disable branch are gated on `boot_active`, so a no-boot machine
  takes the exact current read path. The boot ROM is **not** serialized.
- Proof: a `new_without_boot_is_unchanged` unit test + the full **gbtr golden byte-identical** + mooneye
  battery (none attach a boot ROM).

## Power-on state (what `new_with_boot` sets, vs `new`'s post-boot)

- `Registers::power_on()`: AF/BC/DE/HL = 0, SP = 0, **PC = 0x0000** (the boot ROM runs from 0).
- Bus at power-on, **not** `apply_post_boot_state`: LCDC (FF40) = 0x00 (LCD off — the boot ROM turns it
  on), DIV/timer/PPU/APU at power-on. The boot ROM produces the hand-off state itself.

## Boot-region read map (while `boot_active`)

A CPU read in the boot region returns boot-ROM bytes instead of cart ROM; size selects the region:

| Boot ROM | Class | Region overlaid on cart ROM |
|---|---|---|
| 256 B | DMG / MGB / SGB | `0x0000–0x00FF` |
| 2304 B | CGB / AGB | `0x0000–0x00FF` **and** `0x0200–0x08FF` (the `0x0100–0x01FF` cart-header window stays cart) |

Everything else (VRAM/RAM/IO) is the normal path. The branch is gated on `boot_active` → inert by default.

## FF50 hand-off

While `boot_active`, a write to FF50 with bit 0 set permanently clears `boot_active` (the boot ROM
unmaps itself; `0x0000+` reads then hit cart ROM, PC continues into the cart at `0x0100`). With **no**
boot ROM attached FF50 stays the current no-op (reads `0xFF`, ignores writes) — golden-safe.

## Convergence contract

The direct-init post-boot regs ARE the real boot ROM's hand-off regs. So running `dmg_boot.bin` from
power-on must reach **PC = 0x0100 and `Registers::post_boot(Dmg)`** at the FF50 hand-off — the oracle
(`bootrom_converges_to_post_boot`, skipped if the file is absent).

## Frontend

First milestone: a `--boot <path>` CLI option + `SLOPGB_BOOT` env var → `std::fs` read → `new_with_boot`
when set + readable + size matches the model, else `new()` (missing/bad file falls back, logged).
The bgb-faithful **Options bootrom-path UI** (DMG/CGB path fields) is a deferred second milestone — it
needs a real bgb Options capture (slopgb's Options has no bootrom controls yet).

## Task order (see the session's /tdd-test-plan output)

0 analysis (this doc) → 1 `power_on` regs ‖ 2 interconnect boot fields → 3 boot-region read map → 4 FF50
disable → 5 `new_with_boot` power-on wiring → 6 no-boot golden guard → **golden-gate** → 7 convergence
oracle → 8 CLI/env boot path → 9 (deferred) bgb Options UI capture + wiring.

`bootroms/` is gitignored (not vendored into the repo); tests load `bootroms/dmg_boot.bin` if present,
skip otherwise (like the test-rom harness).

## Known limitation

The boot ROM + `boot_active` are **not serialized** (like the debugger/link transient state). A save
state taken **mid-boot** (during the ~2 s logo while `boot_active` is true) therefore won't resume the
boot mapping — on load `boot_active` is false and `0x0000+` reads hit the cart. Post-boot saves (the
normal case, after FF50 hand-off) are unaffected. Acceptable for the foundation; revisit only if a
mid-boot save is ever wanted (would need to serialize the boot-ROM bytes).

## Status (this milestone)

Tasks 1–6 done (the golden-safe core boot-ROM execution mechanism): `Registers::power_on`/
`Cpu::power_on`, the interconnect `boot_rom`/`boot_active` + `attach_boot_rom`/`boot_rom_byte`
(`interconnect/boot_rom.rs`), the boot-region read overlay + FF50 disable, `GameBoy::new_with_boot`,
and the no-boot golden guard. Empirically golden-safe: **gbtr golden 185/185 byte-identical + mooneye
0-failed**. Remaining: task 7 (convergence oracle with the real `dmg_boot.bin`), task 8 (`--boot`/
`SLOPGB_BOOT` CLI/env wiring), task 9 (bgb-faithful Options bootrom-path UI — needs a real bgb capture).

## Task 9 — Options System-tab bootrom-path UI (1:1 with bgb)

Real bgb reference: `docs/bgb-reference/options/options-system.png` — the System tab's RIGHT side has
**DMG bootrom: / GBC bootrom: / SGB bootrom:** path fields (each a text box + a `...` browse button)
and a **bootroms enabled** checkbox.

Plan (functional 1:1, dep-free, frontend-only → golden-safe):
1. `Settings`: `bootroms_enabled: bool` + `bootrom_dmg/gbc/sgb: String` (default off/empty).
2. `BootromSlot{Dmg,Gbc,Sgb}` + `Field::BootromsEnabled` + `Field::PickBootrom(slot)` +
   `OptionsOutcome::PickBootrom(slot)`.
3. `on_click`: the checkbox flips the flag; a `...` button returns the outcome (like ConfigureKeyboard).
4. `system()` builder: render the bootrom group (3 label+box+`...` rows + checkbox) matching the capture.
5. Route the outcome: `PathPurpose::Bootrom(slot)` → the shared path modal over the dialog → write the
   path into `options.working.bootrom_<slot>` (OK/Apply commits, Cancel reverts).
6. Resolve a Settings boot ROM on ROM load (enabled + model-matched slot path + size-valid → those bytes;
   overrides `--boot`); re-resolve on apply_settings + each load.
7. Live-screenshot verify slopgb's System tab vs bgb on :0 (real captures).
