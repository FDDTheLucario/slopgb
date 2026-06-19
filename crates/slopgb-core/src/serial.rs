//! Serial port (FF01 SB, FF02 SC). Timer/serial work package.
//!
//! The internal serial clock is derived from the DIV counter, so transfer
//! timing depends on DIV phase (mooneye: `serial/boot_sclk_align-dmgABCmgb`;
//! gambatte: `serial/`). No link cable peer: incoming bits read as 1.
//!
//! Clock model (SameBoy Core/timing.c `GB_serial_master_edge` +
//! Core/memory.c `GB_IO_SC` case; gambatte-core memory.cpp SC write):
//! every *falling edge of DIV counter bit 7* (bit 2 with the CGB fast-clock
//! bit, SC bit 1) toggles an internal master flip-flop, and a bit is
//! shifted when the flip-flop toggles to *low* — i.e. the shift clock is a
//! divide-by-2 of the bit-7 edge stream: 8192 Hz (one shift per 512
//! T-cycles), 262144 Hz fast. Crucially, **any SC write resets the
//! flip-flop to low** (SameBoy forces a master edge when it is high), so
//! the first bit of a transfer shifts on the *second* bit-7 falling edge
//! after the SC write — gambatte's completion formula
//! `cc - (cc - div_last_reset) % 0x100 + 0x200 * 8` (memory.cpp) is exactly
//! this. The forced edge also shifts a bit of the *old* transfer when one
//! was mid-flight (its counter was reset first, so it can never complete or
//! raise IF). In CGB double speed the DIV counter itself runs twice as
//! fast, yielding the documented 16384 Hz / 524288 Hz rates.

use std::collections::VecDeque;

/// Cap on the harness output buffer: a frontend that never calls
/// `take_output` must not grow it without limit. 64 KiB is far more text
/// than any test ROM prints; completions past the cap are dropped.
const OUT_CAPTURE_CAP: usize = 64 * 1024;

/// Cap on each link byte queue. Several transfers can complete (or arrive) in
/// one emulated frame, so the link bytes are queued — but bounded, so a peer
/// flooding faster than the frontend drains can't grow them without limit.
const LINK_QUEUE_CAP: usize = 256;

#[derive(Clone)]
pub struct Serial {
    cgb: bool,
    sb: u8,
    sc: u8,
    /// Bits shifted since the last SC write (counts up; IF + transfer end
    /// at 8 — SameBoy `serial_count`).
    shifted: u8,
    /// Serial master flip-flop: toggled by each falling edge of the DIV
    /// clock bit, shifts on the high->low toggle, force-reset to false by
    /// SC writes (SameBoy `serial_master_clock`).
    master_clock: bool,
    /// DIV counter as of the previous `tick`, for edge detection.
    prev_div: u16,
    /// Bits shifted out so far (MSB first): at completion the low 8 bits
    /// are the byte a link-cable peer would have received.
    out_shift: u8,
    /// Completed internal-clock transfer bytes awaiting [`Self::take_output`]
    /// (test-harness hook: blargg ROMs print via SB/SC).
    out_buf: Vec<u8>,
    /// --- Link cable (frontend TCP peer; all golden-safe / inert when off) ---
    /// Whether a link peer is attached. Defaults `false`; only the frontend
    /// `GameBoy::link_connect` ever sets it, so every golden path keeps it off.
    link_connected: bool,
    /// Peer bytes to shift into upcoming internal-clock (master) transfers,
    /// MSB-first, one consumed per completed transfer. **Empty** ⇒ the no-peer
    /// path: 1s shift in, exactly as the cable-disconnected hardware. The
    /// connected incoming-bit branch is gated on the front being present, so a
    /// disconnected (empty-queue) port is byte-identical. A queue (not a single
    /// slot) so multiple peer bytes arriving in one frame aren't lost.
    link_in: VecDeque<u8>,
    /// Bytes master transfers shifted out, awaiting [`Self::take_link_send`]
    /// for the frontend to ship to the peer. Only pushed while
    /// [`Self::link_connected`]; queued (not a single slot) so multiple
    /// completions in one frame aren't lost.
    link_out: VecDeque<u8>,
    /// Lockstep stall: a connected internal-clock master clocked all 8 bits but
    /// had no peer byte buffered, so the transfer is **paused** at completion —
    /// SC bit 7 still set, IF not yet raised — until the frontend delivers the
    /// peer's byte ([`Self::push_link_in`]). Gates further DIV clocking so the
    /// master holds exactly one byte-time. Only ever set while
    /// [`Self::link_connected`]; defaults false ⇒ every golden path is
    /// byte-identical. Transient (not serialized).
    link_master_waiting: bool,
    /// Whether an armed slave is treated as a per-transfer yield point
    /// ([`Self::link_slave_armed`]). The frontend clears this after a lockstep
    /// wait times out with the master idle, so a slave whose peer isn't clocking
    /// runs full frames (no freeze) instead of stalling per instruction; any
    /// peer packet re-enables it. Default true; only consulted while
    /// [`Self::link_connected`], so golden-irrelevant. Transient (not serialized).
    link_slave_yield: bool,
}

impl Serial {
    // Deliberately no `Default` (unlike Timer/Joypad): construction needs the
    // CGB-mode flag, and there is no sensible model-independent default.
    pub fn new(cgb: bool) -> Self {
        Self {
            cgb,
            sb: 0,
            sc: 0,
            shifted: 0,
            master_clock: false,
            prev_div: 0,
            out_shift: 0,
            out_buf: Vec::new(),
            link_connected: false,
            link_in: VecDeque::new(),
            link_out: VecDeque::new(),
            link_master_waiting: false,
            link_slave_yield: true,
        }
    }

    /// Switch CGB ↔ DMG serial mode (the fast-clock bit + SC read mask). Only
    /// the boot ROM's KEY0/FF4C DMG-lock flips this at runtime; otherwise the
    /// mode is fixed at construction.
    pub fn set_cgb(&mut self, cgb: bool) {
        self.cgb = cgb;
    }

    /// DIV counter bit whose falling edges toggle the master flip-flop
    /// (half the shift rate; see the module docs).
    fn clock_mask(&self) -> u16 {
        if self.cgb && self.sc & 0x02 != 0 {
            1 << 2 // shifts at 262144 Hz (524288 Hz in double speed)
        } else {
            1 << 7 // shifts at 8192 Hz (16384 Hz in double speed)
        }
    }

    /// One master-clock edge (SameBoy `GB_serial_master_edge`): toggle the
    /// flip-flop; on the high->low toggle an active internal-clock transfer
    /// (SC bits 7 and 0 both set — with an external clock and no peer the
    /// transfer never advances) shifts one bit. Returns IF bits (bit 3).
    fn master_edge(&mut self) -> u8 {
        self.master_clock = !self.master_clock;
        // A stalled lockstep master holds at completion: keep toggling the
        // flip-flop (phase) but shift nothing until the peer byte arrives.
        if self.master_clock || self.sc & 0x81 != 0x81 || self.link_master_waiting {
            return 0;
        }
        // MSB out first. Incoming bit: the front peer byte shifts in MSB-first
        // when attached (peeked, stable across the 8 shifts, consumed at
        // completion); with no peer (`link_in` empty) it is the literal `1` of
        // the cable-disconnected hardware — the only golden path, byte-identical.
        let incoming = match self.link_in.front() {
            Some(&peer) => (peer >> (7 - self.shifted)) & 1,
            None => 1,
        };
        self.out_shift = (self.out_shift << 1) | (self.sb >> 7);
        self.sb = (self.sb << 1) | incoming;
        self.shifted += 1;
        if self.shifted == 8 {
            self.shifted = 0;
            // Only internal-clock transfers reach completion: capture the
            // outgoing byte for the harness. An SC rewrite mid-transfer
            // restarts the bit counter, so the captured byte is the last
            // 8 bits actually shifted out.
            if self.out_buf.len() < OUT_CAPTURE_CAP {
                self.out_buf.push(self.out_shift);
            }
            if self.link_connected {
                // Link: ship the outgoing byte to the frontend (once), then
                // either complete with the buffered peer byte or **stall**
                // until the frontend delivers it (lockstep — see
                // `link_master_waiting`). Both queues are inert when
                // disconnected, so this whole branch is golden-safe.
                if self.link_out.len() < LINK_QUEUE_CAP {
                    self.link_out.push_back(self.out_shift);
                }
                match self.link_in.pop_front() {
                    Some(peer) => {
                        // Peer byte was ready: overwrite SB with it (robust no
                        // matter when in the transfer it arrived) and complete.
                        self.sb = peer;
                        self.sc &= 0x7F;
                        return 0x08;
                    }
                    None => {
                        // No peer byte yet: pause at completion, IF withheld.
                        self.link_master_waiting = true;
                        return 0;
                    }
                }
            }
            // Disconnected (golden path): the 1s shifted in stay in SB.
            self.sc &= 0x7F; // transfer-in-progress flag clears itself
            return 0x08;
        }
        0
    }

    /// Advance one M-cycle. `div` is the timer's internal 16-bit DIV counter
    /// *after* this cycle's increment (a DIV-write reset shows up as a jump
    /// and its high->low clock-bit transition counts as an edge, like the
    /// timer). Returns IF bits (bit 3 = serial).
    pub fn tick(&mut self, div: u16) -> u8 {
        let mask = self.clock_mask();
        let falling = self.prev_div & mask != 0 && div & mask == 0;
        self.prev_div = div;
        if falling { self.master_edge() } else { 0 }
    }

    /// FF04 (DIV) write: the counter snaps to 0 *within* the write's
    /// M-cycle, so a high clock bit is a falling edge right there — the
    /// once-per-M-cycle [`Self::tick`] sampling would miss it for the CGB
    /// fast clock, whose bit (period 8 T-cycles) is high again by the next
    /// sample. `div_before` is the counter value being reset (this cycle's
    /// post-increment value, i.e. what the last `tick` saw). Returns IF
    /// bits like `tick` — the edge can be a transfer's 8th shift.
    pub fn div_write(&mut self, div_before: u16) -> u8 {
        let mask = self.clock_mask();
        self.prev_div = 0;
        if div_before & mask != 0 {
            self.master_edge()
        } else {
            0
        }
    }

    /// Drain the bytes shifted out by completed internal-clock transfers
    /// (test-harness hook; see [`OUT_CAPTURE_CAP`]).
    pub(crate) fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out_buf)
    }

    // --- Link cable (frontend TCP peer; inert when disconnected) ------------

    /// Attach/detach a link peer. Detaching also clears any pending peer bytes
    /// and queued sends so a later reconnect starts clean. If a master was
    /// **stalled** awaiting the peer (lockstep), unplugging completes that
    /// transfer with the cable-open value (SB ← 0xFF, SC bit7 cleared) and
    /// returns the serial IF bits (`0x08`) so the emulated CPU's serial wait
    /// can't hang; otherwise returns `0`.
    pub(crate) fn set_link_connected(&mut self, on: bool) -> u8 {
        self.link_connected = on;
        if on {
            return 0;
        }
        let iff = if self.link_master_waiting {
            self.sb = 0xFF; // open cable reads 1s
            self.sc &= 0x7F;
            self.shifted = 0;
            self.link_master_waiting = false;
            0x08
        } else {
            0
        };
        self.link_in.clear();
        self.link_out.clear();
        iff
    }

    /// Whether a link peer is attached.
    pub(crate) fn link_connected(&self) -> bool {
        self.link_connected
    }

    /// Whether a connected master transfer is paused awaiting the peer byte
    /// (lockstep stall). Always false when disconnected — the run loop uses
    /// this to yield control to the frontend pump. See [`Self::link_master_waiting`].
    pub(crate) fn link_master_waiting(&self) -> bool {
        self.link_master_waiting
    }

    /// Whether a connected port is an **armed external-clock slave** (SC bit7
    /// set, bit0 clear) waiting for the master's byte, and the slave yield is
    /// enabled. A yield point like the master stall, so the frontend can deliver
    /// the byte per-transfer instead of once per frame (speedup). Always false
    /// when disconnected — golden-safe.
    pub(crate) fn link_slave_armed(&self) -> bool {
        self.link_connected && self.link_slave_yield && self.sc & 0x81 == 0x80
    }

    /// Enable/disable the armed-slave yield. The frontend disables it when a
    /// lockstep wait times out (master idle) so the slave runs full frames, and
    /// re-enables it on any peer packet. Inert when no slave is armed.
    pub(crate) fn set_link_slave_yield(&mut self, on: bool) {
        self.link_slave_yield = on;
    }

    /// Deliver a peer byte. If a master is **stalled** awaiting it (lockstep),
    /// complete that transfer now — SB ← `byte`, clear SC bit 7, clear the
    /// stall — and return the serial IF bits (`0x08`). Otherwise enqueue it
    /// (MSB-first, FIFO; bounded by [`LINK_QUEUE_CAP`]) for an upcoming master
    /// transfer to shift in, returning `0`.
    pub(crate) fn push_link_in(&mut self, byte: u8) -> u8 {
        if self.link_master_waiting {
            self.sb = byte;
            self.sc &= 0x7F; // transfer done
            self.shifted = 0;
            self.link_master_waiting = false;
            return 0x08;
        }
        if self.link_in.len() < LINK_QUEUE_CAP {
            self.link_in.push_back(byte);
        }
        0
    }

    /// Drain the next byte a completed master transfer shifted out (for the
    /// frontend to ship to the peer), oldest first. `None` when nothing is
    /// queued.
    pub(crate) fn take_link_send(&mut self) -> Option<u8> {
        self.link_out.pop_front()
    }

    /// Complete a pending **external-clock** (slave) transfer with the peer's
    /// (master's) byte: a slave is armed when SC bit 7 is set and bit 0 is
    /// clear (external clock) — with no peer it never advances. Swaps SB with
    /// `master_byte` (the slave receives the master's byte and its own old SB
    /// goes back out), clears the transfer-in-progress flag, and returns
    /// `(outgoing_byte, IF bits)`. When not armed it is a no-op returning
    /// `(None, 0)` — so a disconnected/idle port is unchanged.
    pub(crate) fn link_slave_transfer(&mut self, master_byte: u8) -> (Option<u8>, u8) {
        // Armed slave: transfer in progress (bit 7) on the external clock
        // (bit 0 clear). On CGB the fast-clock bit (1) is don't-care here.
        if self.sc & 0x81 != 0x80 {
            return (None, 0);
        }
        let outgoing = self.sb;
        self.sb = master_byte;
        self.sc &= 0x7F; // transfer done
        self.shifted = 0;
        (Some(outgoing), 0x08)
    }

    /// Read FF01/FF02.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF01 => self.sb,
            // SC unused bits read 1: DMG implements bits 7 and 0 only,
            // CGB adds bit 1 (fast clock).
            0xFF02 => {
                if self.cgb {
                    0x7C | self.sc
                } else {
                    0x7E | self.sc
                }
            }
            _ => 0xFF,
        }
    }

    /// Write FF01/FF02.
    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF01 => self.sb = value,
            0xFF02 => {
                // SameBoy Core/memory.c, GB_IO_SC case, in this order:
                // 1. the bit counter resets (so the forced edge below can
                //    shift but never complete a transfer or raise IF);
                // 2. a high master flip-flop is forced through an edge,
                //    resetting the shift-clock phase — this shifts one bit
                //    of the *old* transfer if one was active;
                // 3. the new SC value is stored: bit 7 set starts/restarts
                //    a transfer needing 8 more counter steps (minus the
                //    forced shift, which already counted one), bit 7 clear
                //    aborts.
                self.shifted = 0;
                if self.master_clock {
                    let iff = self.master_edge();
                    // The counter reset above makes completion (and thus
                    // IF) impossible on this edge.
                    debug_assert_eq!(iff, 0, "forced SC-write edge raised IF");
                }
                let mask = if self.cgb { 0x83 } else { 0x81 };
                self.sc = value & mask;
                // A SC write redefines the transfer (restart or abort): clear
                // any lockstep stall so the fresh transfer is not frozen by the
                // clocking gate. Inert unless a master was waiting (link only).
                self.link_master_waiting = false;
            }
            _ => {}
        }
    }
}

// --- Save state (manual serialization; see `crate::state`) ---
impl Serial {
    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        w.bool(self.cgb);
        w.u8(self.sb);
        w.u8(self.sc);
        w.u8(self.shifted);
        w.bool(self.master_clock);
        w.u16(self.prev_div);
        w.u8(self.out_shift);
        w.u32(self.out_buf.len() as u32);
        w.bytes(&self.out_buf);
    }
    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.cgb = r.bool()?;
        self.sb = r.u8()?;
        self.sc = r.u8()?;
        self.shifted = r.u8()?;
        // Live paths keep `shifted` in 0..=7 (reset at 8 + on every SC write); a
        // tampered/foreign state with `shifted > 7` would make the master shift
        // `7 - shifted` underflow, so reject it (the load stays atomic — the
        // machine is restored into a clone, so a rejected file leaves it intact).
        if self.shifted > 7 {
            return Err(crate::state::StateError::Truncated);
        }
        self.master_clock = r.bool()?;
        self.prev_div = r.u16()?;
        self.out_shift = r.u8()?;
        let n = r.u32()? as usize;
        self.out_buf = r.bytes_vec(n)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "serial_tests.rs"]
mod tests;
