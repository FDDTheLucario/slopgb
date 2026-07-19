use super::*;
use crate::dsp::SDsp;
use crate::spc700::Spc700;

#[test]
fn header_registers_and_regions_land_at_the_spec_offsets() {
    let mut spc = Spc700::new();
    spc.a = 0x11;
    spc.x = 0x22;
    spc.y = 0x33;
    spc.sp = 0x44;
    spc.pc = 0x0400;
    // Mark ARAM start + end and a DSP register so we can find them in the file.
    spc.apu_ram_mut()[0] = 0xAA;
    spc.apu_ram_mut()[0xFFFF] = 0xBB;
    let mut dsp = SDsp::new();
    dsp.write(0x4C, 0x07); // KON

    let f = build_spc_file(&spc, &dsp);

    assert_eq!(f.len(), SPC_FILE_LEN);
    assert_eq!(&f[..33], MAGIC);
    assert_eq!(f[0x23], 0x1A, "has-ID666 flag");
    // registers
    assert_eq!([f[0x25], f[0x26]], [0x00, 0x04], "PC = $0400 LE");
    assert_eq!(f[0x27], 0x11);
    assert_eq!(f[0x28], 0x22);
    assert_eq!(f[0x29], 0x33);
    assert_eq!(f[0x2B], 0x44, "SP");
    // ARAM at $100
    assert_eq!(f[0x100], 0xAA, "ARAM byte 0");
    assert_eq!(f[0x100 + 0xFFFF], 0xBB, "ARAM byte $FFFF");
    // DSP regs at $10100
    assert_eq!(f[0x1_0100 + 0x4C], 0x07, "DSP KON reg");
    // extra RAM mirrors ARAM's top 64 bytes
    assert_eq!(f[0x1_01FF], 0xBB, "extra-RAM mirrors ARAM $FFFF");
}
