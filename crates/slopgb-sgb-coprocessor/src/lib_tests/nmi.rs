//! The SNES clocking loop: vblank NMI delivery, RDNMI/HVBJOY shadows, the
//! resident handler's RAM-vector dispatch and register preservation.

use super::*;

#[test]
fn vblank_nmi_fires_once_per_frame_when_enabled() {
    let Some(mut cop) = nmi_counting_cop() else {
        return;
    };
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0340, 1),
        vec![2],
        "exactly one NMI per frame across two frames"
    );
}

#[test]
fn no_nmi_without_the_guest_enabling_it() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0xFFFA, &[0x00, 0x92]).unwrap();
        cpu.write_ram(0x9000, &[0xCB, 0x80, 0xFD]).unwrap(); // WAI loop, no $4200
        cpu.write_ram(0x9200, &[0xEE, 0x40, 0x03, 0x40]).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224 * 2);
    assert_eq!(
        cop.debug_cpu_ram(0x0340, 1),
        vec![0],
        "NMITIMEN bit 7 gates NMI"
    );
}

/// RDNMI/HVBJOY shadows: the guest spins on HVBJOY bit 7, then reads RDNMI
/// twice — first read shows the flag + CPU version, the second shows the
/// read-acknowledge (bit 7 cleared, guest-side).
#[test]
fn rdnmi_and_hvbjoy_shadows_follow_the_frame() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        // wait: LDA $4212 / BPL wait / LDA $4210 / STA $0341 / LDA $4210 /
        // STA $0342 / STP.
        let prog = [
            0xAD, 0x12, 0x42, // LDA $4212 (HVBJOY)
            0x10, 0xFB, // BPL -5 (spin until vblank bit sets)
            0xAD, 0x10, 0x42, // LDA $4210 (RDNMI)
            0x8D, 0x41, 0x03, // STA $0341
            0xAD, 0x10, 0x42, // LDA $4210 (again)
            0x8D, 0x42, 0x03, // STA $0342
            0xDB, // STP
        ];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224);
    let first = cop.debug_cpu_ram(0x0341, 1)[0];
    let second = cop.debug_cpu_ram(0x0342, 1)[0];
    assert_eq!(first & 0x80, 0x80, "RDNMI flag set inside vblank");
    assert_eq!(first & 0x0F, 0x02, "CPU version bits");
    assert_eq!(second & 0x80, 0, "read acknowledged the flag");
}

/// The resident NMI handler dispatches through the RAM vector at $00:00BB
/// (fullsnes SGB notes: the hookable NMI vector JUMP clobbers). Empty vector
/// -> the NMI is a no-op for the program; installed vector -> the hook runs.
#[test]
fn nmi_dispatches_through_the_ram_vector() {
    let Some(mut cop) = nmi_counting_cop() else {
        return;
    };
    // Point the RAM vector at a counting hook: INC $0344 / RTI... the hook
    // is entered by JML, so return with RTI (the handler's JML replaced its
    // own frame — the interrupt frame is still on the stack).
    {
        let mut cpu = cop.cpu.borrow_mut();
        cpu.write_ram(0x9300, &[0xEE, 0x44, 0x03, 0x40]).unwrap(); // INC $0344 / RTI
        cpu.write_ram(0x00BB, &[0x00, 0x93, 0x00]).unwrap(); // [$00BB] = $00:9300
        // Note: nmi_counting_cop's own $FFFA override is replaced back with
        // the resident handler so the RAM-vector path is what runs.
        cpu.write_ram(0xFFFA, &[0x30, 0xBE]).unwrap();
    }
    cop.clock(70_224);
    assert_eq!(
        cop.debug_cpu_ram(0x0344, 1),
        vec![1],
        "the hook behind the RAM vector ran"
    );
    assert_eq!(
        cop.debug_cpu_ram(0x0340, 1),
        vec![0],
        "the test vector override was replaced"
    );
}

/// The resident NMI handler preserves the interrupted program's A on the
/// empty-vector path (the BIOS-only case): A survives across an NMI.
#[test]
fn nmi_handler_preserves_a_with_empty_vector() {
    let Some(mut cop) = build_cop(48_000) else {
        return;
    };
    {
        let mut cpu = cop.cpu.borrow_mut();
        // LDA #$81 / STA $4200 (enable NMI) / LDA #$5A / WAI / STA $0346 / STP
        let prog = [
            0xA9, 0x81, 0x8D, 0x00, 0x42, 0xA9, 0x5A, 0xCB, 0x8D, 0x46, 0x03, 0xDB,
        ];
        cpu.write_ram(0x9000, &prog).unwrap();
        cpu.set_pc(0x9000).unwrap();
    }
    cop.clock(70_224);
    assert_eq!(
        cop.debug_cpu_ram(0x0346, 1),
        vec![0x5A],
        "A survived the NMI round trip"
    );
}
