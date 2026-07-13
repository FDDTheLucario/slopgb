//! Reset / IPL ROM / comm-port / DSP-seam tests.

use super::*;

#[test]
fn reset_enters_ipl_at_ffc0() {
    let mut s = Spc700::new();
    assert_eq!(s.pc, 0xFFC0, "reset vector points at the IPL start");
    assert!(s.ipl_enabled());
    assert_eq!(s.read8(0xFFC0), 0xCD, "first IPL byte (MOV X,#imm)");
    assert_eq!(s.read8(0xFFFE), 0xC0, "reset vector low");
    assert_eq!(s.read8(0xFFFF), 0xFF, "reset vector high");
}

#[test]
fn ipl_overlay_toggles_with_f1_bit7() {
    let mut s = Spc700::new();
    // A write into the IPL region lands in the underlying RAM.
    s.write8(0xFFC0, 0x42);
    assert_eq!(s.read8(0xFFC0), 0xCD, "IPL mapped → ROM shadows the read");
    s.write8(0x00F1, 0x00); // clear bit 7 → unmap IPL
    assert!(!s.ipl_enabled());
    assert_eq!(s.read8(0xFFC0), 0x42, "IPL unmapped → underlying RAM");
}

#[test]
fn ipl_handshake_emits_aa_bb() {
    // Running the IPL boot loader far enough writes $AA/$BB to ports 0/1 (the
    // documented SNES-side handshake) then spins waiting for $CC on port 0.
    let mut s = Spc700::new();
    for _ in 0..5000 {
        let _ = s.step();
    }
    assert_eq!(s.snes_read_port(0), 0xAA);
    assert_eq!(s.snes_read_port(1), 0xBB);
}

#[test]
fn comm_ports_are_separate_latches() {
    let mut s = Spc700::new();
    // APU write to $F4 is what the SNES reads back.
    s.write8(0x00F4, 0x12);
    assert_eq!(s.snes_read_port(0), 0x12);
    // APU read of $F4 sees the SNES-written input latch, not its own output.
    s.snes_write_port(0, 0x99);
    assert_eq!(s.read8(0x00F4), 0x99);
}

#[test]
fn control_strobes_clear_input_ports() {
    let mut s = Spc700::new();
    s.snes_write_port(0, 0xAB);
    s.snes_write_port(1, 0xCD);
    s.snes_write_port(2, 0xEF);
    s.snes_write_port(3, 0x12);
    s.write8(0x00F1, 0x10); // bit 4: clear input ports 0 & 1
    assert_eq!(s.read8(0x00F4), 0x00);
    assert_eq!(s.read8(0x00F5), 0x00);
    assert_eq!(s.read8(0x00F6), 0xEF, "ports 2/3 untouched by bit 4");
    s.write8(0x00F1, 0x20); // bit 5: clear input ports 2 & 3
    assert_eq!(s.read8(0x00F6), 0x00);
    assert_eq!(s.read8(0x00F7), 0x00);
}

#[test]
fn dsp_shadow_is_self_consistent() {
    let mut s = Spc700::new();
    s.write8(0x00F2, 0x10); // select DSP reg $10
    s.write8(0x00F3, 0x55); // write it
    s.write8(0x00F2, 0x90); // $90 mirrors $10 for reads
    assert_eq!(s.read8(0x00F3), 0x55);
    // Writes with the address' bit 7 set are ignored by the (shadow) DSP.
    s.write8(0x00F3, 0xEE);
    s.write8(0x00F2, 0x10);
    assert_eq!(s.read8(0x00F3), 0x55, "mirror write ignored");
}

#[test]
fn dsp_seam_forwards_to_attached_dsp() {
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Log {
        writes: Vec<(u8, u8)>,
        ticks: u32,
    }
    struct MockDsp(Rc<RefCell<Log>>);
    impl Dsp for MockDsp {
        fn read(&mut self, addr: u8) -> u8 {
            addr // echo the raw address so we can check pass-through
        }
        fn write(&mut self, addr: u8, val: u8) {
            self.0.borrow_mut().writes.push((addr, val));
        }
        fn tick(&mut self, cycles: u32) {
            self.0.borrow_mut().ticks += cycles;
        }
    }

    let log = Rc::new(RefCell::new(Log::default()));
    let mut s = Spc700::new();
    s.attach_dsp(Box::new(MockDsp(log.clone())));

    s.write8(0x00F2, 0x42);
    s.write8(0x00F3, 0x7E);
    assert_eq!(log.borrow().writes, vec![(0x42, 0x7E)], "raw $F2 forwarded");
    // The raw $F2 address (incl. mirror bit) is passed to the DSP on read.
    s.write8(0x00F2, 0x93);
    assert_eq!(s.read8(0x00F3), 0x93);
    // Executing an instruction ticks the DSP by that instruction's cycles.
    let _ = s.step();
    assert!(log.borrow().ticks > 0);
}

#[test]
fn stopped_cpu_still_ticks_timers() {
    let mut s = Spc700::new();
    s.stopped = true;
    let c = s.step();
    assert_eq!(c, 2);
    assert!(s.stopped, "STOP/SLEEP persists until reset");
}
