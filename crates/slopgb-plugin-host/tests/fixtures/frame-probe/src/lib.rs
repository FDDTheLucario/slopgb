//! Round-trip fixture: reads a register and a memory byte, logs both — so the
//! host test can assert the guest observed the same machine the host sees.

use slopgb_plugin_api::{GameBoyView, Plugin, Reg, slopgb_plugin};

struct FrameProbe;

impl Plugin for FrameProbe {
    fn new() -> Self {
        FrameProbe
    }

    fn on_frame(&mut self, gb: &GameBoyView) {
        let pc = gb.reg(Reg::Pc);
        let op = gb.read(pc);
        gb.log(&format!("pc={pc:04X} op={op:02X}"));
    }
}

slopgb_plugin!(FrameProbe);
