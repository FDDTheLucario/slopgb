//! Pure picker logic — **no filesystem access**. Everything here derives from
//! `self.entries` + view state, so it is unit-testable via
//! [`Picker::with_entries`]. The one exception is [`Picker::navigate_to`], which
//! reads a new directory through [`crate::source`]; it is exercised by the
//! integration test against a temp dir, not the pure unit tests.

use super::*;
use std::path::Path;

impl Picker {
    // ---- derived listing (the heart) -----------------------------------

    /// The sorted + filtered listing the user actually sees, as references into
    /// `self.entries`. Order: directories first, then by `sort_key`/`sort_dir`,
    /// case-insensitive, stable. Hidden (dot-prefixed) entries are dropped
    /// unless `show_hidden`. Files failing the extension filter are dropped
    /// unless `all_files` (or `filters` is empty); directories are never
    /// extension-filtered.
    #[must_use]
    pub(crate) fn visible(&self) -> Vec<&Entry> {
        let mut v: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .filter(|e| {
                e.is_dir
                    || self.all_files
                    || self.filters.is_empty()
                    || self.filters.contains(&extension(&e.name))
            })
            .collect();
        v.sort_by(|a, b| {
            dir_rank(a)
                .cmp(&dir_rank(b))
                .then_with(|| {
                    let key_cmp = key_order(self.sort_key, a, b);
                    if self.sort_dir == SortDir::Desc { key_cmp.reverse() } else { key_cmp }
                })
                .then_with(|| name_key(a).cmp(&name_key(b)))
        });
        v
    }

    // ---- selection + scroll --------------------------------------------

    /// The currently highlighted entry, if any.
    #[must_use]
    pub(crate) fn selected(&self) -> Option<&Entry> {
        self.visible().get(self.sel).copied()
    }

    /// Move the selection by `delta` rows, clamped to `[0, len-1]`; clears the
    /// type-ahead buffer. `delta` may be negative.
    pub(crate) fn move_sel(&mut self, delta: isize) {
        let len = self.visible().len();
        let max = len.saturating_sub(1) as isize;
        let target = (self.sel as isize + delta).clamp(0, max);
        self.sel = target as usize;
        self.search.clear();
        self.clamp_scroll();
    }

    /// Re-clamp `offset` so the selection stays within `[offset, offset+viewport)`.
    pub(crate) fn clamp_scroll(&mut self) {
        let len = self.visible().len();
        let viewport = self.viewport.max(1);
        if self.sel < self.offset {
            self.offset = self.sel;
        } else if self.sel >= self.offset + viewport {
            self.offset = self.sel + 1 - viewport;
        }
        let max_offset = len.saturating_sub(viewport);
        if self.offset > max_offset {
            self.offset = max_offset;
        }
    }

    // ---- type-ahead -----------------------------------------------------

    /// Append `ch` to the type-ahead buffer and jump the selection to the first
    /// visible entry whose (lowercased) name starts with the buffer. A repeated
    /// press of the same single char with no match cycles to the next entry
    /// starting with that char.
    ///
    /// The buffer only ever clears on a nav key (see the `search.clear()` calls
    /// throughout this file) or an explicit [`Picker::clear_search`] — this
    /// crate is std-only with no clock, so it never times a buffer out itself.
    /// A host that wants "pause then a new letter starts a fresh search" should
    /// call `clear_search` after its own idle timeout.
    pub(crate) fn typeahead(&mut self, ch: char) {
        self.search.push(ch);
        let query = self.search.to_lowercase();
        let visible = self.visible();
        if visible.is_empty() {
            self.clamp_scroll();
            return;
        }
        let first_char = query.chars().next();
        let is_single_repeated = first_char.is_some_and(|c| query.chars().all(|x| x == c));

        if is_single_repeated {
            let c = first_char.expect("checked above");
            let matches: Vec<usize> = visible
                .iter()
                .enumerate()
                .filter(|(_, e)| e.name.to_lowercase().starts_with(c))
                .map(|(i, _)| i)
                .collect();
            if !matches.is_empty() {
                let current_matches =
                    visible.get(self.sel).is_some_and(|e| e.name.to_lowercase().starts_with(c));
                self.sel = if current_matches {
                    // Repeated press while already on a match: cycle to the next
                    // one, wrapping past the end.
                    matches.iter().find(|&&i| i > self.sel).copied().unwrap_or(matches[0])
                } else {
                    matches[0]
                };
            }
        } else if let Some(i) = visible.iter().position(|e| e.name.to_lowercase().starts_with(query.as_str())) {
            self.sel = i;
        }
        self.clamp_scroll();
    }

    /// Empty the type-ahead buffer without moving the selection. Every nav key
    /// already does this internally (see [`Self::typeahead`]'s doc); this is
    /// the explicit hook for a host that wants to time a search out itself
    /// (this crate is std-only with no clock, so it can't do that on its own).
    pub fn clear_search(&mut self) {
        self.search.clear();
    }

    // ---- enter / navigation intent (pure) ------------------------------

    /// Resolve what Enter on the current selection means, **without** touching
    /// the fs: a directory -> `Navigate(path)`, a file in Open mode ->
    /// `Pick(path)`, otherwise `None`. Paths are `self.cwd.join(name)`.
    #[must_use]
    pub(crate) fn resolve_enter(&self) -> EnterAction {
        match self.selected() {
            Some(e) if e.is_dir => EnterAction::Navigate(self.cwd.join(&e.name)),
            Some(e) if self.mode == Mode::Open => EnterAction::Pick(self.cwd.join(&e.name)),
            _ => EnterAction::None,
        }
    }

    /// Execute a resolved [`EnterAction`] against picker state.
    fn execute_enter(&mut self) -> Outcome {
        match self.resolve_enter() {
            EnterAction::Navigate(p) => {
                self.navigate_to(p);
                Outcome::None
            }
            EnterAction::Pick(p) => Outcome::Picked(p),
            EnterAction::None => Outcome::None,
        }
    }

    /// The parent of `cwd`, if it has one.
    #[must_use]
    pub(crate) fn parent(&self) -> Option<PathBuf> {
        self.cwd.parent().map(Path::to_path_buf)
    }

    /// Replace the listing by reading `path` from disk (via [`crate::source`]);
    /// reset selection/scroll/search and update `cwd`. On error keep `cwd` and
    /// set `status`. THIS TOUCHES THE FS — integration-tested, not unit-tested.
    pub(crate) fn navigate_to(&mut self, path: PathBuf) {
        match crate::source::read_dir(&path) {
            Ok(e) => {
                self.entries = e;
                self.cwd = path;
                self.sel = 0;
                self.offset = 0;
                self.search.clear();
                self.status.clear();
            }
            Err(e) => {
                self.status = format!("cannot read directory: {e}");
            }
        }
    }

    // ---- path bar (editable) -------------------------------------------

    /// Longest common prefix of visible names that start with the path bar's
    /// final component — used by Tab completion.
    #[must_use]
    pub(crate) fn path_completion(&self) -> Option<String> {
        let start = final_component_start(&self.path_edit);
        let prefix = &self.path_edit[start..];
        let candidates: Vec<&str> = self
            .visible()
            .iter()
            .map(|e| e.name.as_str())
            .filter(|name| name.starts_with(prefix))
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let lcp = longest_common_prefix(&candidates);
        if lcp.len() > prefix.len() { Some(lcp) } else { None }
    }

    // ---- input dispatch -------------------------------------------------

    /// Feed one semantic key. Dispatch by `self.focus`; see the `on_key_*`
    /// helpers below for each focus's behavior.
    pub fn on_key(&mut self, key: Key) -> Outcome {
        match self.focus {
            Focus::Browse => self.on_key_browse(key),
            Focus::PathBar => self.on_key_pathbar(key),
            Focus::SaveName => self.on_key_savename(key),
        }
    }

    /// `Browse` focus: arrows/page/home/end move; Enter runs `resolve_enter`;
    /// Back / Backspace go to `parent()`; Char feeds `typeahead` (or, in
    /// `Mode::Save`, starts editing the save-name field — Save mode has no
    /// other way to enter a filename, so a Char typed while browsing commits
    /// to naming rather than searching); the toggle keys flip sort/hidden/
    /// all-files; FocusPath enters the path bar; Cancel -> `Outcome::Cancelled`.
    fn on_key_browse(&mut self, key: Key) -> Outcome {
        match key {
            Key::Up => {
                self.move_sel(-1);
                Outcome::None
            }
            Key::Down => {
                self.move_sel(1);
                Outcome::None
            }
            Key::PageUp => {
                self.move_sel(-(self.viewport as isize));
                Outcome::None
            }
            Key::PageDown => {
                self.move_sel(self.viewport as isize);
                Outcome::None
            }
            Key::Home => {
                self.sel = 0;
                self.search.clear();
                self.clamp_scroll();
                Outcome::None
            }
            Key::End => {
                self.sel = self.visible().len().saturating_sub(1);
                self.search.clear();
                self.clamp_scroll();
                Outcome::None
            }
            Key::Enter => self.execute_enter(),
            Key::Back | Key::Backspace => {
                if let Some(p) = self.parent() {
                    self.navigate_to(p);
                }
                Outcome::None
            }
            Key::Char(c) => {
                // ponytail: Save mode has no dedicated "start naming" key, so a
                // Char typed in Browse commits to the save-name field instead of
                // type-ahead search (see doc comment above).
                if self.mode == Mode::Save {
                    self.focus = Focus::SaveName;
                    self.save_name.push(c);
                } else {
                    self.typeahead(c);
                }
                Outcome::None
            }
            Key::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.sel = 0;
                self.offset = 0;
                self.clamp_scroll();
                Outcome::None
            }
            Key::ToggleAllFiles => {
                self.all_files = !self.all_files;
                self.sel = 0;
                self.offset = 0;
                self.clamp_scroll();
                Outcome::None
            }
            Key::CycleSort => {
                self.sort_key = match self.sort_key {
                    SortKey::Name => SortKey::Size,
                    SortKey::Size => SortKey::Mtime,
                    SortKey::Mtime => SortKey::Kind,
                    SortKey::Kind => SortKey::Name,
                };
                self.clamp_scroll();
                Outcome::None
            }
            Key::ToggleSortDir => {
                self.sort_dir = match self.sort_dir {
                    SortDir::Asc => SortDir::Desc,
                    SortDir::Desc => SortDir::Asc,
                };
                Outcome::None
            }
            Key::FocusPath => {
                self.focus = Focus::PathBar;
                self.path_edit = self.cwd.display().to_string();
                Outcome::None
            }
            Key::Cancel => Outcome::Cancelled,
            Key::Tab => Outcome::None,
        }
    }

    /// `PathBar` focus: Char/Backspace edit `path_edit`; Tab completes; Enter
    /// navigates to the typed path; Cancel returns to `Browse` (not cancel).
    fn on_key_pathbar(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(c) => {
                self.path_edit.push(c);
                Outcome::None
            }
            Key::Backspace => {
                self.path_edit.pop();
                Outcome::None
            }
            Key::Tab => {
                if let Some(completed) = self.path_completion() {
                    let start = final_component_start(&self.path_edit);
                    self.path_edit.truncate(start);
                    self.path_edit.push_str(&completed);
                }
                Outcome::None
            }
            Key::Enter => {
                let path = PathBuf::from(&self.path_edit);
                self.navigate_to(path);
                self.focus = Focus::Browse;
                Outcome::None
            }
            Key::Cancel => {
                self.focus = Focus::Browse;
                Outcome::None
            }
            _ => Outcome::None,
        }
    }

    /// `SaveName` focus: Char/Backspace edit `save_name`; Enter picks
    /// `cwd.join(save_name)` with the two-stage overwrite confirm (existence is
    /// checked against the in-memory `entries` list, never the fs, to stay
    /// pure); Cancel returns to `Browse`.
    fn on_key_savename(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(c) => {
                self.save_name.push(c);
                self.overwrite_pending = false;
                Outcome::None
            }
            Key::Backspace => {
                self.save_name.pop();
                self.overwrite_pending = false;
                Outcome::None
            }
            Key::Enter => {
                if self.save_name.is_empty() {
                    return Outcome::None;
                }
                let target = self.cwd.join(&self.save_name);
                let exists_as_file =
                    self.entries.iter().any(|e| !e.is_dir && e.name == self.save_name);
                if exists_as_file && !self.overwrite_pending {
                    self.overwrite_pending = true;
                    Outcome::None
                } else {
                    Outcome::Picked(target)
                }
            }
            Key::Cancel => {
                self.focus = Focus::Browse;
                self.overwrite_pending = false;
                Outcome::None
            }
            _ => Outcome::None,
        }
    }

    /// Mouse: highlight the visible row at absolute index `idx` (no-op if out of
    /// range). Clears type-ahead.
    pub fn on_click(&mut self, idx: usize) {
        if idx < self.visible().len() {
            self.sel = idx;
            self.search.clear();
            self.clamp_scroll();
        }
    }

    /// Mouse: activate (double-click / open) the visible row at `idx` — same
    /// effect as selecting it then pressing Enter.
    pub fn on_activate(&mut self, idx: usize) -> Outcome {
        self.on_click(idx);
        self.execute_enter()
    }

    // ---- view-model -----------------------------------------------------

    /// Build the drawable [`View`] for a window of `viewport_rows` rows, and
    /// record that height so later `PageUp`/`PageDown` use the real page size.
    /// Takes `&mut self` because a picker is a live widget: the host calls this
    /// once per frame with its actual row count, which is exactly when the page
    /// size is known. Formats the size/mtime columns and composes the status.
    // ponytail: visible() is re-sorted here and again inside clamp_scroll — two
    // sorts/frame on a directory listing, trivially cheap; revisit only if a
    // huge dir ever shows up on a profile.
    pub fn view(&mut self, viewport_rows: usize) -> View {
        self.viewport = viewport_rows.max(1);
        self.clamp_scroll();
        let visible = self.visible();
        let total = visible.len();
        let viewport = self.viewport;
        let offset = self.offset;

        let end = (offset + viewport).min(total);
        let rows: Vec<Row> = visible[offset..end]
            .iter()
            .map(|e| Row {
                name: e.name.clone(),
                is_dir: e.is_dir,
                size: fmt_size(e.size),
                mtime: fmt_mtime(e.mtime),
            })
            .collect();

        let highlight = self.sel.checked_sub(offset).filter(|h| *h < rows.len());

        let status = if !self.status.is_empty() {
            self.status.clone()
        } else {
            let mut s = format!("{total} item{}", if total == 1 { "" } else { "s" });
            if self.show_hidden {
                s.push_str(" · .*");
            }
            if !self.search.is_empty() {
                s.push_str(" · ");
                s.push_str(&self.search);
            }
            s
        };

        View {
            title: self.title.clone(),
            path_bar: if self.focus == Focus::PathBar {
                self.path_edit.clone()
            } else {
                self.cwd.display().to_string()
            },
            path_focused: self.focus == Focus::PathBar,
            rows,
            highlight,
            offset,
            total,
            status,
            save_name: (self.mode == Mode::Save).then(|| self.save_name.clone()),
            overwrite_pending: self.overwrite_pending,
        }
    }
}

// ---- free helpers (pure) -------------------------------------------------

/// Dirs sort before files regardless of key/direction.
fn dir_rank(e: &Entry) -> u8 {
    if e.is_dir {
        0
    } else {
        1
    }
}

/// Case-insensitive name key (the stable tie-break, and the `Name` sort key).
fn name_key(e: &Entry) -> String {
    e.name.to_lowercase()
}

/// Lowercased extension (chars after the last '.'), empty string if none.
fn extension(name: &str) -> String {
    match name.rfind('.') {
        Some(i) => name[i + 1..].to_lowercase(),
        None => String::new(),
    }
}

fn key_order(key: SortKey, a: &Entry, b: &Entry) -> std::cmp::Ordering {
    match key {
        SortKey::Name => name_key(a).cmp(&name_key(b)),
        SortKey::Size => a.size.unwrap_or(0).cmp(&b.size.unwrap_or(0)),
        SortKey::Mtime => a.mtime.unwrap_or(0).cmp(&b.mtime.unwrap_or(0)),
        SortKey::Kind => (extension(&a.name), name_key(a)).cmp(&(extension(&b.name), name_key(b))),
    }
}

/// Byte offset of the final path component in an edit-buffer string (0 if it
/// has no separator).
fn final_component_start(s: &str) -> usize {
    match s.rfind(std::path::MAIN_SEPARATOR) {
        Some(i) => i + std::path::MAIN_SEPARATOR.len_utf8(),
        None => 0,
    }
}

/// Longest common prefix of a set of strings (empty if the set is empty).
fn longest_common_prefix(strs: &[&str]) -> String {
    let mut iter = strs.iter();
    let Some(first) = iter.next() else {
        return String::new();
    };
    let mut prefix = (*first).to_string();
    for s in iter {
        let mut end = 0;
        for (a, b) in prefix.chars().zip(s.chars()) {
            if a != b {
                break;
            }
            end += a.len_utf8();
        }
        prefix.truncate(end);
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

/// Human-readable byte size, e.g. `1.2 KB`. Empty string for `None`.
#[must_use]
pub(crate) fn fmt_size(size: Option<u64>) -> String {
    let Some(bytes) = size else {
        return String::new();
    };
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx < units.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    format!("{value:.1} {}", units[unit_idx])
}

/// `mtime` (epoch seconds) as a short `YYYY-MM-DD` string, std-only. Empty for
/// `None`.
#[must_use]
pub(crate) fn fmt_mtime(mtime: Option<u64>) -> String {
    let Some(secs) = mtime else {
        return String::new();
    };
    let days = (secs / 86400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days-since-epoch -> (year, month, day). Howard Hinnant's `civil_from_days`
/// algorithm (public domain), std-only, no external date library.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
