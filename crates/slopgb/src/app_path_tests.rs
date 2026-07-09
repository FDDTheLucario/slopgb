use super::*;
use crate::cli::Options;
use crate::session::Session;
use crate::windows::options::BootromSlot;
use slopgb_core::Model;

/// A blank no-ROM `App`, mirroring `main_tests::blank_app` (private to that
/// module, so this file needs its own copy).
fn blank_app() -> App {
    let opts = Options {
        rom: None,
        model: None,
        scale: 3,
        mute: true,
        boot: None,
        mcp_port: None,
    };
    App::new(opts, Session::blank(Model::Dmg), false, None)
}

#[test]
fn prompt_default_prefills_link_and_mcp_only() {
    assert_eq!(
        prompt_default(PathPurpose::LinkConnect),
        format!("localhost:{}", crate::link::DEFAULT_PORT)
    );
    assert_eq!(
        prompt_default(PathPurpose::McpStart),
        crate::mcp::DEFAULT_PORT.to_string()
    );
    // File purposes still open blank.
    assert_eq!(prompt_default(PathPurpose::LoadRom), "");
    assert_eq!(prompt_default(PathPurpose::SaveState), "");
}

#[test]
fn mcp_start_path_action_boots_and_stops_the_server() {
    // The full menu wiring: SubChoice::McpStart → open_path_prompt → this action
    // (typed port) → mcp.start. Port 0 = an OS-chosen ephemeral port.
    let mut app = blank_app();
    assert!(!app.mcp.is_active());
    app.run_path_action(PathPurpose::McpStart, std::path::Path::new("0"));
    assert!(app.mcp.is_active(), "server started via the menu path");
    assert!(app.mcp.port().is_some());
    app.mcp.stop();
    assert!(!app.mcp.is_active());
}

#[test]
fn sym_sidecar_found_only_when_the_file_exists() {
    // Auto-load derivation: rom.with_extension("sym"), gated on exists().
    let dir = std::env::temp_dir().join(format!("slopgb_symsc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rom = dir.join("game.gb");
    std::fs::write(&rom, b"x").unwrap();
    assert_eq!(sym_sidecar(&rom), None, "no sidecar -> None");
    let sym = dir.join("game.sym");
    std::fs::write(&sym, b"").unwrap();
    assert_eq!(sym_sidecar(&rom), Some(sym), "sidecar present -> Some");
    // Extensionless ROM: game.sym for "noext" would be "noext.sym", absent.
    let noext = dir.join("noext");
    std::fs::write(&noext, b"x").unwrap();
    assert_eq!(sym_sidecar(&noext), None, "no matching sidecar -> None");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn link_connect_never_opens_a_file_picker() {
    // host:port is not a file — it must go straight to the typed modal.
    assert_eq!(pick_kind(PathPurpose::LinkConnect), PickKind::None);
}

#[test]
fn save_state_uses_the_save_dialog() {
    assert_eq!(pick_kind(PathPurpose::SaveState), PickKind::Save);
}

#[test]
fn file_purposes_use_the_open_dialog() {
    assert_eq!(pick_kind(PathPurpose::LoadRom), PickKind::Open);
    assert_eq!(pick_kind(PathPurpose::LoadState), PickKind::Open);
    assert_eq!(pick_kind(PathPurpose::SymbolFile), PickKind::Open);
    assert_eq!(
        pick_kind(PathPurpose::Bootrom(BootromSlot::Dmg)),
        PickKind::Open
    );
}

// ---- fallback_last_click reset (double-click state doesn't leak across a
// picker open/close) --------------------------------------------------------

#[test]
fn opening_the_fallback_picker_clears_a_stale_double_click_timer() {
    let mut app = blank_app();
    app.fallback_last_click = Some((std::time::Instant::now(), 10, 10));
    app.open_fallback_picker("Load ROM", PathPurpose::LoadRom, PickKind::Open);
    assert!(
        app.fallback_last_click.is_none(),
        "a single click right after reopen must not be read as a double-click"
    );
}

#[test]
fn cancelling_the_fallback_picker_clears_the_double_click_timer() {
    let mut app = blank_app();
    app.open_fallback_picker("Load ROM", PathPurpose::LoadRom, PickKind::Open);
    app.fallback_last_click = Some((std::time::Instant::now(), 10, 10));
    app.resolve_fallback_picker(Some(PickerOutcome::Cancelled));
    assert!(app.fallback_picker.is_none());
    assert!(app.fallback_last_click.is_none());
}

#[test]
fn picking_from_the_fallback_picker_clears_the_double_click_timer() {
    let mut app = blank_app();
    app.open_fallback_picker("Load state", PathPurpose::LoadState, PickKind::Open);
    app.fallback_last_click = Some((std::time::Instant::now(), 10, 10));
    // A nonexistent path: `run_path_action`'s `LoadState` arm just logs and
    // returns on error, so this stays a pure state check.
    app.resolve_fallback_picker(Some(PickerOutcome::Picked(PathBuf::from(
        "/does/not/exist",
    ))));
    assert!(app.fallback_picker.is_none());
    assert!(app.fallback_last_click.is_none());
}
