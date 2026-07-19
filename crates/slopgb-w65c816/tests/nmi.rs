//! Hardware NMI dispatch (a pin, not an opcode — no SingleStepTests vectors
//! exist), per the WDC W65C816S datasheet: vectors `$00FFFA` (emulation) /
//! `$00FFEA` (native); native mode pushes the program bank first; the
//! emulation-mode pushed P has bit 4 clear (the hardware-interrupt signature
//! distinguishing NMI/IRQ from BRK); D clears, I sets, the program bank
//! zeroes, and a `WAI`-ing CPU wakes.

use slopgb_w65c816::{Bus, Cpu};

struct Ram(Vec<u8>);

impl Bus for Ram {
    fn read(&mut self, a: u32) -> u8 {
        self.0[(a & 0xFFFF) as usize]
    }
    fn write(&mut self, a: u32, v: u8) {
        self.0[(a & 0xFFFF) as usize] = v;
    }
}

#[test]
fn emulation_mode_nmi_dispatch() {
    let mut ram = Ram(vec![0; 0x1_0000]);
    ram.0[0xFFFA] = 0x00;
    ram.0[0xFFFB] = 0x90; // vector -> $9000
    let mut cpu = Cpu::new();
    cpu.regs.pc = 0x1234;
    cpu.regs.p = 0x38; // D set (must clear) + the emulation M/X bits
    cpu.regs.s = 0x01FD; // mid-page: pushes stay off the page-wrap edge
    let s0 = usize::from(cpu.regs.s);
    let cycles = cpu.nmi(&mut ram);
    assert_eq!(cpu.regs.pc, 0x9000, "vectored through $FFFA");
    assert_eq!(cpu.regs.pbr, 0);
    assert_ne!(cpu.regs.p & 0x04, 0, "I set");
    assert_eq!(cpu.regs.p & 0x08, 0, "D cleared");
    assert_eq!(ram.0[s0], 0x12, "PCH pushed");
    assert_eq!(ram.0[s0 - 1], 0x34, "PCL pushed");
    assert_eq!(
        ram.0[s0 - 2] & 0x10,
        0,
        "pushed P bit 4 clear: hardware-interrupt signature"
    );
    assert_ne!(
        ram.0[s0 - 2] & 0x08,
        0,
        "pushed P is pre-clear (D still set)"
    );
    assert!(cycles >= 7, "an interrupt sequence spends cycles");
}

#[test]
fn native_mode_nmi_pushes_pbr_and_uses_ffea() {
    let mut ram = Ram(vec![0; 0x1_0000]);
    ram.0[0xFFEA] = 0x00;
    ram.0[0xFFEB] = 0x91; // vector -> $9100
    let mut cpu = Cpu::new();
    cpu.regs.e = false;
    cpu.regs.s = 0x1FF0;
    cpu.regs.pbr = 0x7E;
    cpu.regs.pc = 0xABCD;
    cpu.regs.p = 0x30;
    cpu.nmi(&mut ram);
    assert_eq!(cpu.regs.pc, 0x9100, "vectored through $FFEA");
    assert_eq!(cpu.regs.pbr, 0, "program bank zeroed");
    assert_eq!(ram.0[0x1FF0], 0x7E, "PBR pushed first");
    assert_eq!(ram.0[0x1FEF], 0xAB, "PCH");
    assert_eq!(ram.0[0x1FEE], 0xCD, "PCL");
    assert_eq!(
        ram.0[0x1FED], 0x30,
        "full P pushed pre-I, bit 4 kept in native"
    );
}

#[test]
fn nmi_wakes_wai() {
    let mut ram = Ram(vec![0; 0x1_0000]);
    let mut cpu = Cpu::new();
    cpu.waiting = true;
    cpu.nmi(&mut ram);
    assert!(!cpu.waiting, "WAI released by the interrupt");
}
