// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2026 Richard Moch

//! The plan's whole worked example (`docs/headless-plan.md`, "## Worked
//! example" — the same `SRC` `parser_tests.rs`'s
//! `worked_example_from_the_plan_parses` already pins verbatim), run end to
//! end against fake `gb:` builtins that emulate a plausible game. Proves
//! every construct in the language actually executes, not just parses:
//! `within`/`else`, `repeat`/`until`, `for`, both `gb:read` shapes,
//! byte-list comparison, `check` accumulating, `argv`, and the two battery
//! calls.

use crate::{Interp, Outcome, Value};
use std::cell::Cell;
use std::rc::Rc;

const SRC: &str = r#"-- Usage: slopgb --headless case.slp <rom.gbc> <in.sav> <out.sav>

gb:load_rom(argv[1], CGB)
gb:load_battery(argv[2])
gb:set_rtc(1767225600)   -- pin the clock: a host-clock-dependent run is flaky by design
gb:load_symbols("game.sym")

-- Drive the title screen until the game reports it has processed the save.
within 6000 {
  repeat {
    gb:tap(BTN_START, 3); gb:wait_frames(17)
    gb:tap(BTN_A, 3);     gb:wait_frames(17)
  } until gb:read("sSaveVersion") == 2
} else {
  check(false, "save was never processed")
}

-- Through the confirmation prompt, then let the map load.
for i = 1, 8 {
  gb:tap(BTN_A, 3); gb:wait_frames(37)
}
gb:wait_frames(600)

let table = gb:read("wResultTable", 4)
print("table=" .. hex(table) .. " state=" .. gb:read("wGameState"))

check(table == [27, 2, 10, 4], "result table")
check(gb:read("wGameState") == 0, "game state after load")

gb:save_battery(argv[3])
"#;

/// Wires the fake `gb:` machine the worked example drives: `wait_frames`
/// advances a shared frame counter (which is also the `within`/`repeat`
/// clock, one tick per frame), `sSaveVersion` reports `2` once enough frames
/// have passed, and `wResultTable`/`wGameState` report the "loaded" values
/// once the frame count clears the load threshold.
fn build_interp() -> (Interp<'static>, Rc<Cell<u64>>) {
    let frames = Rc::new(Cell::new(0u64));
    let mut interp = Interp::new();

    let clock_read = frames.clone();
    interp.set_clock(1, move || clock_read.get());

    for name in [
        "gb:load_rom",
        "gb:load_battery",
        "gb:set_rtc",
        "gb:load_symbols",
        "gb:tap",
        "gb:save_battery",
    ] {
        interp.register_builtin(name, |_args| Ok(Value::Nil));
    }

    let wait_frames = frames.clone();
    interp.register_builtin("gb:wait_frames", move |args| match args {
        [Value::Int(n)] if *n >= 0 => {
            wait_frames.set(wait_frames.get() + *n as u64);
            Ok(Value::Nil)
        }
        _ => Err("wait_frames(n) expects one non-negative Int".to_string()),
    });

    let read_frames = frames.clone();
    interp.register_builtin("gb:read", move |args| {
        // Save processed once 100 frames of title-screen mashing have
        // passed (the repeat loop taps every 34 frames, so this lands on
        // its 3rd iteration, at frame 102). The result table/state "load"
        // well before the script's own final wait — the `for` loop alone
        // adds 8 * 37 = 296 more frames, plus a final `wait_frames(600)` —
        // so any threshold comfortably under 102 + 296 = 398 works.
        let loaded_after = 300;
        match args {
            [Value::Str(sym)] if sym == "sSaveVersion" => {
                Ok(Value::Int(if read_frames.get() >= 100 { 2 } else { 0 }))
            }
            [Value::Str(sym)] if sym == "wGameState" => {
                Ok(Value::Int(if read_frames.get() >= loaded_after {
                    0
                } else {
                    9
                }))
            }
            [Value::Str(sym), Value::Int(4)] if sym == "wResultTable" => {
                if read_frames.get() >= loaded_after {
                    Ok(Value::Bytes(vec![27, 2, 10, 4]))
                } else {
                    Ok(Value::Bytes(vec![0, 0, 0, 0]))
                }
            }
            _ => Err(format!("unexpected gb:read({args:?})")),
        }
    });

    interp.register_const("CGB", Value::Int(1));
    interp.register_const("BTN_START", Value::Int(1));
    interp.register_const("BTN_A", Value::Int(2));

    (interp, frames)
}

#[test]
fn worked_example_runs_to_ok_when_the_game_behaves() {
    let (mut interp, _frames) = build_interp();
    interp.set_argv(vec![
        "rom.gbc".to_string(),
        "in.sav".to_string(),
        "out.sav".to_string(),
    ]);
    match interp.run(SRC) {
        Outcome::Ok { checks: 2 } => {}
        other => panic!("expected Outcome::Ok with 2 checks, got {other:?}"),
    }
}

#[test]
fn worked_example_reports_failed_checks_when_the_game_misbehaves() {
    // Same fake machine, but wGameState never reports the "loaded" 0 —
    // stays at 9, so the second check must fail (and only that one).
    let (mut interp, frames) = build_interp();
    interp.set_argv(vec![
        "rom.gbc".to_string(),
        "in.sav".to_string(),
        "out.sav".to_string(),
    ]);
    // Reset gb:read to a variant whose wGameState never clears.
    let read_frames = frames.clone();
    interp.register_builtin("gb:read", move |args| match args {
        [Value::Str(sym)] if sym == "sSaveVersion" => {
            Ok(Value::Int(if read_frames.get() >= 100 { 2 } else { 0 }))
        }
        [Value::Str(sym)] if sym == "wGameState" => Ok(Value::Int(9)),
        [Value::Str(sym), Value::Int(4)] if sym == "wResultTable" => {
            Ok(Value::Bytes(vec![27, 2, 10, 4]))
        }
        _ => Err(format!("unexpected gb:read({args:?})")),
    });
    match interp.run(SRC) {
        Outcome::Failed { checks, failures } => {
            assert_eq!(checks, 2);
            assert_eq!(failures, vec!["game state after load".to_string()]);
        }
        other => panic!("expected Outcome::Failed, got {other:?}"),
    }
}
