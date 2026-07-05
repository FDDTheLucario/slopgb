//! `App` path-modal handling: the shared text prompt used for Load ROM / Save &
//! Load state / Link connect / bootrom paths / `.sym` symbol load, and the
//! recent-ROMs bookkeeping. Split out of `main.rs` to keep it under the size cap.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::filepicker::{self, PickResult};
use crate::ui::dialog::DialogResult;
use crate::{App, PathPurpose, link, push_recent_into, symbols};

/// Which native dialog (if any) a path purpose should try before the typed
/// modal. `LinkConnect` is a `host:port`, not a file, so it never picks; a
/// `SaveState` writes a new file, so it uses the save dialog; the rest open an
/// existing file. Pure → unit-tested (it must never offer a file picker for
/// `LinkConnect`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickKind {
    Open,
    Save,
    None,
}

#[must_use]
pub(crate) fn pick_kind(purpose: PathPurpose) -> PickKind {
    match purpose {
        PathPurpose::SaveState | PathPurpose::CdlSave => PickKind::Save,
        PathPurpose::LinkConnect => PickKind::None,
        _ => PickKind::Open,
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
    /// Open a path entry for `purpose`. Prefer a **native file dialog** (a
    /// dep-free shell-out, [`crate::filepicker`]); only fall back to the typed
    /// modal over the LCD when no picker tool is installed (or the purpose isn't
    /// a file — link `host:port`). A user-cancelled native dialog just closes.
    pub(crate) fn open_path_prompt(&mut self, title: &str, purpose: PathPurpose) {
        let picked = match pick_kind(purpose) {
            PickKind::Open => filepicker::pick_open(),
            PickKind::Save => filepicker::pick_save(),
            // Not a file (link host:port): go straight to the typed modal.
            PickKind::None => PickResult::Unavailable,
        };
        match picked {
            PickResult::Picked(path) => {
                self.run_path_action(purpose, &path);
                self.request_game_redraw();
            }
            // The user backed out of the native dialog — don't nag with the modal.
            PickResult::Cancelled => self.request_game_redraw(),
            // No picker available → the typed-path modal (the old behaviour).
            PickResult::Unavailable => self.open_path_modal(title, purpose),
        }
    }

    /// The typed-path modal over the LCD (fallback when no native picker exists).
    /// It lives on the game window and only captures keys there, so raise + focus
    /// the game window — else a prompt triggered from a tool window (e.g. the
    /// debugger "Load symbols...") would appear hidden behind it and seem
    /// unresponsive.
    fn open_path_modal(&mut self, title: &str, purpose: PathPurpose) {
        self.path_purpose = purpose;
        self.path_dialog = Some(crate::ui::dialog::InputDialog::new(title, false));
        if let Some(w) = &self.window {
            w.focus_window();
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

    /// Carry out an accepted path entry per its purpose.
    fn run_path_action(&mut self, purpose: PathPurpose, path: &Path) {
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
            PathPurpose::Bootrom(slot) => {
                // Write the typed path into the open Options dialog's working
                // scratch; OK/Apply commits it to settings, Cancel reverts.
                if let Some(o) = &mut self.options {
                    *slot.path_mut(&mut o.working) = path.to_string_lossy().into_owned();
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
                    match <[u8; 65536]>::try_from(dec.as_slice()) {
                        Ok(arr) => {
                            self.session.gb.load_cdl(&arr);
                            println!("slopgb: loaded CDL from {}", path.display());
                        }
                        Err(_) => eprintln!("slopgb: bad CDL file (expected 64 KiB of flags)"),
                    }
                }
                Err(e) => eprintln!("slopgb: load CDL failed: {e}"),
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
    }
}

#[cfg(test)]
#[path = "app_path_tests.rs"]
mod tests;
