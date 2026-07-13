//! Unit tests for the serial port + lockstep link cable (`serial.rs`).

use super::*;

/// A save state with `shifted > 7` (only a tampered/foreign file — live
/// paths keep it 0..=7) is rejected, so the master shift's `7 - shifted`
/// can never underflow-panic with a link peer attached.
#[test]
fn read_state_rejects_out_of_range_shifted() {
    let mut w = crate::state::Writer::new();
    Serial::new(false).write_state(&mut w);
    let mut bytes = w.into_vec();
    bytes[3] = 200; // shifted is the 4th field (cgb, sb, sc, shifted)
    let mut r = crate::state::Reader::new(&bytes);
    let mut s = Serial::new(false);
    assert!(matches!(
        s.read_state(&mut r),
        Err(crate::state::StateError::Truncated)
    ));
}

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

// ---- link cable ----

/// Task 1 (lockstep): a connected master reaching its 8th shift with no
/// buffered peer byte **stalls** — SC bit 7 stays set, no IF is raised, the
/// outgoing byte ships to `link_out` exactly once, and further DIV edges
/// shift nothing. (Old lossy model completed with 1s here, corrupting
/// Pokémon trades.)
#[test]
fn connected_master_stalls_without_peer_byte() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true); // connected, but NO peer byte buffered
    s.write(0xFF01, 0x5A);
    s.write(0xFF02, 0x81);
    // Run well past the would-be completion (8 * 512) — must NOT raise IF.
    assert_eq!(
        run_until_irq(&mut s, &mut div, 4000),
        None,
        "stalled: no IF"
    );
    assert_eq!(s.read(0xFF02) & 0x80, 0x80, "transfer still in progress");
    assert!(s.link_master_waiting(), "master waits for the peer byte");
    // Outgoing byte shipped to the frontend exactly once.
    assert_eq!(s.take_link_send(), Some(0x5A));
    assert_eq!(s.take_link_send(), None, "shipped exactly once");
}

/// Task 2: feeding the peer byte to a stalled master completes it —
/// SB=peer, SC bit7 clear, serial IF (0x08) returned, stall cleared.
#[test]
fn push_link_in_completes_stalled_master() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.write(0xFF01, 0x12);
    s.write(0xFF02, 0x81);
    assert_eq!(run_until_irq(&mut s, &mut div, 4000), None); // stalls
    assert!(s.link_master_waiting());
    assert_eq!(s.push_link_in(0x9C), 0x08, "delivery raises serial IF");
    assert!(!s.link_master_waiting());
    assert_eq!(s.read(0xFF01), 0x9C, "SB holds the peer byte");
    assert_eq!(s.read(0xFF02) & 0x80, 0, "transfer flag cleared");
}

/// Task 4: disconnecting while a master is stalled unblocks the CPU — SB
/// reads the cable-open 0xFF, SC bit7 clears, serial IF (0x08) is returned,
/// and the stall + link queues are cleared (no hung transfer).
#[test]
fn disconnect_while_stalled_completes_with_open_bus() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.write(0xFF01, 0x12);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 4000); // stalls
    assert!(s.link_master_waiting());
    assert_eq!(
        s.set_link_connected(false),
        0x08,
        "disconnect raises serial IF"
    );
    assert!(!s.link_master_waiting());
    assert_eq!(s.read(0xFF01), 0xFF, "open cable reads 1s");
    assert_eq!(s.read(0xFF02) & 0x80, 0, "transfer flag cleared");
    assert_eq!(s.take_link_send(), None, "queues cleared");
}

/// Disconnecting with no stall in flight raises no spurious IF.
#[test]
fn disconnect_without_stall_raises_no_if() {
    let mut s = Serial::new(false);
    s.set_link_connected(true);
    assert_eq!(s.set_link_connected(false), 0, "no transfer to complete");
}

/// A non-stalled connected master enqueues the byte (returns 0, no IF) for
/// the next transfer to shift in.
#[test]
fn push_link_in_enqueues_when_not_stalled() {
    let mut s = Serial::new(false);
    s.set_link_connected(true);
    assert_eq!(s.push_link_in(0xA5), 0, "enqueue, no IF");
    let mut div = 0u16;
    s.write(0xFF01, 0x00);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 2000).unwrap();
    assert_eq!(s.read(0xFF01), 0xA5);
}

/// A SC rewrite restarting the transfer clears a stall so the fresh
/// transfer is not frozen by the clocking gate.
#[test]
fn sc_write_clears_stall() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.write(0xFF01, 0x11);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 4000); // stalls
    assert!(s.link_master_waiting());
    s.write(0xFF02, 0x81); // restart
    assert!(!s.link_master_waiting(), "restart cleared the stall");
}

/// Golden-safety: with no peer attached (`link_in` empty) a master transfer
/// shifts in 1s exactly as the cable-disconnected hardware. Same assertion
/// as `transfer_completes_on_eighth_shift` — guards the new injection
/// branch against perturbing the no-peer path.
#[test]
fn disconnected_master_transfer_byte_identical() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    // NOT connected — the golden path. (Under lockstep a *connected* master
    // with no peer byte stalls instead, so this guards that the new
    // `link_connected` branch is never entered when disconnected.)
    s.write(0xFF01, 0x00);
    s.write(0xFF02, 0x81);
    assert_eq!(run_until_irq(&mut s, &mut div, 2000), Some(8 * 512));
    assert_eq!(s.read(0xFF01), 0xFF); // 1s shifted in — unchanged
}

/// A connected master transfer shifts the injected peer byte in MSB-first;
/// after 8 shifts SB holds the full peer byte.
#[test]
fn connected_master_shifts_in_peer_byte() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.push_link_in(0xA5);
    s.write(0xFF01, 0x00);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 2000).unwrap();
    assert_eq!(s.read(0xFF01), 0xA5);
}

/// Partial MSB-first order: after 4 of 8 shifts of peer 0xF0, SB top
/// nibble holds 0xF0's top nibble shifted down (`0x0F`).
#[test]
fn connected_master_incoming_is_msb_first() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.push_link_in(0xF0);
    s.write(0xFF01, 0x00);
    s.write(0xFF02, 0x81);
    while div < 4 * 512 {
        step(&mut s, &mut div);
    }
    assert_eq!(s.read(0xFF01), 0x0F); // four 1s (0xF0 MSBs) in the low bits
}

/// A completed master transfer queues its outgoing byte for the frontend
/// only while connected; disconnected leaves nothing. (Peer byte pre-fed so
/// the transfer completes rather than stalling.)
#[test]
fn connected_master_completion_queues_send() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.push_link_in(0xFF); // peer byte ready → completes, no stall
    s.write(0xFF01, 0x3C);
    s.write(0xFF02, 0x81);
    assert_eq!(s.take_link_send(), None, "nothing before completion");
    run_until_irq(&mut s, &mut div, 2000).unwrap();
    assert_eq!(s.take_link_send(), Some(0x3C));
    assert_eq!(s.take_link_send(), None, "take drains");
}

#[test]
fn disconnected_take_link_send_is_none() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.write(0xFF01, 0x3C);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 2000).unwrap();
    assert_eq!(s.take_link_send(), None);
}

/// The peer byte is consumed per transfer: a second transfer with no fresh
/// byte **stalls** (lockstep) rather than reusing a stale byte or reading 1s.
#[test]
fn link_in_consumed_after_one_transfer() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.push_link_in(0xA5);
    s.write(0xFF01, 0x00);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 2000).unwrap();
    assert_eq!(s.read(0xFF01), 0xA5);
    s.write(0xFF01, 0x00);
    s.write(0xFF02, 0x81); // no new peer byte
    assert_eq!(
        run_until_irq(&mut s, &mut div, 2000),
        None,
        "stalls; no stale peer byte reused"
    );
    assert!(s.link_master_waiting());
}

/// Task 3: an armed external-clock slave completes when the frontend
/// delivers the master's byte — SB ↔ master byte, SC bit 7 cleared, IF
/// bit 3 raised, the slave's old SB returned for the peer.
#[test]
fn slave_pending_completes_and_returns_byte() {
    let mut s = Serial::new(false);
    s.write(0xFF01, 0x34); // slave's outgoing byte
    s.write(0xFF02, 0x80); // bit7 set, external clock (bit0 clear): armed
    let (out, iff) = s.link_slave_transfer(0x12);
    assert_eq!(out, Some(0x34));
    assert_eq!(iff, 0x08);
    assert_eq!(s.read(0xFF01), 0x12, "slave received the master byte");
    assert_eq!(s.read(0xFF02) & 0x80, 0, "transfer flag cleared");
}

/// An idle/unarmed port (no transfer pending) is a no-op for a delivered
/// byte — SB unchanged, no IF.
#[test]
fn slave_not_pending_returns_none_sb_unchanged() {
    let mut s = Serial::new(false);
    s.write(0xFF01, 0x34);
    s.write(0xFF02, 0x00); // no transfer pending
    let (out, iff) = s.link_slave_transfer(0x12);
    assert_eq!(out, None);
    assert_eq!(iff, 0);
    assert_eq!(s.read(0xFF01), 0x34, "SB untouched");
}

/// A master (internal-clock, bit 0 set) port is not a slave: delivering a
/// byte must not hijack it.
#[test]
fn internal_clock_port_is_not_a_slave() {
    let mut s = Serial::new(false);
    s.write(0xFF01, 0x34);
    s.write(0xFF02, 0x81); // internal clock pending — a master, not slave
    let (out, iff) = s.link_slave_transfer(0x12);
    assert_eq!(out, None);
    assert_eq!(iff, 0);
    assert_eq!(s.read(0xFF01), 0x34);
}

/// Detaching the peer clears pending link state.
#[test]
fn disconnect_clears_link_state() {
    let mut s = Serial::new(false);
    s.set_link_connected(true);
    s.push_link_in(0xA5);
    s.set_link_connected(false);
    assert!(!s.link_connected());
    // No peer byte: a master transfer falls back to 1s.
    let mut div = 0u16;
    s.write(0xFF01, 0x00);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 2000).unwrap();
    assert_eq!(s.read(0xFF01), 0xFF);
    assert_eq!(s.take_link_send(), None);
}

/// The link bytes are queued, not single-slotted: several transfers
/// completing (or peer bytes arriving) before the frontend drains preserve
/// every byte in FIFO order — guards the multi-transfer-per-frame loss.
#[test]
fn link_queues_multiple_bytes_in_order() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    // Two peer bytes queued before either transfer; two transfers consume
    // them in order, and both outgoing bytes queue up for the frontend.
    s.push_link_in(0x11);
    s.push_link_in(0x22);
    s.write(0xFF01, 0xA0);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 2000).unwrap();
    assert_eq!(s.read(0xFF01), 0x11, "first transfer shifts in 0x11");
    s.write(0xFF01, 0xB0);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 2000).unwrap();
    assert_eq!(s.read(0xFF01), 0x22, "second transfer shifts in 0x22");
    // Both outgoing bytes are still queued (FIFO), none overwritten.
    assert_eq!(s.take_link_send(), Some(0xA0));
    assert_eq!(s.take_link_send(), Some(0xB0));
    assert_eq!(s.take_link_send(), None);
}

/// Task 6: two Serials wired master ↔ slave exchange a byte both ways with
/// no socket — proves the byte-exchange mechanism end to end.
#[test]
fn loopback_master_slave_byte_exchange() {
    let mut master = Serial::new(false);
    let mut slave = Serial::new(false);
    master.set_link_connected(true);
    slave.set_link_connected(true);
    let mut div = 0u16;

    // Slave arms with its byte (external clock), master with its byte.
    slave.write(0xFF01, 0x34);
    slave.write(0xFF02, 0x80);
    master.write(0xFF01, 0x12);
    // Frontend pre-exchange: feed the master the slave's pending byte.
    master.push_link_in(0x34);
    master.write(0xFF02, 0x81); // master clocks the transfer

    let done = run_until_irq(&mut master, &mut div, 2000);
    assert!(done.is_some(), "master transfer completes");
    // Master shifted the slave's byte in and its own byte out.
    assert_eq!(master.read(0xFF01), 0x34);
    let sent = master.take_link_send().expect("master queued its byte");
    assert_eq!(sent, 0x12);

    // Deliver the master's byte to the armed slave.
    let (slave_out, iff) = slave.link_slave_transfer(sent);
    assert_eq!(slave_out, Some(0x34));
    assert_eq!(iff, 0x08);
    assert_eq!(slave.read(0xFF01), 0x12, "slave received the master byte");
}

/// Task 8: the transient link fields (the master-waiting stall + the
/// queues) are never serialized — a save-state round-trip leaves a fresh,
/// non-stalled, disconnected port (so the on-disk format stays golden-safe).
#[test]
fn save_state_drops_link_state() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.write(0xFF01, 0x12);
    s.write(0xFF02, 0x81);
    run_until_irq(&mut s, &mut div, 4000); // stalls; link_out holds a byte
    assert!(s.link_master_waiting());
    let mut w = crate::state::Writer::new();
    s.write_state(&mut w);
    let bytes = w.into_vec();
    let mut r = crate::state::Reader::new(&bytes);
    let mut fresh = Serial::new(false);
    fresh.read_state(&mut r).unwrap();
    assert!(!fresh.link_master_waiting(), "stall not serialized");
    assert!(!fresh.link_connected(), "connection not serialized");
    assert_eq!(fresh.take_link_send(), None, "send queue not serialized");
}

/// Task 9 golden guard: a connected master whose peer always replies 0xFF
/// (idle/open line) completes with SB=0xFF — byte-identical to the
/// disconnected golden path, so the lockstep stall can't regress the
/// documented no-peer behavior.
#[test]
fn connected_master_with_open_peer_matches_golden() {
    let mut s = Serial::new(false);
    let mut div = 0u16;
    s.set_link_connected(true);
    s.write(0xFF01, 0x00);
    s.write(0xFF02, 0x81);
    assert_eq!(run_until_irq(&mut s, &mut div, 4000), None, "stalls");
    assert_eq!(s.take_link_send(), Some(0x00), "shipped its outgoing byte");
    assert_eq!(s.push_link_in(0xFF), 0x08, "open-line reply completes it");
    assert_eq!(s.read(0xFF01), 0xFF, "SB matches the no-peer 1s value");
    assert_eq!(s.read(0xFF02) & 0x80, 0, "completed");
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
