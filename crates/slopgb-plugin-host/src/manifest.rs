//! A coprocessor's self-describing **manifest** — what a plugin declares about
//! itself so the host binds it by identity/role instead of by filename, and
//! surfaces its contributed CLI flags. The guest emits it (see
//! `slopgb_plugin_api::Coprocessor::MANIFEST`); this parses it.
//!
//! Wire format: line-based UTF-8, one record per line, TAB-separated, first
//! field = record type. Unknown record types are ignored, so the schema grows
//! without an ABI break.

/// A parsed coprocessor manifest.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Stable logical identity + role key (e.g. `"msu1"`). What a caller binds
    /// by, replacing the filename convention.
    pub id: String,
    /// Human display label for UI / logs.
    pub name: String,
    /// Capability roles this coprocessor can fill (a caller's slot requires one).
    pub provides: Vec<String>,
    /// CLI flags this plugin contributes to the frontend.
    pub flags: Vec<FlagContribution>,
    /// Menu rows this plugin contributes (e.g. an "Export ..." action).
    pub menus: Vec<MenuContribution>,
}

/// A CLI flag a plugin contributes: the flag `name` (without dashes), an `arg`
/// hint (`none` / `path` / `dir` / `string`), one-line `help`, and a `default`
/// value used when the flag is not given explicitly. `default` may be empty (no
/// default), a literal, or one of the ambient tokens `$rom_dir` / `$rom_path` /
/// `$plugins_dir`, expanded by the [`crate::PluginRegistry`] against its current
/// ambient context.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FlagContribution {
    pub name: String,
    pub arg: String,
    pub help: String,
    pub default: String,
}

/// A menu row a plugin contributes: a `label` to show, the guest `export` (a
/// coprocessor entry point, e.g. a save-state-shaped dump) it invokes, and the
/// file `ext` the result should be saved as (without the dot).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MenuContribution {
    pub label: String,
    pub export: String,
    pub ext: String,
}

impl Manifest {
    /// Parse a manifest blob. Returns `None` for an empty/whitespace-only blob
    /// (an undeclared coprocessor) or non-UTF-8 bytes; otherwise the parsed
    /// manifest, skipping blank lines and unrecognized record types.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        if text.trim().is_empty() {
            return None;
        }
        let mut m = Manifest::default();
        for line in text.lines() {
            let mut f = line.split('\t');
            match f.next() {
                Some("id") => m.id = f.next().unwrap_or_default().to_string(),
                Some("name") => m.name = f.next().unwrap_or_default().to_string(),
                Some("provides") => {
                    if let Some(role) = f.next().filter(|r| !r.is_empty()) {
                        m.provides.push(role.to_string());
                    }
                }
                Some("flag") => {
                    if let Some(name) = f.next().filter(|n| !n.is_empty()) {
                        m.flags.push(FlagContribution {
                            name: name.to_string(),
                            arg: f.next().unwrap_or_default().to_string(),
                            help: f.next().unwrap_or_default().to_string(),
                            default: f.next().unwrap_or_default().to_string(),
                        });
                    }
                }
                Some("menu") => {
                    if let Some(label) = f.next().filter(|l| !l.is_empty()) {
                        m.menus.push(MenuContribution {
                            label: label.to_string(),
                            export: f.next().unwrap_or_default().to_string(),
                            ext: f.next().unwrap_or_default().to_string(),
                        });
                    }
                }
                // Blank line or a record type this host version doesn't know
                // (a newer plugin's `requires` / etc.) — ignored.
                _ => {}
            }
        }
        Some(m)
    }
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
