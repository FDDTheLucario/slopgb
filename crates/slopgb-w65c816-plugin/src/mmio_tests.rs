use super::*;

/// CPU writes into the captured windows land in the ring in order, as
/// `(addr_lo, addr_hi, val)` triples the host drains.
#[test]
fn writes_are_captured_in_order() {
    let mut m = Mmio::new();
    assert!(m.cpu_write(0x2100, 0x8F), "INIDISP captured");
    assert!(m.cpu_write(0x4200, 0x81), "NMITIMEN captured");
    assert!(m.cpu_write(0x420B, 0x01), "MDMAEN captured");
    assert!(!m.cpu_write(0x2140, 0x00), "APU ports are not MMIO");
    assert!(!m.cpu_write(0x6000, 0x00), "ICD2 is not MMIO");
    let drained = m.host_drain_up_to(usize::MAX);
    assert_eq!(
        drained,
        vec![(0x2100, 0x8F), (0x4200, 0x81), (0x420B, 0x01)],
        "in write order"
    );
    assert!(m.host_drain_up_to(usize::MAX).is_empty(), "drain consumes");
}

/// The ring is bounded: past capacity the newest writes are dropped (the
/// causal prefix is preserved) and the sticky overflow flag arms.
#[test]
fn ring_overflow_drops_newest_and_flags() {
    let mut m = Mmio::new();
    for i in 0..(MMIO_RING_CAP + 10) {
        m.cpu_write(0x2100, i as u8);
    }
    let drained = m.host_drain_up_to(usize::MAX);
    assert_eq!(drained.len(), MMIO_RING_CAP);
    assert_eq!(drained[0].1, 0, "oldest kept");
    assert!(m.overflowed(), "overflow sticky flag armed");
}

/// Host-poked shadows serve CPU reads of the $4200-$421F block; RDNMI
/// ($4210) and TIMEUP ($4211) clear their bit 7 on read (fullsnes: "gets
/// also reset after reading from this register").
#[test]
fn read_shadows_and_read_clear_semantics() {
    let mut m = Mmio::new();
    m.host_set_shadow(0x12, 0xC1); // HVBJOY: vblank + hblank + busy
    assert_eq!(m.cpu_read(0x4212), Some(0xC1));
    assert_eq!(m.cpu_read(0x4212), Some(0xC1), "HVBJOY is a plain shadow");

    m.host_set_shadow(0x10, 0x82); // RDNMI: NMI flag + CPU version 2
    assert_eq!(m.cpu_read(0x4210), Some(0x82));
    assert_eq!(m.cpu_read(0x4210), Some(0x02), "bit 7 cleared by the read");

    m.host_set_shadow(0x11, 0x80); // TIMEUP: IRQ flag
    assert_eq!(m.cpu_read(0x4211), Some(0x80));
    assert_eq!(m.cpu_read(0x4211), Some(0x00), "bit 7 cleared by the read");

    m.host_set_shadow(0x18, 0xEF); // JOY1L autopoll shadow
    assert_eq!(m.cpu_read(0x4218), Some(0xEF));

    assert_eq!(m.cpu_read(0x2100), None, "write-only window reads open bus");
    assert_eq!(
        m.cpu_read(0x4300),
        None,
        "DMA regs unshadowed (write-capture)"
    );
}

/// $4016/$4017 (manual joypad serial port) read host-fed bytes.
#[test]
fn manual_joypad_port_shadows() {
    let mut m = Mmio::new();
    m.host_set_joy_serial_byte(0, 0x01);
    m.host_set_joy_serial_byte(1, 0x02);
    assert_eq!(m.cpu_read(0x4016), Some(0x01));
    assert_eq!(m.cpu_read(0x4017), Some(0x02));
    // A single-byte update never disturbs the sibling.
    m.host_set_joy_serial_byte(1, 0x7F);
    assert_eq!(m.cpu_read(0x4016), Some(0x01));
    assert_eq!(m.cpu_read(0x4017), Some(0x7F));
}

/// The WRAM B-bus access ports `$2180-$2183` (WMDATA + the 17-bit WMADD)
/// are captured writes — their address/auto-increment state machine lives
/// host-side with the DMA engine (fullsnes "SNES Memory Work RAM Access").
#[test]
fn wmdata_ports_are_captured() {
    let mut m = Mmio::new();
    assert!(m.cpu_write(0x2180, 0x42), "WMDATA");
    assert!(m.cpu_write(0x2181, 0x00), "WMADDL");
    assert!(m.cpu_write(0x2183, 0x01), "WMADDH");
    assert!(!m.cpu_write(0x2184, 0x00), "past WMADDH: open bus");
    assert!(!m.cpu_write(0x217F, 0x00), "below the window: open bus");
}

/// A nonzero MDMAEN (`$420B`) write arms the DMA stall — the CPU pauses
/// until the host has drained the ring and executed the transfer (fullsnes
/// 420Bh: "The CPU is paused during the transfer"). Zero starts nothing.
#[test]
fn mdmaen_write_arms_the_dma_stall() {
    let mut m = Mmio::new();
    assert!(!m.dma_stall());
    m.cpu_write(0x420B, 0x00);
    assert!(!m.dma_stall(), "MDMAEN=0 starts no transfer");
    m.cpu_write(0x420B, 0x01);
    assert!(m.dma_stall());
    m.host_clear_dma_stall();
    assert!(!m.dma_stall());
}

/// The CPU multiply unit (fullsnes 4202h/4203h): WRMPYB kicks an 8x8
/// multiply of WRMPYA, product readable at RDMPYL/H ($4216/$4217). The
/// result completes within the write here (no 8-cycle garbage window).
#[test]
fn multiply_registers_compute_the_product() {
    let mut m = Mmio::new();
    m.cpu_write(0x4202, 40);
    m.cpu_write(0x4203, 3);
    assert_eq!(m.cpu_read(0x4216), Some(120), "RDMPYL");
    assert_eq!(m.cpu_read(0x4217), Some(0), "RDMPYH");
    // 16-bit product: 200 * 200 = 40000 = $9C40.
    m.cpu_write(0x4202, 200);
    m.cpu_write(0x4203, 200);
    assert_eq!(m.cpu_read(0x4216), Some(0x40));
    assert_eq!(m.cpu_read(0x4217), Some(0x9C));
    // WRMPYA latches: a second WRMPYB reuses it.
    m.cpu_write(0x4203, 2);
    assert_eq!(m.cpu_read(0x4216), Some(0x90), "200*2 = $190");
    assert_eq!(m.cpu_read(0x4217), Some(0x01));
}

/// The CPU divide unit (fullsnes 4204h-4206h): WRDIVB kicks WRDIV / divisor
/// — quotient at RDDIVL/H ($4214/$4215), remainder at RDMPYL/H. Division by
/// zero yields quotient $FFFF and remainder = dividend.
#[test]
fn divide_registers_compute_quotient_and_remainder() {
    let mut m = Mmio::new();
    m.cpu_write(0x4204, 0xE8); // 1000 = $03E8
    m.cpu_write(0x4205, 0x03);
    m.cpu_write(0x4206, 7);
    assert_eq!(m.cpu_read(0x4214), Some(142), "RDDIVL: 1000/7");
    assert_eq!(m.cpu_read(0x4215), Some(0));
    assert_eq!(m.cpu_read(0x4216), Some(6), "RDMPYL: 1000%7");
    assert_eq!(m.cpu_read(0x4217), Some(0));
    m.cpu_write(0x4206, 0);
    assert_eq!(m.cpu_read(0x4214), Some(0xFF), "div0 quotient $FFFF");
    assert_eq!(m.cpu_read(0x4215), Some(0xFF));
    assert_eq!(m.cpu_read(0x4216), Some(0xE8), "div0 remainder = dividend");
    assert_eq!(m.cpu_read(0x4217), Some(0x03));
}

/// A short host read drains only what it can carry; the rest stays queued.
#[test]
fn partial_drain_keeps_the_tail() {
    let mut m = Mmio::new();
    for i in 0..5u8 {
        m.cpu_write(0x2100, i);
    }
    let first = m.host_drain_up_to(2);
    assert_eq!(first, vec![(0x2100, 0), (0x2100, 1)]);
    let rest = m.host_drain_up_to(99);
    assert_eq!(rest, vec![(0x2100, 2), (0x2100, 3), (0x2100, 4)]);
}
