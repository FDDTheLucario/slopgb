//! The roles-and-slots plugin registry: a single surface over every
//! `SUBSYSTEM`-tier contributing unit — a loaded wasm plugin or a native
//! orchestrator (e.g. the frontend's SGB coprocessor) — queried uniformly for
//! "who fills role X", "what CLI flags/menu rows exist", and "what is flag Y's
//! effective value" against the current ambient context (ROM path, plugins
//! dir). Building or querying an empty registry never touches `slopgb-core`.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::coprocessor::LoadedCoprocessor;
use crate::manifest::{FlagContribution, Manifest, MenuContribution};

/// Ambient context every unit's contributed defaults resolve against.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub rom_dir: Option<PathBuf>,
    pub rom_path: Option<PathBuf>,
    pub plugins_dir: Option<PathBuf>,
}

/// One contributing unit: a loaded wasm plugin (`source` = its file name, e.g.
/// `"msu1.wasm"`) or a native orchestrator (`source` = e.g. `"SgbCoprocessor"`).
#[derive(Debug)]
pub struct Unit {
    pub source: String,
    pub manifest: Manifest,
}

/// A registry-construction failure.
#[derive(Debug)]
pub enum RegistryError {
    /// The scanned directory could not be read.
    Io(String),
    /// Two units both declare the same capability role.
    DuplicateRole {
        role: String,
        first: String,
        second: String,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::Io(e) => write!(f, "{e}"),
            RegistryError::DuplicateRole {
                role,
                first,
                second,
            } => write!(f, "two plugins provide role '{role}': {first}, {second}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// The roles-and-slots plugin registry: every unit found by [`Self::scan`] plus
/// any natively [`Self::register`]ed one, the ambient [`Context`] their default
/// flag values resolve against, and any explicit flag values set from the CLI.
#[derive(Default, Debug)]
pub struct PluginRegistry {
    units: Vec<Unit>,
    context: Context,
    explicit_flags: Vec<(String, String)>,
}

impl PluginRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan every `*.wasm` in `dir` in a deterministic (sorted-by-filename)
    /// order, load each through [`LoadedCoprocessor::load`] and read its
    /// manifest. A file that fails to load or declares no manifest is a
    /// loader mismatch (a tier-1/tier-2 plugin, or a non-plugin file), not an
    /// error, and is skipped silently. A duplicate role is a hard error.
    pub fn scan(dir: &Path) -> Result<Self, RegistryError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|e| RegistryError::Io(format!("{}: {e}", dir.display())))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|p| p.extension().is_some_and(|e| e == "wasm"))
            .collect();
        paths.sort();

        let mut registry = Self::new();
        for path in paths {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let Ok(mut cop) = LoadedCoprocessor::load(&bytes) else {
                continue;
            };
            let Some(manifest) = cop.manifest() else {
                continue;
            };
            registry.register(&name, manifest)?;
        }
        Ok(registry)
    }

    /// Register a unit's contributions under `source`. A role also provided by
    /// an already-registered unit is a hard error naming both sources.
    pub fn register(&mut self, source: &str, manifest: Manifest) -> Result<(), RegistryError> {
        for role in &manifest.provides {
            if let Some(existing) = self.unit_for_role(role) {
                return Err(RegistryError::DuplicateRole {
                    role: role.clone(),
                    first: existing.source.clone(),
                    second: source.to_string(),
                });
            }
        }
        self.units.push(Unit {
            source: source.to_string(),
            manifest,
        });
        Ok(())
    }

    pub fn set_context(&mut self, ctx: Context) {
        self.context = ctx;
    }

    #[must_use]
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Record an explicit value for a contributed flag (from the CLI/env),
    /// overriding its declared default.
    pub fn set_flag(&mut self, name: &str, value: &str) {
        match self.explicit_flags.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = value.to_string(),
            None => self
                .explicit_flags
                .push((name.to_string(), value.to_string())),
        }
    }

    /// The effective value of flag `name`: the explicit value if set, else its
    /// declared default with any ambient token expanded against the current
    /// context, else `None`. Lazy: a `set_context` after this call is read on
    /// the next `flag` call, no re-registration needed.
    #[must_use]
    pub fn flag(&self, name: &str) -> Option<String> {
        if let Some((_, v)) = self.explicit_flags.iter().find(|(n, _)| n == name) {
            return Some(v.clone());
        }
        let fc = self
            .units
            .iter()
            .flat_map(|u| u.manifest.flags.iter())
            .find(|f| f.name == name)?;
        if fc.default.is_empty() {
            return None;
        }
        self.expand(&fc.default)
    }

    /// Expand an ambient token against the current context; a literal default
    /// passes through unchanged. `None` when the token names ambient context
    /// that hasn't been set.
    fn expand(&self, default: &str) -> Option<String> {
        let path = match default {
            "$rom_dir" => &self.context.rom_dir,
            "$rom_path" => &self.context.rom_path,
            "$plugins_dir" => &self.context.plugins_dir,
            literal => return Some(literal.to_string()),
        };
        path.as_ref().map(|p| p.display().to_string())
    }

    /// Every declared flag, paired with its declaring source.
    #[must_use]
    pub fn flags(&self) -> Vec<(&str, &FlagContribution)> {
        self.units
            .iter()
            .flat_map(|u| u.manifest.flags.iter().map(move |f| (u.source.as_str(), f)))
            .collect()
    }

    /// Every declared menu row, paired with its declaring source.
    #[must_use]
    pub fn menus(&self) -> Vec<(&str, &MenuContribution)> {
        self.units
            .iter()
            .flat_map(|u| u.manifest.menus.iter().map(move |m| (u.source.as_str(), m)))
            .collect()
    }

    /// The unit filling `role`, if any.
    #[must_use]
    pub fn unit_for_role(&self, role: &str) -> Option<&Unit> {
        self.units
            .iter()
            .find(|u| u.manifest.provides.iter().any(|r| r == role))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    #[must_use]
    pub fn units(&self) -> &[Unit] {
        &self.units
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
