//! The SNES CPU-side MMIO surface between the hosted 65C816 and the
//! orchestrating host: a bounded **write-capture ring** for the PPU B-bus
//! (`$2100-$213F`) and CPU I/O (`$4200-$44FF`) windows, and **host-fed read
//! shadows** for the registers takeover programs poll (`$4200-$421F`,
//! `$4016/$4017`). The host drains the ring and refreshes the shadows
//! between `run_until` slices through the host window (`HW_*` in `lib.rs`);
//! per-register semantics cite nocash fullsnes.

/// Ring capacity in captured writes. A host flush covers ~2.5 K CPU cycles,
/// so even a pathological store loop cannot legitimately outrun this by
/// much; overflow drops the newest writes and arms a sticky flag.
pub const MMIO_RING_CAP: usize = 512;
/// Serialized [`Mmio::save_state`] length.
pub(crate) const MMIO_STATE_LEN: usize = 2 + 1 + MMIO_RING_CAP * 3 + 0x20 + 2;

/// Captured-write ring + read shadows.
pub(crate) struct Mmio {
    /// `(addr, val)` pairs in write order, oldest first.
    ring: Vec<(u16, u8)>,
    /// The ring hit capacity and newest writes were dropped (sticky until
    /// the host drains).
    overflow: bool,
    /// Host-fed images for CPU reads of `$4200 + i` (RDNMI, TIMEUP, HVBJOY,
    /// the autopoll pads…). Read side effects run in [`Self::cpu_read`].
    shadow: [u8; 0x20],
    /// Host-fed images for `$4016`/`$4017` (manual joypad serial reads).
    joy_serial: [u8; 2],
}

impl Mmio {
    pub(crate) fn new() -> Self {
        Mmio {
            ring: Vec::new(),
            overflow: false,
            shadow: [0; 0x20],
            joy_serial: [0; 2],
        }
    }

    /// Whether `addr` (bank-local, system bank) is in a captured window:
    /// the PPU B-bus registers `$2100-$213F` (minus the `$2140-$2143` APU
    /// ports, routed earlier) or the CPU I/O block `$4000-$44FF`.
    fn captured(addr: u16) -> bool {
        matches!(addr, 0x2100..=0x213F | 0x4000..=0x44FF)
    }

    /// Observe a CPU write; returns whether it was captured. Full writes
    /// (including to shadowed registers like NMITIMEN) enter the ring — the
    /// host is the consumer of every MMIO side effect.
    pub(crate) fn cpu_write(&mut self, addr: u16, val: u8) -> bool {
        if !Self::captured(addr) {
            return false;
        }
        if self.ring.len() >= MMIO_RING_CAP {
            self.overflow = true;
        } else {
            self.ring.push((addr, val));
        }
        true
    }

    /// Serve a CPU read from a shadowed register, or `None` for open bus.
    /// RDNMI (`$4210`) and TIMEUP (`$4211`) clear their bit 7 on read
    /// (fullsnes: the flag "gets also reset after reading from this
    /// register"); everything else is a plain shadow byte.
    pub(crate) fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x4016 | 0x4017 => Some(self.joy_serial[usize::from(addr - 0x4016)]),
            0x4200..=0x421F => {
                let i = usize::from(addr - 0x4200);
                let v = self.shadow[i];
                if addr == 0x4210 || addr == 0x4211 {
                    self.shadow[i] = v & 0x7F;
                }
                Some(v)
            }
            _ => None,
        }
    }

    // -- Host halves ---------------------------------------------------------

    /// Drain at most `n` captured writes, oldest first, leaving the rest
    /// queued (a host read shorter than the full ring must not lose writes).
    pub(crate) fn host_drain_up_to(&mut self, n: usize) -> Vec<(u16, u8)> {
        if n >= self.ring.len() {
            return std::mem::take(&mut self.ring);
        }
        let rest = self.ring.split_off(n);
        std::mem::replace(&mut self.ring, rest)
    }

    /// Whether writes were dropped since the last [`Self::host_drain`].
    pub(crate) fn overflowed(&mut self) -> bool {
        std::mem::take(&mut self.overflow)
    }

    /// Set the shadow byte for `$4200 + i`.
    pub(crate) fn host_set_shadow(&mut self, i: u8, v: u8) {
        if usize::from(i) < self.shadow.len() {
            self.shadow[usize::from(i)] = v;
        }
    }

    /// Set one `$4016`/`$4017` serial-read byte (`i` = 0 or 1); the sibling
    /// byte is untouched.
    pub(crate) fn host_set_joy_serial_byte(&mut self, i: usize, v: u8) {
        if i < 2 {
            self.joy_serial[i] = v;
        }
    }

    // -- Save state ----------------------------------------------------------

    pub(crate) fn save_state(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&(self.ring.len() as u16).to_le_bytes());
        buf.push(u8::from(self.overflow));
        for &(a, v) in &self.ring {
            buf.extend_from_slice(&[a as u8, (a >> 8) as u8, v]);
        }
        for _ in self.ring.len()..MMIO_RING_CAP {
            buf.extend_from_slice(&[0, 0, 0]);
        }
        buf.extend_from_slice(&self.shadow);
        buf.extend_from_slice(&self.joy_serial);
    }

    /// Restore from exactly [`MMIO_STATE_LEN`] bytes; a wrong-length or
    /// over-count slice is ignored (the block keeps its state).
    pub(crate) fn load_state(&mut self, b: &[u8]) {
        if b.len() != MMIO_STATE_LEN {
            return;
        }
        let n = usize::from(u16::from_le_bytes([b[0], b[1]]));
        if n > MMIO_RING_CAP {
            return;
        }
        self.overflow = b[2] != 0;
        self.ring = (0..n)
            .map(|i| {
                let e = &b[3 + i * 3..6 + i * 3];
                (u16::from(e[0]) | u16::from(e[1]) << 8, e[2])
            })
            .collect();
        let off = 3 + MMIO_RING_CAP * 3;
        self.shadow.copy_from_slice(&b[off..off + 0x20]);
        self.joy_serial.copy_from_slice(&b[off + 0x20..off + 0x22]);
    }
}

#[cfg(test)]
#[path = "mmio_tests.rs"]
mod tests;
