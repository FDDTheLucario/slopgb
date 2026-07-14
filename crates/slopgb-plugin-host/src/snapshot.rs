//! [`Snapshot`] — an owned, frame-consistent copy of the observable machine
//! state a plugin can read, so host import functions serve reads without
//! borrowing the `GameBoy` (wasmi store data must be `'static`).

use slopgb_core::GameBoy;
use slopgb_plugin_api::Reg;

/// One frame's worth of the bank-0 address space and the exposed registers.
pub struct Snapshot {
    mem: Box<[u8; 0x1_0000]>,
    regs: [u16; Reg::ALL.len()],
}

impl Snapshot {
    /// Copy the machine's observable state. Read-only `&self` on the `GameBoy`,
    /// so it never advances a cycle or perturbs emulation.
    #[must_use]
    pub fn capture(gb: &GameBoy) -> Self {
        let mut mem = Box::new([0u8; 0x1_0000]);
        for (addr, slot) in mem.iter_mut().enumerate() {
            *slot = gb.debug_read(addr as u16);
        }
        let r = gb.cpu_regs();
        let mut regs = [0u16; Reg::ALL.len()];
        regs[Reg::Af.index() as usize] = r.af();
        regs[Reg::Bc.index() as usize] = r.bc();
        regs[Reg::De.index() as usize] = r.de();
        regs[Reg::Hl.index() as usize] = r.hl();
        regs[Reg::Sp.index() as usize] = r.sp;
        regs[Reg::Pc.index() as usize] = r.pc;
        regs[Reg::Lcdc.index() as usize] = u16::from(gb.debug_read(0xFF40));
        regs[Reg::Stat.index() as usize] = u16::from(gb.debug_read(0xFF41));
        regs[Reg::Ly.index() as usize] = u16::from(gb.debug_read(0xFF44));
        Self { mem, regs }
    }

    /// A zeroed snapshot, for a store that has not pumped a frame yet.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            mem: Box::new([0u8; 0x1_0000]),
            regs: [0u16; Reg::ALL.len()],
        }
    }

    /// One byte of the captured bank-0 address space.
    #[must_use]
    pub fn read(&self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }

    /// One captured register/IO value.
    #[must_use]
    pub fn reg(&self, reg: Reg) -> u16 {
        self.regs[reg.index() as usize]
    }
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
