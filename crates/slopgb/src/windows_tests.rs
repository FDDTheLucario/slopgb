use super::*;
use slopgb_core::{GameBoy, Model};

fn machine() -> GameBoy {
    GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).expect("zeroed rom loads")
}

#[test]
fn render_each_tool_window_fills_background_and_draws_content() {
    let theme = Theme::BGB;
    let gb = machine();
    for kind in [ToolWindow::Debugger, ToolWindow::Vram, ToolWindow::IoMap] {
        let (w, h) = (640usize, 480usize);
        let mut buf = vec![0xDEAD_BEEF_u32; w * h];
        {
            let mut c = Canvas::new(&mut buf, w, h);
            render(kind, &gb, &mut c, &theme);
        }
        // The whole surface was painted (no leftover sentinel) and the window
        // background + some text ink are present.
        assert!(
            !buf.contains(&0xDEAD_BEEF),
            "{kind:?}: surface fully painted"
        );
        assert!(buf.contains(&theme.bg), "{kind:?}: background filled");
        assert!(buf.contains(&theme.text), "{kind:?}: content drawn");
    }
}

#[test]
fn render_is_side_effect_free_on_the_machine() {
    // Rendering must not advance or mutate emulation (it takes &GameBoy).
    let gb = machine();
    let before = (gb.cycles(), gb.frame_count(), gb.cpu_regs().pc);
    let (w, h) = (320usize, 240usize);
    let mut buf = vec![0u32; w * h];
    let mut c = Canvas::new(&mut buf, w, h);
    render(ToolWindow::Debugger, &gb, &mut c, &Theme::BGB);
    assert_eq!((gb.cycles(), gb.frame_count(), gb.cpu_regs().pc), before);
}
