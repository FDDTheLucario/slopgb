//! Block moves `MVN` and `MVP`. Each moves `C + 1` bytes between banks, one byte
//! per loop iteration; the CPU re-fetches the opcode and the two bank operands
//! every iteration (so `PC` sits on the opcode until the move finishes, making
//! the move interruptible). `MVN` counts source/dest up, `MVP` down. The move
//! yields when the cycle budget is reached (see `step_bounded`). Per the WDC
//! W65C816S datasheet and vectors.

use super::*;

impl Cpu {
    /// `MVN`: block move, addresses ascending.
    pub(crate) fn mvn(&mut self, bus: &mut impl Bus) {
        self.block_move(bus, 1);
    }

    /// `MVP`: block move, addresses descending.
    pub(crate) fn mvp(&mut self, bus: &mut impl Bus) {
        self.block_move(bus, -1);
    }

    /// `delta` is `+1` for `MVN`, `-1` for `MVP`. The opcode of the first
    /// iteration was already fetched by `step`; later iterations re-fetch it.
    fn block_move(&mut self, bus: &mut impl Bus, delta: i32) {
        let opcode_pc = self.regs.pc.wrapping_sub(1);
        let wide = self.regs.idx16();
        loop {
            let dst_bank = self.fetch8(bus);
            // The move yields here (after re-fetching the opcode + dest bank) so
            // a resumed instruction re-reads its operands.
            if self.cap_reached() {
                return;
            }
            let src_bank = self.fetch8(bus) as u32;
            self.regs.dbr = dst_bank;
            let value = self.read8(bus, (src_bank << 16) | self.regs.x as u32);
            self.write8(bus, ((dst_bank as u32) << 16) | self.regs.y as u32, value);
            self.io();
            self.io();
            self.regs.x = step_index(self.regs.x, delta, wide);
            self.regs.y = step_index(self.regs.y, delta, wide);
            let done = self.regs.a == 0;
            self.regs.a = self.regs.a.wrapping_sub(1);
            if done {
                // C wrapped from 0x0000 to 0xFFFF: the move is complete and PC is
                // left just past the instruction.
                return;
            }
            // Not done: point PC back at the opcode and re-fetch it.
            self.regs.pc = opcode_pc;
            self.fetch8(bus);
        }
    }
}

/// Advance a block-move index by `delta`, honouring the index width.
fn step_index(index: u16, delta: i32, wide: bool) -> u16 {
    let stepped = (index as i32 + delta) as u16;
    if wide { stepped } else { stepped & 0x00FF }
}
