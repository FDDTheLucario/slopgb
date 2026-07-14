//! A sparse flat bus for the vectors: 24-bit address space backed by a map,
//! seeded from each vector's `initial.ram`, logging every access in order so the
//! harness can check it against the vector's `cycles` list.

use std::collections::HashMap;

use slopgb_w65c816::Bus;

/// One recorded bus access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Access {
    pub addr: u32,
    pub val: u8,
    pub write: bool,
}

/// Flat 24-bit RAM that records reads and writes.
#[derive(Default)]
pub struct VecBus {
    pub mem: HashMap<u32, u8>,
    pub log: Vec<Access>,
}

impl VecBus {
    /// Seed memory from an `[[addr, val], ...]` list and clear the access log.
    pub fn seed(&mut self, ram: &[(u32, u8)]) {
        self.mem.clear();
        self.log.clear();
        self.mem.extend(ram.iter().copied());
    }
}

impl Bus for VecBus {
    fn read(&mut self, addr: u32) -> u8 {
        let val = self.mem.get(&addr).copied().unwrap_or(0);
        self.log.push(Access {
            addr,
            val,
            write: false,
        });
        val
    }

    fn write(&mut self, addr: u32, val: u8) {
        self.mem.insert(addr, val);
        self.log.push(Access {
            addr,
            val,
            write: true,
        });
    }
}
