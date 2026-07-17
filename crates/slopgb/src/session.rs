//! One loaded ROM: the live [`GameBoy`], battery-RAM persistence (atomic
//! `.sav` writes + autosave), in-memory quick-save snapshots, and cartridge-
//! header parsing for the "Cart info" box.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use slopgb_core::{CLOCK_HZ, CartridgeError, DEFAULT_SAMPLE_RATE, GameBoy, Model, RamInit};
use slopgb_sgb_coprocessor::{CPU_WASM, SPC_WASM, SgbCoprocessor};

use crate::windows::options::ModelChoice;

/// Autosave battery RAM every 5 seconds of emulated time.
const AUTOSAVE_CYCLES: u64 = 5 * CLOCK_HZ as u64;

/// Capture a rewind snapshot every N emulated frames (~30/s at 60 fps).
const REWIND_INTERVAL_FRAMES: u64 = 2;
/// Rewind ring cap — ~20 s of backward playback at the capture rate.
const REWIND_MAX_STATES: usize = 600;

pub(crate) struct Session {
    pub(crate) gb: GameBoy,
    /// Original ROM image, kept for reset.
    pub(crate) rom_bytes: Vec<u8>,
    pub(crate) model: Model,
    /// ROM file stem, for the window title.
    pub(crate) title: String,
    sav_path: PathBuf,
    /// Last battery-RAM image written to disk (dirty check). Holds the canonical
    /// timestamp-free `save_data` image so the dirty check stays stable even when
    /// the VBA export stamps a moving wall clock into the file.
    last_saved: Option<Vec<u8>>,
    /// Options → System → "Save RTC in SAV file (VBA compatible)": write the MBC3
    /// RTC as VBA's `.sav` footer (raw SRAM + a wall-clock-stamped footer) so
    /// other emulators read the clock. Off = slopgb's own block. Set by the
    /// frontend from settings; only affects RTC carts.
    rtc_vba_export: bool,
    /// Options → System → "Save BGB legacy RTC files": also write the RTC to a
    /// separate `<rom>.rtc` sidecar. Set by the frontend from settings.
    rtc_bgb_legacy: bool,
    /// Emulated-cycle deadline for the next autosave.
    next_autosave: u64,
    /// In-memory quick-save snapshot (bgb State → Quick Save / Quick Load): a
    /// whole-machine clone, boxed (a `GameBoy` is large). `None` until the first
    /// Quick Save. A ROM change (`load_dropped`) builds a fresh `Session`, so it
    /// resets to `None`; it deliberately **survives a reset** so a Quick Load can
    /// undo the reset (bgb's behavior — the snapshot is the same ROM).
    quick_state: Option<Box<GameBoy>>,
    /// System → "Rewind enabled": a bounded ring of recent save states (oldest
    /// dropped when full), captured every [`REWIND_INTERVAL_FRAMES`] while
    /// playing. Empty until rewind is enabled; cleared on reset / ROM change.
    rewind: std::collections::VecDeque<Vec<u8>>,
    /// Frame count at which the next rewind snapshot is taken.
    next_rewind_frame: u64,
    /// Boot-ROM configuration captured at load, so a power-cycle (`reset`) or a
    /// model switch (`set_model`) re-runs the boot ROM (logo + chime) like bgb,
    /// instead of silently replaying the post-boot state.
    boot: OwnedBootSpec,
    /// Optional user-supplied SGB BIOS bytes (`--sgb-bios`/`SLOPGB_SGB_BIOS`),
    /// kept so a power-cycle / model switch re-applies it to the fresh machine
    /// (firmware persists across a reset). `None` = no BIOS. A no-op off SGB.
    sgb_bios: Option<Vec<u8>>,
    /// Plugins directory (`--plugins`/`SLOPGB_PLUGINS_DIR`, or the UI browse). The
    /// SGB coprocessor is a plugin: when this dir holds `spc700.wasm` +
    /// `w65c816.wasm` and the machine is SGB, the combined 65C816+SPC700+S-DSP
    /// coprocessor auto-loads over the built-in HLE `SgbApu` at inject time. `None`
    /// (or a dir missing either wasm) → the built-in `SgbApu` stands (golden-safe
    /// default). Kept so a power-cycle / model switch re-injects it.
    plugins_dir: Option<PathBuf>,
    /// Overlay the built-in default SGB border on a non-SGB machine — bgb's
    /// "GBC + initial SGB border" system mode (`ModelChoice::CgbBorder`). A
    /// machine property, so a power-cycle (`reset`) re-applies it.
    sgb_border: bool,
    /// Power-on RAM initialisation (`--ram-init`), re-applied on a power-cycle.
    /// `None` = the deterministic 0xFF cart-SRAM default.
    ram_init: Option<RamInit>,
}

impl Session {
    /// A no-ROM session for the bgb-style blank startup: a valid blank machine
    /// (a 32 KiB ROM-only image of zeros) frozen at power-on, with no title, no
    /// `.sav` file, and no snapshot. The blank cart has no battery, so
    /// [`flush_save`](Self::flush_save) is a pure no-op (no stray file). The
    /// frontend gates emulation off until a real ROM is loaded.
    pub(crate) fn blank(model: Model) -> Self {
        let rom_bytes = vec![0u8; 0x8000];
        // A 32 KiB zero image is a valid ROM-only cart (header type $00, size $00)
        // and always constructs for every model.
        let gb = GameBoy::new(model, rom_bytes.clone())
            .expect("blank 32 KiB ROM-only image is always a valid cartridge");
        Self {
            gb,
            rom_bytes,
            model,
            title: String::new(),
            sav_path: PathBuf::new(),
            last_saved: None,
            rtc_vba_export: false,
            rtc_bgb_legacy: false,
            next_autosave: AUTOSAVE_CYCLES,
            quick_state: None,
            rewind: std::collections::VecDeque::new(),
            next_rewind_frame: 0,
            boot: OwnedBootSpec::default(),
            sgb_bios: None,
            plugins_dir: None,
            sgb_border: false,
            ram_init: None,
        }
    }

    /// Load a ROM, pick its model (CLI override beats header auto-detect),
    /// and restore `<rom>.sav` if present. `boot` selects the boot ROM to
    /// execute from power-on for the resolved model (Options paths over
    /// `--boot`); none/none-matching starts post-boot.
    pub(crate) fn load(
        path: &Path,
        choice: ModelChoice,
        boot: &BootSpec,
        ram_init: Option<RamInit>,
    ) -> Result<Self, String> {
        let rom_bytes =
            fs::read(path).map_err(|e| format!("cannot read ROM '{}': {e}", path.display()))?;
        let (model, sgb_border) = choice.resolve(&rom_bytes);
        let mut gb = build_gb(
            model,
            rom_bytes.clone(),
            boot.resolve(model).as_deref(),
            sgb_border,
            ram_init,
        )
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
            rtc_vba_export: false,
            rtc_bgb_legacy: false,
            next_autosave: AUTOSAVE_CYCLES,
            quick_state: None,
            rewind: std::collections::VecDeque::new(),
            next_rewind_frame: 0,
            boot: boot.to_owned(),
            sgb_bios: None,
            plugins_dir: None,
            sgb_border,
            ram_init,
        })
    }

    /// Update the power-on RAM init used by the next `reset`/`set_model` rebuild
    /// (Options → System → "uninitialized RAM at power-on"). Power-on state, so
    /// it does not touch the currently running machine's RAM.
    pub(crate) fn set_ram_init(&mut self, ram_init: Option<RamInit>) {
        self.ram_init = ram_init;
    }

    /// Install the optional user-supplied SGB BIOS (`--sgb-bios`) and keep it so
    /// a later `reset`/`set_model` re-applies it. A no-op off SGB. The border
    /// and title palette are not extracted (HLE) — see `GameBoy::load_sgb_bios`.
    pub(crate) fn set_sgb_bios(&mut self, bios: Option<Vec<u8>>) {
        self.sgb_bios = bios;
        self.apply_sgb_bios();
    }

    /// Re-apply the kept SGB BIOS to the current (freshly built) machine.
    fn apply_sgb_bios(&mut self) {
        if let Some(bios) = &self.sgb_bios {
            self.gb.load_sgb_bios(bios);
        }
    }

    /// Set the plugins directory the SGB coprocessor auto-loads from, then apply
    /// it to the current machine. Kept so a `reset`/`set_model` rebuild re-injects
    /// from the same place.
    pub(crate) fn set_plugins_dir(&mut self, dir: Option<PathBuf>) {
        self.plugins_dir = dir;
        self.apply_sgb_coprocessor();
    }

    /// Inject the combined coprocessor into the current (freshly built) machine
    /// when its plugin is present. The SGB SNES-side chips run only from a loaded
    /// plugin: with `spc700.wasm` + `w65c816.wasm` in the plugins dir and an SGB
    /// machine, the combined 65C816+SPC700+S-DSP coprocessor replaces the built-in
    /// HLE `SgbApu`. A missing plugin (or non-SGB machine) leaves the `SgbApu` in
    /// place — the golden-safe default — silently, since absence is the norm; only
    /// a present-but-broken plugin is logged. Built at the core's default output
    /// rate (the GameBoy APU's rate) so the two streams stay sample-aligned.
    fn apply_sgb_coprocessor(&mut self) {
        // Off SGB the machine holds no coprocessor slot; skip the wasm load
        // entirely (`set_audio_coprocessor` would drop the box anyway).
        if !matches!(self.model, Model::Sgb | Model::Sgb2) {
            return;
        }
        let Some(dir) = &self.plugins_dir else { return };
        // No coprocessor plugin in the dir → HLE default, not an error.
        if !dir.join(SPC_WASM).exists() || !dir.join(CPU_WASM).exists() {
            return;
        }
        match SgbCoprocessor::load(dir, DEFAULT_SAMPLE_RATE) {
            Ok(cop) => self.gb.set_audio_coprocessor(Box::new(cop)),
            Err(e) => eprintln!("slopgb: {e}; using the built-in SGB APU"),
        }
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
        // The restored machine's cycle counter jumped back; re-anchor the
        // autosave deadline to it (else periodic battery-RAM flush is suppressed
        // until emulated time replays past the stale deadline).
        self.next_autosave = self.gb.cycles().saturating_add(AUTOSAVE_CYCLES);
        true
    }

    /// Capture a rewind snapshot if the interval has elapsed (System → "Rewind
    /// enabled"; the caller invokes this only while playing with rewind on).
    pub(crate) fn capture_rewind(&mut self) {
        let frame = self.gb.frame_count();
        if frame < self.next_rewind_frame {
            return;
        }
        self.next_rewind_frame = frame + REWIND_INTERVAL_FRAMES;
        if self.rewind.len() >= REWIND_MAX_STATES {
            self.rewind.pop_front();
        }
        self.rewind.push_back(self.gb.save_state());
    }

    /// Restore (and drop) the most recent rewind snapshot. Returns whether one
    /// was available — `false` means the ring is empty (nothing to rewind to).
    pub(crate) fn rewind_step(&mut self) -> bool {
        match self.rewind.pop_back() {
            Some(bytes) => {
                let _ = self.gb.load_state(&bytes);
                self.next_autosave = self.gb.cycles().saturating_add(AUTOSAVE_CYCLES);
                self.next_rewind_frame = 0; // recapture promptly once play resumes
                true
            }
            None => false,
        }
    }

    /// Drop the rewind ring — a reset / ROM change makes the states stale.
    pub(crate) fn clear_rewind(&mut self) {
        self.rewind.clear();
        self.next_rewind_frame = 0;
    }

    /// Save state to disk (bgb File / State → Save state): write the serialized
    /// machine to `path` via a temp-file-then-rename (same durability as the
    /// battery `.sav` write — an interrupted save can't destroy a prior good
    /// state file). Returns an error string (logged by the caller) on I/O error.
    pub(crate) fn save_state_to(&self, path: &Path) -> Result<(), String> {
        write_atomic(path, &self.gb.save_state()).map_err(|e| format!("{e}"))
    }

    /// Load state from disk (bgb File / State → Load state): read `path` and
    /// restore the machine. The restore is atomic — a bad/foreign/corrupt file
    /// leaves the running machine intact ([`GameBoy::load_state`]). Returns an
    /// error string (logged by the caller) on I/O or validation failure.
    pub(crate) fn load_state_from(&mut self, path: &Path) -> Result<(), String> {
        let bytes = fs::read(path).map_err(|e| format!("{e}"))?;
        self.gb.load_state(&bytes).map_err(|e| format!("{e}"))?;
        // Re-anchor autosave to the restored (earlier) cycle counter, as in
        // `quick_load` / `reset`.
        self.next_autosave = self.gb.cycles().saturating_add(AUTOSAVE_CYCLES);
        Ok(())
    }

    /// Power-cycle: fresh machine, save RAM reloaded from disk. Re-runs the boot
    /// ROM (logo + chime) when one is configured for the model, like bgb.
    pub(crate) fn reset(&mut self) {
        self.flush_save();
        self.clear_rewind(); // the pre-reset states no longer apply
        let boot = self.boot.resolve(self.model);
        match build_gb(
            self.model,
            self.rom_bytes.clone(),
            boot.as_deref(),
            self.sgb_border,
            self.ram_init,
        ) {
            Ok(mut gb) => {
                if let Ok(data) = fs::read(&self.sav_path) {
                    let _ = gb.load_save_data(&data); // rejection already warned at load
                }
                self.gb = gb;
                self.apply_sgb_bios();
                self.apply_sgb_coprocessor();
                self.next_autosave = AUTOSAVE_CYCLES;
            }
            // Can't happen (the same image loaded before), but never panic.
            Err(e) => eprintln!("slopgb: reset failed: {e}"),
        }
    }

    /// Switch the emulated system (Options → System → Emulated system): resolve
    /// `choice` against the ROM header and rebuild the machine, reloading battery
    /// RAM. A no-op (returns `false`) when the resolved model *and* border already
    /// match, so re-applying Options doesn't needlessly power-cycle.
    pub(crate) fn set_model(&mut self, choice: ModelChoice) -> bool {
        let (model, sgb_border) = choice.resolve(&self.rom_bytes);
        if model == self.model && sgb_border == self.sgb_border {
            return false;
        }
        self.flush_save();
        let boot = self.boot.resolve(model);
        match build_gb(
            model,
            self.rom_bytes.clone(),
            boot.as_deref(),
            sgb_border,
            self.ram_init,
        ) {
            Ok(mut gb) => {
                if let Ok(data) = fs::read(&self.sav_path) {
                    let _ = gb.load_save_data(&data);
                }
                self.gb = gb;
                self.model = model;
                self.sgb_border = sgb_border;
                self.apply_sgb_bios();
                self.apply_sgb_coprocessor();
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

    /// Options → System → "Save RTC in SAV file (VBA compatible)": choose the
    /// `.sav` RTC layout. Only affects the next write of an RTC cart.
    pub(crate) fn set_rtc_vba_export(&mut self, on: bool) {
        self.rtc_vba_export = on;
    }

    /// Options → System → "Save BGB legacy RTC files": also write a `<rom>.rtc`
    /// sidecar. Only affects the next write of an RTC cart.
    pub(crate) fn set_rtc_bgb_legacy(&mut self, on: bool) {
        self.rtc_bgb_legacy = on;
    }

    /// Write the `<rom>.rtc` sidecar (the shared 48-byte RTC footer with a fresh
    /// wall-clock stamp) when the legacy-RTC option is on and the cart has an
    /// RTC. Called after a `.sav` write so it tracks the same dirty edge.
    fn write_rtc_sidecar(&self) {
        if !self.rtc_bgb_legacy {
            return;
        }
        let Some((live, latched)) = self.gb.rtc_state() else {
            return;
        };
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let footer = crate::rtc_export::vba_footer(live, latched, secs);
        let rtc_path = self.sav_path.with_extension("rtc");
        if let Err(e) = write_atomic(&rtc_path, &footer) {
            eprintln!(
                "slopgb: cannot write RTC file '{}': {e}",
                rtc_path.display()
            );
        }
    }

    /// The battery image to persist. With the VBA-RTC toggle on and an RTC cart,
    /// this is the raw SRAM plus a wall-clock-stamped VBA footer (readable by
    /// VBA / mGBA / SameBoy); otherwise slopgb's own `save_data` block.
    fn save_image(&self) -> Option<Vec<u8>> {
        if self.rtc_vba_export {
            if let (Some(mut ram), Some((live, latched))) =
                (self.gb.battery_sram(), self.gb.rtc_state())
            {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                ram.extend_from_slice(&crate::rtc_export::vba_footer(live, latched, secs));
                return Some(ram);
            }
        }
        self.gb.save_data()
    }

    /// Write battery RAM to `<rom>.sav` if it changed since the last write. The
    /// dirty check compares the canonical (timestamp-free) `save_data` image, so
    /// the VBA export's moving wall clock never forces a redundant write.
    pub(crate) fn flush_save(&mut self) {
        let Some(canonical) = self.gb.save_data() else {
            return; // cartridge has no battery RAM
        };
        if self.last_saved.as_deref() == Some(canonical.as_slice()) {
            return;
        }
        let image = self.save_image().unwrap_or_else(|| canonical.clone());
        match write_atomic(&self.sav_path, &image) {
            Ok(()) => {
                self.last_saved = Some(canonical);
                // Sidecar `<rom>.rtc` shares the same dirty edge (no-op unless
                // the legacy-RTC option is on and the cart has an RTC).
                self.write_rtc_sidecar();
            }
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
/// existing file: the data blocks are durable before the rename can commit.
/// Creates the parent dir if missing.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
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

/// Whether a boot ROM of `len` bytes matches the model's class — 256 B for
/// DMG/MGB/SGB, 2304 B for CGB/AGB (the two boot-ROM sizes slopgb maps).
pub(crate) fn boot_size_ok(model: Model, len: usize) -> bool {
    if model.is_cgb() {
        len == 0x900
    } else {
        len == 0x100
    }
}

/// Which boot ROM to execute on a ROM load. The Options bootrom paths (when
/// `enabled`) take precedence over the `--boot`/`SLOPGB_BOOT` `fallback`; the
/// slot is chosen by the resolved model.
pub(crate) struct BootSpec<'a> {
    pub enabled: bool,
    pub dmg: &'a str,
    pub gbc: &'a str,
    pub sgb: &'a str,
    pub fallback: Option<&'a [u8]>,
}

impl BootSpec<'_> {
    /// No boot ROM (the default).
    pub(crate) const NONE: BootSpec<'static> = BootSpec {
        enabled: false,
        dmg: "",
        gbc: "",
        sgb: "",
        fallback: None,
    };

    /// Only the `--boot`/`SLOPGB_BOOT` fallback (no Options paths) — for the
    /// startup load, before the Options dialog has been touched.
    pub(crate) fn cli(fallback: Option<&[u8]>) -> BootSpec<'_> {
        BootSpec {
            fallback,
            ..BootSpec::NONE
        }
    }

    /// Resolve the boot ROM bytes for `model`: the enabled, size-valid slot path
    /// read from disk, else the CLI/env fallback, else `None`.
    fn resolve(&self, model: Model) -> Option<Vec<u8>> {
        if self.enabled {
            let path = if model.is_cgb() {
                self.gbc
            } else if matches!(model, Model::Sgb | Model::Sgb2) {
                self.sgb
            } else {
                self.dmg
            };
            if !path.is_empty() {
                match fs::read(path) {
                    Ok(b) if boot_size_ok(model, b.len()) => return Some(b),
                    Ok(b) => eprintln!(
                        "slopgb: bootrom '{path}' is {} bytes (wrong for {model:?}); skipping",
                        b.len()
                    ),
                    Err(e) => eprintln!("slopgb: cannot read bootrom '{path}': {e}"),
                }
            }
        }
        self.fallback.map(<[u8]>::to_vec)
    }

    /// Capture the (borrowed) spec into an owned copy a [`Session`] can keep, so
    /// a later `reset`/`set_model` can re-resolve the boot ROM per model.
    fn to_owned(&self) -> OwnedBootSpec {
        OwnedBootSpec {
            enabled: self.enabled,
            dmg: self.dmg.to_owned(),
            gbc: self.gbc.to_owned(),
            sgb: self.sgb.to_owned(),
            fallback: self.fallback.map(<[u8]>::to_vec),
        }
    }
}

/// An owned [`BootSpec`] a [`Session`] keeps so a power-cycle / model switch can
/// re-resolve the boot ROM for the (possibly new) model. The default is "no boot
/// ROM" (matching [`BootSpec::NONE`]).
#[derive(Clone, Default)]
pub(crate) struct OwnedBootSpec {
    enabled: bool,
    dmg: String,
    gbc: String,
    sgb: String,
    fallback: Option<Vec<u8>>,
}

impl OwnedBootSpec {
    /// Resolve the boot ROM bytes for `model` (see [`BootSpec::resolve`]).
    fn resolve(&self, model: Model) -> Option<Vec<u8>> {
        BootSpec {
            enabled: self.enabled,
            dmg: &self.dmg,
            gbc: &self.gbc,
            sgb: &self.sgb,
            fallback: self.fallback.as_deref(),
        }
        .resolve(model)
    }
}

/// Build the machine: **execute** `boot` from power-on (bgb's boot ROM) when it
/// is present and the right size for `model`, else the direct post-boot install.
/// A wrong-size boot ROM falls back to no-boot (logged, non-fatal).
fn build_gb(
    model: Model,
    rom: Vec<u8>,
    boot: Option<&[u8]>,
    sgb_border: bool,
    ram_init: Option<RamInit>,
) -> Result<GameBoy, CartridgeError> {
    // "GBC + initial SGB border": grab the game's own SGB border from an initial
    // SGB run BEFORE `rom` is consumed by the build below, then show it while the
    // real machine runs in CGB color. Only SGB-capable ROMs upload a border; the
    // rest fall back to the built-in default. `capture_initial_sgb_border`
    // returns as soon as the game uploads (≈200 frames for Pokémon G/S), so the
    // 600-frame cap only bites on a ROM that never does.
    let border = if sgb_border && GameBoy::rom_supports_sgb(&rom) {
        GameBoy::capture_initial_sgb_border(&rom, 600)
    } else {
        None
    };
    let build = || match boot {
        Some(b) if boot_size_ok(model, b.len()) => GameBoy::new_with_boot(model, rom, b.to_vec()),
        Some(b) => {
            let needs = if model.is_cgb() { 2304 } else { 256 };
            eprintln!(
                "slopgb: ignoring boot ROM ({} bytes — {model:?} needs {needs}); booting post-boot",
                b.len()
            );
            GameBoy::new(model, rom)
        }
        None => GameBoy::new(model, rom),
    };
    let mut gb = build()?;
    // Power-on RAM init (before any `.sav` load, which overwrites cart SRAM):
    // `None` keeps the deterministic 0xFF cart-SRAM default (byte-identical to
    // `GameBoy::new`); a frontend `--ram-init` overrides it.
    if let Some(init) = ram_init {
        gb.init_ram(init);
    }
    if sgb_border {
        match &border {
            Some(b) => gb.install_sgb_border(b),
            None => gb.enable_sgb_border(), // no game border → the default one
        }
    }
    Ok(gb)
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
