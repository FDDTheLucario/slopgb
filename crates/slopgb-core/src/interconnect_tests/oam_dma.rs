//! `interconnect_tests` — oam_dma tests (split for file size).

use super::*;

#[test]
fn oam_dma_setup_cycle_leaves_oam_accessible() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0); // cycle W
    // Cycle W+1: setup delay, OAM still reads its old content
    // (oam_dma_start executes an opcode from OAM here).
    assert_eq!(b.read(0xFE00), 0x00);
    // Cycle W+2: byte 0 is in flight, OAM reads $FF.
    assert_eq!(b.read(0xFE00), 0xFF);
}

/// acceptance/oam_dma_timing: OAM unlocks exactly 162 M-cycles after
/// the FF46 write cycle (1 setup + 160 transfer + the access cycle).
#[test]
fn oam_dma_timing_exact() {
    for (extra, expected) in [(0u32, 0xFF), (1, 0x80)] {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0);
        ticks(&mut b, 160 + extra);
        assert_eq!(b.read(0xFE00), expected, "extra={extra}");
    }
}

#[test]
fn oam_dma_copies_all_160_bytes() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE00), 0x80);
    assert_eq!(b.read(0xFE9F), 0x80u8.wrapping_add(159));
}

/// The PPU's OAM view disconnects while the OAM DMA controller owns
/// OAM — for the dots of cycles W+3 .. W+162 around an FF46 write at
/// cycle W: gambatte memory.cpp timestamps startOamDma at the byte-0
/// copy step (the end of our W+2) and endOamDma one step past byte
/// 159 (the end of our W+162), and the OamReader latches real OAM up
/// to each timestamp. 160 disconnected M-cycles total; the gambatte
/// oamdma/late_sp* `_1`/`_2` pairs pin both edges at M-cycle
/// granularity per scanned sprite slot.
#[test]
fn oam_dma_disconnects_ppu_scan_for_160_cycles() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0); // cycle W
    b.tick();
    assert!(!b.ppu.oam_dma_scan_disconnected(), "W+1: setup delay");
    b.tick();
    assert!(
        !b.ppu.oam_dma_scan_disconnected(),
        "W+2: byte 0 lands at the cycle's end"
    );
    for k in 3..163 {
        b.tick();
        assert!(b.ppu.oam_dma_scan_disconnected(), "W+{k}");
    }
    b.tick();
    assert!(!b.ppu.oam_dma_scan_disconnected(), "W+163: reconnected");
}

/// HALT gates the controller's clock mid-transfer: the disconnect
/// level persists for the whole freeze (gambatte updateOamDma's
/// halted() path advances no position and never reaches endOamDma —
/// the OamReader source stays rdisabledRam; dmg08-verified by
/// gambatte oamdma_late_halt_stat_1/_2).
#[test]
fn oam_dma_disconnect_persists_through_halt_freeze() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 10);
    assert!(b.ppu.oam_dma_scan_disconnected());
    b.set_cpu_halted(true);
    ticks(&mut b, 400); // far past the un-frozen end of the transfer
    assert!(
        b.ppu.oam_dma_scan_disconnected(),
        "frozen transfer still owns OAM"
    );
    b.set_cpu_halted(false);
    // The remaining ~152 bytes finish after the wake; then reconnect.
    ticks(&mut b, 160);
    assert!(!b.ppu.oam_dma_scan_disconnected());
}

#[test]
fn oam_dma_reg_reads_back_last_write() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF46, 0x90);
    assert_eq!(b.read(0xFF46), 0x90);
    b.write(0xFF46, 0x8F); // restart mid-transfer
    assert_eq!(b.read(0xFF46), 0x8F);
}

/// acceptance/oam_dma/sources-GS: source pages $E0-$FF re-read WRAM,
/// including $FE/$FF -> $DE00/$DF00.
#[test]
fn oam_dma_high_sources_read_wram_echo() {
    for (page, base) in [(0xE0u8, 0x80u8), (0xFE, 0x21), (0xFF, 0x42)] {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        fill_wram(&mut b, 0xDE00, 0x21, 0x100);
        fill_wram(&mut b, 0xDF00, 0x42, 0x100);
        b.write(0xFF46, page);
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFE00), base, "page {page:02X}");
        assert_eq!(b.read(0xFE01), base + 1, "page {page:02X}");
    }
}

#[test]
fn oam_dma_from_rom_and_vram() {
    let mut b = ic(Model::Dmg);
    b.write(0x9000, 0x77); // LCD off: VRAM writable
    b.write(0xFF46, 0x10); // ROM pattern page
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE00), 0x5A);
    b.write(0xFF46, 0x90);
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE00), 0x77);
}

#[test]
fn oam_writes_dropped_and_reads_ff_during_dma() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    b.tick(); // setup
    b.write(0xFE10, 0x99); // transfer running: dropped
    assert_eq!(b.read(0xFEA0), 0xFF); // prohibited area also $FF
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE10), 0x90, "DMA value, not the CPU write");
}

/// gbctr bus conflicts: a CPU read on the bus the DMA is using returns
/// the byte the DMA is transferring; the other bus is unaffected.
/// (Write at cycle W; byte i is in flight at cycle W+2+i, so reads at
/// W+3, W+4, ... observe bytes 1, 2, ...)
#[test]
fn oam_dma_bus_conflicts() {
    // ROM source (external bus): ROM/WRAM reads conflict on DMG, VRAM
    // reads do not.
    let mut b = ic(Model::Dmg);
    b.write(0x8500, 0x33);
    b.write(0xFF46, 0x10); // cycle W
    b.tick(); // W+1 setup
    b.tick(); // W+2: byte 0 in flight
    assert_eq!(b.read(0x4242), 0x5A ^ 1, "ROM read sees DMA byte 1");
    assert_eq!(b.read(0xC000), 0x5A ^ 2, "DMG WRAM shares the bus");
    assert_eq!(b.read(0x8500), 0x33, "VRAM bus unaffected");

    // VRAM source: external bus unaffected.
    let mut b = ic(Model::Dmg);
    b.write(0x8000, 0x44);
    b.write(0x8001, 0x45);
    b.write(0xFF46, 0x80);
    b.tick();
    b.tick();
    assert_eq!(b.read(0x9999), 0x45, "VRAM read sees DMA byte 1");
    assert_eq!(b.read(0x1000), 0x5A, "external bus unaffected");
}

/// The OAM DMA controller runs on the CPU core clock, which HALT gates
/// off (the PPU keeps its own clock): a transfer in progress does not
/// proceed while the CPU is halted. Bytes already copied stay, the byte
/// in flight never commits, the rest of OAM keeps its old contents, and
/// the transfer resumes exactly where it stopped when the CPU wakes.
/// Hardware-verified by madness/mgb_oam_dma_halt_sprites.s: halting
/// after the third byte's read leaves that OAM byte un-replaced, and the
/// PPU renders from the old/new mixture indefinitely.
#[test]
fn oam_dma_freezes_while_cpu_halted() {
    let mut b = ic(Model::Mgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write_no_tick(0xFE02, 0x30); // old OAM byte the freeze must keep
    b.write(0xFF46, 0xC0); // cycle W
    b.tick(); // W+1: setup delay
    b.tick(); // W+2: byte 0 in flight
    b.tick(); // W+3: byte 1 in flight
    b.set_cpu_halted(true);
    // Frozen for hundreds of M-cycles: no progress. (On hardware the
    // halted CPU performs no bus accesses, so these reads observe
    // unobservable state: raw OAM, no bus conflict — LCD is off here.)
    for _ in 0..200 {
        assert_eq!(b.read(0xFE00), 0x80, "copied byte 0 stays");
    }
    assert_eq!(b.read(0xFE01), 0x81, "copied byte 1 stays");
    assert_eq!(b.read(0xFE02), 0x30, "frozen: old OAM byte persists");
    assert_eq!(b.read(0xC000), 0x80, "no DMA traffic on the external bus");
    // Waking copies byte 2 in the release's catch-up cycle (see
    // `halt_wake_advances_oam_dma_one_catchup_cycle`); 157 transfer
    // cycles remain after it.
    b.set_cpu_halted(false);
    ticks(&mut b, 156);
    assert_eq!(b.read(0xFE00), 0xFF, "byte 159 in flight: OAM blocked");
    assert_eq!(b.read(0xFE00), 0x80, "transfer complete");
    assert_eq!(b.read(0xFE02), 0x82, "resumed transfer replaced the byte");
    assert_eq!(b.read(0xFE9F), 0x80u8.wrapping_add(159));
}

/// Releasing the core-clock gate advances a frozen OAM DMA by one
/// catch-up M-cycle *at the release itself*, before the CPU's first
/// post-wake cycle: the controller's clock restarts with the halt
/// exit, one M-cycle ahead of the CPU pipeline (SameBoy sm83_cpu.c
/// `GB_cpu_run` halt exit: `gb->dma_cycles = 4; GB_dma_run(gb)` on
/// both the IME=0 resume and the dispatch path, while `GB_dma_run`
/// itself returns early whenever `gb->halted`). Hardware-pinned by
/// gambatte oamdma/oamdmasrc80_halt_lycirq_read8000 /
/// _m2irq_read8000 (out81, both models), dma/hdma_transition_oamdma_2
/// (out67) and dma/hdma_transition_speedchange_oamdma (out71), all of
/// which observe the in-flight source index after a wake.
#[test]
fn halt_wake_advances_oam_dma_one_catchup_cycle() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    b.write(0xFF46, 0xC0); // cycle W
    ticks(&mut b, 6); // W+2..W+6 copy idx 0..4
    b.set_cpu_halted(true);
    ticks(&mut b, 50);
    assert_eq!(b.peek_no_io(0xFE05), 0x00, "frozen");
    b.set_cpu_halted(false);
    assert_eq!(b.peek_no_io(0xFE05), 0x55, "catch-up copy at the gate release");
    assert_eq!(b.peek_no_io(0xFE06), 0x00, "exactly one cycle of catch-up");
    b.tick(); // copies idx 6, committing at the next cycle's head
    b.tick();
    assert_eq!(b.peek_no_io(0xFE06), 0x56);
}

/// The speed-switch pause releases the same core-clock gate but
/// performs *no* catch-up cycle: the next OAM DMA byte copies on the
/// first machine cycle after the pause, not at the release (gambatte
/// oamdma/oamdmasrcC0_speedchange_readC000 out11 pins the exact
/// post-pause in-flight index, one position below a caught-up resume;
/// SameBoy's `speed_switch_halt_countdown` expiry likewise just clears
/// `halted` with no `GB_dma_run` call, unlike its halt-exit paths).
#[test]
fn speed_switch_pause_exit_does_not_catch_up_oam_dma() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    b.write(0xFF4D, 0x01); // arm the switch
    b.write(0xFF46, 0xC0); // cycle W
    ticks(&mut b, 6); // W+2..W+6 copy idx 0..4
    assert!(b.stop(0x0000, false)); // read cycle copies idx 5, then pause
    assert_eq!(b.peek_no_io(0xFE05), 0x55);
    assert_eq!(b.peek_no_io(0xFE06), 0x00, "frozen across the pause, no catch-up");
    b.tick(); // copies idx 6: the first post-pause cycle...
    b.tick(); // ...committing at the next cycle's head
    assert_eq!(
        b.peek_no_io(0xFE06),
        0x56,
        "resumes on the first post-pause cycle"
    );
}

/// The FF46 1 M-cycle setup delay counts on the same gated clock, so a
/// CPU halting right after the FF46 write freezes the transfer before
/// its first byte (companion to `oam_dma_freezes_while_cpu_halted`).
#[test]
fn oam_dma_setup_delay_freezes_while_cpu_halted() {
    let mut b = ic(Model::Mgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    b.set_cpu_halted(true);
    for _ in 0..10 {
        assert_eq!(b.read(0xFE00), 0x00, "setup delay frozen: no transfer");
    }
    // The release's catch-up cycle elapses the setup delay; the next
    // cycle copies byte 0.
    b.set_cpu_halted(false);
    assert_eq!(b.read(0xFE00), 0xFF, "byte 0 in flight");
    ticks(&mut b, 159);
    assert_eq!(b.read(0xFE00), 0x80, "transfer complete");
}

/// Gating the clock mid-transfer hands the PPU the frozen in-flight
/// access (OAM index + source byte) for the MGB OAM scan glitch
/// (madness/mgb_oam_dma_halt_sprites.s); ungating (or freezing with no
/// transfer / only the setup delay in flight) hands over nothing.
#[test]
fn cpu_halt_hands_frozen_dma_access_to_ppu() {
    let mut b = ic(Model::Mgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.set_cpu_halted(true);
    assert_eq!(b.ppu.oam_dma_freeze(), None, "no transfer running");
    b.set_cpu_halted(false);
    b.write(0xFF46, 0xC0); // cycle W
    b.set_cpu_halted(true);
    assert_eq!(b.ppu.oam_dma_freeze(), None, "setup delay: no OAM access");
    b.set_cpu_halted(false); // catch-up cycle: setup delay elapses
    b.tick(); // byte 0 in flight
    b.tick(); // byte 1 in flight
    b.set_cpu_halted(true);
    assert_eq!(
        b.ppu.oam_dma_freeze(),
        Some((2, 0x82)),
        "byte 2 frozen mid-access"
    );
    b.set_cpu_halted(false);
    assert_eq!(b.ppu.oam_dma_freeze(), None, "cleared on wake");
}

/// DMG: a CPU write on pages the running transfer occupies derails
/// into the in-flight OAM slot (pure CPU byte for a ROM source) and
/// never reaches the addressed memory
/// (oamdma_src0000_busypushC001_dmg08_out55AA1234: both pushed bytes
/// land in OAM $9D/$9E, the WRAM/SRAM marker bytes survive).
#[test]
fn dmg_conflicted_write_lands_in_oam_slot_not_memory() {
    let mut b = ic(Model::Dmg);
    b.write_no_tick(0xC050, 0x34); // marker
    b.write(0xFF46, 0x10); // ROM source, cycle W
    b.tick(); // W+1 setup
    b.tick(); // W+2: byte 0 in flight
    // Cycle W+3: byte 1 (ROM $1001 = $5B) is in flight; the WRAM write
    // is on the conflicting external bus.
    b.write(0xC050, 0xAA);
    ticks(&mut b, 165); // run the transfer out
    assert_eq!(b.read(0xFE01), 0xAA, "CPU byte replaced DMA byte 1");
    assert_eq!(b.read(0xFE02), 0x58, "byte 2 unmolested (ROM $1002)");
    assert_eq!(b.read(0xC050), 0x34, "memory write suppressed");
}

/// DMG WRAM-source conflict wire-ANDs the CPU byte into the in-flight
/// byte (oamdma_srcC000_busypushC001_dmg08_out45221234: $65&$55=$45,
/// $76&$AA=$22).
#[test]
fn dmg_wram_source_write_conflict_is_wired_and() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    b.tick();
    b.tick();
    b.write(0x4000, 0x55); // ROM page: same external bus on DMG
    ticks(&mut b, 165);
    assert_eq!(b.read(0xFE01), 0x81 & 0x55, "wired-AND of DMA and CPU byte");
}

/// CGB VRAM-source conflicts: a conflicted write puts $00 in the slot
/// (oamdma_src8000_busypush8001_cgb04c_out00761234), and a conflicted
/// read returns the in-flight byte but zeroes the OAM slot afterwards
/// (gambatte memory.cpp nontrivial_read: `ioamhram_[oamDmaPos_] = 0`
/// for vram sources). DMG keeps the pure CPU byte on writes
/// (src8000_busypush8001_dmg08_out55761234).
#[test]
fn cgb_vram_source_conflicts_zero_oam() {
    for (model, expect_w) in [(Model::Cgb, 0x00), (Model::Dmg, 0x55)] {
        let mut b = ic(model);
        b.write(0x8000, 0x44);
        b.write(0x8001, 0x45);
        b.write(0x8002, 0x46);
        b.write(0xFF46, 0x80);
        b.tick();
        b.tick(); // byte 0 in flight
        b.write(0x9123, 0x55); // byte 1 cycle: VRAM-bus write conflict
        assert_eq!(b.read(0x9456), 0x46, "byte 2 cycle: conflicted read");
        ticks(&mut b, 162);
        assert_eq!(b.read(0xFE01), expect_w, "{model:?}: write conflict");
        let expect_r = if model.is_cgb() { 0x00 } else { 0x46 };
        assert_eq!(b.read(0xFE02), expect_r, "{model:?}: read zeroes slot");
    }
}

/// CGB: ROM/SRAM-source transfers conflict with the WRAM pages too,
/// but accesses there are redirected to WRAM bank 0 / the banked page
/// (selected by FF46 bit 4) at offset `addr & 0xFFF` — they never
/// touch OAM (oamdma_src0000_busypopDFFF_cgb04c_out657655AA: a $DFFF
/// read mid-transfer returns WRAM0[$FFF];
/// oamdma_srcE000_busypushC001_cgb04c_outFFAA1255: the $C000 write
/// lands in WRAM0[0], read back as $55 post-DMA).
#[test]
fn cgb_conflict_wram_access_redirects_to_ff46_bank() {
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write_no_tick(0xCFFF, 0x21);
    b.write_no_tick(0xDFFF, 0x43);
    b.write(0xFF46, 0x00); // ROM source, FF46 bit 4 = 0
    b.tick();
    b.tick();
    assert_eq!(b.read(0xDFFF), 0x21, "read redirected to WRAM0[$FFF]");
    b.write(0xD123, 0x99); // redirected to WRAM0[$123]
    ticks(&mut b, 162);
    assert_eq!(b.read(0xC123), 0x99, "write landed in WRAM bank 0");
    assert_eq!(b.read(0xD123), 0x00, "addressed cell untouched");
    assert_eq!(b.read(0xFE02), 0x00, "OAM untouched by the redirect");

    // FF46 bit 4 set: the banked page is addressed instead.
    let mut b = ic(Model::Cgb);
    b.write_no_tick(0xD456, 0x77);
    b.write(0xFF46, 0x10); // ROM source, bit 4 = 1
    b.tick();
    b.tick();
    assert_eq!(b.read(0xC456), 0x77, "read redirected to banked WRAM page");
}

/// CGB WRAM-source transfers conflict only with the WRAM pages, and
/// CPU writes there are swallowed entirely
/// (oamdma_srcC000_busypushE001_cgb04c_out65761234: markers intact,
/// OAM untouched).
#[test]
fn cgb_wram_source_wram_write_swallowed() {
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write_no_tick(0xC050, 0x34);
    b.write(0xFF46, 0xC0);
    b.tick();
    b.tick();
    b.write(0xC050, 0xAA);
    ticks(&mut b, 165);
    assert_eq!(b.read(0xFE01), 0x81, "OAM untouched");
    assert_eq!(b.read(0xC050), 0x34, "write swallowed");
}

/// CGB: FF46 ≥ $E0 is an invalid source — the engine reads $FF
/// (gambatte memory.cpp oamDmaSrcPtr → rdisabledRam; every
/// srcE000/EF00/F000/FE00/FF00 cgb04c expectation shows $FF OAM
/// bytes) while conflicting like a ROM source
/// (srcE000_busypush8001_cgb04c_outFFAA1255). DMG keeps the WRAM echo
/// (mooneye sources-GS, `oam_dma_high_sources_read_wram_echo`).
#[test]
fn cgb_high_sources_read_ff_and_conflict() {
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xE0);
    b.tick();
    b.tick(); // byte 0 in flight
    assert_eq!(b.read(0x4000), 0xFF, "ROM page read sees the $FF byte");
    b.write(0x4000, 0xAA); // conflicted write lands in the OAM slot
    ticks(&mut b, 162);
    assert_eq!(b.read(0xFE00), 0xFF);
    assert_eq!(b.read(0xFE02), 0xAA, "CPU byte in slot 2");
    assert_eq!(b.read(0xFE9F), 0xFF);
}

/// Restarting a transfer retargets the in-flight run immediately: the
/// handover copies before the new transfer's byte 0 read from the NEW
/// source at the old indices (gambatte memory.cpp FF46 handler updates
/// ioamhram_[0x146] + oamDmaInitSetup before the next copy;
/// hardware-pinned by oamdma_src8000_srcchange0000_busyread0000_1/2.
/// mooneye oam_dma_restart restarts with the same page and cannot
/// discriminate).
#[test]
fn oam_dma_restart_handover_copies_from_new_source() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160); // old source
    fill_wram(&mut b, 0xD000, 0x10, 160); // new source
    b.write(0xFF46, 0xC0); // cycle W
    b.tick(); // W+1 setup
    b.tick(); // W+2 old byte 0
    b.write(0xFF46, 0xD0); // cycle W+3: old byte 1 copied, then retarget
    // Cycle W+4 (new setup): the handover copy reads the NEW source at
    // the old index 2. Observe it through the external-bus conflict.
    assert_eq!(b.read(0x0000), 0x12, "handover byte came from $D002");
    // Cycle W+5: new transfer byte 0.
    assert_eq!(b.read(0x0000), 0x10);
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE00), 0x10);
    assert_eq!(b.read(0xFE05), 0x15);
}
