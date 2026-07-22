//! `App` presentation: the game-window redraw (frame-source select, postfx,
//! modal overlays) and the DMG-palette / no-ROM blank-frame plumbing. Split out
//! of `main.rs` to keep it under the size cap.

use slopgb_core::{SCREEN_H, SCREEN_PIXELS, SCREEN_W, SGB_BORDER_H, SGB_BORDER_W};

use crate::ui::dialog;
use crate::windows::mainwin::WindowSizeChoice;
use crate::{App, cheat_ui, postfx, windows};

impl App {
    /// Push the current DMG palette to the live machine and rebuild the no-ROM
    /// blank frame from its lightest shade. Called after every machine (re)build
    /// (startup, ROM load) since `GameBoy::new` resets the palette to the core
    /// grayscale default; Options OK/Apply applies the palette through its own
    /// path (`apply_settings`).
    pub(crate) fn apply_palette(&mut self) {
        self.session.gb.set_dmg_palette(self.settings.dmg_palette);
        // Graphics → "disable SGB colors" is a display option like the palette,
        // so it rides the same apply path (Options apply + every ROM load).
        self.session
            .gb
            .set_sgb_mono(self.settings.disable_sgb_colors);
        self.blank_frame = blank_frame(self.settings.dmg_palette[0]);
    }

    pub(crate) fn redraw(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Some(video) = self.video.as_mut() else {
            return;
        };
        // With no ROM loaded the LCD shows a solid lightest-shade blank (bgb's
        // pale-green off screen); the machine is frozen so its own front buffer
        // never paints. On an SGB with a border loaded (CHR_TRN+PCT_TRN), the
        // 256×224 composite replaces the bare 160×144 frame automatically — the
        // blit letterboxes whichever size it gets.
        // A full-takeover SGB coprocessor renders the SNES side itself; a
        // fresh 256×224 frame (converted here) replaces the GB composite
        // until the next ROM load. Absent coprocessor/PPU: never `Some`.
        if let Some(f) = self.session.gb.take_snes_frame() {
            self.snes_frame = Some(f.iter().map(|&c| postfx::snes_rgb555_px(c)).collect());
        }
        let (mut frame, mut src_w, mut src_h): (&[u32], usize, usize) = if self.rom_loaded {
            match (&self.snes_frame, self.session.gb.sgb_border()) {
                (Some(s), _) => (&s[..], SGB_BORDER_W, SGB_BORDER_H),
                (None, Some(b)) => (&b[..], SGB_BORDER_W, SGB_BORDER_H),
                (None, None) => (&self.session.gb.frame()[..], SCREEN_W, SCREEN_H),
            }
        } else {
            (&self.blank_frame[..], SCREEN_W, SCREEN_H)
        };
        // Outline the sprite hovered in the VRAM viewer's OAM tab, drawn into the
        // frame pre-blit so it scales with the screen. The core frame is immutable
        // (golden-safe), so XOR the perimeter into a scratch copy instead; the
        // presentation filters below then treat the outlined copy as the frame.
        if let Some(r) = self.tools.oam_hover_rect(&self.session.gb) {
            // SGB composites the 160×144 screen at (48,40) inside the 256×224 border.
            let (ox, oy) = if src_w == SGB_BORDER_W {
                (48, 40)
            } else {
                (0, 0)
            };
            self.overlay_frame.clear();
            self.overlay_frame.extend_from_slice(frame);
            invert_outline(
                &mut self.overlay_frame,
                src_w,
                src_h,
                r.x + ox,
                r.y + oy,
                r.w,
                r.h,
            );
            frame = &self.overlay_frame;
        }
        // Presentation filters (frontend-only, golden-safe): copy the core frame
        // into the scratch buffer and filter it in place, then present that.
        if postfx::any_active(&self.settings) {
            self.postfx_buf.clear();
            self.postfx_buf.extend_from_slice(frame);
            postfx::apply(&mut self.postfx_buf, &self.prev_frame, &self.settings);
            self.prev_frame.clear();
            self.prev_frame.extend_from_slice(frame);
            frame = &self.postfx_buf[..];
        } else if !self.prev_frame.is_empty() {
            self.prev_frame.clear(); // drop history so re-enabling blend starts fresh
        }
        // Graphics → "doubler": scale2x the (filtered) frame to 2×, presented in
        // its place; the blit then scales/letterboxes the larger image.
        if self.settings.doubler {
            postfx::scale2x(frame, src_w, src_h, &mut self.scale_buf);
            frame = &self.scale_buf[..];
            src_w *= 2;
            src_h *= 2;
        }
        // The right-click menu is its own window now (see `menupopup`), so it is
        // not part of the game-window overlay. The remaining overlays (info box /
        // Options / path modal / key wizard) stay centred/modal here. (Captures
        // locals, not `self`, so the disjoint field borrows stay clean.)
        let info = self.info_box.as_ref();
        let cheat = self.cheat_dialog.as_ref();
        let cheat_list = &self.cheats;
        let path_dlg = self.path_dialog.as_ref();
        // `&mut` (not `&ref`, unlike the other overlays): the picker's `view()`
        // is a live widget call, not a plain read — see `file_picker.rs`.
        // Still a disjoint field borrow from `video`/`options`/etc above, and
        // `video.draw`'s overlay is `FnOnce`, so moving this `Option<&mut _>`
        // into the closure (called exactly once) borrow-checks cleanly.
        let picker = self.file_picker.as_mut();
        let options = self.options.as_ref();
        let wizard = self.key_wizard.as_ref();
        let gp_wizard = self.gamepad_wizard.as_ref();
        let theme = self.settings.theme.resolve(&self.custom_themes);
        let stretch = self.window_size == WindowSizeChoice::FullscreenStretched;
        if let Err(e) = video.draw(window, frame, src_w, src_h, stretch, |canvas| {
            // The info box / Load-ROM modal draw on top of everything (modal).
            if let Some(i) = info {
                windows::mainwin::render_info(canvas, i, &theme);
            }
            // The Cheat dialog draws as a modal over the LCD.
            if let Some(cd) = cheat {
                cheat_ui::render(canvas, cd, cheat_list, &theme);
            }
            // The Options control panel draws on top of the menus/info box.
            if let Some(o) = options {
                windows::options::render(canvas, o, &theme);
            }
            // A path modal draws above Options too — it can float over the dialog
            // (the bootrom `...` browse) as well as stand alone.
            if let Some(d) = path_dlg {
                let area = canvas.bounds();
                dialog::render(canvas, area, d, &theme);
            }
            // The in-app file browser is the same kind of standalone
            // overlay as the path modal (never open at the same time as it).
            if let Some(fp) = picker {
                let area = canvas.bounds();
                fp.render(canvas, area.w, area.h, &theme);
            }
            // The key-rebind wizard floats above even the Options dialog.
            if let Some(w) = wizard {
                w.render(canvas, &theme);
            }
            // The controller-rebind wizard shares the same modal slot.
            if let Some(w) = gp_wizard {
                w.render(canvas, &theme);
            }
        }) {
            eprintln!("slopgb: failed to present frame: {e}");
        }
    }
}

/// A solid LCD frame filled with `color` (the palette's lightest shade) — the
/// no-ROM blank screen. A free function so the fill is unit-testable.
pub(crate) fn blank_frame(color: u32) -> Box<[u32; SCREEN_PIXELS]> {
    Box::new([color; SCREEN_PIXELS])
}

/// XOR the RGB of the 1-pixel perimeter of the `w`×`h` box at `(x, y)` in a
/// `fw`×`fh` frame, clipped to the frame. Inverting whatever it covers keeps the
/// outline self-contrasting on any background; the blit forces alpha opaque after.
/// Corner pixels are hit once (the side runs skip the top/bottom rows) so a double
/// XOR can't cancel them back to invisible.
fn invert_outline(frame: &mut [u32], fw: usize, fh: usize, x: i32, y: i32, w: i32, h: i32) {
    let mut xor = |px: i32, py: i32| {
        if (0..fw as i32).contains(&px) && (0..fh as i32).contains(&py) {
            frame[py as usize * fw + px as usize] ^= 0x00FF_FFFF;
        }
    };
    for cx in x..x + w {
        xor(cx, y);
        xor(cx, y + h - 1);
    }
    for cy in (y + 1)..(y + h - 1) {
        xor(x, cy);
        xor(x + w - 1, cy);
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::invert_outline;

    #[test]
    fn outline_inverts_perimeter_once_and_clips() {
        // 4×4 frame, box covering it all: perimeter (12 px) flips, center (2×2) untouched.
        let mut f = [0u32; 16];
        invert_outline(&mut f, 4, 4, 0, 0, 4, 4);
        for (i, &px) in f.iter().enumerate() {
            let (x, y) = (i % 4, i / 4);
            let edge = x == 0 || x == 3 || y == 0 || y == 3;
            assert_eq!(px, if edge { 0x00FF_FFFF } else { 0 }, "px {i}");
        }
        // Off-frame box: fully clipped, no panic, no change.
        let mut g = [7u32; 16];
        invert_outline(&mut g, 4, 4, -8, -8, 4, 4);
        assert_eq!(g, [7u32; 16]);
    }
}
