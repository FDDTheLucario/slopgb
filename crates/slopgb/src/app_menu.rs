//! `App` game-window menu handling: the bgb right-click main menu, its
//! submenus (Window size / Sound channel / Other / State), the info boxes
//! (Cart info / System info / About), and the window-size resize. The menu
//! *widgets* live in [`crate::windows::mainwin`].

use winit::dpi::LogicalSize;
use winit::event::MouseButton;
use winit::event_loop::ActiveEventLoop;
use winit::window::Fullscreen;

use slopgb_core::{CLOCK_HZ, SCREEN_H, SCREEN_W};

use crate::input::Action;
use crate::ui::canvas::Rect;
use crate::windows::mainwin::{
    InfoBox, MainMenu, MenuEffect, SubChoice, SubKind, SubMenu, WindowSizeChoice,
};
use crate::{App, ui};

impl App {
    /// A left/right press on the game window. Right-click (re)opens the bgb
    /// main-menu at the pointer (closing any submenu); left-click applies a
    /// submenu choice, runs a main-menu row's action, opens its submenu, or
    /// dismisses (a click off both popups closes them).
    pub(crate) fn on_game_click(&mut self, button: MouseButton, event_loop: &ActiveEventLoop) {
        let (px, py) = self.game_cursor;
        // The Load-ROM modal is topmost (MN4): route the click to OK/Cancel.
        if self.path_dialog.is_some() {
            let area = self.window_area();
            if let Some(r) = self
                .path_dialog
                .as_ref()
                .map(|d| ui::dialog::click(d, area, px, py))
            {
                self.resolve_path_dialog(r);
            }
            return;
        }
        // An open info box is modal: any click dismisses it and is swallowed.
        if self.info_box.take().is_some() {
            self.request_game_redraw();
            return;
        }
        if button == MouseButton::Right {
            self.main_submenu = None;
            self.main_menu = Some(MainMenu::open((px, py), !self.muted));
            self.request_game_redraw();
            return;
        }
        // A click on the open submenu applies its choice and closes everything.
        if let Some(sub) = &self.main_submenu {
            if let Some(choice) = sub.choice_at(px, py) {
                self.apply_sub_choice(choice, event_loop);
                self.main_submenu = None;
                self.main_menu = None;
                self.request_game_redraw();
                return;
            }
            // Off the submenu: close it, then let the main menu handle the click.
            self.main_submenu = None;
        }
        let Some(menu) = self.main_menu.take() else {
            return;
        };
        match menu.effect_at(px, py) {
            MenuEffect::Run(act) => {
                self.request_game_redraw();
                self.run_action(act, event_loop);
            }
            MenuEffect::Submenu(kind) => {
                // Keep the main menu open and hang the child off its row.
                if let Some(row) = menu.row_rect(MenuEffect::Submenu(kind)) {
                    self.main_submenu = Some(self.open_submenu(kind, row));
                }
                self.main_menu = Some(menu);
                self.request_game_redraw();
            }
            MenuEffect::None => self.request_game_redraw(), // dismissed
        }
    }

    /// Build the child submenu for `kind`, seeding its check-marks from the
    /// live state (current window size / per-channel mute), hung off `row`.
    fn open_submenu(&self, kind: SubKind, row: Rect) -> SubMenu {
        match kind {
            SubKind::WindowSize => SubMenu::window_size(row, self.window_size),
            SubKind::SoundChannel => SubMenu::sound_channel(row, self.channel_mutes()),
            SubKind::Other => SubMenu::other(row),
            SubKind::State => SubMenu::state(row),
            SubKind::RecentRoms => SubMenu::recent_roms(row, &self.recent_names()),
        }
    }

    /// The live mute state of sound channels 1-4 (the APU is the single source
    /// of truth), for the "Sound channel" submenu check-marks.
    fn channel_mutes(&self) -> [bool; 4] {
        [1, 2, 3, 4].map(|ch| self.session.gb.channel_muted(ch))
    }

    /// Apply a submenu activation: dispatch on the variant to the window-size
    /// resize, a per-channel mute toggle, or an "Other" action (open the VRAM
    /// viewer / show an info box).
    fn apply_sub_choice(&mut self, choice: SubChoice, event_loop: &ActiveEventLoop) {
        match choice {
            SubChoice::WindowSize(c) => self.apply_window_size(c),
            SubChoice::SoundChannel(ch) => {
                let now = self.session.gb.channel_muted(ch);
                self.session.gb.set_channel_mute(ch, !now);
            }
            SubChoice::OpenVram => {
                self.run_action(Action::ToggleTool(ui::ToolWindow::Vram), event_loop);
            }
            SubChoice::CartInfo => self.info_box = Some(self.cart_info_box()),
            SubChoice::SystemInfo => self.info_box = Some(self.system_info_box()),
            SubChoice::About => self.info_box = Some(about_box()),
            // State → Quick Save / Quick Load (MN6): an in-memory whole-machine
            // snapshot. A load resyncs pacing + repaints (the LCD jumped).
            SubChoice::QuickSave => self.session.quick_save(),
            SubChoice::QuickLoad => {
                if self.session.quick_load() {
                    self.resync_pacing();
                    self.update_title();
                    self.request_game_redraw();
                }
            }
            // Recent ROMs → reload that entry (MN4); clone the path out first so
            // the load can borrow `self` mutably.
            SubChoice::LoadRecent(i) => {
                if let Some(p) = self.recent.get(i).cloned() {
                    self.load_dropped(&p);
                }
            }
        }
    }

    /// Cartridge-header facts for the Other → "Cart info" box, parsed from the
    /// loaded ROM image (the frontend already holds it for reset).
    fn cart_info_box(&self) -> InfoBox {
        InfoBox::new(
            "Cart info",
            crate::session::cart_info_lines(&self.session.rom_bytes),
        )
    }

    /// Emulated-model facts for the Other → "System info" box.
    fn system_info_box(&self) -> InfoBox {
        InfoBox::new(
            "System info",
            vec![
                format!("model: {:?}", self.session.model),
                format!("clock: {} Hz", CLOCK_HZ),
                format!("double speed: {}", self.session.gb.double_speed()),
            ],
        )
    }

    /// Apply a "Window size" submenu choice: an integer scale resizes the window
    /// (and leaves fullscreen), a fullscreen mode goes borderless. `window_size`
    /// records the active choice for the submenu check-mark + the stretched blit.
    fn apply_window_size(&mut self, choice: WindowSizeChoice) {
        self.window_size = choice;
        let Some(window) = &self.window else {
            return;
        };
        match choice {
            WindowSizeChoice::Scale(n) => {
                window.set_fullscreen(None);
                let _ = window.request_inner_size(LogicalSize::new(
                    f64::from(SCREEN_W as u32 * n),
                    f64::from(SCREEN_H as u32 * n),
                ));
            }
            WindowSizeChoice::Fullscreen | WindowSizeChoice::FullscreenStretched => {
                window.set_fullscreen(Some(Fullscreen::Borderless(None)));
            }
        }
        self.request_game_redraw();
    }
}

/// A short label for the active window size (Options info box).
pub(crate) fn scale_label(size: WindowSizeChoice) -> String {
    match size {
        WindowSizeChoice::Scale(n) => format!("{n}x"),
        WindowSizeChoice::Fullscreen => "fullscreen".into(),
        WindowSizeChoice::FullscreenStretched => "fullscreen (stretched)".into(),
    }
}

/// The Other → "About..." info box.
fn about_box() -> InfoBox {
    InfoBox::new(
        "About slopgb",
        vec![
            format!("slopgb {}", env!("CARGO_PKG_VERSION")),
            "cycle-accurate GB/GBC emulator".into(),
            "bgb-style debugger UI clone".into(),
        ],
    )
}
