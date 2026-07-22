//! Joypad autopoll ($4200 bit 0 → $4218-$421F) and the pad feed back to the
//! GB joypad matrix.

use super::*;

/// The GB→SNES bit mapping (fullsnes 4218h): every mapped button lands on
/// its SNES bit, unmapped SNES bits (Y/X/L/R + the id nibble) stay clear.
#[test]
fn joy1_mapping_covers_the_gb_matrix() {
    assert_eq!(joy1_bytes(0x0F, 0x0F), [0x00, 0x00], "idle");
    assert_eq!(joy1_bytes(0x00, 0x00), [0x80, 0xBF], "everything pressed");
    assert_eq!(
        joy1_bytes(0x07, 0x07),
        [0x00, 0x14],
        "Start (bit 12) + Down (bit 10)"
    );
    assert_eq!(
        joy1_bytes(0x0E, 0x0E),
        [0x80, 0x01],
        "A (bit 7) + Right (bit 8)"
    );
}

/// End to end: the guest enables autopoll (NMITIMEN bit 0), sees the HVBJOY
/// busy bit pulse at vblank, and after it clears reads the pushed GB input
/// from the JOY1 shadows — values become valid when busy drops (fullsnes:
/// reads during the poll window are unreliable).
#[test]
fn joypad_autopoll_serves_input_after_the_busy_pulse() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        let prog = [
            0xA9, 0x01, 0x8D, 0x00, 0x42, // LDA #$01 / STA $4200 (autopoll on)
            0xAD, 0x12, 0x42, 0x29, 0x01, 0xF0, 0xF9, // w1: busy set?
            0x8D, 0x60, 0x04, // STA $0460 (records 1)
            0xAD, 0x12, 0x42, 0x29, 0x01, 0xD0, 0xF9, // w2: busy clear?
            0xAD, 0x18, 0x42, 0x8D, 0x61, 0x04, // JOY1L -> $0461
            0xAD, 0x19, 0x42, 0x8D, 0x62, 0x04, // JOY1H -> $0462
            0xDB, // STP
        ];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.set_input(0x0E, 0x0E); // Right + A pressed (active-low GB nibbles)
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0460, 3),
        vec![0x01, 0x80, 0x01],
        "busy pulse seen, then JOY1L = A, JOY1H = Right"
    );
}

/// Without NMITIMEN bit 0 the JOY shadows never move — the seam is inert
/// until the guest itself asks for autopoll.
#[test]
fn no_autopoll_without_the_guest_enabling_it() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        // loop: copy JOY1L to $0470 forever (no $4200 write).
        let prog = [0xAD, 0x18, 0x42, 0x8D, 0x70, 0x04, 0x80, 0xF8];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.set_input(0x00, 0x00); // everything pressed
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0470, 1),
        vec![0x00],
        "JOY1 shadow untouched with autopoll disabled"
    );
}

/// The pad feed preserves sub-flush latch sequences and passes the local
/// matrix through when idle: a guest writing $3F / $01 / $00 back to back
/// (the takeover init's one-shot Select+Start trigger chased by an ACK
/// sandwich) surfaces each value in order — each dwelling long enough for
/// GB polls — and afterwards the player's own buttons flow (the resident
/// BIOS's continuous pad forward).
#[test]
fn pad_feed_replays_latch_sequences_then_passes_the_matrix_through() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    assert_eq!(cop.joypad_feed(), None, "matrix untouched before takeover");
    let prog = [
        0xA9, 0x3F, 0x8D, 0x04, 0x60, // LDA #$3F / STA $6004
        0xA9, 0x01, 0x8D, 0x04, 0x60, // LDA #$01 / STA $6004
        0xA9, 0x00, 0x8D, 0x04, 0x60, // LDA #$00 / STA $6004
        0xDB, // STP
    ];
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(8192);
    let mut seen = Vec::new();
    for _ in 0..8192 {
        let f = cop.joypad_feed().expect("taken over");
        if seen.last() != Some(&f[0]) {
            seen.push(f[0]);
        }
    }
    assert_eq!(
        seen[..3],
        [0x3F, 0x01, 0x00],
        "every latch write surfaced, in order"
    );
    assert_eq!(
        seen.get(3),
        Some(&0xFF),
        "then the idle matrix (nothing pressed) passes through"
    );
    // Queue drained: the local matrix (Select+Start = buttons $3, dpad $F,
    // active low) passes through as the latch byte.
    cop.set_input(0x0F, 0x03);
    assert_eq!(
        cop.joypad_feed(),
        Some([0x3F, 0xFF, 0xFF, 0xFF]),
        "player input forwards while the SNES side is idle"
    );
}
