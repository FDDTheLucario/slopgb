//! One loaded ROM: the live [`GameBoy`], battery-RAM persistence (atomic
//! `.sav` writes + autosave), in-memory quick-save snapshots, and cartridge-
//! header parsing for the "Cart info" box.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use slopgb_core::{CLOCK_HZ, GameBoy, Model};

/// Autosave battery RAM every 5 seconds of emulated time.
const AUTOSAVE_CYCLES: u64 = 5 * CLOCK_HZ as u64;

pub(crate) struct Session {
    pub(crate) gb: GameBoy,
    /// Original ROM image, kept for reset.
    pub(crate) rom_bytes: Vec<u8>,
    pub(crate) model: Model,
    /// ROM file stem, for the window title.
    pub(crate) title: String,
    sav_path: PathBuf,
    /// Last battery-RAM image written to disk (dirty check).
    last_saved: Option<Vec<u8>>,
    /// Emulated-cycle deadline for the next autosave.
    next_autosave: u64,
    /// In-memory quick-save snapshot (bgb State → Quick Save / Quick Load): a
    /// whole-machine clone, boxed (a `GameBoy` is large). `None` until the first
    /// Quick Save. A ROM change (`load_dropped`) builds a fresh `Session`, so it
    /// resets to `None`; it deliberately **survives a reset** so a Quick Load can
    /// undo the reset (bgb's behavior — the snapshot is the same ROM).
    quick_state: Option<Box<GameBoy>>,
}

impl Session {
    /// Load a ROM, pick its model (CLI override beats header auto-detect),
    /// and restore `<rom>.sav` if present.
    pub(crate) fn load(path: &Path, model_override: Option<Model>) -> Result<Self, String> {
        let rom_bytes =
            fs::read(path).map_err(|e| format!("cannot read ROM '{}': {e}", path.display()))?;
        let model = model_override.unwrap_or_else(|| GameBoy::auto_model(&rom_bytes));
        let mut gb = GameBoy::new(model, rom_bytes.clone())
            .map_err(|e| format!("cannot load ROM '{}': {e}", path.display()))?;
        let sav_path = path.with_extension("sav");
        let mut last_saved = None;
        match fs::read(&sav_path) {
            Ok(data) => {
                if gb.load_save_data(&data) {
                    last_saved = Some(data);
                } else {
                    eprintln!(
                        "slopgb: ignoring save file '{}' (wrong size or no battery)",
                        sav_path.display()
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!(
                "slopgb: cannot read save file '{}': {e}",
                sav_path.display()
            ),
        }
        let title = path
            .file_stem()
            .map_or_else(|| "rom".to_owned(), |s| s.to_string_lossy().into_owned());
        Ok(Self {
            gb,
            rom_bytes,
            model,
            title,
            sav_path,
            last_saved,
            next_autosave: AUTOSAVE_CYCLES,
            quick_state: None,
        })
    }

    /// Quick Save (bgb State → Quick Save): snapshot the whole machine into
    /// memory, replacing any previous quick-save.
    pub(crate) fn quick_save(&mut self) {
        self.quick_state = Some(Box::new(self.gb.clone()));
    }

    /// Quick Load (bgb State → Quick Load): restore the last Quick Save, if any.
    /// Returns whether a snapshot was restored (so the caller can resync pacing
    /// / redraw only on a real load).
    pub(crate) fn quick_load(&mut self) -> bool {
        let Some(snap) = &self.quick_state else {
            return false;
        };
        self.gb = (**snap).clone();
        true
    }

    /// Power-cycle: fresh machine, save RAM reloaded from disk.
    pub(crate) fn reset(&mut self) {
        self.flush_save();
        match GameBoy::new(self.model, self.rom_bytes.clone()) {
            Ok(mut gb) => {
                if let Ok(data) = fs::read(&self.sav_path) {
                    let _ = gb.load_save_data(&data); // rejection already warned at load
                }
                self.gb = gb;
                self.next_autosave = AUTOSAVE_CYCLES;
            }
            // Can't happen (the same image loaded before), but never panic.
            Err(e) => eprintln!("slopgb: reset failed: {e}"),
        }
    }

    /// Switch the emulated system (Options → System → Emulated system): rebuild
    /// the machine from the ROM with `model_override` (`None` = auto-detect),
    /// reloading battery RAM. A no-op (returns `false`) when the resolved model
    /// already matches, so re-applying Options doesn't needlessly power-cycle.
    pub(crate) fn set_model(&mut self, model_override: Option<Model>) -> bool {
        let model = model_override.unwrap_or_else(|| GameBoy::auto_model(&self.rom_bytes));
        if model == self.model {
            return false;
        }
        self.flush_save();
        match GameBoy::new(model, self.rom_bytes.clone()) {
            Ok(mut gb) => {
                if let Ok(data) = fs::read(&self.sav_path) {
                    let _ = gb.load_save_data(&data);
                }
                self.gb = gb;
                self.model = model;
                self.next_autosave = AUTOSAVE_CYCLES;
                self.quick_state = None; // a different machine — old snapshot is stale
                true
            }
            Err(e) => {
                eprintln!("slopgb: model switch failed: {e}");
                false
            }
        }
    }

    /// Write battery RAM to `<rom>.sav` if it changed since the last write.
    pub(crate) fn flush_save(&mut self) {
        let Some(data) = self.gb.save_data() else {
            return; // cartridge has no battery RAM
        };
        if self.last_saved.as_deref() == Some(data.as_slice()) {
            return;
        }
        match write_atomic(&self.sav_path, &data) {
            Ok(()) => self.last_saved = Some(data),
            Err(e) => eprintln!(
                "slopgb: cannot write save file '{}': {e}",
                self.sav_path.display()
            ),
        }
    }

    /// Flush battery RAM at most once per [`AUTOSAVE_CYCLES`] of emulated time.
    pub(crate) fn autosave(&mut self) {
        if self.gb.cycles() >= self.next_autosave {
            self.next_autosave = self.gb.cycles().saturating_add(AUTOSAVE_CYCLES);
            self.flush_save();
        }
    }
}

/// Write `data` to `path` via a temp file, fsync and rename, so a crash —
/// of the process or the whole machine — mid-write never truncates an
/// existing save: the data blocks are durable before the rename can commit.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("sav.tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    // Best effort: also persist the rename itself (the directory entry), so
    // power loss right after the rename can't roll back to the old contents.
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

/// Cartridge-header facts (Pan Docs "The Cartridge Header") for the Other →
/// "Cart info" box, parsed straight from the ROM image.
pub(crate) fn cart_info_lines(rom: &[u8]) -> Vec<String> {
    if rom.len() < 0x150 {
        return vec!["(ROM too small for a header)".into()];
    }
    let title: String = rom[0x134..0x143]
        .iter()
        .take_while(|&&b| b != 0)
        .filter(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect();
    let cgb = match rom[0x143] {
        0xC0 => "CGB only",
        0x80 => "CGB+DMG",
        _ => "DMG",
    };
    let ram = match rom[0x149] {
        0 => "none",
        1 => "2 KiB",
        2 => "8 KiB",
        3 => "32 KiB",
        4 => "128 KiB",
        5 => "64 KiB",
        _ => "?",
    };
    // 32 KiB << header byte; a malformed (too-large) byte yields 0, never a
    // shift-overflow panic.
    let rom_kb = 32u32.checked_shl(u32::from(rom[0x148])).unwrap_or(0);
    vec![
        format!("title: {}", title.trim()),
        format!("type:  {:02X} {}", rom[0x147], cart_type_name(rom[0x147])),
        format!("rom:   {rom_kb} KiB"),
        format!("ram:   {ram}"),
        format!("cgb:   {cgb}"),
    ]
}

/// The MBC / mapper family for a cartridge-type byte (header `$0147`).
fn cart_type_name(t: u8) -> &'static str {
    match t {
        0x00 => "ROM ONLY",
        0x01..=0x03 => "MBC1",
        0x05 | 0x06 => "MBC2",
        0x08 | 0x09 => "ROM+RAM",
        0x0B..=0x0D => "MMM01",
        0x0F..=0x13 => "MBC3",
        0x19..=0x1E => "MBC5",
        0x20 => "MBC6",
        0x22 => "MBC7",
        0xFC => "POCKET CAMERA",
        0xFD => "BANDAI TAMA5",
        0xFE => "HuC3",
        0xFF => "HuC1",
        _ => "?",
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
