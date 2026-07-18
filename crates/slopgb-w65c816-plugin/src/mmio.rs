//! The SNES CPU-side MMIO surface between the hosted 65C816 and the
//! orchestrating host: a bounded **write-capture ring** for the PPU B-bus
//! (`$2100-$213F`) and CPU I/O (`$4200-$44FF`) windows, and **host-fed read
//! shadows** for the registers takeover programs poll (`$4200-$421F`,
//! `$4016/$4017`). The host drains the ring and refreshes the shadows
//! between `run_until` slices through the host window (`HW_*` in `lib.rs`);
//! per-register semantics cite nocash fullsnes.

/// Ring capacity in captured writes. A host flush covers ~2.5 K CPU cycles
/// and MMIO stores come as LDA/STA pairs (≥7 cycles each), so a real
/// program stays well under this; overflow drops the newest writes and arms
/// a sticky flag. If a dropped write is a `$420B` trigger, its DMA is lost
/// too (the host still un-stalls the CPU) — the overflow warning is the
/// only trace, so the cap must stay comfortably above the legitimate rate.
// Sized for one whole flush window's writes: the mediation rounds let the
// CPU run far ahead of real time inside a flush, so a takeover's tile
// upload banks thousands of captured writes between host drains.
pub const MMIO_RING_CAP: usize = 16384;
/// Serialized [`Mmio::save_state`] length.
pub(crate) const MMIO_STATE_LEN: usize = 2 + 1 + MMIO_RING_CAP * 3 + 0x20 + 2 + 1;

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
    /// A nonzero MDMAEN (`$420B`) write pauses the CPU until the host has
    /// executed the transfer (fullsnes 420Bh: "The CPU is paused during the
    /// transfer") — so post-trigger code never sees a half-applied DMA.
    dma_stall: bool,
}

impl Mmio {
    pub(crate) fn new() -> Self {
        Mmio {
            ring: Vec::new(),
            overflow: false,
            shadow: [0; 0x20],
            joy_serial: [0; 2],
            dma_stall: false,
        }
    }

    /// Whether `addr` (bank-local, system bank) is in a captured window:
    /// the PPU B-bus registers `$2100-$213F` (minus the `$2140-$2143` APU
    /// ports, routed earlier), the WRAM access ports `$2180-$2183`, or the
    /// CPU I/O block `$4000-$44FF`.
    fn captured(addr: u16) -> bool {
        matches!(addr, 0x2100..=0x213F | 0x2180..=0x2183 | 0x4000..=0x44FF)
    }

    /// Observe a CPU write; returns whether it was captured. Full writes
    /// (including to shadowed registers like NMITIMEN) enter the ring — the
    /// host is the consumer of every MMIO side effect.
    pub(crate) fn cpu_write(&mut self, addr: u16, val: u8) -> bool {
        if !Self::captured(addr) {
            return false;
        }
        if addr == 0x420B && val != 0 {
            self.dma_stall = true;
        }
        // The CPU multiply/divide unit (fullsnes 4202h-4206h) is served
        // plugin-side: programs read the result registers a handful of
        // cycles after the kick — far inside one host flush, so a host
        // round trip could never answer in time. Results land complete
        // (no partial-result garbage window). The write-only operand
        // latches live in their own shadow slots ($4202/$4204/$4205 —
        // never host-fed), so the unit adds no serialized state.
        match addr {
            0x4202 | 0x4204 | 0x4205 => {
                self.shadow[usize::from(addr - 0x4200)] = val;
            }
            // WRMPYB: product = WRMPYA * val -> RDMPYL/H.
            0x4203 => {
                let prod = u16::from(self.shadow[0x02]) * u16::from(val);
                self.shadow[0x16] = prod as u8;
                self.shadow[0x17] = (prod >> 8) as u8;
            }
            // WRDIVB: WRDIV / val -> RDDIVL/H, remainder -> RDMPYL/H;
            // divide by zero: quotient $FFFF, remainder = dividend.
            0x4206 => {
                let dividend = u16::from_le_bytes([self.shadow[0x04], self.shadow[0x05]]);
                let (q, r) = match val {
                    0 => (0xFFFF, dividend),
                    d => (dividend / u16::from(d), dividend % u16::from(d)),
                };
                self.shadow[0x14] = q as u8;
                self.shadow[0x15] = (q >> 8) as u8;
                self.shadow[0x16] = r as u8;
                self.shadow[0x17] = (r >> 8) as u8;
            }
            _ => {}
        }
        if self.ring.len() >= MMIO_RING_CAP {
            self.overflow = true;
        } else {
            self.ring.push((addr, val));
        }
        true
    }

    /// Whether a `$420B` write is awaiting host DMA service.
    pub(crate) fn dma_stall(&self) -> bool {
        self.dma_stall
    }

    /// Host acknowledgment: the transfer ran, the CPU resumes.
    pub(crate) fn host_clear_dma_stall(&mut self) {
        self.dma_stall = false;
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

    /// Captured writes waiting in the ring (a peek — nothing drains).
    pub(crate) fn pending(&self) -> usize {
        self.ring.len()
    }

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
        buf.push(u8::from(self.dma_stall));
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
        self.dma_stall = b[off + 0x22] != 0;
    }
}

#[cfg(test)]
#[path = "mmio_tests.rs"]
mod tests;
