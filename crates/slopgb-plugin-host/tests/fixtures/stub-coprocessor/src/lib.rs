//! Coprocessor round-trip fixture: a comm-port latch whose read value folds in
//! the current cycle, so the host test can prove reset, run_until, port_write,
//! and port_read all cross the boundary correctly.

use slopgb_plugin_api::{Coprocessor, slopgb_coprocessor_plugin};

struct Stub {
    cycle: u64,
    ports: [u8; 4],
}

impl Coprocessor for Stub {
    fn new() -> Self {
        Stub {
            cycle: 0,
            ports: [0; 4],
        }
    }

    fn reset(&mut self) {
        self.cycle = 0;
        self.ports = [0; 4];
    }

    fn run_until(&mut self, target_cycle: u64) -> u64 {
        self.cycle = target_cycle;
        target_cycle
    }

    fn port_write(&mut self, port: u8, val: u8) {
        self.ports[(port & 3) as usize] = val;
    }

    fn port_read(&mut self, port: u8) -> u8 {
        self.ports[(port & 3) as usize].wrapping_add((self.cycle & 0xFF) as u8)
    }
}

slopgb_coprocessor_plugin!(Stub);
