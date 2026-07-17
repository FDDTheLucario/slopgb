use super::*;

/// A distinct 16-byte packet for mailbox tests.
fn pkt() -> [u8; 16] {
    core::array::from_fn(|i| (i as u8) * 3 + 1)
}

/// fullsnes "SGB Port 6002h" + "SGB Port 7000h-700Fh": depositing a packet
/// raises the available flag; reading `$7000` (and only `$7000`) resets it.
#[test]
fn packet_deposit_read_and_flag_clear() {
    let mut i = Icd2::new();
    assert_eq!(i.cpu_read(0x6002), 0, "no packet at power-on");
    i.host_deposit_packet(&pkt());
    assert_eq!(i.cpu_read(0x6002), 1, "packet available");
    assert_eq!(i.cpu_read(0x7005), pkt()[5]);
    assert_eq!(i.cpu_read(0x700F), pkt()[15]);
    assert_eq!(i.cpu_read(0x6002), 1, "reads of 7001h-700Fh do not clear");
    assert_eq!(i.cpu_read(0x7000), pkt()[0]);
    assert_eq!(i.cpu_read(0x6002), 0, "reading 7000h resets the flag");
}

/// fullsnes "SGB Port 6004h-6007h": pad writes latch per player and are
/// host-visible; the sticky written-flag arms only after the first write.
#[test]
fn pad_latches_and_sticky_written_flag() {
    let mut i = Icd2::new();
    let (pads, written) = i.host_pads();
    assert_eq!(
        pads, [0xFF; 4],
        "latches idle (nothing pressed) at power-on"
    );
    assert!(!written, "sticky flag clear until the program writes");
    i.cpu_write(0x6004, 0xEF); // player 1: A pressed
    i.cpu_write(0x6006, 0xF7); // player 3: Down pressed
    let (pads, written) = i.host_pads();
    assert_eq!(pads, [0xEF, 0xFF, 0xF7, 0xFF]);
    assert!(written, "sticky flag armed");
}

/// fullsnes "SGB Port 7800h" + "SGB Port 6001h": `$6001` selects the read row
/// and resets the index; reads auto-increment; indices 320-511 return `$FF`;
/// after 512 the index wraps within the same row.
#[test]
fn char_buffer_row_select_autoinc_pad_and_wrap() {
    let mut i = Icd2::new();
    let row: Vec<u8> = (0..320).map(|n| (n % 251) as u8).collect();
    i.host_load_char_row(1, &row);
    i.cpu_write(0x6001, 1);
    assert_eq!(i.cpu_read(0x7800), row[0]);
    assert_eq!(i.cpu_read(0x7800), row[1]);
    for _ in 2..320 {
        i.cpu_read(0x7800);
    }
    assert_eq!(i.cpu_read(0x7800), 0xFF, "index 320 reads black padding");
    for _ in 321..512 {
        i.cpu_read(0x7800);
    }
    assert_eq!(
        i.cpu_read(0x7800),
        row[0],
        "index wraps to 0 after 512 reads"
    );
    // Re-selecting a row resets the index.
    i.cpu_read(0x7800);
    i.cpu_write(0x6001, 1);
    assert_eq!(i.cpu_read(0x7800), row[0], "6001h write resets the index");
}

/// fullsnes "SGB Port 6000h": bits 7-3 the current LCD character row, bits
/// 1-0 the current write-buffer number (both host-maintained shadows).
#[test]
fn lcd_row_status_read() {
    let mut i = Icd2::new();
    i.host_set_lcd_row(0x11, 2);
    assert_eq!(i.cpu_read(0x6000), (0x11 << 3) | 2);
}

/// fullsnes "SGB Port 6003h": control writes are captured for the host (the
/// GB-reset bit is host-visible, never wired to the GB core).
#[test]
fn control_write_captured() {
    let mut i = Icd2::new();
    assert_eq!(i.host_control(), 0x01, "power-on: no reset, 4MHz speed");
    i.cpu_write(0x6003, 0x8A);
    assert_eq!(i.host_control(), 0x8A);
}

/// fullsnes "SGB Port 600Fh" + the SGB I/O map garbage table for
/// `[600Fh]=21h` chips: 6001h/6004h-6005h reads mirror 6000h, 6003h/6006h-6007h
/// mirror 6002h, 6008h-600Eh mirror 600Fh.
#[test]
fn version_and_write_only_read_garbage() {
    let mut i = Icd2::new();
    i.host_set_lcd_row(5, 1);
    i.host_deposit_packet(&pkt());
    assert_eq!(i.cpu_read(0x600F), 0x21, "chip version");
    assert_eq!(i.cpu_read(0x6001), i.cpu_read(0x6000));
    assert_eq!(i.cpu_read(0x6004), i.cpu_read(0x6000));
    assert_eq!(i.cpu_read(0x6003), i.cpu_read(0x6002));
    assert_eq!(i.cpu_read(0x6007), i.cpu_read(0x6002));
    assert_eq!(i.cpu_read(0x6008), 0x21);
    assert_eq!(i.cpu_read(0x600E), 0x21);
}

/// The chip decodes only A0-A3 and A11-A15 (fullsnes SGB I/O map), so each
/// block's registers mirror every 16 bytes: `$6010` reads as `$6000`, `$7010`
/// as `$7000` (including its flag-clearing side effect), `$7808` as `$7800`.
#[test]
fn sparse_decode_mirrors() {
    let mut i = Icd2::new();
    i.host_set_lcd_row(3, 0);
    i.host_deposit_packet(&pkt());
    assert_eq!(i.cpu_read(0x6010), i.cpu_read(0x6000));
    assert_eq!(i.cpu_read(0x67F0), i.cpu_read(0x6000));
    assert_eq!(
        i.cpu_read(0x7013),
        pkt()[3],
        "$7010 block mirrors the mailbox"
    );
    assert_eq!(i.cpu_read(0x7010), pkt()[0]);
    assert_eq!(
        i.cpu_read(0x6002),
        0,
        "mirrored $7000 read clears the flag too"
    );
    // $7801-$780F mirror $7800 (same port, same auto-increment).
    let row: Vec<u8> = (0..320).map(|n| n as u8).collect();
    i.host_load_char_row(0, &row);
    i.cpu_write(0x6001, 0);
    assert_eq!(i.cpu_read(0x7808), row[0]);
    assert_eq!(i.cpu_read(0x7800), row[1]);
}
