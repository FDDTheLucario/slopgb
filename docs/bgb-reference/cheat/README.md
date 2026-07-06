# bgb Cheat UI — captured reference

Real bgb 1.6.4 screenshots (driven under wine/Xvfb, `bgbtest.gb` loaded). The
slopgb cheat engine is modeled on these — never invented.

## What bgb's cheat system is

Opened from the game window's right-click menu → **Cheat… (F10)**.

- **`cheats-dialog-empty.png`** — the Cheat window: a scrollable **list** of
  cheats, and two rows of buttons:
  - **Add · Edit · Delete · Enable all · Load** + an **Advanced** checkbox
  - **Enable · Disable · Poke · Disable all · Save**
- **`add-cheat-dialog.png`** — the Add/Edit dialog: a **Comment** field + a
  **Code** field + OK / Cancel.
- **`cheats-list-advanced.png`** — a cheat added, Advanced ON. The list row reads
  `01FF0AC1   S--   (C10A)=FF   infinite lives`:
  - `01FF0AC1` — the raw code.
  - `S--` — type flags (S = GameShark).
  - `(C10A)=FF` — the **decoded** effect (Advanced-only): write `FF` to `C10A`.
  - `infinite lives` — the comment.

## Decoding (verified from the screenshot)

**GameShark** = 8 hex `ttvvaaaa`: type `tt` (`01` = write RAM each frame), value
`vv`, address `aaaa` stored **little-endian**. `01FF0AC1` → value `FF`, address
`C10A` — exactly bgb's `(C10A)=FF`. Applied every frame (a RAM poke), which is
why bgb re-applies it continuously.

**Game Genie** (ROM patch, `AAA-BBB[-CCC]`) is accepted in the Code field too;
it patches ROM reads (needs a core read hook — recognized but not yet applied in
slopgb).

## slopgb mapping

`crates/slopgb/src/cheat.rs` (model + code parse + pokes + file codec) +
`cheat_ui.rs` (the dialog). Feature-complete vs bgb:

- **GameShark** RAM pokes re-apply each frame via `debug_write` (the freeze
  list's golden-safe path).
- **Game Genie** decodes to a `GgPatch` applied in `Cartridge::read_rom` — a
  default-off, golden-safe core hook pushed via `GameBoy::set_gg_patches`.
- **Two-field Add/Edit** (Comment / Code) like bgb; Tab switches fields.
- **Advanced** toggle shows the decoded `(addr)=val` column.
- **Load / Save** read/write a cheat file (`+ code comment` per line, `-` =
  disabled) via the shared path modal.
- Buttons: Add / Edit / Delete / Enable / Disable / Enable all / Disable all /
  Poke / Load / Save / Advanced / Close.

See [`../../ui-state/game-menu.md`](../../ui-state/game-menu.md).
