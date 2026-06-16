//! Execution-profiler (MB5) tests for the debugger window: the profiler
//! dropdown's radio modes + live count, and the disasm per-line count overlay.

use super::*;
use crate::input::Action;
use crate::ui::Theme;
use crate::ui::canvas::{Canvas, Rect};

/// The default debugger window size (mirrors `debugger_tests::AREA`).
const AREA: Rect = Rect::new(0, 0, 760, 560);

#[test]
fn profiler_menu_reflects_mode_count_and_commands() {
    let l = DebuggerLayout::for_size(AREA.w, AREA.h);
    // Default state = stopped: "stop" is the checked radio, count is zero.
    let st = DebuggerState::default();
    let m = menubar_menu(5, l.menu, &st, 0x0100);
    assert_eq!(m.items.len(), 5);
    assert!(
        !m.items[0].checked && !m.items[1].checked && m.items[2].checked,
        "stop is the active mode by default"
    );
    assert!(m.items[4].label.contains("0 addresses seen"));
    assert!(
        !m.items[4].enabled,
        "the count row is a non-clickable label"
    );
    // The mode/clear rows carry their profiler commands.
    assert_eq!(m.choices[0], MenuChoice::Command(Action::ProfilerLogging));
    assert_eq!(m.choices[1], MenuChoice::Command(Action::ProfilerBreak));
    assert_eq!(m.choices[2], MenuChoice::Command(Action::ProfilerStop));
    assert_eq!(m.choices[3], MenuChoice::Command(Action::ProfilerClear));

    // Logging mode active with a live "N addresses seen".
    let st = DebuggerState {
        prof: ProfilerView {
            logging: true,
            brk: false,
            seen: 7,
        },
        ..DebuggerState::default()
    };
    let m = menubar_menu(5, l.menu, &st, 0x0100);
    assert!(
        m.items[0].checked && !m.items[2].checked,
        "logging mode active"
    );
    assert!(m.items[4].label.contains("7 addresses seen"));

    // Break mode active: the break row checks, logging does not.
    let st = DebuggerState {
        prof: ProfilerView {
            logging: true,
            brk: true,
            seen: 0,
        },
        ..DebuggerState::default()
    };
    let m = menubar_menu(5, l.menu, &st, 0x0100);
    assert!(
        m.items[1].checked && !m.items[0].checked,
        "break mode active"
    );
}

#[test]
fn profile_counts_overlay_only_rows_with_a_tally() {
    use crate::ui::text::line_height;
    let t = Theme::BGB;
    let lh = line_height() as usize;
    let (w, h) = (200usize, lh * 3);
    let rect = Rect::new(0, 0, w as i32, h as i32);
    let rows = vec![
        DisasmRow {
            addr: 0x0100,
            text: String::new(),
        },
        DisasmRow {
            addr: 0x0101,
            text: String::new(),
        },
    ];
    const BG: u32 = 0x00AA_AAAA;
    let ink = |counts: &dyn Fn(u16) -> u64| {
        let mut buf = vec![BG; w * h];
        {
            let mut c = Canvas::new(&mut buf, w, h);
            render_profile_counts(&mut c, rect, &rows, counts, &t);
        }
        buf.iter().filter(|&&p| p != BG).count()
    };
    assert_eq!(ink(&|_| 0), 0, "no tally -> no overlay drawn");
    assert!(
        ink(&|a| u64::from(a == 0x0100) * 5) > 0,
        "a nonzero tally draws an overlay"
    );
}
