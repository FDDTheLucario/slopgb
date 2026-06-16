# bgb Joypad tab — functional clone plan (/tdd-test-plan)

Make slopgb's Options → **Joypad** tab functionally 1:1 with bgb (currently fully inert).
Frontend-only (`crate slopgb`), core untouched (golden-safe). Ground truth = real bgb
captures `docs/bgb-reference/options/options-joypad.png` + `joypad-keyconfig.png`.

**bgb keyboard-config wizard (captured):** a *sequential* 8-step dialog over the order
`right, left, up, down, A, B, select, start`. Each step shows a GB illustration (current
button red), `press and hold the button for <name>`, `currently mapped to: <key>`, and
three buttons **Cancel** (abort all), **Skip/clear** (unbind + advance), **Skip/keep**
(keep + advance). A keypress binds the current button and advances. bgb defaults
right/left/up/down=arrows, A=S, B=A, select=Shift, start=Enter (slopgb keeps its existing
Z/X/Enter/Shift defaults — rebindable via the wizard).

**"allow pressing L+R or U+D":** unchecked by default in bgb = SOCD filter ON.

```xml
<plan goal="functional bgb Joypad options tab (frontend-only, golden-safe)">
  <task id="1" model="sonnet" deps="none">
    <do>New keymap.rs: KeyBindings { keys: [Option<KeyCode>; 8] } indexed by Button; default() = Z/X/Enter/ShiftRight + arrows; button_for(KeyCode)->Option<Button>, key_for(Button)->Option<KeyCode>, set(Button,KeyCode) clears the code from any other button, key_name(KeyCode)->&'static str.</do>
    <test>keymap_tests: default A=KeyZ/B=KeyX/Start=Enter/Select=ShiftRight/dpad arrows; button_for(KeyZ)==Some(A); set(A,KeyQ) then button_for(KeyZ)==None and button_for(KeyQ)==Some(A); set re-using KeyX onto A unbinds B; key_name(ArrowUp)=="Up".</test>
    <done>keymap module compiles, all KeyBindings unit tests pass.</done>
  </task>
  <task id="2" model="sonnet" deps="1">
    <do>App owns `bindings: KeyBindings`; in handle_key resolve a held button via self.bindings.button_for(code) (press/release, global, any focus) BEFORE input::map; remove the 8 Action::Button arms from input::map (keep Turbo/Pause/Reset/Quit/F9 + focus arms).</do>
    <test>input_tests updated: map no longer returns Action::Button for KeyZ/arrows (returns None or non-button); a new app-level/keymap test asserts pressing the bound code presses the right core Button via a real GameBoy.</test>
    <done>buttons still work via bindings; input::map button arms gone; all input + app tests green.</done>
  </task>
  <task id="3" model="opus" deps="1">
    <do>KeyConfigWizard state machine in keymap.rs: ORDER=[Right,Left,Up,Down,A,B,Select,Start]; open(current: KeyBindings); fields step:usize, working:KeyBindings; bind_key(KeyCode) sets current button + advances, skip_keep() advances, skip_clear() unbinds+advances, cancel(); current_button(), prompt_name(), done()->Option<KeyBindings> (Some when past last step).</do>
    <test>wizard_tests: starts at Right; bind_key(KeyW) sets Right=KeyW and step=1; skip_keep keeps; skip_clear unbinds; after 8 advances done()==Some(working); cancel() leaves done()==None; order matches the bgb sequence.</test>
    <done>wizard state machine drives all 8 steps deterministically; tests green.</done>
    <why>the sequential bind/skip/clear/cancel state machine is the subtle piece — off-by-one on step/commit, dup-key handling on bind.</why>
  </task>
  <task id="4" model="sonnet" deps="3">
    <do>Render + hit-test for the wizard overlay (faithful to joypad-keyconfig.png): centred box, GB d-pad+buttons illustration with current button highlighted, the two text lines, three buttons; WizardButton hit-rects (Cancel/SkipClear/SkipKeep); button_at(px,py)->Option<WizardButton>.</do>
    <test>wizard_render_tests: button_at over each of the 3 button rects returns the right variant and misses outside; render writes ink (non-blank) into the canvas for the current prompt.</test>
    <done>wizard draws over the LCD and its 3 buttons hit-test; tests green.</done>
  </task>
  <task id="5" model="sonnet" deps="4">
    <do>OptionsState.on_click returns a non-applying signal when "configure keyboard" is clicked (new OptionsOutcome::ConfigureKeyboard or an out-param) so main can open the wizard; mark that joypad control live (field) without mutating Settings.</do>
    <test>options_tests: clicking the configure-keyboard button rect yields the ConfigureKeyboard outcome and does NOT close the dialog or change working settings.</test>
    <done>configure-keyboard click routes out of OptionsState; tests green.</done>
  </task>
  <task id="6" model="sonnet" deps="2,4,5">
    <do>App.key_wizard: Option<KeyConfigWizard>; on ConfigureKeyboard open it from self.bindings; handle_key captures all keys while open (keypress->bind_key, Esc->cancel) and on done() commits working to self.bindings; on_game_click routes clicks to button_at (Cancel/SkipKeep/SkipClear); redraw renders it via the overlay closure.</do>
    <test>app/keymap integration test: open wizard, feed 8 binds, assert self.bindings updated to the new keys; cancel path leaves bindings unchanged.</test>
    <done>configure-keyboard end-to-end: Options button opens wizard, binds commit; tests green.</done>
  </task>
  <task id="7" model="sonnet" deps="none">
    <do>Settings.allow_opposing: bool (default false); Joypad tab makes "allow pressing L+R or U+D" a live Field::AllowOpposing check; apply_settings stores App.allow_opposing; free fn socd_filter applied in the button-press path: when !allow_opposing, pressing a direction releases its opposite (last-wins).</do>
    <test>socd_tests: with filter on, press Left while Right held releases Right (and vice versa for U/D); with filter off both stay; Settings::default().allow_opposing==false; reset_defaults restores it.</test>
    <done>opposing-direction filter toggles live from the checkbox; tests green.</done>
  </task>
  <task id="8" model="haiku" deps="none">
    <do>Add the inert controls missing vs options-joypad.png to the joypad() builder: Screenshots dropdown (bmp), Rapid speed dropdown (2 2), Mappable button records groupbox + Audio/Video/Audio channels checks, joystick-ID field "0" + label; keep faithful (inert).</do>
    <test>joypad_controls_test: controls(Joypad,..) contains the new dropdowns/groupbox/checks/field and they carry field==None (inert), count matches the capture.</test>
    <done>joypad tab control list matches the capture 1:1; tests green.</done>
    <why>pure faithful transcription of a static control list — mechanical.</why>
  </task>
</plan>
```

One-line summary: **8 tasks (1 haiku, 6 sonnet, 1 opus)**; critical path 1 → 3 → 4 → 5 → 6 (wizard), with 7 (SOCD) and 8 (transcription) parallel/independent.
