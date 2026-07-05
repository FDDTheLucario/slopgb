# Frontend / bgb-UI implementation state

Per-area implementation state of the `crates/slopgb` frontend (the bgb functional
clone). The parallel of [`docs/hardware-state/`](../hardware-state/README.md), but
for the UI. **Read the relevant file before touching that UI area.**

| File | Covers |
|---|---|
| [game-menu.md](game-menu.md) | Game-window right-click menu + submenus (Window size, Sound channel, Other, State, Recent ROMs), info boxes, screenshot |
| [debugger.md](debugger.md) | Debugger window: focus keys, context menus, modal prompts, menu bar, Search, Evaluate, profiler, symbols, memory viewer, disassembler, UX |
| [options.md](options.md) | Options dialog: 8 tabs, live settings, Exceptions mask, Joypad rebind/SOCD, live input timing, bootrom UI, pure-bgb mode |
| [viewers.md](viewers.md) | VRAM viewer (CGB-attr-aware, resize, Palettes), I/O map, `&self` introspection accessors |
| [save-states-and-link.md](save-states-and-link.md) | Quick + on-disk save states, serial link cable (TCP, byte-level lockstep) |
| [startup-and-boot.md](startup-and-boot.md) | No-ROM blank-LCD startup, opt-in boot-ROM execution |
| [frontend-layout.md](frontend-layout.md) | Module split satisfying the <1000-line cap; key types/entry points |

## The golden-safe law (the one invariant)

Every core change made *for the UI* is read-only `&self` debug introspection
(`slopgb_core::debug` + a few `GameBoy` accessors) — it never advances a cycle or
mutates state, so the gbtr golden frame-hash stays **byte-identical**. Mutating
hooks (link, profiler, exception mask, channel mute) are **gated off by default**
(`link_connected`/`None`/`0`) so every golden path is byte-identical.

- **Do** verify any core touch with `cargo test -p slopgb-core --test gbtr` (the
  `golden_fingerprint` case) + the mooneye matrix.
- **Don't** add a core hook that runs on a golden path. Gate it on an opt-in flag
  that defaults to inert.

## Never invent bgb's UI — capture it

bgb runs under wine here; clone its UI from real screenshots, never from memory.

- Plans: [`bgb-clone-plan.md`](../bgb-clone-plan.md) (windows),
  [`bgb-rclick-menu-plan.md`](../bgb-rclick-menu-plan.md) (menus).
- Analysis-gated design decisions (keybinding routing, breakpoint/cursor state,
  save-state/reverse-exec/link scope): [`bgb-menu-design.md`](../bgb-menu-design.md).
- Real-screenshot spec + re-capture rig + gotchas:
  [`bgb-reference/`](../bgb-reference/README.md) (captured menus in `menus/`).

### Screenshot / drive rig

| To do this | How |
|---|---|
| Screenshot slopgb's own windows | `import -window "slopgb — debugger"` (by title; `-window root` misses them under a compositor) |
| Drive slopgb tab/checkbox UX | synthetic xdotool **clicks** reach winit windows; plain `xdotool key` does **not** |
| Open bgb's menus | synthetic wine clicks work — `click 3` (right-click menu), menubar `click 1` (dropdown) |
