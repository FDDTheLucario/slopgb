//! `tests` — core tests (split for file size).

use super::*;

#[test]
fn register_readback_masks_after_writing_zero() {
    for (addr, mask) in MASKS {
        let mut h = H::dmg();
        h.w(addr, 0x00);
        assert_eq!(h.r(addr), mask, "addr {addr:04X}");
    }
}

#[test]
fn register_readback_all_ones_after_writing_ff() {
    for (addr, _) in MASKS {
        let mut h = H::dmg();
        h.w(addr, 0xFF);
        assert_eq!(h.r(addr), 0xFF, "addr {addr:04X}");
    }
}

#[test]
fn unmapped_ff27_to_ff2f_read_ff_and_ignore_writes() {
    let mut h = H::dmg();
    for addr in 0xFF27..=0xFF2F {
        h.w(addr, 0x00);
        assert_eq!(h.r(addr), 0xFF, "addr {addr:04X}");
    }
}

#[test]
fn nr52_reads_70_plus_power_and_status() {
    let mut h = H::dmg();
    assert_eq!(h.r(0xFF26), 0xF0); // powered on, no channels
    h.start_ch1();
    assert_eq!(h.r(0xFF26), 0xF1);
    h.w(0xFF26, 0x00);
    assert_eq!(h.r(0xFF26), 0x70);
    h.w(0xFF26, 0xFF); // only bit 7 is writable
    assert_eq!(h.r(0xFF26), 0xF0);
}

#[test]
fn wave_ram_round_trips_while_channel_off() {
    let mut h = H::dmg();
    for i in 0..16u16 {
        h.w(0xFF30 + i, (i as u8) << 4 | 0x0A);
    }
    for i in 0..16u16 {
        assert_eq!(h.r(0xFF30 + i), (i as u8) << 4 | 0x0A);
    }
}

#[test]
fn power_off_clears_all_registers() {
    let mut h = H::dmg();
    for (addr, _) in MASKS {
        h.w(addr, 0xFF);
    }
    h.w(0xFF26, 0x00);
    h.w(0xFF26, 0x80);
    for (addr, mask) in MASKS {
        assert_eq!(h.r(addr), mask, "addr {addr:04X}");
    }
}

#[test]
fn writes_ignored_while_powered_off() {
    let mut h = H::dmg();
    h.w(0xFF26, 0x00);
    h.w(0xFF12, 0xF0);
    h.w(0xFF24, 0x77);
    h.w(0xFF25, 0xFF);
    h.w(0xFF26, 0x80);
    assert_eq!(h.r(0xFF12), 0x00);
    assert_eq!(h.r(0xFF24), 0x00);
    assert_eq!(h.r(0xFF25), 0x00);
}

#[test]
fn dmg_length_counters_writable_while_off() {
    let mut h = H::dmg();
    h.w(0xFF26, 0x00);
    h.w(0xFF11, 64 - 12);
    h.w(0xFF1B, 0x00); // wave: 256
    assert_eq!(h.apu.ch1.length.counter, 12);
    assert_eq!(h.apu.ch3.length.counter, 256);
    // The duty bits are NOT stored.
    h.w(0xFF26, 0x80);
    assert_eq!(h.r(0xFF11), 0x3F);
}

#[test]
fn cgb_length_writes_ignored_and_counters_cleared_while_off() {
    let mut h = H::cgb();
    h.w(0xFF11, 64 - 12); // counter 12 while on
    h.w(0xFF26, 0x00);
    assert_eq!(h.apu.ch1.length.counter, 0, "CGB power-off clears");
    h.w(0xFF11, 64 - 30);
    assert_eq!(h.apu.ch1.length.counter, 0, "write while off ignored");
}

#[test]
fn dmg_length_counters_survive_power_off() {
    let mut h = H::dmg();
    h.w(0xFF11, 64 - 12);
    h.w(0xFF26, 0x00);
    assert_eq!(h.apu.ch1.length.counter, 12);
}

#[test]
fn power_on_resets_frame_sequencer_duty_and_wave_buffer() {
    let mut h = H::dmg();
    h.start_ch1();
    h.ticks(2048 * 3 + 100); // div_divider 3, duty somewhere
    h.apu.ch3.sample_byte = 0xAA;
    h.w(0xFF26, 0x00);
    h.w(0xFF26, 0x80);
    assert_eq!(h.apu.div_divider, 0);
    assert_eq!(h.apu.ch1.duty_pos, 0);
    assert_eq!(h.apu.ch2.duty_pos, 0);
    assert_eq!(h.apu.ch3.sample_byte, 0);
}

#[test]
fn frame_sequencer_does_not_run_while_off() {
    let mut h = H::dmg();
    h.w(0xFF26, 0x00);
    // Re-arm a length counter on DMG and make sure nothing clocks it.
    h.w(0xFF11, 63);
    for _ in 0..32 {
        h.fs_edge();
    }
    assert_eq!(h.apu.ch1.length.counter, 1);
    assert_eq!(h.apu.div_divider, 0);
}

#[test]
fn wave_ram_writable_while_powered_off() {
    let mut h = H::dmg();
    h.w(0xFF26, 0x00);
    h.w(0xFF30, 0x12);
    assert_eq!(h.r(0xFF30), 0x12);
    h.w(0xFF26, 0x80);
    assert_eq!(h.r(0xFF30), 0x12, "wave RAM survives power off");
}

#[test]
fn wave_ram_reads_current_byte_at_max_frequency_on_dmg() {
    let mut h = H::dmg();
    for i in 0..16u16 {
        h.w(0xFF30 + i, i as u8);
    }
    h.w(0xFF1A, 0x80);
    h.w(0xFF1C, 0x20);
    h.w(0xFF1D, 0xFF);
    h.w(0xFF1E, 0x87); // trigger, freq 0x7FF: fetch every 2 T-cycles
    h.ticks(2); // 8 T: first fetch happened
    let current = h.apu.ch3.ram[usize::from(h.apu.ch3.position >> 1)];
    for i in 0..16u16 {
        assert_eq!(h.r(0xFF30 + i), current);
    }
}

#[test]
fn wave_ram_reads_ff_at_low_frequency_on_dmg() {
    let mut h = H::dmg();
    h.w(0xFF1A, 0x80);
    h.w(0xFF1D, 0x00);
    h.w(0xFF1E, 0x80); // freq 0: period 4096
    h.ticks(4);
    assert_eq!(h.r(0xFF30), 0xFF);
    h.w(0xFF30, 0x55);
    assert_eq!(h.apu.ch3.ram[0], 0x00, "write lost outside window");
}

#[test]
fn wave_ram_raw_accessor_ignores_read_gating() {
    // The I/O viewer needs a STABLE view of wave RAM: `read_ram` is gated (0xFF
    // at low freq, the volatile current byte at max freq, on DMG), so the debug
    // accessor must return the raw stored bytes regardless.
    let mut h = H::dmg();
    for i in 0..16u16 {
        h.w(0xFF30 + i, i as u8);
    }
    h.w(0xFF1A, 0x80);
    h.w(0xFF1D, 0x00);
    h.w(0xFF1E, 0x80); // freq 0: period 4096 — gated reads return 0xFF
    h.ticks(4);
    assert_eq!(h.r(0xFF30), 0xFF, "gated read is unreliable");
    let raw = h.apu.wave_ram();
    for (i, &b) in raw.iter().enumerate() {
        assert_eq!(b, i as u8, "raw accessor returns the stored byte");
    }
}

#[test]
fn wave_retrigger_corrupts_ram_on_dmg_only() {
    for cgb in [false, true] {
        let mut h = if cgb { H::cgb() } else { H::dmg() };
        for i in 0..16u16 {
            h.w(0xFF30 + i, i as u8);
        }
        h.w(0xFF1A, 0x80);
        h.w(0xFF1D, 0xFF);
        h.w(0xFF1E, 0x87);
        h.ticks(3); // 12 T: position 3, fetch just happened
        h.w(0xFF1E, 0x87); // retrigger: next read would be byte 2
        if cgb {
            assert_eq!(h.apu.ch3.ram[0], 0, "no corruption on CGB");
        } else {
            assert_eq!(h.apu.ch3.ram[0], 2, "byte 0 takes the read byte");
        }
    }
}

#[test]
fn all_four_status_bits() {
    let mut h = H::dmg();
    h.w(0xFF12, 0xF0);
    h.w(0xFF14, 0x80);
    h.w(0xFF17, 0xF0);
    h.w(0xFF19, 0x80);
    h.w(0xFF1A, 0x80);
    h.w(0xFF1E, 0x80);
    h.w(0xFF21, 0xF0);
    h.w(0xFF23, 0x80);
    assert_eq!(h.r(0xFF26), 0xFF);
}

#[test]
fn dac_off_kills_channel_and_trigger_cannot_revive() {
    let mut h = H::dmg();
    h.start_ch1();
    assert!(h.ch_on(1));
    h.w(0xFF12, 0x00); // DAC off
    assert!(!h.ch_on(1));
    h.w(0xFF14, 0x80); // trigger with DAC off
    assert!(!h.ch_on(1));
    // But trigger side effects still ran: zero length reloaded.
    assert_eq!(h.apu.ch1.length.counter, 64);
}

#[test]
fn wave_dac_is_nr30_bit7() {
    let mut h = H::dmg();
    h.w(0xFF1A, 0x80);
    h.w(0xFF1E, 0x80);
    assert!(h.ch_on(3));
    h.w(0xFF1A, 0x00);
    assert!(!h.ch_on(3));
}
