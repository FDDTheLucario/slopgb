//! `interconnect_tests` — hdma tests (split for file size).

use super::*;

/// A GDMA write only *requests* the transfer; the copy steals the bus
/// at the head of the CPU's next machine cycle — 8 M-cycles per block
/// (2 bytes per M-cycle at normal speed) plus one teardown M-cycle
/// (gambatte memory.cpp dma(): `cc += 2 + 2 * doubleSpeed` per byte,
/// `cc += 4` at the end; see `service_vram_dma` for the seam).
#[test]
fn gdma_steals_the_next_machine_cycle_plus_teardown() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x40);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    let before = b.cycles();
    b.write(0xFF55, 0x03); // 4 blocks = 64 bytes, requested
    assert_eq!(b.cycles() - before, 4, "the write cycle only flags");
    assert_eq!(b.peek_no_io(0x8000), 0x00, "nothing copied yet");
    let before = b.cycles();
    b.tick(); // the steal precedes this op's own cycle
    assert_eq!(b.cycles() - before, (4 * 8 + 1 + 1) * 4, "stall + teardown");
    assert_eq!(b.peek_no_io(0x8000), 0x40);
    assert_eq!(b.peek_no_io(0x803F), 0x7F);
    assert_eq!(b.read(0xFF55), 0xFF, "completed");
    // HDMA1-4 are write-only.
    assert_eq!(b.read(0xFF51), 0xFF);
    assert_eq!(b.read(0xFF54), 0xFF);
}

#[test]
fn gdma_continues_from_incremented_addresses() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x00, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF55, 0x00); // one block
    b.tick();
    b.write(0xFF55, 0x00); // next block continues at +0x10
    b.tick();
    assert_eq!(b.read(0x8010), 0x10);
    assert_eq!(b.read(0x801F), 0x1F);
}

/// FF51-FF54 write straight into the *live* DMA address counters
/// (gambatte memory.cpp cases 0x51-0x54: `dmaSource_ = data << 8 |
/// (dmaSource_ & 0xFF)` etc.; SameBoy's GB_IO_HDMA1-4 handlers agree):
/// rewriting only FF51 after blocks have copied keeps the incremented
/// low byte, so the next transfer reads from (new high byte | live low
/// byte), not from a fresh xx00.
#[test]
fn hdma_partial_src_rewrite_blends_live_counter() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x00, 0x30);
    fill_wram(&mut b, 0xD030, 0xA0, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF55, 0x02); // 3 blocks: src counter is then 0xC030
    b.tick();
    b.write(0xFF51, 0xD0); // rewrite the high byte only
    b.write(0xFF55, 0x00); // 1 block: src 0xD030.., dst continues at 0x30
    b.tick();
    assert_eq!(b.read(0x8030), 0xA0, "live low byte kept: src 0xD030");
    assert_eq!(b.read(0x803F), 0xAF);
}

/// VRAM and 0xE000+ are not valid VRAM-DMA sources (Pan Docs "CGB
/// DMA"); the engine copies 0xFF instead of looping VRAM back into
/// itself (SameBoy GB_hdma_run only drives the bus for ROM/SRAM/WRAM
/// sources; everything else yields the idle data-bus value).
#[test]
fn gdma_invalid_sources_fill_destination_with_ff() {
    for src in [0x8000u16, 0x9000, 0xE000, 0xF000] {
        let mut b = ic_cgb_mode();
        // Distinct data at the would-be source and the destination.
        b.write(0x8000, 0x12);
        b.write(0x9000, 0x34);
        for i in 0..16 {
            b.write(0x9800 + i, 0x55);
        }
        setup_gdma_regs(&mut b, src, 0x1800);
        b.write(0xFF55, 0x00); // one block
        b.tick();
        for i in 0..16 {
            assert_eq!(b.read(0x9800 + i), 0xFF, "src {src:04X} byte {i}");
        }
    }
}

/// The destination is a full 16-bit counter: a transfer reaching
/// 0x10000 terminates there with FF55 bit 7 latched — it does *not*
/// wrap back into VRAM (gambatte memory.cpp dma(): `if (dmaDest +
/// length >= 0x10000) { length = 0x10000 - dmaDest; ioamhram_[0x155]
/// |= 0x80; }`, hardware-captured by gambatte dma/dma_dst_wrap_2;
/// FF53 keeps the full high byte, masked only at the VRAM write).
/// This replaces the earlier SameBoy-derived wrap-to-0x8000 model,
/// which that capture contradicts.
#[test]
fn gdma_terminates_at_dest_0x10000_crossing() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0xFFF0);
    b.write(0xFF55, 0x01); // 2 blocks requested, only one fits
    b.tick();
    assert_eq!(
        b.peek_no_io(0x9FF0),
        0x40,
        "dest 0xFFF0 masks to VRAM 0x1FF0"
    );
    assert_eq!(b.peek_no_io(0x9FFF), 0x4F);
    assert_eq!(b.peek_no_io(0x8000), 0x00, "no wrap into a second block");
    // With the display off the truncated GDMA still retires its whole
    // length (gambatte dma(): `if (!(lcdc & en) && gdmaReqFlagged)
    // dmaLength = 0`), reading back $FF.
    assert_eq!(b.read(0xFF55), 0xFF);
}

#[test]
fn hblank_dma_one_block_per_hblank() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91); // LCD on: glitched line, hblank from ~dot 250
    b.write(0xFF55, 0x81); // hblank DMA, 2 blocks (PPU at dot 4)
    assert_eq!(b.read(0xFF55), 0x01, "2 blocks remaining reads 1");
    assert_eq!(b.peek_no_io(0x8000), 0x00, "nothing copied before hblank");
    // Run into the glitched line's hblank; the block transfer steals
    // 8 M-cycles + 1 teardown at the next boundary.
    ticks(&mut b, 90); // ~dot 400 incl. the stall
    assert_eq!(b.read(0xFF55), 0x00, "one block left");
    assert_eq!(b.peek_no_io(0x8000), 0x40);
    assert_eq!(b.peek_no_io(0x800F), 0x4F);
    assert_eq!(
        b.peek_no_io(0x8010),
        0x00,
        "second block waits for next hblank"
    );
    // Run well into line 1's hblank.
    ticks(&mut b, 100);
    assert_eq!(b.read(0xFF55), 0xFF, "done");
    assert_eq!(b.peek_no_io(0x8010), 0x50);
    assert_eq!(b.peek_no_io(0x801F), 0x5F);
}

/// Cancelling latches bit 7 plus the *written* length bits — the
/// FF55 write replaces the live count before the cancel takes effect
/// (gambatte memory.cpp case 0x55: `ioamhram_[0x155] = data & 0x7F`
/// precedes the `|= 0x80`; SameBoy sets hdma_steps_left first, too).
#[test]
fn hblank_dma_cancel_sets_bit7_and_latches_written_length() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x80);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x87); // 8 blocks
    ticks(&mut b, 90); // first hblank: one block done
    assert_eq!(b.read(0xFF55), 0x06);
    b.write(0xFF55, 0x02); // cancel, writing length bits 0x02
    assert_eq!(b.read(0xFF55), 0x82, "bit 7 + the written length bits");
    ticks(&mut b, 101); // into line 1's hblank
    assert_eq!(b.peek_no_io(0x8010), 0x00, "no further blocks after cancel");
}

/// Enabling HBlank DMA with the LCD off copies one block immediately
/// and leaves the transfer armed (gambatte video.cpp enableHdma's
/// LCD-off branch flags a request at once; SameBoy GB_IO_HDMA5:
/// `(STAT & 3) == 0 && display_state != 7 → hdma_on = true`).
#[test]
fn hblank_enable_with_lcd_off_copies_one_block_immediately() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF55, 0x81); // LCD is off
    b.tick();
    assert_eq!(b.peek_no_io(0x8000), 0x40);
    assert_eq!(b.peek_no_io(0x800F), 0x4F);
    assert_eq!(b.peek_no_io(0x8010), 0x00, "exactly one block");
    assert_eq!(b.read(0xFF55), 0x00, "armed, one block left");
    // The remaining block fires at the first mode-0 entry after the
    // display comes on.
    b.write(0xFF40, 0x91);
    ticks(&mut b, 90);
    assert_eq!(b.peek_no_io(0x8010), 0x50);
    assert_eq!(b.read(0xFF55), 0xFF, "completed");
}

/// Enabling HBlank DMA inside the hblank window fires the first block
/// in that same hblank; within 3 dots of the line end it waits for
/// the next one (gambatte video.cpp enableHdma →
/// `isHdmaPeriod(...)`: `ly < 144 && cc + 3 + 3 * ds <
/// lyCounter.time() && cc >= m0TimeOfCurrentLy`).
#[test]
fn hblank_enable_inside_window_fires_immediately() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    while !b.ppu.hblank_active() {
        b.tick();
    }
    b.write(0xFF55, 0x80); // 1 block, enabled mid-hblank
    b.tick();
    assert_eq!(b.peek_no_io(0x8000), 0x40);
    assert_eq!(b.read(0xFF55), 0xFF, "completed in the same hblank");
}

/// The window cutoff: in double speed (2-dot M-cycles) an enable
/// landing 2 dots before the line end is outside the window and
/// waits for the next hblank.
#[test]
fn hblank_enable_past_window_cutoff_waits() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    b.stop(0x0000, true); // double speed, instantly
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    // Glitched enable line: 452 dots, hblank from ~dot 250. Park 2
    // dots before its end (dot 450 = 225 double-speed M-cycles).
    ticks(&mut b, 224);
    assert!(b.ppu.hblank_active(), "still in the glitch line's hblank");
    b.write(0xFF55, 0x80); // PPU at dot 450: 2 dots left < 3-dot margin
    b.tick();
    assert_eq!(
        b.peek_no_io(0x8000),
        0x00,
        "no block this close to line end"
    );
    assert_eq!(b.read(0xFF55), 0x00, "armed, nothing copied");
    // The next line's mode-0 entry fires it.
    ticks(&mut b, 250);
    assert_eq!(b.peek_no_io(0x8000), 0x40);
}

/// The block/CPU-access race has M-cycle granularity: a block flagged
/// in an earlier M-cycle steals the bus at the head of the next bus
/// operation (the racing access loses), while an access whose own
/// tick contains the trigger still commits first (the gambatte
/// hdma_late_destl/_wrambank/_length `_1`/`_2` adjacent-cycle pairs:
/// shifting the same code by one cycle flips the winner).
#[test]
fn hblank_block_race_has_machine_cycle_granularity() {
    // Calibrate: machine cycles from arming to the trigger dot.
    let lead_ticks = {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x10);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x80);
        let mut n = 0u32;
        while !b.ppu.hdma_trigger_level() {
            b.tick();
            n += 1;
        }
        n
    };
    // Trigger during tick N, dest write afterwards: the steal heads
    // the write — the block uses the old destination.
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x80);
    ticks(&mut b, lead_ticks);
    b.write(0xFF53, 0x90);
    assert_eq!(b.peek_no_io(0x8000), 0x40, "block first: old dest");
    assert_eq!(b.peek_no_io(0x9000), 0x00);
    // Trigger inside the write's own tick: the write commits first
    // and the block uses the new destination.
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x80);
    ticks(&mut b, lead_ticks - 1);
    b.write(0xFF53, 0x90); // this op's tick contains the trigger
    b.tick(); // the steal happens here
    assert_eq!(b.peek_no_io(0x9000), 0x40, "write first: new dest");
    assert_eq!(b.peek_no_io(0x8000), 0x00);
}

/// HBlank DMA never proceeds while the core clock is gated: a block
/// flagged before HALT is deferred and re-flagged at wake, where it
/// copies without the teardown M-cycle (gambatte Memory::halt →
/// haltHdmaState_ = hdma_requested; video.h flagHdmaReq is suppressed
/// while halted; Memory::event intevent_dma: `cc -= 4` for the
/// deferred block).
#[test]
fn hblank_block_defers_while_core_clock_gated() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x80);
    // Stop on the tick that flags the block (the trigger leads the
    // hblank by one dot) so the clock gate lands before any bus op
    // can service the request.
    while !b.ppu.hdma_trigger_level() {
        b.tick();
    }
    b.set_cpu_halted(true);
    ticks(&mut b, 300); // crosses further hblanks: nothing copies
    assert_eq!(b.peek_no_io(0x8000), 0x00);
    assert_eq!(b.read_no_tick(0xFF55), 0x00, "still armed");
    b.set_cpu_halted(false); // wake re-flags the deferred block
    let before = b.cycles();
    b.tick(); // the steal heads this op
    assert_eq!(b.cycles() - before, (8 + 1) * 4, "no teardown cycle");
    assert_eq!(b.peek_no_io(0x8000), 0x40);
    assert_eq!(b.read_no_tick(0xFF55), 0xFF);
}

/// A halt that begins *outside* the hblank window fires a block on a
/// wake landing inside one; a halt that begins inside it does not
/// retrigger the same hblank (gambatte haltHdmaState_ low vs high).
#[test]
fn halt_wake_inside_hblank_window_fires_block_once() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x80);
    // Halt right after arming, before the first hblank (state Low).
    b.set_cpu_halted(true);
    while !b.ppu.hblank_active() {
        b.tick();
    }
    b.set_cpu_halted(false); // wake inside the window: block fires
    b.tick();
    assert_eq!(b.peek_no_io(0x8000), 0x40);
    // Re-arm inside the same hblank, halt, wake immediately: the halt
    // began inside the window (state High) — no retrigger.
    setup_gdma_regs(&mut b, 0xC000, 0x0010);
    assert!(b.ppu.hblank_active());
    b.write(0xFF55, 0x80);
    // (the enable itself fired a request: let it run, then re-halt)
    b.tick();
    assert_eq!(b.peek_no_io(0x8010), 0x40);
}

/// Disabling the display kills an armed HBlank transfer: FF55 keeps
/// reading "active" but no further block ever copies, even after the
/// display returns (gambatte video.cpp lcdcChange: the disable branch
/// parks every memevent, and only an armed-while-off transfer is
/// re-anchored by the enable branch).
#[test]
fn lcd_disable_kills_hblank_arming_but_not_ff55() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x81); // armed with the LCD on, before any hblank
    b.write(0xFF40, 0x11); // display off
    ticks(&mut b, 300);
    assert_eq!(b.peek_no_io(0x8000), 0x00, "arming died with the display");
    assert_eq!(b.read(0xFF55), 0x01, "FF55 reads active (stale)");
    b.write(0xFF40, 0x91); // re-enabling does not revive it
    ticks(&mut b, 500);
    assert_eq!(b.peek_no_io(0x8000), 0x00);
}

/// The pending-block × speed-switch matrix (gambatte Memory::stop):
/// entering double speed the request survives into the pause and the
/// gated service aborts the transfer with the count latched; leaving
/// double speed it is deferred and completes normally after the pause
/// (hdma_transition_speedchange_hdmalen*_hdma5 = $80|len vs
/// hdma_late_m3speedchange_hdma5_*_ds_1 = still active).
#[test]
fn speed_switch_aborts_pending_hblank_block_entering_double_speed() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF4D, 0x01); // arm first: any later bus op would
    b.write(0xFF55, 0x81); // service the request (2 blocks)
    while !b.ppu.hdma_trigger_level() {
        b.tick();
    }
    // The request flagged during the last tick is still pending when
    // STOP executes (gambatte: prefetched = hdmaReqFlagged).
    assert!(b.stop(0x0000, false));
    assert_eq!(b.peek_no_io(0x8000), 0x40, "the block still copied");
    assert_eq!(b.peek_no_io(0x800F), 0x4F);
    assert_eq!(b.read(0xFF55), 0x81, "aborted: bit 7 + armed count");
    ticks(&mut b, 300);
    assert_eq!(b.peek_no_io(0x8010), 0x00, "no further blocks");
}

#[test]
fn speed_switch_defers_pending_hblank_block_leaving_double_speed() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    assert!(b.stop(0x0000, true)); // enter double speed instantly
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF4D, 0x01); // arm first (see the abort test above)
    b.write(0xFF55, 0x81);
    while !b.ppu.hdma_trigger_level() {
        b.tick();
    }
    assert!(b.stop(0x0000, false)); // back to normal speed, with pause
    assert_eq!(b.read_no_tick(0xFF55), 0x01, "still active");
    assert_eq!(
        b.peek_no_io(0x8000),
        0x00,
        "block deferred across the pause"
    );
    b.tick();
    assert_eq!(b.peek_no_io(0x8000), 0x40);
    assert_eq!(b.read_no_tick(0xFF55), 0x00);
}

/// While a VRAM DMA owns the bus, a concurrently running OAM DMA keeps
/// advancing one position per M-cycle but performs no source reads of
/// its own: each advance latches the VRAM DMA's bus traffic instead,
/// writing the stolen byte to OAM[hdma_src & 0xFF] — the *address* the
/// VRAM DMA drove, not the OAM DMA's own position (gambatte-core
/// memory.cpp `dma()`: `ioamhram_[src & 0xFF] = data` once per 4 cc,
/// gated `cc - 3 > lOamDmaUpdate`, which at normal speed lands the
/// advance on the *second* byte of each 2-byte stolen M-cycle —
/// hardware-pinned by dma/hdma_transition_oamdma_1's 50 9E 52 9C and
/// oamdma/oamdmasrcC000_hdmasrc0000's single 94 capture).
#[test]
fn vram_dma_steal_advances_oam_dma_capturing_the_bus() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    for i in 0..0xA0 {
        b.write(0xFE00 + i, 0xF0);
    }
    setup_gdma_regs(&mut b, 0x1000, 0x0000); // ROM pattern i ^ 0x5A
    b.write(0xFF46, 0xC0); // cycle W: OAM DMA from WRAM
    ticks(&mut b, 5); // W+2..W+5 copy idx 0..3
    b.write(0xFF55, 0x00); // W+6 copies idx 4, then flags 1 GDMA block
    b.tick(); // steal: 8 M-cycles (idx 5..12 advance) + teardown (idx 13)
    for _ in 0..160 {
        b.tick(); // let the transfer finish
    }
    let rom = |i: u8| i ^ 0x5A;
    // Positions copied normally before the steal, even slots: kept.
    assert_eq!(b.peek_no_io(0xFE00), 0x50);
    assert_eq!(b.peek_no_io(0xFE02), 0x52);
    assert_eq!(b.peek_no_io(0xFE04), 0x54);
    // Captures land at OAM[src & 0xFF] of the second stolen byte of
    // each M-cycle — the odd HDMA source offsets — overwriting the
    // earlier normal copies of idx 1/3.
    assert_eq!(b.peek_no_io(0xFE01), rom(0x01), "capture over earlier copy");
    assert_eq!(b.peek_no_io(0xFE03), rom(0x03), "capture over earlier copy");
    // Positions 5..12 advanced during the steal without copying their
    // own source: odd ones hold captures, even ones keep the prefill.
    assert_eq!(b.peek_no_io(0xFE05), rom(0x05));
    assert_eq!(b.peek_no_io(0xFE07), rom(0x07));
    assert_eq!(b.peek_no_io(0xFE09), rom(0x09));
    assert_eq!(b.peek_no_io(0xFE0B), rom(0x0B));
    for i in [0x06u16, 0x08, 0x0A, 0x0C] {
        assert_eq!(b.peek_no_io(0xFE00 + i), 0xF0, "idx {i:#x} skipped");
    }
    // Captures at offsets 0x0D/0x0F are overwritten again by the
    // normal copies resuming at idx 13 (teardown cycle onward).
    assert_eq!(b.peek_no_io(0xFE0D), 0x5D);
    assert_eq!(b.peek_no_io(0xFE0F), 0x5F);
    assert_eq!(b.peek_no_io(0xFE10), 0x60);
    assert_eq!(b.peek_no_io(0xFE9F), 0xEF);
}

/// A captured bus byte whose address low byte is ≥ 0xA0 lands in the
/// CGB-C extra OAM RAM behind FEA0-FEFF, decoded with the same bits
/// 3-4 alias (gambatte memory.cpp dma(): `ioamhram_[p & 0xE7] = data`
/// for `p >= oam_size`, skipped on AGB).
#[test]
fn vram_dma_steal_capture_reaches_extra_oam_ram() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    setup_gdma_regs(&mut b, 0x10A0, 0x0000);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 5);
    b.write(0xFF55, 0x00);
    b.tick();
    for _ in 0..170 {
        b.tick(); // transfer done, OAM idle again
    }
    // Captures land at odd offsets 0xA1..0xAF; the bits-3/4 alias
    // folds 0xA9/0xAB onto the 0xA1/0xA3 cells, so the later capture
    // wins each cell.
    assert_eq!(b.read(0xFEA1), 0xA9 ^ 0x5A);
    assert_eq!(b.read(0xFEA3), 0xAB ^ 0x5A);
    assert_eq!(b.read(0xFEA9), 0xA9 ^ 0x5A, "bits 3-4 alias");
}

/// In double speed the VRAM DMA copies one byte per stolen M-cycle, so
/// *every* stolen byte advances the OAM DMA and is captured (gambatte
/// dma(): `cc += 2 + 2 * doubleSpeed` per byte vs the 4-cc advance
/// period).
#[test]
fn vram_dma_steal_captures_every_byte_in_double_speed() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    assert!(b.stop(0x0000, true)); // enter double speed instantly
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    for i in 0..0xA0 {
        b.write(0xFE00 + i, 0xF0);
    }
    setup_gdma_regs(&mut b, 0x1000, 0x0000);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 5);
    b.write(0xFF55, 0x00);
    b.tick(); // steal: 16 M-cycles, one advance + capture per byte
    for _ in 0..160 {
        b.tick();
    }
    // All 16 block offsets captured — including 0..=4, whose earlier
    // normal copies are overwritten; positions 5..=20 advanced during
    // the steal, so none of the captures is re-copied afterwards.
    for i in 0..16u16 {
        assert_eq!(b.peek_no_io(0xFE00 + i), (i as u8) ^ 0x5A, "offset {i:#x}");
    }
    // Positions 16..=20 advanced during the steal too: no capture
    // (the block only drove offsets 0..=15), no copy — prefill stays.
    for i in 16..21u16 {
        assert_eq!(b.peek_no_io(0xFE00 + i), 0xF0, "idx {i:#x} skipped");
    }
    assert_eq!(b.peek_no_io(0xFE15), 0x65, "normal copies resume at idx 21");
}

/// A block serviced while the core clock is gated (the speed-switch
/// pause) advances nothing: the OAM DMA controller is frozen with the
/// CPU (gambatte dma(): the advance is gated on `!halted()`).
#[test]
fn vram_dma_steal_does_not_advance_a_halt_frozen_oam_dma() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    for i in 0..0xA0 {
        b.write(0xFE00 + i, 0xF0);
    }
    setup_gdma_regs(&mut b, 0x1000, 0x0000);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 5); // idx 0..3 copied
    b.set_cpu_halted(true);
    b.vram_dma_req = Some(VramDmaReq::Gdma);
    b.run_vram_dma();
    assert_eq!(b.peek_no_io(0xFE01), 0x51, "no capture while frozen");
    assert_eq!(b.peek_no_io(0xFE05), 0xF0, "no position consumed");
    assert_eq!(b.dma_run.unwrap().idx, 4, "frozen position kept");
    b.set_cpu_halted(false);
    ticks(&mut b, 170);
    assert_eq!(b.peek_no_io(0xFE05), 0x55, "transfer resumed normally");
    assert_eq!(b.peek_no_io(0xFE9F), 0xEF);
}

/// The OAM DMA setup delay keeps counting during a steal: the start
/// promotion happens on a stolen advance, which captures instead of
/// copying byte 0 (gambatte dma(): `if (oamDmaPos_ == oamDmaStartPos_)
/// startOamDma(...)` inside the steal loop).
#[test]
fn vram_dma_steal_counts_oam_dma_startup_delay() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    for i in 0..0xA0 {
        b.write(0xFE00 + i, 0xF0);
    }
    setup_gdma_regs(&mut b, 0x1000, 0x0000);
    b.write(0xFF46, 0xC0); // cycle W: delay = 1 at commit
    b.write(0xFF55, 0x00); // W+1 ticks delay to 0, then flags the GDMA
    b.tick(); // steal precedes this cycle: the start promotes inside it
    for _ in 0..170 {
        b.tick();
    }
    // Steal advance 1 (2nd stolen byte, offset 1): promote, idx 0
    // consumed by the capture at OAM[1]. Advances 2..8: idx 1..7
    // consumed, captures at offsets 3/5/7/9/B/D/F. Normal copies
    // resume at idx 8 (teardown cycle), overwriting captures 9/B/D/F.
    assert_eq!(b.peek_no_io(0xFE00), 0xF0, "byte 0's copy was stolen");
    assert_eq!(b.peek_no_io(0xFE01), 0x01 ^ 0x5A, "capture during promote");
    assert_eq!(b.peek_no_io(0xFE03), 0x03 ^ 0x5A);
    assert_eq!(b.peek_no_io(0xFE02), 0xF0, "idx 2 skipped (capture at 3)");
    assert_eq!(b.peek_no_io(0xFE07), 0x07 ^ 0x5A);
    assert_eq!(b.peek_no_io(0xFE08), 0x58, "normal copies resume at idx 8");
    assert_eq!(b.peek_no_io(0xFE09), 0x59, "capture at 9 re-copied");
}
