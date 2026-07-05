//! Keyboard mapping: physical key codes to Game Boy buttons and frontend
//! actions, **focus-dependent** like bgb (see [`map`] / [`Focus`]).
//!
//! Global (any focus): Z=A, X=B, Enter=Start, Right Shift / Backspace=Select,
//! arrows=D-pad, Tab (held)=turbo, P=pause, R=reset, Esc=quit, F9=break toggle.
//! Game-window F-keys: F2/F3/F4 open the debugger / VRAM / I-O-map windows.
//! Debugger-window F-keys: F2 toggle breakpoint, F3 step over, F4 run to cursor,
//! F7 trace (step), F8 step out, Ctrl+G go to, F5/F10 open VRAM/iomap.
//!
//! F1 (DMG palette toggle) is intentionally unmapped: `Ppu::set_dmg_palette`
//! is not reachable through the public `GameBoy` API. See the frontend report
//! for the requested core addition.

use slopgb_core::{Button, GameBoy};
use std::collections::HashSet;

use winit::keyboard::{KeyCode, ModifiersState};

use crate::ui::ToolWindow;

/// Apply queued joypad ops (`(button, pressed)`) to the machine at a sub-frame
/// `offset` (T-cycles into the current frame), draining `ops`. A no-op when
/// empty.
///
/// **Why the offset:** a press changes the joypad register and (on a press
/// edge) requests the joypad interrupt, which the program services at the
/// current LCD line (`LY`). If the frontend only ever applied input at frame
/// boundaries, every press would fire the interrupt at the same `LY` — the
/// "Incorrect behavior" the `tellinglys` ROM (input-entropy test) reports.
/// Stepping a wall-clock-derived offset into the frame before the press makes
/// the interrupt land on a varied line, matching real hardware ("Pass! Joypad
/// interrupt timing is realistic"). `offset` is kept below one frame so the
/// caller's `run_frame` then finishes the same frame.
pub fn apply_input(gb: &mut GameBoy, ops: &mut Vec<(Button, bool)>, offset: u32) {
    if ops.is_empty() {
        return;
    }
    let target = gb.cycles().wrapping_add(u64::from(offset));
    while gb.cycles() < target {
        gb.step();
    }
    for (button, pressed) in ops.drain(..) {
        if pressed {
            gb.press(button);
        } else {
            gb.release(button);
        }
    }
}

/// Which window currently has focus — the key map is focus-dependent, exactly
/// like bgb (the debugger's F-keys differ from the game window's). Resolved in
/// `main` from the window the key event arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The game LCD window — the only window that drives the GB joypad.
    Game,
    /// A non-debugger tool window (VRAM / I/O map / memory viewer): the game-style
    /// hotkeys apply, but Game Boy buttons do **not** (a viewer must not move the
    /// joypad).
    Viewer,
    /// The debugger window.
    Debugger,
}

/// What a mapped key does. Game Boy buttons are *not* here: they resolve through
/// the rebindable [`crate::keymap::KeyBindings`] in `App::handle_key` before this
/// map is consulted, so the Joypad "configure keyboard" wizard can remap them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Uncapped emulation speed while held.
    Turbo,
    /// Toggle pause (on press).
    Pause,
    /// Power-cycle the machine, reloading save RAM (on press).
    Reset,
    /// Quit the emulator (on press).
    Quit,
    /// Open/close a bgb-style debug tool window (on press).
    ToggleTool(ToolWindow),
    /// Toggle the debugger break (freeze emulation) — F9, focus-independent.
    DbgBreak,
    /// Single-step one instruction (bgb "Trace") — debugger F7, on press.
    DbgStep,
    /// Step over a call/rst — debugger F3, on press.
    DbgStepOver,
    /// Toggle a breakpoint at the cursor — debugger F2, on press.
    DbgToggleBreakpoint,
    /// Run to the cursor — debugger F4, on press.
    DbgRunToCursor,
    /// Jump PC to the cursor without running — debugger F6, on press.
    DbgJumpToCursor,
    /// Open the breakpoint manager (list) — debugger Ctrl+H, on press.
    DbgManageBreakpoints,
    /// Open the watchpoint manager (list) — debugger Ctrl+J, on press.
    DbgManageWatchpoints,
    /// Re-center the disasm on PC (unpin) — debugger Ctrl+A, on press.
    DbgGoToPc,
    /// Step out of the current subroutine — debugger F8, on press.
    DbgStepOut,
    /// Open the Go-to address prompt — debugger Ctrl+G, on press.
    DbgGoto,
    /// Toggle audio output (bgb's main-menu "Enable sound"). Menu-only — no key
    /// binds it, so [`map`] never returns it; it rides the shared `run_action`.
    ToggleSound,
    /// Save the current frame to a BMP (bgb's main-menu "Save screenshot").
    /// Menu-only — no key binds it.
    SaveScreenshot,
    /// Dump the whole 64 KiB address space to a file (debugger File →
    /// "save memory_dump..."). Menu-only.
    DbgSaveMemDump,
    /// Show the Options info box (main menu "Options...", F11). Menu-only stub.
    MainOptions,
    /// Show the Cheats info box (main menu "Cheat...", F10). Menu-only stub.
    MainCheats,
    /// Execution profiler → "logging mode": tally each executed PC (MB5).
    /// Menu-only (Execution-profiler dropdown).
    ProfilerLogging,
    /// Execution profiler → "break mode": logging + halt on each address's first
    /// execution. Menu-only.
    ProfilerBreak,
    /// Execution profiler → "stop": disable profiling. Menu-only.
    ProfilerStop,
    /// Execution profiler → "clear buffer": zero the tally. Menu-only.
    ProfilerClear,
    /// Open the Search-string prompt (Search → Search string, Ctrl+F).
    DbgSearch,
    /// Re-run the search from after the last hit (Continue search, Ctrl+C).
    DbgContinueSearch,
    /// Go to the next bookmark-or-breakpoint (Ctrl+N).
    DbgNextBookmark,
    /// Go to the previous bookmark-or-breakpoint (Ctrl+B).
    DbgPrevBookmark,
    /// Set numbered bookmark slot N at the cursor (Ctrl+Shift+digit).
    DbgSetBookmark(u8),
    /// Jump to numbered bookmark slot N (Ctrl+digit).
    DbgGotoBookmark(u8),
    /// Open the Evaluate-expression prompt (Debug → Evaluate expression). Menu-only.
    DbgEvaluate,
    /// Evaluate the entered expression + show the result. Internal (from accept).
    DbgEvalRun,
    /// Zero the regs-pane `cnt` user-clock counter (Debug → Set user clocks
    /// counter). Menu-only.
    DbgSetUserClocks,
    /// Open the game-window "Load ROM" path prompt (main menu). Menu-only.
    MainLoadRom,
    /// Export the disassembly of the current region to a text file (debugger
    /// File → "save asm..."). Menu-only.
    DbgSaveAsm,
    /// Open the on-disk Save-state path prompt (debugger File → "Save state...",
    /// Ctrl+W).
    DbgSaveState,
    /// Open the on-disk Load-state path prompt (debugger File → "Load state...",
    /// Ctrl+L).
    DbgLoadState,
    /// Open the `.sym` symbol-file load prompt (debugger Debug → "Load symbols...").
    DbgLoadSymbols,
    /// Copy 16 hex bytes at the cursor to the clipboard (disasm/memory right-click
    /// "Copy data", RM10). Menu-only; carries the clicked address.
    DbgCopyData(u16),
    /// Copy 16 disassembled rows at the cursor to the clipboard (disasm/memory
    /// right-click "Copy code", RM10). Menu-only; carries the clicked address.
    DbgCopyCode(u16),
    /// Scroll the debugger memory pane by `n` rows of 16 bytes (arrow keys: ±1).
    DbgMemScroll(i32),
    /// Page the debugger memory pane by one visible page in direction `±1`
    /// (PageUp/PageDown); the page size is the pane's visible row count.
    DbgMemPage(i32),
}

/// Tracks which physical keys currently hold each button, so two keys mapped
/// to the same button (Right Shift and Backspace both press Select) don't
/// release it while the other key is still physically held.
#[derive(Default)]
pub struct ButtonTracker {
    /// Currently held mapped keys. At most one entry per `KeyCode`; tiny, so
    /// a Vec beats a set.
    held: Vec<(KeyCode, Button)>,
}

impl ButtonTracker {
    /// Record a key press for `button`.
    pub fn press(&mut self, code: KeyCode, button: Button) {
        if !self.held.iter().any(|&(c, _)| c == code) {
            self.held.push((code, button));
        }
    }

    /// Record a key release for `button`. Returns true if the button should
    /// actually be released (no other held key still maps to it).
    pub fn release(&mut self, code: KeyCode, button: Button) -> bool {
        self.held.retain(|&(c, _)| c != code);
        !self.held.iter().any(|&(_, b)| b == button)
    }

    /// Whether any currently-held key maps to `button` (for the SOCD filter's
    /// resurrection of a still-held opposite direction).
    #[must_use]
    pub fn is_held(&self, button: Button) -> bool {
        self.held.iter().any(|&(_, b)| b == button)
    }

    /// Forget all held keys (e.g. on focus loss, when release events for
    /// currently held keys will never arrive).
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

/// Map a physical key to its action under the current window `focus`, if any.
///
/// Global keys (turbo/pause/reset/quit, and **F9** break which stays
/// focus-independent so a frozen machine is always resumable) bind in any focus.
/// Game Boy *buttons* are not handled here — they resolve through the rebindable
/// [`crate::keymap::KeyBindings`] before this is called. The F-keys then split
/// by focus, matching bgb: in the **game** window
/// F2/F3/F4 open the debugger/VRAM/iomap windows; in the **debugger** window
/// they become bgb's debugger keys (F2 toggle breakpoint, F3 step over, F4 run
/// to cursor, F6 jump to cursor, F7 trace, F8 step out, Ctrl+G go to, F5/F10
/// open VRAM/iomap). Keys for not-yet-built features (F12 load ROM) stay unmapped.
#[must_use]
pub fn map(code: KeyCode, mods: ModifiersState, focus: Focus) -> Option<Action> {
    // Global (any focus): emulator controls + the break toggle. Game Boy
    // buttons are resolved earlier, through `App.bindings` (rebindable).
    let global = match code {
        KeyCode::Tab => Some(Action::Turbo),
        KeyCode::KeyP => Some(Action::Pause),
        KeyCode::KeyR => Some(Action::Reset),
        // Escape is deliberately *not* bound here: bgb shows the debugger on Esc
        // (never quits). `App::handle_key` intercepts it so it can honour the
        // "pressing Esc shows debugger" Options flag + the focus. See BUG-1.
        KeyCode::F9 => Some(Action::DbgBreak),
        _ => None,
    };
    if global.is_some() {
        return global;
    }
    match focus {
        Focus::Debugger => match code {
            KeyCode::F2 => Some(Action::DbgToggleBreakpoint),
            KeyCode::F3 => Some(Action::DbgStepOver),
            KeyCode::F4 => Some(Action::DbgRunToCursor),
            KeyCode::F6 => Some(Action::DbgJumpToCursor),
            KeyCode::F7 => Some(Action::DbgStep),
            KeyCode::F8 => Some(Action::DbgStepOut),
            KeyCode::F5 => Some(Action::ToggleTool(ToolWindow::Vram)),
            KeyCode::F10 => Some(Action::ToggleTool(ToolWindow::IoMap)),
            KeyCode::KeyG if mods.control_key() => Some(Action::DbgGoto),
            KeyCode::KeyH if mods.control_key() => Some(Action::DbgManageBreakpoints),
            KeyCode::KeyJ if mods.control_key() => Some(Action::DbgManageWatchpoints),
            KeyCode::KeyA if mods.control_key() => Some(Action::DbgGoToPc),
            KeyCode::KeyF if mods.control_key() => Some(Action::DbgSearch),
            KeyCode::KeyC if mods.control_key() => Some(Action::DbgContinueSearch),
            KeyCode::KeyN if mods.control_key() => Some(Action::DbgNextBookmark),
            KeyCode::KeyB if mods.control_key() => Some(Action::DbgPrevBookmark),
            KeyCode::KeyW if mods.control_key() => Some(Action::DbgSaveState),
            KeyCode::KeyL if mods.control_key() => Some(Action::DbgLoadState),
            // Memory-pane navigation (bgb): arrows scroll a row, PageUp/Down a page.
            KeyCode::ArrowUp => Some(Action::DbgMemScroll(-1)),
            KeyCode::ArrowDown => Some(Action::DbgMemScroll(1)),
            KeyCode::PageUp => Some(Action::DbgMemPage(-1)),
            KeyCode::PageDown => Some(Action::DbgMemPage(1)),
            // Ctrl+Shift+digit sets a numbered bookmark; Ctrl+digit jumps to it
            // (bgb). Placed after the named Ctrl keys so they take precedence.
            _ if mods.control_key() => digit_of(code).map(|d| {
                if mods.shift_key() {
                    Action::DbgSetBookmark(d)
                } else {
                    Action::DbgGotoBookmark(d)
                }
            }),
            _ => None,
        },
        // The game window and the viewers share the same hotkeys; only the game
        // window additionally drives the joypad (gated in `App::handle_key`).
        Focus::Game | Focus::Viewer => match code {
            KeyCode::F2 => Some(Action::ToggleTool(ToolWindow::Debugger)),
            KeyCode::F3 => Some(Action::ToggleTool(ToolWindow::Vram)),
            KeyCode::F4 => Some(Action::ToggleTool(ToolWindow::IoMap)),
            // bgb's main-menu "Options..." hotkey (F11).
            KeyCode::F11 => Some(Action::MainOptions),
            _ => None,
        },
    }
}

/// The digit 0-9 a key represents (top-row or numpad), for the numbered-bookmark
/// shortcuts; `None` for any other key.
fn digit_of(code: KeyCode) -> Option<u8> {
    Some(match code {
        KeyCode::Digit0 | KeyCode::Numpad0 => 0,
        KeyCode::Digit1 | KeyCode::Numpad1 => 1,
        KeyCode::Digit2 | KeyCode::Numpad2 => 2,
        KeyCode::Digit3 | KeyCode::Numpad3 => 3,
        KeyCode::Digit4 | KeyCode::Numpad4 => 4,
        KeyCode::Digit5 | KeyCode::Numpad5 => 5,
        KeyCode::Digit6 | KeyCode::Numpad6 => 6,
        KeyCode::Digit7 | KeyCode::Numpad7 => 7,
        KeyCode::Digit8 | KeyCode::Numpad8 => 8,
        KeyCode::Digit9 | KeyCode::Numpad9 => 9,
        _ => return None,
    })
}

/// Decide whether a key event should be acted on, maintaining `held` (the set of
/// physically-held keys). A `Pressed` for a key already in `held` is a key-repeat
/// and returns `false`, so a held step key (F7/F3/F8) fires exactly once — a
/// platform-independent guard, because winit's [`KeyEvent::repeat`] flag is
/// unreliable on some Wayland compositors. The first press inserts and returns
/// `true`; every release removes the key and returns `true` (releases must always
/// be honored so a button can't stick).
///
/// [`KeyEvent::repeat`]: winit::event::KeyEvent::repeat
pub fn accept_key(held: &mut HashSet<KeyCode>, code: KeyCode, pressed: bool) -> bool {
    if pressed {
        held.insert(code)
    } else {
        held.remove(&code);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slopgb_core::Model;

    const NONE: ModifiersState = ModifiersState::empty();
    const CTRL: ModifiersState = ModifiersState::CONTROL;

    /// `apply_input` at varied sub-frame offsets lands the press on varied LCD
    /// lines (LY) — the input entropy `tellinglys` checks. A frontend that only
    /// pressed at frame boundaries would read the same LY every time and fail.
    #[test]
    fn input_offset_varies_the_joypad_line() {
        // Post-boot DMG has the LCD on (LCDC = 0x91), so LY advances with cycles.
        fn rom() -> Vec<u8> {
            let mut r = vec![0u8; 0x8000];
            r[0x100..0x102].copy_from_slice(&[0x18, 0xFE]); // jr -2 (idle loop)
            r
        }
        let ly_at = |offset: u32| -> u8 {
            let mut gb = GameBoy::new(Model::Dmg, rom()).unwrap();
            let mut ops = vec![(Button::A, true)];
            apply_input(&mut gb, &mut ops, offset);
            assert!(ops.is_empty(), "ops drained");
            gb.debug_read(0xFF44) // LY at the moment the press is applied
        };
        // One T-line is 456 T-cycles; sample lines across the frame.
        let lines: Vec<u8> = [4, 36, 68, 100, 130]
            .iter()
            .map(|&l| ly_at(l * 456))
            .collect();
        let distinct: std::collections::HashSet<u8> = lines.iter().copied().collect();
        assert!(
            distinct.len() >= 4,
            "varied offsets give varied LY (>=4 distinct): {lines:?}"
        );
        // Sanity: an offset of 0 (frame boundary, the old behavior) is the low end.
        assert!(ly_at(0) < ly_at(100 * 456), "offset advances the line");
    }

    /// Map under game focus, no modifiers (the common case).
    fn g(code: KeyCode) -> Option<Action> {
        map(code, NONE, Focus::Game)
    }
    /// Map under debugger focus, no modifiers.
    fn d(code: KeyCode) -> Option<Action> {
        map(code, NONE, Focus::Debugger)
    }

    #[test]
    fn debugger_ctrl_search_and_bookmark_keys() {
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
        let dc = |c| map(c, CTRL, Focus::Debugger);
        assert_eq!(dc(KeyCode::KeyF), Some(Action::DbgSearch));
        assert_eq!(dc(KeyCode::KeyC), Some(Action::DbgContinueSearch));
        assert_eq!(dc(KeyCode::KeyN), Some(Action::DbgNextBookmark));
        assert_eq!(dc(KeyCode::KeyB), Some(Action::DbgPrevBookmark));
        // Ctrl+digit jumps to a bookmark; Ctrl+Shift+digit sets one.
        assert_eq!(dc(KeyCode::Digit3), Some(Action::DbgGotoBookmark(3)));
        assert_eq!(dc(KeyCode::Numpad7), Some(Action::DbgGotoBookmark(7)));
        assert_eq!(
            map(KeyCode::Digit3, ctrl_shift, Focus::Debugger),
            Some(Action::DbgSetBookmark(3))
        );
        // A named Ctrl key still wins over the digit catch-all.
        assert_eq!(dc(KeyCode::KeyA), Some(Action::DbgGoToPc));
        // On-disk save states: Ctrl+W save / Ctrl+L load (debugger focus only).
        assert_eq!(dc(KeyCode::KeyW), Some(Action::DbgSaveState));
        assert_eq!(dc(KeyCode::KeyL), Some(Action::DbgLoadState));
        assert_eq!(map(KeyCode::KeyW, CTRL, Focus::Game), None);
        // Search/bookmark keys are debugger-focus only; the game keeps its map.
        assert_eq!(map(KeyCode::KeyF, CTRL, Focus::Game), None);
        assert_eq!(map(KeyCode::Digit3, CTRL, Focus::Game), None);
        // No modifier: a bare digit is unmapped in the debugger.
        assert_eq!(d(KeyCode::Digit3), None);
    }

    #[test]
    fn button_keys_are_not_in_the_action_map() {
        // In the game window, Game Boy buttons resolve through `App.bindings`
        // (rebindable) before `map` is consulted, so the default button keys are
        // unmapped here. (A tool window doesn't drive the joypad — `handle_key`
        // gates button resolution on `Focus::Game`.)
        assert_eq!(map(KeyCode::ArrowUp, NONE, Focus::Game), None);
        assert_eq!(map(KeyCode::ArrowRight, NONE, Focus::Game), None);
        for f in [Focus::Game, Focus::Debugger] {
            assert_eq!(map(KeyCode::KeyZ, NONE, f), None);
            assert_eq!(map(KeyCode::KeyX, NONE, f), None);
            assert_eq!(map(KeyCode::Enter, NONE, f), None);
        }
    }

    #[test]
    fn viewer_focus_has_game_hotkeys_but_no_joypad() {
        // A non-debugger tool window shares the game hotkeys (F-keys/Options)...
        let v = |c| map(c, NONE, Focus::Viewer);
        assert_eq!(v(KeyCode::F3), Some(Action::ToggleTool(ToolWindow::Vram)));
        assert_eq!(v(KeyCode::F11), Some(Action::MainOptions));
        // ...but button-mapped keys stay unmapped (handle_key gates the joypad on
        // Focus::Game, so a viewer never moves the D-pad).
        assert_eq!(v(KeyCode::ArrowUp), None);
        assert_eq!(v(KeyCode::KeyZ), None);
    }

    #[test]
    fn debugger_arrows_and_pages_scroll_memory() {
        // In the debugger window arrows scroll the memory pane (the game window
        // would consume them as the D-pad before `map` is reached).
        let d = |c| map(c, NONE, Focus::Debugger);
        assert_eq!(d(KeyCode::ArrowUp), Some(Action::DbgMemScroll(-1)));
        assert_eq!(d(KeyCode::ArrowDown), Some(Action::DbgMemScroll(1)));
        assert_eq!(d(KeyCode::PageUp), Some(Action::DbgMemPage(-1)));
        assert_eq!(d(KeyCode::PageDown), Some(Action::DbgMemPage(1)));
    }

    #[test]
    fn unmapped_keys_do_nothing() {
        assert_eq!(g(KeyCode::KeyQ), None);
        assert_eq!(g(KeyCode::F1), None); // palette toggle: needs core API
        // F6 in the *game* focus is still unmapped (debugger-only key).
        assert_eq!(g(KeyCode::F6), None);
        // Not-yet-built debugger keys stay unmapped (no dead actions).
        assert_eq!(d(KeyCode::F12), None); // load ROM (MB2/MN4)
    }

    #[test]
    fn game_focus_function_keys_open_the_tool_windows() {
        assert_eq!(
            g(KeyCode::F2),
            Some(Action::ToggleTool(ToolWindow::Debugger))
        );
        assert_eq!(g(KeyCode::F3), Some(Action::ToggleTool(ToolWindow::Vram)));
        assert_eq!(g(KeyCode::F4), Some(Action::ToggleTool(ToolWindow::IoMap)));
    }

    #[test]
    fn debugger_focus_function_keys_are_bgb_debugger_keys() {
        assert_eq!(d(KeyCode::F2), Some(Action::DbgToggleBreakpoint));
        assert_eq!(d(KeyCode::F3), Some(Action::DbgStepOver));
        assert_eq!(d(KeyCode::F4), Some(Action::DbgRunToCursor));
        assert_eq!(d(KeyCode::F6), Some(Action::DbgJumpToCursor));
        assert_eq!(d(KeyCode::F7), Some(Action::DbgStep));
        assert_eq!(d(KeyCode::F8), Some(Action::DbgStepOut));
        // F8 step out is debugger-only — the game window keeps it unmapped.
        assert_eq!(g(KeyCode::F8), None);
        assert_eq!(d(KeyCode::F5), Some(Action::ToggleTool(ToolWindow::Vram)));
        assert_eq!(d(KeyCode::F10), Some(Action::ToggleTool(ToolWindow::IoMap)));
        // Ctrl+G is Go to; plain G does nothing.
        assert_eq!(
            map(KeyCode::KeyG, CTRL, Focus::Debugger),
            Some(Action::DbgGoto)
        );
        assert_eq!(d(KeyCode::KeyG), None);
        // Ctrl+H / Ctrl+J open the breakpoint / watchpoint managers (RM15).
        assert_eq!(
            map(KeyCode::KeyH, CTRL, Focus::Debugger),
            Some(Action::DbgManageBreakpoints)
        );
        assert_eq!(
            map(KeyCode::KeyJ, CTRL, Focus::Debugger),
            Some(Action::DbgManageWatchpoints)
        );
        // They are debugger-only + need Ctrl.
        assert_eq!(map(KeyCode::KeyH, CTRL, Focus::Game), None);
        assert_eq!(d(KeyCode::KeyH), None);
        // Ctrl+A re-centers the disasm on PC (Search → go to PC).
        assert_eq!(
            map(KeyCode::KeyA, CTRL, Focus::Debugger),
            Some(Action::DbgGoToPc)
        );
        assert_eq!(map(KeyCode::KeyA, CTRL, Focus::Game), None);
        assert_eq!(d(KeyCode::KeyA), None, "needs Ctrl");
    }

    #[test]
    fn f9_break_is_focus_independent() {
        // Resume-safety: F9 toggles break from any focus.
        assert_eq!(g(KeyCode::F9), Some(Action::DbgBreak));
        assert_eq!(d(KeyCode::F9), Some(Action::DbgBreak));
    }

    #[test]
    fn escape_is_unmapped_in_every_focus() {
        // bgb shows the debugger on Esc; it never quits the emulator. `map` no
        // longer binds Escape at all — `App::handle_key` intercepts it directly
        // so it can honour the "pressing Esc shows debugger" Options flag and the
        // focus (the pure map can't see runtime settings). See BUG-1.
        for f in [Focus::Game, Focus::Viewer, Focus::Debugger] {
            assert_eq!(
                map(KeyCode::Escape, NONE, f),
                None,
                "Escape unmapped ({f:?})"
            );
        }
    }

    #[test]
    fn tracker_keeps_select_held_while_either_key_is_down() {
        let mut t = ButtonTracker::default();
        t.press(KeyCode::ShiftRight, Button::Select);
        t.press(KeyCode::Backspace, Button::Select);
        // Backspace still holds Select after Right Shift is released.
        assert!(!t.release(KeyCode::ShiftRight, Button::Select));
        assert!(t.release(KeyCode::Backspace, Button::Select));
    }

    #[test]
    fn tracker_releases_independent_buttons_independently() {
        let mut t = ButtonTracker::default();
        t.press(KeyCode::KeyZ, Button::A);
        t.press(KeyCode::KeyX, Button::B);
        assert!(t.release(KeyCode::KeyZ, Button::A));
        assert!(t.release(KeyCode::KeyX, Button::B));
    }

    #[test]
    fn tracker_is_held_reports_button_state() {
        let mut t = ButtonTracker::default();
        assert!(!t.is_held(Button::Left));
        t.press(KeyCode::ArrowLeft, Button::Left);
        assert!(t.is_held(Button::Left));
        assert!(!t.is_held(Button::Right));
        t.release(KeyCode::ArrowLeft, Button::Left);
        assert!(!t.is_held(Button::Left));
    }

    #[test]
    fn tracker_release_without_press_still_releases() {
        // A release whose press was never seen (key held before focus gain)
        // must not leave the button stuck.
        let mut t = ButtonTracker::default();
        assert!(t.release(KeyCode::KeyZ, Button::A));
    }

    #[test]
    fn tracker_ignores_duplicate_presses() {
        let mut t = ButtonTracker::default();
        t.press(KeyCode::ShiftRight, Button::Select);
        t.press(KeyCode::ShiftRight, Button::Select);
        assert!(t.release(KeyCode::ShiftRight, Button::Select));
    }

    #[test]
    fn tracker_clear_forgets_held_keys() {
        let mut t = ButtonTracker::default();
        t.press(KeyCode::ShiftRight, Button::Select);
        t.clear();
        assert!(t.release(KeyCode::Backspace, Button::Select));
    }

    #[test]
    fn accept_key_filters_held_repeats_but_honors_releases() {
        let mut held = HashSet::new();
        assert!(accept_key(&mut held, KeyCode::F7, true), "first press acts");
        assert!(
            !accept_key(&mut held, KeyCode::F7, true),
            "held repeat ignored"
        );
        assert!(
            !accept_key(&mut held, KeyCode::F7, true),
            "still ignored while held"
        );
        assert!(accept_key(&mut held, KeyCode::F7, false), "release acts");
        assert!(held.is_empty(), "release clears the held entry");
        assert!(
            accept_key(&mut held, KeyCode::F7, true),
            "a fresh press after release acts again"
        );
        // A release for a never-pressed key is still honored (no stuck button).
        assert!(accept_key(&mut held, KeyCode::F3, false));
    }
}
