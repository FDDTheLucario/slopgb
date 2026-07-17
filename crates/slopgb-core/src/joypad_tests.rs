use super::*;

#[test]
fn post_boot_read_is_cf() {
    // Both columns selected, nothing pressed (DMG/CGB post-boot P1).
    assert_eq!(Joypad::new(false).read(), 0xCF);
}

#[test]
fn deselected_columns_read_all_ones() {
    let mut j = Joypad::new(false);
    j.write(0x30);
    j.press(Button::A);
    j.press(Button::Down);
    assert_eq!(j.read(), 0xFF);
}

#[test]
fn only_select_bits_are_writable() {
    let mut j = Joypad::new(false);
    j.write(0xCF); // bits 0-3 and 6-7 ignored
    assert_eq!(j.read(), 0xCF);
    j.write(0xFF);
    assert_eq!(j.read(), 0xFF);
}

#[test]
fn dpad_press_reads_active_low_and_raises_irq() {
    let mut j = Joypad::new(false);
    j.write(0x20); // select d-pad column only
    j.press(Button::Right);
    assert_eq!(j.read(), 0xEE); // bit 0 low
    assert_eq!(j.take_irq(), 0x10);
    assert_eq!(j.take_irq(), 0, "take_irq clears the latch");
}

#[test]
fn button_press_reads_active_low_and_raises_irq() {
    let mut j = Joypad::new(false);
    j.write(0x10); // select button column only
    j.press(Button::Start);
    assert_eq!(j.read(), 0xD7); // bit 3 low
    assert_eq!(j.take_irq(), 0x10);
}

#[test]
fn unselected_press_no_irq_until_column_selected() {
    let mut j = Joypad::new(false);
    j.write(0x30); // nothing selected
    j.press(Button::A);
    assert_eq!(j.read(), 0xFF);
    assert_eq!(j.take_irq(), 0);
    // Selecting the button column exposes the held A: line falls -> IRQ.
    j.write(0x10);
    assert_eq!(j.read(), 0xDE);
    assert_eq!(j.take_irq(), 0x10);
}

#[test]
fn release_restores_line_without_irq() {
    let mut j = Joypad::new(false);
    j.write(0x20);
    j.press(Button::Up);
    j.take_irq();
    j.release(Button::Up);
    assert_eq!(j.read(), 0xEF);
    assert_eq!(j.take_irq(), 0);
}

#[test]
fn both_columns_selected_are_anded() {
    let mut j = Joypad::new(false);
    j.write(0x00);
    j.press(Button::Right); // d-pad bit 0
    j.press(Button::B); // button bit 1
    assert_eq!(j.read(), 0xCC); // 0b1110 & 0b1101 = 0b1100
}

#[test]
fn repeated_press_does_not_relatch_irq() {
    let mut j = Joypad::new(false);
    j.write(0x10);
    j.press(Button::A);
    assert_eq!(j.take_irq(), 0x10);
    j.press(Button::A); // line already low: no new edge
    assert_eq!(j.take_irq(), 0);
}

#[test]
fn deselecting_produces_no_irq() {
    let mut j = Joypad::new(false);
    j.write(0x10);
    j.press(Button::A);
    j.take_irq();
    j.write(0x30); // line rises: no IRQ
    assert_eq!(j.take_irq(), 0);
}

#[test]
fn impossible_dpad_combo_passes_through() {
    // Hardware cannot reject Left+Right; the frontend may send it and
    // the matrix reports it honestly.
    let mut j = Joypad::new(false);
    j.write(0x20);
    j.press(Button::Left);
    j.press(Button::Right);
    assert_eq!(j.read(), 0xEC);
}

// ---- SGB command packets / MLT_REQ ----

/// SGB-enabled joypad as the post-boot hwio replay leaves it: the boot
/// ROM's header transfer ended with the line idle, P1 = $30
/// (model::HWIO_SGB), which arms the packet receiver.
fn sgb_joypad() -> Joypad {
    let mut j = Joypad::new(true);
    j.write(0x30);
    j
}

/// Send one 16-byte command packet exactly like SameSuite's
/// `SendSgbPacket` (sgb/command_mlt_req.asm): reset pulse, 128 data
/// bits LSB-first ("1" = P15 low = $10, "0" = P14 low = $20, each
/// followed by $30), then a "0" stop bit.
fn send_packet(j: &mut Joypad, data: &[u8; 16]) {
    j.write(0x00);
    j.write(0x30);
    for &byte in data {
        for bit in 0..8 {
            j.write(if byte >> bit & 1 != 0 { 0x10 } else { 0x20 });
            j.write(0x30);
        }
    }
    j.write(0x20);
    j.write(0x30);
}

/// MLT_REQ packet: command $11, length 1, one data byte.
fn mlt_req(mode: u8) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0] = 0x89;
    p[1] = mode;
    p
}

/// One joypad-ID increment: P15 low then high (SameSuite `Increment`).
fn sgb_increment(j: &mut Joypad) {
    j.write(0x10);
    j.write(0x30);
}

/// Full trace of SameSuite sgb/command_mlt_req.asm: every `ldff a,(rP1)`
/// of the ROM in order, against its hardware-verified CorrectResults
/// table. Covers mode switches, ID increments, the per-packet
/// increments ("before it gets ANDed"), and the glitched mode 2.
#[test]
fn sgb_command_mlt_req_trace() {
    let mut j = sgb_joypad();
    let mut results = Vec::new();

    send_packet(&mut j, &mlt_req(1));
    results.push(j.read());
    sgb_increment(&mut j);
    results.push(j.read());

    send_packet(&mut j, &mlt_req(0));
    send_packet(&mut j, &mlt_req(1));
    results.push(j.read());

    send_packet(&mut j, &mlt_req(0));
    send_packet(&mut j, &mlt_req(2));
    results.push(j.read());
    sgb_increment(&mut j);
    results.push(j.read());

    send_packet(&mut j, &mlt_req(0));
    send_packet(&mut j, &mlt_req(3));
    results.push(j.read());
    for _ in 0..3 {
        sgb_increment(&mut j);
        results.push(j.read());
    }

    // Switching 4 -> 2 players; the MLT_REQ_1 packet itself increments
    // the player 5 times (reset edge + four "1" bits) before the new
    // count masks it.
    for increments in 0..4 {
        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(3));
        for _ in 0..increments {
            sgb_increment(&mut j);
        }
        send_packet(&mut j, &mlt_req(1));
        results.push(j.read());
    }

    // How many times sending a packet increments: MLT_REQ_3 carries
    // six edges (reset + five "1" bits).
    send_packet(&mut j, &mlt_req(0));
    send_packet(&mut j, &mlt_req(3));
    results.push(j.read());
    send_packet(&mut j, &mlt_req(3));
    results.push(j.read());

    // Glitched mode 2 entered from 4-player mode with players 0-3.
    for increments in 0..4 {
        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(3));
        for _ in 0..increments {
            sgb_increment(&mut j);
        }
        send_packet(&mut j, &mlt_req(2));
        results.push(j.read());
    }

    // Incrementing within the glitched mode (no effect: odd count).
    send_packet(&mut j, &mlt_req(0));
    send_packet(&mut j, &mlt_req(3));
    send_packet(&mut j, &mlt_req(2));
    sgb_increment(&mut j);
    results.push(j.read());
    sgb_increment(&mut j);
    results.push(j.read());

    send_packet(&mut j, &mlt_req(0));
    send_packet(&mut j, &mlt_req(3));
    sgb_increment(&mut j);
    send_packet(&mut j, &mlt_req(2));
    sgb_increment(&mut j);
    results.push(j.read());
    sgb_increment(&mut j);
    results.push(j.read());

    send_packet(&mut j, &mlt_req(0));
    send_packet(&mut j, &mlt_req(3));
    sgb_increment(&mut j);
    sgb_increment(&mut j);
    send_packet(&mut j, &mlt_req(2));
    sgb_increment(&mut j);
    results.push(j.read());

    // CorrectResults of sgb/command_mlt_req.asm (hardware-verified).
    assert_eq!(
        results,
        [
            0xFF, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFD, //
            0xFC, 0xFE, 0xFF, 0xFE, 0xFF, 0xFF, 0xFD, 0xFD, //
            0xFD, 0xFF, 0xFF, 0xFD, 0xFD, 0xFD, 0xFD, 0xFF,
        ]
    );
}

/// Full trace of SameSuite sgb/command_mlt_req_1_incrementing.asm: the
/// joypad ID advances exactly on writes taking P15 from low to high,
/// whatever P14 does.
#[test]
fn sgb_mlt_req_increment_is_p15_rising_edge() {
    let mut j = sgb_joypad();
    send_packet(&mut j, &mlt_req(1));
    let mut results = Vec::new();
    for seq in [
        &[0x10u8, 0x30][..],       // increments
        &[0x20, 0x30],             // does not increment
        &[0x10, 0x20, 0x30],       // increments (once)
        &[0x10, 0x20, 0x10, 0x30], // two edges: wraps back
        &[0x10, 0x10, 0x30],       // increments
        &[0x00, 0x10, 0x30],       // increments
        &[0x10, 0x00, 0x30],       // increments
        &[0x00, 0x30],             // increments
    ] {
        for &v in seq {
            j.write(v);
        }
        results.push(j.read());
    }
    // CorrectResults of sgb/command_mlt_req_1_incrementing.asm.
    assert_eq!(results, [0xFE, 0xFE, 0xFF, 0xFF, 0xFE, 0xFF, 0xFE, 0xFF]);
}

/// Single-player mode: reads with both lines high stay plain key
/// reads, and P15 edges never advance anything.
#[test]
fn sgb_single_player_reads_are_plain() {
    let mut j = sgb_joypad();
    sgb_increment(&mut j);
    assert_eq!(j.read(), 0xFF);
    send_packet(&mut j, &mlt_req(1));
    send_packet(&mut j, &mlt_req(0));
    sgb_increment(&mut j);
    assert_eq!(j.read(), 0xFF);
}

/// In multiplayer mode, selecting a key column still reads the matrix
/// (the host joypad is player 1); only the both-lines-high read shows
/// the ID.
#[test]
fn sgb_multiplayer_key_reads_keep_working() {
    let mut j = sgb_joypad();
    send_packet(&mut j, &mlt_req(1));
    j.press(Button::Start);
    j.write(0x10); // select button column
    assert_eq!(j.read(), 0xD7);
    j.write(0x30);
    assert_eq!(j.read() & 0x0F, 0x0E, "ID read: P15 edge advanced to 2");
}

/// A "1" pulse in stop-bit position corrupts the packet: the command
/// must not execute (SameBoy GB_sgb_write case 1).
#[test]
fn sgb_corrupt_stop_bit_discards_packet() {
    let mut j = sgb_joypad();
    let p = mlt_req(1);
    j.write(0x00);
    j.write(0x30);
    for &byte in &p {
        for bit in 0..8 {
            j.write(if byte >> bit & 1 != 0 { 0x10 } else { 0x20 });
            j.write(0x30);
        }
    }
    j.write(0x10); // corrupt: "1" where the stop bit belongs
    j.write(0x30);
    assert_eq!(j.read(), 0xFF, "command dropped: still single player");
    // The receiver recovers: a fresh packet works.
    send_packet(&mut j, &mlt_req(1));
    sgb_increment(&mut j);
    assert_eq!(j.read(), 0xFE);
}

/// A reset pulse mid-packet restarts the command from scratch.
#[test]
fn sgb_reset_pulse_mid_packet_restarts() {
    let mut j = sgb_joypad();
    // Half a packet of "1" bits, then a reset and a full MLT_REQ_1.
    j.write(0x00);
    j.write(0x30);
    for _ in 0..64 {
        j.write(0x10);
        j.write(0x30);
    }
    send_packet(&mut j, &mlt_req(1));
    sgb_increment(&mut j);
    assert_eq!(j.read(), 0xFE, "MLT_REQ executed cleanly after restart");
}

/// Non-SGB joypads ignore the packet protocol entirely.
#[test]
fn non_sgb_ignores_packets() {
    let mut j = Joypad::new(false);
    j.write(0x30);
    send_packet(&mut j, &mlt_req(1));
    sgb_increment(&mut j);
    assert_eq!(j.read(), 0xFF);
}

// ---- SGB raw-packet tee (the ICD2 mailbox feed) ----

/// Every completed 16-byte packet is teed for the SNES-side coprocessor,
/// and the tee is a tee: MLT_REQ still executes locally.
#[test]
fn sgb_packet_tee_captures_packet_and_mlt_req_still_executes() {
    let mut j = sgb_joypad();
    let p = mlt_req(1);
    send_packet(&mut j, &p);
    assert_eq!(j.take_sgb_packet(), Some(p));
    assert_eq!(j.take_sgb_packet(), None, "queue drained");
    sgb_increment(&mut j);
    assert_eq!(j.read(), 0xFE, "MLT_REQ executed despite the tee");
}

/// A multi-packet command arrives at the ICD2 one 16-byte packet at a
/// time (fullsnes "SGB Port 7000h-700Fh"), not as the assembled command.
#[test]
fn sgb_packet_tee_yields_each_packet_of_a_multi_packet_command() {
    let mut j = sgb_joypad();
    let mut p1 = [0u8; 16];
    p1[0] = 0x02; // PAL01 header claiming two packets
    let p2 = [0xA5u8; 16];
    send_packet(&mut j, &p1);
    send_packet(&mut j, &p2);
    assert_eq!(j.take_sgb_packet(), Some(p1));
    assert_eq!(j.take_sgb_packet(), Some(p2));
    assert_eq!(j.take_sgb_packet(), None);
}

/// An undrained queue (no coprocessor attached) stays bounded, dropping
/// the oldest packets.
#[test]
fn sgb_packet_queue_is_bounded_dropping_oldest() {
    let mut j = sgb_joypad();
    for i in 0..20u8 {
        // $1A has no function (fullsnes: commands $1A-$1F point to RET),
        // so flooding it has no HLE side effects.
        let mut p = [0u8; 16];
        p[0] = (0x1A << 3) | 1;
        p[1] = i;
        send_packet(&mut j, &p);
    }
    let first = j.take_sgb_packet().expect("queue non-empty");
    assert_eq!(first[1], 4, "oldest 4 of 20 dropped at the cap");
    let mut drained = 1;
    while j.take_sgb_packet().is_some() {
        drained += 1;
    }
    assert_eq!(drained, 16, "cap holds 16 packets");
}

/// A corrupt packet (a "1" pulse in stop position) is dropped wholesale —
/// it never reaches the tee.
#[test]
fn sgb_corrupt_packet_is_not_teed() {
    let mut j = sgb_joypad();
    let p = mlt_req(1);
    j.write(0x00);
    j.write(0x30);
    for &byte in &p {
        for bit in 0..8 {
            j.write(if byte >> bit & 1 != 0 { 0x10 } else { 0x20 });
            j.write(0x30);
        }
    }
    j.write(0x10); // corrupt: "1" where the stop bit belongs
    j.write(0x30);
    assert_eq!(j.take_sgb_packet(), None);
}

/// Packets pending delivery to the coprocessor survive a save state.
#[test]
fn sgb_packet_queue_round_trips_save_state() {
    let mut j = sgb_joypad();
    let p = mlt_req(1);
    send_packet(&mut j, &p);
    let mut w = crate::state::Writer::new();
    j.write_state(&mut w);
    let bytes = w.into_vec();
    let mut t = Joypad::new(false);
    let mut r = crate::state::Reader::new(&bytes);
    t.read_state(&mut r).unwrap();
    assert_eq!(t.take_sgb_packet(), Some(p));
    assert_eq!(t.take_sgb_packet(), None);
}

/// Non-SGB joypads never tee anything.
#[test]
fn non_sgb_take_packet_is_none() {
    let mut j = Joypad::new(false);
    j.write(0x30);
    send_packet(&mut j, &mlt_req(1));
    assert_eq!(j.take_sgb_packet(), None);
}

/// One SGB header packet as `boot/slopgb_sgb_boot.asm` (`SgbHandshake`)
/// builds it: command `$F1 + 2k`, then 14 header bytes, then their 8-bit
/// sum as a checksum. The `$F1` family is a single-packet command.
fn sgb_header_packet(k: u8, chunk: &[u8; 14]) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0] = 0xF1 + 2 * k;
    let mut sum = 0u8;
    for i in 0..14 {
        p[1 + i] = chunk[i];
        sum = sum.wrapping_add(chunk[i]);
    }
    p[15] = sum;
    p
}

/// The six pulse-coded command packets the slopgb SGB boot ROM transfers to
/// the SNES (the cart header at `$0104`, 84 bytes) must decode byte-exactly
/// through the real ICD2 receiver — the clean-room "handshake packet format
/// is correct" oracle. This pins the wire format the boot ROM emits: `$00`
/// reset / `$10`-`$20` data (LSB-first) / `$30` idle / `$20` stop pulses,
/// 16-byte packets, the `$F1,$F3,$F5,$F7,$F9,$FB` command sequence, and the
/// per-packet checksum — matching `SgbSendByte`/`SgbHandshake` in the asm.
#[test]
fn sgb_boot_header_handshake_decodes() {
    // 84 bytes standing in for the cart region $0104..$0158 the boot ROM
    // walks (a recognizable ramp, so a bit-order slip would be visible).
    let header: Vec<u8> = (0u8..84).collect();
    let mut j = sgb_joypad();
    for k in 0..6u8 {
        let chunk: [u8; 14] = header[k as usize * 14..k as usize * 14 + 14]
            .try_into()
            .unwrap();
        let packet = sgb_header_packet(k, &chunk);
        send_packet(&mut j, &packet);
        assert_eq!(
            j.take_sgb_command().as_deref(),
            Some(&packet[..]),
            "header packet {k} (command ${:02X}) decodes byte-exactly",
            packet[0],
        );
    }
    // Command sequence is $F1,$F3,$F5,$F7,$F9,$FB and every packet ends with
    // the checksum of its 14 payload bytes.
    let p3 = sgb_header_packet(3, &[10u8; 14]);
    assert_eq!(p3[0], 0xF7);
    assert_eq!(p3[15], 140, "checksum = 14 * 10");
}
