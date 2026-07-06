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

`crates/slopgb/src/cheat.rs` (model + code parse + per-frame pokes) +
`cheat_ui.rs` (the dialog). GameShark RAM pokes re-apply each frame via
`debug_write` — the same golden-safe path the freeze list uses. slopgb's Add/Edit
reuses the shared single-line modal with a `comment = code` convention (bgb has
two fields); Load/Save `.cht` files are not yet implemented. See
[`../../ui-state/game-menu.md`](../../ui-state/game-menu.md).
