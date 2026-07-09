//! # slopfp
//!
//! A **pure** file-picker state machine + view-model. It never draws, never
//! knows about winit / softbuffer / a terminal / any framework, and pulls in
//! **zero dependencies** (std only, `forbid(unsafe_code)`). The host owns the
//! screen: it feeds semantic [`Key`] events in, and each frame reads a [`View`]
//! out and renders it however it likes (a pixel buffer, a TUI, HTML...).
//!
//! That split is the entire design:
//! * [`model`] — pure logic over an in-memory `Vec<Entry>` (sort / filter /
//!   selection / scroll / type-ahead / path editing / save-name). **No fs.**
//!   Testable by building a [`Picker`] with [`Picker::with_entries`].
//! * [`source`] — the only module that touches `std::fs` (read a directory,
//!   resolve home / roots, make a folder). Tested against a real temp dir.
//! * this file — the shared types and the [`Picker`] struct + its constructors.
//!
//! ## Host contract
//! ```ignore
//! let mut p = Picker::new(Mode::Open, start_dir, &["gb", "gbc"]);
//! // per input event, translate your framework's event to a `Key`:
//! match p.on_key(Key::Down) { Outcome::Picked(path) => .., Outcome::Cancelled => .., Outcome::None => .. }
//! // per frame:
//! let v = p.view(visible_row_count);
//! // draw v.title, v.path_bar, v.rows (each a name + size + mtime column), highlight v.highlight...
//! ```
//! A directory change is a side effect of `on_key`/`on_activate` (it reads the
//! new directory via [`source`]); the caller does nothing special for it.

#![forbid(unsafe_code)]

use std::path::PathBuf;

pub mod model;
pub mod source;

/// One row in the current directory listing. `size`/`mtime` are `None` for
/// entries whose metadata could not be read (still shown — never a hard error).
/// `mtime` is seconds since the Unix epoch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub mtime: Option<u64>,
}

impl Entry {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        is_dir: bool,
        size: Option<u64>,
        mtime: Option<u64>,
    ) -> Self {
        Self {
            name: name.into(),
            is_dir,
            size,
            mtime,
        }
    }
}

/// Open (return an existing file) vs Save (return `cwd`/typed-name, overwrite-confirmed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Open,
    Save,
}

/// Column the listing is ordered by. Directories always sort before files
/// regardless of key (a stable file-manager invariant).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Mtime,
    Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Which sub-widget currently eats `Char`/`Backspace`/`Enter`: the list
/// (`Browse`), the editable path bar (`PathBar`), or the Save filename field
/// (`SaveName`). Type-ahead in `Browse` uses `Char` too — see [`model`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Browse,
    PathBar,
    SaveName,
}

/// A semantic input event. The host maps its framework's keys/mouse to these
/// (e.g. winit `KeyCode::ArrowUp` -> `Key::Up`, a printable `key.text` char ->
/// `Key::Char(c)`). Keeping this framework-neutral is what makes the crate
/// reusable — mirror the slopgb `dialog_key_from` translator in your adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// Open the highlighted dir / pick the highlighted file / submit path-bar
    /// or save-name (see [`Picker::on_key`]).
    Enter,
    /// Go to the parent directory (or, while editing, this is not used — use
    /// `Backspace`). Think "Backspace in the list = up a level".
    Back,
    /// Esc: two-stage — leave path/save editing first, else cancel the picker.
    Cancel,
    Char(char),
    Backspace,
    /// Path-bar tab-completion (longest common prefix of matching entries).
    Tab,
    ToggleHidden,
    ToggleAllFiles,
    CycleSort,
    ToggleSortDir,
    /// Focus the editable path bar for typing/pasting a full path.
    FocusPath,
}

/// The result of feeding a [`Key`]. `Picked` / `Cancelled` mean the host should
/// close the picker; `None` means keep it open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    Picked(PathBuf),
    Cancelled,
}

/// A drawable snapshot of picker state — the ONLY thing the host renders. All
/// formatting (human size, mtime) is already done here so the host just draws
/// strings; it holds no references into the picker, so it is trivially cloned
/// or handed across a frame boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub title: String,
    /// The current directory, or the in-progress edit buffer when the path bar
    /// is focused.
    pub path_bar: String,
    pub path_focused: bool,
    /// Exactly the visible window (`viewport_rows` at most), already sliced by
    /// the scroll offset.
    pub rows: Vec<Row>,
    /// Index **within `rows`** of the highlighted entry, or `None` if the window
    /// is empty or the selection is off-window.
    pub highlight: Option<usize>,
    pub offset: usize,
    pub total: usize,
    /// A one-line status: item count, active filter, type-ahead buffer, or an
    /// error (e.g. permission denied) surfaced from the last directory read.
    pub status: String,
    /// `Some(filename_buffer)` in [`Mode::Save`]; `None` in Open mode. When
    /// `overwrite_pending` is set the host should show a confirm hint.
    pub save_name: Option<String>,
    pub overwrite_pending: bool,
}

/// One rendered row. `size`/`mtime` are preformatted display strings (empty for
/// directories or unreadable metadata).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub is_dir: bool,
    pub size: String,
    pub mtime: String,
}

/// What pressing Enter on the current selection means, resolved **purely** (no
/// fs). `on_key` executes it: `Navigate` reads the new dir via [`source`],
/// `Pick` becomes an [`Outcome::Picked`]. Split out so the intent is unit
/// testable without a disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EnterAction {
    Navigate(PathBuf),
    Pick(PathBuf),
    None,
}

/// The picker. Construct with [`Picker::new`] (reads the start dir from disk) or
/// [`Picker::with_entries`] (pure, for tests). Fields are `pub(crate)` so the
/// [`model`] impl and its sibling tests can reach them; the public surface is
/// the methods on the [`model`] impl block (`on_key`, `on_click`, `on_activate`,
/// `view`).
pub struct Picker {
    pub(crate) mode: Mode,
    pub(crate) cwd: PathBuf,
    /// Raw listing as read (unsorted, unfiltered); [`model`] derives the visible
    /// list on demand.
    pub(crate) entries: Vec<Entry>,
    /// Selection index into the *visible* (sorted+filtered) list.
    pub(crate) sel: usize,
    /// Scroll offset into the visible list.
    pub(crate) offset: usize,
    /// Last `viewport_rows` seen by [`view`]; page keys use it. Defaults to 1.
    pub(crate) viewport: usize,
    pub(crate) sort_key: SortKey,
    pub(crate) sort_dir: SortDir,
    pub(crate) show_hidden: bool,
    /// Lowercased extensions (no dot) to keep; empty = no extension filtering.
    pub(crate) filters: Vec<String>,
    /// When true, the extension filter is ignored (show every file).
    pub(crate) all_files: bool,
    /// Type-ahead accumulator (Browse focus); cleared by any nav key.
    pub(crate) search: String,
    pub(crate) focus: Focus,
    /// Path-bar edit buffer (valid when `focus == PathBar`).
    pub(crate) path_edit: String,
    /// Save filename buffer (Save mode).
    pub(crate) save_name: String,
    /// Set after an Enter that targets an existing file in Save mode; a second
    /// Enter confirms the overwrite.
    pub(crate) overwrite_pending: bool,
    pub(crate) title: String,
    /// Last non-fatal error/status (e.g. "permission denied").
    pub(crate) status: String,
}

impl Picker {
    /// Build a picker rooted at `cwd`, reading its contents from disk now.
    /// `filters` are extensions without the dot (case-insensitive). A read
    /// error leaves an empty listing and a status message rather than failing.
    #[must_use]
    pub fn new(mode: Mode, cwd: impl Into<PathBuf>, filters: &[&str]) -> Self {
        let cwd = cwd.into();
        let (entries, status) = match source::read_dir(&cwd) {
            Ok(e) => (e, String::new()),
            Err(e) => (Vec::new(), format!("cannot read directory: {e}")),
        };
        let mut p = Self::with_entries(mode, cwd, entries);
        p.filters = filters.iter().map(|s| s.to_ascii_lowercase()).collect();
        p.all_files = p.filters.is_empty();
        p.status = status;
        p
    }

    /// Pure constructor: build a picker over an in-memory listing without
    /// touching the filesystem. This is the seam the [`model`] unit tests use.
    #[must_use]
    pub fn with_entries(mode: Mode, cwd: impl Into<PathBuf>, entries: Vec<Entry>) -> Self {
        Self {
            mode,
            cwd: cwd.into(),
            entries,
            sel: 0,
            offset: 0,
            viewport: 1,
            sort_key: SortKey::Name,
            sort_dir: SortDir::Asc,
            show_hidden: false,
            filters: Vec::new(),
            all_files: true,
            search: String::new(),
            focus: Focus::Browse,
            path_edit: String::new(),
            save_name: String::new(),
            overwrite_pending: false,
            title: String::new(),
            status: String::new(),
        }
    }

    /// Set the title shown in the [`View`] (builder-style, optional).
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The directory currently being browsed.
    #[must_use]
    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }
}
