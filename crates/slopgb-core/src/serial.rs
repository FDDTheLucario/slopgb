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

/// Cap on the harness output buffer: a frontend that never calls
/// `take_output` must not grow it without limit. 64 KiB is far more text
/// than any test ROM prints; completions past the cap are dropped.
const OUT_CAPTURE_CAP: usize = 64 * 1024;

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
        }
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
        if self.master_clock || self.sc & 0x81 != 0x81 {
            return 0;
        }
        // MSB out first; no peer, so 1 bits come in.
        self.out_shift = (self.out_shift << 1) | (self.sb >> 7);
        self.sb = (self.sb << 1) | 1;
        self.shifted += 1;
        if self.shifted == 8 {
            self.shifted = 0;
            self.sc &= 0x7F; // transfer-in-progress flag clears itself
            // Only internal-clock transfers reach completion: capture the
            // outgoing byte for the harness. An SC rewrite mid-transfer
            // restarts the bit counter, so the captured byte is the last
            // 8 bits actually shifted out.
            if self.out_buf.len() < OUT_CAPTURE_CAP {
                self.out_buf.push(self.out_shift);
            }
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
        self.master_clock = r.bool()?;
        self.prev_div = r.u16()?;
        self.out_shift = r.u8()?;
        let n = r.u32()? as usize;
        self.out_buf = r.bytes_vec(n)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Advance one M-cycle: bump the external div by 4 and tick.
    fn step(s: &mut Serial, div: &mut u16) -> u8 {
        *div = div.wrapping_add(4);
        s.tick(*div)
    }

    /// Run until tick returns IF bits; returns the div value at completion.
    fn run_until_irq(s: &mut Serial, div: &mut u16, max_mcycles: u32) -> Option<u16> {
        for _ in 0..max_mcycles {
            if step(s, div) != 0 {
                return Some(*div);
            }
        }
        None
    }

    #[test]
    fn sb_readback() {
        let mut s = Serial::new(false);
        s.write(0xFF01, 0x5A);
        assert_eq!(s.read(0xFF01), 0x5A);
    }

    #[test]
    fn sc_unused_bits_read_one_dmg() {
        let mut s = Serial::new(false);
        s.write(0xFF02, 0x00);
        assert_eq!(s.read(0xFF02), 0x7E);
        s.write(0xFF02, 0x01);
        assert_eq!(s.read(0xFF02), 0x7F);
        s.write(0xFF02, 0x80);
        assert_eq!(s.read(0xFF02), 0xFE);
    }

    #[test]
    fn sc_unused_bits_read_one_cgb() {
        let mut s = Serial::new(true);
        s.write(0xFF02, 0x00);
        assert_eq!(s.read(0xFF02), 0x7C);
        s.write(0xFF02, 0x02);
        assert_eq!(s.read(0xFF02), 0x7E);
        s.write(0xFF02, 0x83);
        assert_eq!(s.read(0xFF02), 0xFF);
    }

    /// A transfer started at div = 0 shifts every 512 T-cycles starting at
    /// div = 512 (second bit-7 falling edge) and completes with IF bit 3 on
    /// the 8th shift.
    #[test]
    fn transfer_completes_on_eighth_shift() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(8 * 512));
        assert_eq!(s.read(0xFF01), 0xFF); // 1s shifted in (no peer)
        assert_eq!(s.read(0xFF02), 0x7F); // bit 7 cleared
    }

    /// Shifts move the MSB out first and pull 1s in.
    #[test]
    fn shifts_msb_first_with_incoming_ones() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0b1010_0000);
        s.write(0xFF02, 0x81);
        while div < 512 {
            assert_eq!(step(&mut s, &mut div), 0);
        }
        assert_eq!(s.read(0xFF01), 0b0100_0001);
        while div < 1024 {
            step(&mut s, &mut div);
        }
        assert_eq!(s.read(0xFF01), 0b1000_0011);
    }

    /// The SC write resets the master flip-flop, so the first shift lands
    /// on the *second* DIV-bit-7 falling edge after the write — for a write
    /// at div = 600 (bit 7 next falls at 768) that is div = 1024, with
    /// completion 7 * 512 later (mooneye boot_sclk_align measures the same
    /// alignment; gambatte memory.cpp: completion =
    /// `cc - (cc - div_reset) % 0x100 + 0x200 * 8` = 600 - 88 + 4096).
    #[test]
    fn transfer_alignment_depends_on_div_phase() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        while div < 600 {
            step(&mut s, &mut div);
        }
        s.write(0xFF02, 0x81);
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(600 - 88 + 4096));
    }

    /// Discriminator against a bit-8 falling-edge model: a transfer started
    /// while DIV bit 8 is high (div = 300) must *not* shift at the upcoming
    /// bit-8 falling edge (div = 512) but at the second bit-7 falling edge
    /// (div = 768), completing at 300 - 44 + 4096 = 4352, not 4096
    /// (gambatte serial/nopx*_start_wait_read_if_*; SameBoy
    /// GB_serial_master_edge divide-by-2 of bit-7 edges).
    #[test]
    fn start_in_high_bit8_phase_shifts_a_half_period_later() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        while div < 300 {
            step(&mut s, &mut div);
        }
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 768 {
            assert_eq!(step(&mut s, &mut div), 0);
        }
        assert_eq!(s.read(0xFF01), 0x01, "first shift exactly at div = 768");
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(4352));
    }

    /// An edge in the M-cycle *before* SC is written does not count: the
    /// write happens after that cycle's tick.
    #[test]
    fn edge_before_sc_write_does_not_shift() {
        let mut s = Serial::new(false);
        let mut div = 252u16;
        s.tick(div); // seed prev_div with bit 7 high (252 = 0xFC)
        div = 256;
        s.tick(div); // falling edge: master flip-flop toggles high
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81); // forces the flip-flop low again (no shift:
        // no transfer was active)
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(768 + 7 * 512));
    }

    /// External clock (SC bit 0 = 0) with no peer: nothing ever happens and
    /// the transfer flag stays set.
    #[test]
    fn external_clock_never_completes() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x42);
        s.write(0xFF02, 0x80);
        assert_eq!(run_until_irq(&mut s, &mut div, 20_000), None);
        assert_eq!(s.read(0xFF01), 0x42);
        assert_eq!(s.read(0xFF02), 0xFE); // bit 7 still set
    }

    /// Clock edges with no transfer in progress do nothing.
    #[test]
    fn idle_edges_do_nothing() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x42);
        for _ in 0..2000 {
            assert_eq!(step(&mut s, &mut div), 0);
        }
        assert_eq!(s.read(0xFF01), 0x42);
    }

    /// CGB fast clock (SC bit 1): master edges on DIV bit 2, i.e. a shift
    /// every 16 T-cycles, full transfer in 128.
    #[test]
    fn cgb_fast_clock_uses_bit2() {
        let mut s = Serial::new(true);
        let mut div = 0u16;
        s.write(0xFF02, 0x83);
        assert_eq!(run_until_irq(&mut s, &mut div, 100), Some(8 * 16));
        assert_eq!(s.read(0xFF01), 0xFF);
    }

    /// DMG has no fast-clock bit; writing it is ignored.
    #[test]
    fn dmg_ignores_fast_clock_bit() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF02, 0x83);
        assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(8 * 512));
    }

    /// A DIV counter reset (DIV write) that drops the clock bit from 1 to 0
    /// is a falling edge and toggles the master flip-flop; with the
    /// flip-flop high (one bit-7 edge since the SC write) that toggle is a
    /// shift, like the timer's falling-edge glitch.
    #[test]
    fn div_reset_can_clock_shifter() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 400 {
            step(&mut s, &mut div); // div = 400 (0x190): bit 7 high,
            // flip-flop high since div = 256
        }
        assert_eq!(s.div_write(div), 0); // flip-flop high->low: one shift
        assert_eq!(s.read(0xFF01), 0x01);
        // The next sampled tick (counter 0 -> 4) must not double-count.
        assert_eq!(s.tick(4), 0);
        assert_eq!(s.read(0xFF01), 0x01);
    }

    /// Fast-clock variant: DIV bit 2 has period 8, so it is high again by
    /// the M-cycle after the write — only the dedicated `div_write` path
    /// can see the reset edge (gambatte serial/start83_late_div_write_*).
    #[test]
    fn cgb_fast_clock_div_reset_edge_is_not_missed() {
        let mut s = Serial::new(true);
        let mut div = 0u16;
        s.write(0xFF02, 0x83);
        step(&mut s, &mut div); // 4
        step(&mut s, &mut div); // 8: bit-2 falling edge, flip-flop high
        step(&mut s, &mut div); // 12: rising
        assert_eq!(s.div_write(div), 0); // bit 2 of 12 high: edge -> shift
        assert_eq!(s.read(0xFF01), 0x01);
        // 0 -> 4 next cycle is a rising edge: no double count.
        assert_eq!(s.tick(4), 0);
        assert_eq!(s.read(0xFF01), 0x01);
    }

    /// A DIV reset with the clock bit low is no edge at all.
    #[test]
    fn div_reset_with_low_clock_bit_does_nothing() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 300 {
            step(&mut s, &mut div); // 300 (0x12C): bit 7 low
        }
        assert_eq!(s.div_write(div), 0);
        assert_eq!(s.read(0xFF01), 0x00, "no shift");
    }

    /// A DIV reset while the flip-flop is low only toggles it high (no
    /// shift); the transfer then continues on the post-reset edge grid.
    /// This is the gambatte `serial/start_late_div_write_wait_read_if_2b`
    /// scenario: SC at div = 44, 7 bits shifted by div = 3712 (shifts at
    /// 512..3584), DIV reset at 3712 (bit 7 high -> edge, flip-flop
    /// low->high), 8th shift and IF on the first post-reset bit-7 falling
    /// edge at counter 256.
    #[test]
    fn div_reset_during_low_flip_flop_delays_completion_to_post_reset_grid() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        while div < 44 {
            step(&mut s, &mut div);
        }
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 3712 {
            assert_eq!(step(&mut s, &mut div), 0, "no IF before the DIV reset");
        }
        assert_eq!(s.read(0xFF01), 0x7F); // 7 of 8 bits shifted
        // DIV write: counter 3712 (bit 7 high) resets -> falling edge,
        // flip-flop toggles low->high without a shift.
        assert_eq!(s.div_write(div), 0);
        div = 4;
        s.tick(div);
        assert_eq!(s.read(0xFF01), 0x7F, "reset edge alone must not shift");
        // First post-reset bit-7 falling edge: counter 252 -> 256.
        assert_eq!(run_until_irq(&mut s, &mut div, 100), Some(256));
        assert_eq!(s.read(0xFF01), 0xFF);
    }

    /// Rewriting SC with bit 7 set mid-transfer restarts the bit counter:
    /// 8 more shifts are needed before completion, and SB keeps the
    /// partially shifted contents (SameBoy Core/memory.c resets its serial
    /// bit counter on every SC write). The flip-flop is low at the rewrite
    /// (a shift just happened), so no forced shift occurs here.
    #[test]
    fn sc_rewrite_mid_transfer_restarts_bit_counter() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 1024 {
            step(&mut s, &mut div); // two bits shifted (at 512, 1024)
        }
        assert_eq!(s.read(0xFF01), 0x03);
        s.write(0xFF02, 0x81); // restart mid-transfer
        assert_eq!(s.read(0xFF01), 0x03, "SB keeps the partial shift");
        while div < 1536 {
            step(&mut s, &mut div); // next edge continues from partial SB
        }
        assert_eq!(s.read(0xFF01), 0x07);
        // 8 fresh shifts from the rewrite: completion at 1024 + 8 * 512,
        // not at the original 8 * 512.
        assert_eq!(
            run_until_irq(&mut s, &mut div, 20_000),
            Some(1024 + 8 * 512)
        );
        assert_eq!(s.read(0xFF01), 0xFF);
        assert_eq!(s.read(0xFF02), 0x7F); // bit 7 cleared on completion
    }

    /// An SC rewrite while the flip-flop is *high* forces a master edge
    /// first (SameBoy GB_IO_SC): the old transfer shifts one bit
    /// immediately — counted toward the restarted transfer's 8 because the
    /// bit counter was reset before the forced edge — and completion lands
    /// 7 shifts later. The forced edge itself can never raise IF.
    #[test]
    fn sc_rewrite_with_high_flip_flop_shifts_immediately() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 800 {
            step(&mut s, &mut div); // shift at 512, flip-flop high at 768
        }
        assert_eq!(s.read(0xFF01), 0x01);
        s.write(0xFF02, 0x81); // forced edge: immediate second shift
        assert_eq!(s.read(0xFF01), 0x03);
        // 7 more shifts: flip-flop high at 1024, shifts at 1280..1280+6*512.
        assert_eq!(
            run_until_irq(&mut s, &mut div, 20_000),
            Some(1280 + 6 * 512)
        );
        assert_eq!(s.read(0xFF01), 0xFF);
    }

    /// Aborting (bit 7 clear) while the flip-flop is high also forces the
    /// edge: one last glitch bit shifts out of the old transfer, then the
    /// port is idle and IF never fires.
    #[test]
    fn sc_abort_with_high_flip_flop_shifts_one_glitch_bit() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 800 {
            step(&mut s, &mut div);
        }
        assert_eq!(s.read(0xFF01), 0x01);
        s.write(0xFF02, 0x01); // abort; flip-flop high -> forced shift
        assert_eq!(s.read(0xFF01), 0x03);
        assert_eq!(run_until_irq(&mut s, &mut div, 20_000), None);
        assert_eq!(s.read(0xFF01), 0x03);
    }

    // ---- harness output capture ----

    /// A completed internal-clock transfer captures the byte that was
    /// shifted out (MSB first) — what a link-cable peer would have
    /// received. This is how blargg test ROMs print: SB <- byte, SC <- $81.
    #[test]
    fn internal_transfer_capture() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x5A);
        s.write(0xFF02, 0x81);
        assert_eq!(s.take_output(), [], "nothing captured before completion");
        run_until_irq(&mut s, &mut div, 20_000).unwrap();
        assert_eq!(s.take_output(), [0x5A]);
        assert_eq!(s.take_output(), [], "take drains the buffer");
    }

    #[test]
    fn capture_accumulates_across_transfers() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        for byte in [0xAB, 0x00, 0xFF] {
            s.write(0xFF01, byte);
            s.write(0xFF02, 0x81);
            run_until_irq(&mut s, &mut div, 20_000).unwrap();
        }
        assert_eq!(s.take_output(), [0xAB, 0x00, 0xFF]);
    }

    /// External-clock transfers (SC = $80) never advance without a peer
    /// and must capture nothing.
    #[test]
    fn external_clock_captures_nothing() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x42);
        s.write(0xFF02, 0x80);
        assert_eq!(run_until_irq(&mut s, &mut div, 20_000), None);
        assert_eq!(s.take_output(), []);
    }

    /// A mid-transfer SC rewrite restarts the bit counter
    /// (`sc_rewrite_mid_transfer_restarts_bit_counter`); the captured byte
    /// is the last 8 bits that actually shifted out, not the original SB.
    #[test]
    fn capture_reflects_outgoing_bits_after_restart() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 1024 {
            step(&mut s, &mut div); // two 0 bits out, SB now 0x03
        }
        s.write(0xFF02, 0x81); // restart: 8 fresh shifts
        run_until_irq(&mut s, &mut div, 20_000).unwrap();
        // Outgoing bits after the restart: six 0s (SB top bits), then the
        // two 1s shifted in earlier reach bit 7.
        assert_eq!(s.take_output(), [0x03]);
    }

    /// The capture buffer is bounded: a harness that never drains cannot
    /// grow it without limit; completions past the cap are dropped.
    #[test]
    fn capture_buffer_is_bounded() {
        let mut s = Serial::new(true);
        let mut div = 0u16;
        for _ in 0..(64 * 1024 + 8) {
            s.write(0xFF02, 0x83); // CGB fast clock: 128 T per transfer
            run_until_irq(&mut s, &mut div, 100).unwrap();
        }
        assert_eq!(s.take_output().len(), 64 * 1024);
    }

    /// Clearing SC bit 7 aborts an in-flight transfer (flip-flop low here:
    /// no forced shift).
    #[test]
    fn sc_write_aborts_transfer() {
        let mut s = Serial::new(false);
        let mut div = 0u16;
        s.write(0xFF01, 0x00);
        s.write(0xFF02, 0x81);
        while div < 1024 {
            step(&mut s, &mut div); // two bits shifted
        }
        assert_eq!(s.read(0xFF01), 0x03);
        s.write(0xFF02, 0x01);
        assert_eq!(run_until_irq(&mut s, &mut div, 20_000), None);
        assert_eq!(s.read(0xFF01), 0x03); // partial data kept
    }
}
