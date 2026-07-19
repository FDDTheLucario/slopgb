//! GP-DMA tests: the host-side engine ($420B / $43x0-$43x6), the WRAM
//! B-bus access ports, and the plugin stall handshake making transfers
//! atomic under the polled capture ring.

use super::*;

// ---- GP-DMA ($420B / $43x0-$43x6) + the WRAM B-bus port ($2180-$2183) ----

/// End to end through the real wasm CPU: a guest program points WMADD at
/// WRAM, configures channel 0 (A→B, increment, one byte to one register —
/// fullsnes 43x0h mode 0), fires MDMAEN, and the bytes staged in the
/// program area land in WRAM through the $2180 WMDATA port. The stall
/// handshake makes the transfer atomic: the post-trigger marker instruction
/// runs only after the host executed the DMA.
#[test]
fn gp_dma_copies_program_area_to_wram_via_wmdata() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9840, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
        let prog = stores(
            &[
                (0x2181, 0x00), // WMADD = $001000 -> $7E:1000
                (0x2182, 0x10),
                (0x2183, 0x00),
                (0x4300, 0x00), // DMAP0: A->B, increment, unit mode 0
                (0x4301, 0x80), // BBAD0: $2180 (WMDATA)
                (0x4302, 0x40), // A1T0 = $9840
                (0x4303, 0x98),
                (0x4304, 0x00), // A1B0 = bank $00
                (0x4305, 0x04), // DAS0 = 4 bytes
                (0x4306, 0x00),
                (0x420B, 0x01), // MDMAEN: start channel 0
                (0x0450, 0xA5), // marker: runs only after the DMA service
            ],
            &[0xDB], // STP
        );
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(4096 * 4);

    assert_eq!(
        cop.debug_cpu_ram(0x7E_1000, 4),
        vec![0xDE, 0xAD, 0xBE, 0xEF],
        "the staged bytes crossed to WRAM through WMDATA"
    );
    assert_eq!(
        cop.debug_cpu_ram(0x0450, 1),
        vec![0xA5],
        "the CPU resumed after the host serviced the stall"
    );
    assert_eq!(cop.wmadd, 0x1004, "WMADD auto-incremented per byte");
    assert_eq!(
        cop.dma_regs[0],
        [0x00, 0x80, 0x44, 0x98, 0x00, 0x00, 0x00],
        "working registers end stepped: A1T final, DAS = 0 (fullsnes 43x5h)"
    );
}

/// Direction bit 7 (B→A) + multi-channel order: both channels read the same
/// WMDATA stream, so channel 0 receiving the first bytes and channel 1 the
/// following ones pins the 0-first-through-7-last order (fullsnes: "executed
/// in order channel 0=first through 7=last"). The RAM-backed program area
/// stands in for a writable A-bus destination.
#[test]
fn gp_dma_b_to_a_direction_and_channel_order() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x7E_3000, &[1, 2, 3, 4, 5, 6]).unwrap();
        let prog = stores(
            &[
                (0x2181, 0x00), // WMADD = $003000 -> $7E:3000
                (0x2182, 0x30),
                (0x2183, 0x00),
                (0x4300, 0x80), // DMAP0: B->A, increment, unit mode 0
                (0x4301, 0x80),
                (0x4302, 0x00), // A1T0 = $9900
                (0x4303, 0x99),
                (0x4304, 0x00),
                (0x4305, 0x03),
                (0x4306, 0x00),
                (0x4310, 0x80), // DMAP1: B->A, increment, unit mode 0
                (0x4311, 0x80),
                (0x4312, 0x00), // A1T1 = $9A00
                (0x4313, 0x9A),
                (0x4314, 0x00),
                (0x4315, 0x03),
                (0x4316, 0x00),
                (0x420B, 0x03), // MDMAEN: channels 0 + 1
            ],
            &[0xDB],
        );
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(4096 * 4);

    assert_eq!(
        cop.debug_cpu_ram(0x9900, 3),
        vec![1, 2, 3],
        "channel 0 drew the stream head"
    );
    assert_eq!(
        cop.debug_cpu_ram(0x9A00, 3),
        vec![4, 5, 6],
        "channel 1 ran after channel 0"
    );
    assert_eq!(cop.wmadd, 0x3006);
}

/// WRAM-to-WRAM DMA is impossible in either direction (fullsnes 2183h "DMA
/// Notes"): with WMDATA on the B-bus and a WRAM A-bus address, the byte
/// movement is suppressed — but the transfer still completes (counters run,
/// the CPU resumes) rather than hanging.
#[test]
fn gp_dma_wram_to_wram_is_suppressed() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x7E_4000, &[0x11, 0x22]).unwrap();
        cpu.write_ram(0x7E_1000, &[0xAA, 0xAA]).unwrap();
        let prog = stores(
            &[
                (0x2181, 0x00), // WMADD -> $7E:1000
                (0x2182, 0x10),
                (0x2183, 0x00),
                (0x4300, 0x00),
                (0x4301, 0x80),
                (0x4302, 0x00), // A1T0 = $7E:4000 (WRAM source)
                (0x4303, 0x40),
                (0x4304, 0x7E),
                (0x4305, 0x02),
                (0x4306, 0x00),
                (0x420B, 0x01),
                (0x0450, 0xA5),
            ],
            &[0xDB],
        );
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(4096 * 4);

    assert_eq!(
        cop.debug_cpu_ram(0x7E_1000, 2),
        vec![0xAA, 0xAA],
        "no bytes moved"
    );
    assert_eq!(cop.debug_cpu_ram(0x0450, 1), vec![0xA5], "no hang");
    assert_eq!(
        cop.dma_regs[0][2..],
        [0x02, 0x40, 0x7E, 0x00, 0x00],
        "the counters still ran to completion"
    );
}

/// Unit patterns + A-bus stepping, driven host-side (the engine is
/// host-observable through WMADD/WRAM): mode 1 alternates B-bus `xx, xx+1`
/// (fullsnes 43x0h table), a fixed A-bus step replicates one source byte
/// (the memfill idiom), and a decrementing step reverses the source.
#[test]
fn gp_dma_unit_patterns_and_a_bus_steps() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    // Mode 1 with BBAD=$82: bytes alternate WMADDM/WMADDH — the final WMADD
    // exposes that the 4-byte transfer walked $2182,$2183,$2182,$2183.
    cop.cpu
        .get_mut()
        .write_ram(0x9840, &[0x34, 0x01, 0x56, 0x00])
        .unwrap();
    cop.dma_regs[2] = [0x01, 0x82, 0x40, 0x98, 0x00, 0x04, 0x00];
    cop.run_gp_dma(0x04);
    assert_eq!(cop.wmadd, 0x5600, "mode 1 alternated the two ports");

    // Fixed A-bus step (DMAP bits 4-3 = 01): one source byte fans out.
    cop.cpu.get_mut().write_ram(0x9850, &[0x77]).unwrap();
    cop.wmadd = 0x2000;
    cop.dma_regs[3] = [0x08, 0x80, 0x50, 0x98, 0x00, 0x03, 0x00];
    cop.run_gp_dma(0x08);
    assert_eq!(
        cop.debug_cpu_ram(0x7E_2000, 3),
        vec![0x77, 0x77, 0x77],
        "fixed-source memfill"
    );
    assert_eq!(
        cop.dma_regs[3][2..4],
        [0x50, 0x98],
        "a fixed A-bus address does not step"
    );

    // Mode 3 (xx, xx, xx+1, xx+1 — fullsnes 43x0h) with BBAD=$80: two bytes
    // land in WRAM through WMDATA, then two overwrite WMADDL — walking a
    // 4-entry unit across both ports.
    cop.cpu
        .get_mut()
        .write_ram(0x9870, &[0x61, 0x62, 0x44, 0x55])
        .unwrap();
    cop.wmadd = 0x2080;
    cop.dma_regs[4] = [0x03, 0x80, 0x70, 0x98, 0x00, 0x04, 0x00];
    cop.run_gp_dma(0x10);
    assert_eq!(
        cop.debug_cpu_ram(0x7E_2080, 2),
        vec![0x61, 0x62],
        "unit bytes 0-1 hit WMDATA"
    );
    assert_eq!(
        cop.wmadd, 0x2055,
        "unit bytes 2-3 hit WMADDL after the two data bytes stepped it"
    );

    // Decrementing A-bus step (bits 4-3 = 10): the source reads backwards.
    cop.cpu
        .get_mut()
        .write_ram(0x9860, &[0x0A, 0x0B, 0x0C])
        .unwrap();
    cop.wmadd = 0x2100;
    cop.dma_regs[1] = [0x10, 0x80, 0x62, 0x98, 0x00, 0x03, 0x00];
    cop.run_gp_dma(0x02);
    assert_eq!(
        cop.debug_cpu_ram(0x7E_2100, 3),
        vec![0x0C, 0x0B, 0x0A],
        "decrement walked $9862 down to $9860"
    );
    assert_eq!(cop.dma_regs[1][2..4], [0x5F, 0x98], "A1T stepped down");
}

/// DAS = 0 means $10000 bytes (fullsnes 43x5h/43x6h), not zero: a fixed-
/// source 64 KB fill covers half of WRAM and leaves WMADD at $10000.
#[test]
fn gp_dma_das_zero_means_64k() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    cop.cpu.get_mut().write_ram(0x9850, &[0x3C]).unwrap();
    cop.dma_regs[0] = [0x08, 0x80, 0x50, 0x98, 0x00, 0x00, 0x00];
    cop.run_gp_dma(0x01);
    assert_eq!(cop.wmadd, 0x1_0000);
    assert_eq!(cop.debug_cpu_ram(0x7E_0000, 1), vec![0x3C]);
    assert_eq!(cop.debug_cpu_ram(0x7E_FFFF, 1), vec![0x3C]);
    assert_eq!(cop.debug_cpu_ram(0x7F_0000, 1), vec![0], "stopped at 64 K");
}

/// A GP-DMA sourcing the ICD2 `$7800` character port reads through the
/// device — its per-read auto-increment advances the row — so a streamed
/// GB screen band lands byte-exact (here into WRAM through the `$2180`
/// WMDATA port; the takeover DMAs the same source into VRAM).
#[test]
fn dma_from_the_char_port_streams_the_loaded_row() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    let mut row = [0u8; 320];
    for (i, b) in row.iter_mut().enumerate() {
        *b = (i as u8) ^ 0x5A;
    }
    cop.cpu.get_mut().write_ram(HW_CHAR_ROWS, &row).unwrap();
    let prog = stores(
        &[
            (0x2181, 0x00), // WMADD = $003000 (WRAM $7E:3000)
            (0x2182, 0x30),
            (0x2183, 0x00),
            (0x4300, 0x00), // A->B, increment, mode 0
            (0x4301, 0x80), // BBAD $2180 (WMDATA)
            (0x4302, 0x00), // A1T = $7800
            (0x4303, 0x78),
            (0x4304, 0x00), // bank 0
            (0x4305, 0x10), // 16 bytes
            (0x4306, 0x00),
            (0x420B, 0x01),
        ],
        &[0xDB],
    );
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224);
    assert_eq!(
        cpu_ram(&cop, 0x7E_3000, 16),
        row[..16].to_vec(),
        "the char row streamed through the device read path"
    );
}
