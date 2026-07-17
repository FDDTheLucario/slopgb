//! `App` path-modal handling: the shared text prompt used for Load ROM / Save &
//! Load state / Link connect / bootrom paths / `.sym` symbol load, and the
//! recent-ROMs bookkeeping. Split out of `main.rs` to keep it under the size cap.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use slopfp::Outcome as PickerOutcome;

use crate::file_picker::FilePicker;
use crate::ui::dialog::DialogResult;
use crate::{App, PathPurpose, link, push_recent_into, symbols};

/// How a path purpose collects its value: the in-app file browser (open- or
/// save-mode) or the typed [`crate::ui::dialog::InputDialog`] modal for a
/// non-file entry (link `host:port` / MCP port). Pure → unit-tested (it must
/// never offer a file browser for `LinkConnect`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathEntry {
    /// Browse for an existing file to open.
    OpenFile,
    /// Browse to a (possibly new) file to save.
    SaveFile,
    /// Browse to and select a directory (the file browser in directory mode).
    Directory,
    /// Type a non-file value (`host:port` / port number) into the text modal.
    Modal,
}

#[must_use]
pub(crate) fn path_entry(purpose: PathPurpose) -> PathEntry {
    match purpose {
        PathPurpose::SaveState | PathPurpose::CdlSave => PathEntry::SaveFile,
        PathPurpose::PluginsDir => PathEntry::Directory,
        PathPurpose::LinkConnect | PathPurpose::McpStart => PathEntry::Modal,
        _ => PathEntry::OpenFile,
    }
}

/// The value the typed modal opens pre-filled with — a sane default the user can
/// accept with Enter (or edit): a localhost peer for the link Connect prompt, the
/// default port for the MCP Start prompt. Empty for every file purpose (they open
/// blank as before).
#[must_use]
pub(crate) fn prompt_default(purpose: PathPurpose) -> String {
    match purpose {
        PathPurpose::LinkConnect => format!("localhost:{}", link::DEFAULT_PORT),
        PathPurpose::McpStart => crate::mcp::DEFAULT_PORT.to_string(),
        _ => String::new(),
    }
}

/// The sidecar `.sym` beside a ROM (`rom.with_extension("sym")`), returned only
/// when that file exists. Backs auto-load of symbols on ROM load. Pure → unit
/// tested. `None` keeps auto-load a silent no-op when no sidecar is present.
#[must_use]
pub(crate) fn sym_sidecar(rom: &Path) -> Option<PathBuf> {
    let sym = rom.with_extension("sym");
    sym.exists().then_some(sym)
}

impl App {
    /// Open a path entry for `purpose`. File purposes use the **in-app** browser
    /// ([`crate::file_picker::FilePicker`], built on `slopfp`) — a
    /// self-contained picker with no dependency on a system dialog utility being
    /// installed. The one non-file purpose (link `host:port` / MCP port) has no
    /// file to browse, so it uses the typed modal.
    pub(crate) fn open_path_prompt(&mut self, title: &str, purpose: PathPurpose) {
        match path_entry(purpose) {
            entry @ (PathEntry::OpenFile | PathEntry::SaveFile | PathEntry::Directory) => {
                self.open_file_picker(title, purpose, entry)
            }
            PathEntry::Modal => self.open_path_modal(title, purpose),
        }
    }

    /// The typed-path modal over the LCD (for the non-file purposes — link
    /// `host:port` / MCP port). It lives on the game window and only captures
    /// keys there, so raise + focus the game window — else a prompt triggered
    /// from a tool window (e.g. the debugger "Load symbols...") would appear
    /// hidden behind it and seem unresponsive.
    fn open_path_modal(&mut self, title: &str, purpose: PathPurpose) {
        self.path_purpose = purpose;
        self.path_dialog = Some(
            crate::ui::dialog::InputDialog::new(title, false).with_initial(prompt_default(purpose)),
        );
        if let Some(w) = &self.window {
            w.focus_window();
        }
        self.request_game_redraw();
    }

    /// The in-app file browser, rooted at the last-loaded ROM's
    /// directory (falling back to the process cwd, then `/`) — same
    /// raise+focus rationale as [`Self::open_path_modal`].
    fn open_file_picker(&mut self, title: &str, purpose: PathPurpose, entry: PathEntry) {
        // The plugins-dir browse starts at the current plugins dir (if set) so the
        // user edits from there; every other purpose starts at the last ROM's dir.
        let start_dir = if purpose == PathPurpose::PluginsDir
            && !self.settings.plugins.dir.is_empty()
        {
            PathBuf::from(&self.settings.plugins.dir)
        } else {
            self.recent
                .first()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
        };
        let mode = match entry {
            PathEntry::SaveFile => slopfp::Mode::Save,
            PathEntry::Directory => slopfp::Mode::Directory,
            _ => slopfp::Mode::Open,
        };
        // ponytail: per-purpose ext filters, add when a purpose needs one.
        self.file_picker = Some(FilePicker::open(purpose, start_dir, &[], title, mode));
        // Reset the double-click timer: a stale click from a previous picker
        // session (same screen spot, still inside the double-click window)
        // must never combine with the first click of this new session.
        self.picker_last_click = None;
        if let Some(w) = &self.window {
            w.focus_window();
        }
        self.request_game_redraw();
    }

    /// Apply an in-app file-picker outcome (shared by the key-feed guard
    /// in `main::handle_key` and the click routing in `app_menu::on_game_click`):
    /// `Picked` runs the picker's own purpose and closes it, `Cancelled` just
    /// closes it, `None`/no-outcome keeps it open. Always repaints (the
    /// picker's own view may have changed even with no outcome — a nav key).
    /// Both close arms null out `picker_last_click` too — else a stale
    /// double-click timer from this session could combine with the first
    /// click of a picker opened later at the same screen spot.
    pub(crate) fn resolve_file_picker(&mut self, outcome: Option<PickerOutcome>) {
        match outcome {
            Some(PickerOutcome::Picked(path)) => {
                let purpose = self
                    .file_picker
                    .as_ref()
                    .expect("outcome came from this picker")
                    .purpose();
                self.file_picker = None;
                self.picker_last_click = None;
                self.run_path_action(purpose, &path);
            }
            Some(PickerOutcome::Cancelled) => {
                self.file_picker = None;
                self.picker_last_click = None;
            }
            _ => {}
        }
        self.request_game_redraw();
    }

    /// Apply a path-modal result: accept routes by [`Self::path_purpose`] (a
    /// blank entry just closes), cancel closes; continue keeps editing.
    pub(crate) fn resolve_path_dialog(&mut self, result: DialogResult) {
        match result {
            DialogResult::Accept(path) => {
                let purpose = self.path_purpose;
                self.path_dialog = None;
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    self.run_path_action(purpose, Path::new(trimmed));
                }
            }
            DialogResult::Cancel => self.path_dialog = None,
            DialogResult::Continue => {}
        }
        self.request_game_redraw();
    }

    /// Carry out an accepted path entry per its purpose. `pub(crate)`: both the
    /// typed modal (this file) and the file-picker guards in `main.rs`/
    /// `app_menu.rs` route their accepted path through this one sink.
    pub(crate) fn run_path_action(&mut self, purpose: PathPurpose, path: &Path) {
        match purpose {
            PathPurpose::LoadRom => self.load_dropped(path),
            PathPurpose::SaveState => match self.session.save_state_to(path) {
                Ok(()) => println!("slopgb: saved state to {}", path.display()),
                Err(e) => eprintln!("slopgb: save state failed: {e}"),
            },
            PathPurpose::LoadState => match self.session.load_state_from(path) {
                Ok(()) => {
                    println!("slopgb: loaded state from {}", path.display());
                    // A state restores a real running machine — leave the no-ROM
                    // blank state (else `should_idle` keeps emulation gated and
                    // the LCD frozen on `blank_frame`).
                    self.rom_loaded = true;
                    self.apply_palette();
                    self.resync_pacing();
                    self.update_title();
                    self.request_game_redraw();
                }
                Err(e) => eprintln!("slopgb: load state failed: {e}"),
            },
            PathPurpose::LinkConnect => {
                // The "path" here is the typed host:port (the shared text modal).
                let (host, port) = link::parse_host_port(&path.to_string_lossy());
                match self.link.connect(host.clone(), port) {
                    Ok(()) => println!("slopgb: link connecting to {host}:{port}"),
                    Err(e) => eprintln!("slopgb: link connect failed: {e}"),
                }
                self.update_title(); // reflect the "connecting :port" status at once
            }
            PathPurpose::McpStart => {
                // The "path" here is the typed port (blank → the default).
                let port = crate::mcp::parse_port(&path.to_string_lossy());
                match self.mcp.start(port) {
                    Ok(()) => println!(
                        "slopgb: MCP server on http://127.0.0.1:{}/",
                        self.mcp.port().unwrap_or(port)
                    ),
                    Err(e) => eprintln!("slopgb: MCP server failed on port {port}: {e}"),
                }
                self.update_title();
            }
            PathPurpose::Bootrom(slot) => {
                // Write the typed path into the open Options dialog's working
                // scratch; OK/Apply commits it to settings, Cancel reverts.
                if let Some(o) = &mut self.options {
                    *slot.path_mut(&mut o.working) = path.to_string_lossy().into_owned();
                }
            }
            PathPurpose::PluginsDir => {
                // Write the plugins dir into the open dialog's working scratch;
                // OK/Apply rescans the new directory, Cancel reverts.
                if let Some(o) = &mut self.options {
                    o.working.plugins.dir = path.to_string_lossy().into_owned();
                }
            }
            PathPurpose::SymbolFile => self.load_symbols(path),
            PathPurpose::CdlSave => match self.session.gb.cdl_flags() {
                Some(flags) => match std::fs::write(path, crate::cdl::rle_encode(flags)) {
                    Ok(()) => println!("slopgb: saved CDL to {}", path.display()),
                    Err(e) => eprintln!("slopgb: save CDL failed: {e}"),
                },
                None => eprintln!("slopgb: CDL not enabled — nothing to save"),
            },
            PathPurpose::CdlLoad => match std::fs::read(path) {
                Ok(bytes) => {
                    let dec = crate::cdl::rle_decode(&bytes);
                    // load_cdl validates the length against this machine's
                    // physical layout (ROM+VRAM+SRAM+WRAM+tail).
                    // ponytail: length-only guard — a .cdl from a *same-size*
                    // ROM/RAM config would still load; embed the cart header
                    // checksum in the file if that ever bites.
                    if self.session.gb.load_cdl(&dec) {
                        println!("slopgb: loaded CDL from {}", path.display());
                    } else {
                        eprintln!(
                            "slopgb: CDL file doesn't match this ROM/RAM layout — not loaded"
                        );
                    }
                }
                Err(e) => eprintln!("slopgb: load CDL failed: {e}"),
            },
            PathPurpose::SettingsExportBgb => {
                crate::settings_file::export_bgb(path, &self.settings, &self.recent);
                println!("slopgb: exported settings to {}", path.display());
            }
            PathPurpose::SettingsImportBgb => {
                let loaded = crate::settings_file::import_bgb(path);
                self.settings = loaded.settings;
                self.recent = loaded.recent;
                // Apply the imported settings live + persist them to the native store.
                self.apply_settings();
                println!("slopgb: imported settings from {}", path.display());
            }
            PathPurpose::CheatSave => match std::fs::write(path, self.cheats.to_file_text()) {
                Ok(()) => println!("slopgb: saved cheats to {}", path.display()),
                Err(e) => eprintln!("slopgb: save cheats failed: {e}"),
            },
            PathPurpose::CheatLoad => match std::fs::read_to_string(path) {
                Ok(text) => {
                    self.cheats.load_file_text(&text);
                    println!("slopgb: loaded cheats from {}", path.display());
                }
                Err(e) => eprintln!("slopgb: load cheats failed: {e}"),
            },
        }
    }

    /// Load a `.sym` symbol file: parse it (tolerant), store as the source of
    /// truth, and push it to the debugger view. A read error is logged (non-fatal,
    /// leaving the previous symbols intact).
    pub(crate) fn load_symbols(&mut self, path: &Path) {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let table = symbols::SymbolTable::parse(&text);
                println!(
                    "slopgb: loaded {} symbols from {}",
                    table.len(),
                    path.display()
                );
                self.symbols = Rc::new(table);
                self.tools.set_symbols(self.symbols.clone());
            }
            Err(e) => eprintln!("slopgb: load symbols failed: {e}"),
        }
    }

    /// Record a successfully loaded ROM in the recent list (MN4). Skipped when
    /// Options → Misc → "freeze recent ROMs menu" is set (bgb pins the list).
    pub(crate) fn push_recent(&mut self, path: &Path) {
        if self.settings.freeze_recent {
            return;
        }
        push_recent_into(&mut self.recent, path);
        // Persist immediately so the list survives a crash (bgb saves on exit).
        crate::settings_file::save(&self.settings, &self.recent);
    }
}

#[cfg(test)]
#[path = "app_path_tests.rs"]
mod tests;
