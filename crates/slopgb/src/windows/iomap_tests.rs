use super::*;

const T: Theme = Theme::BGB;

#[test]
fn reg_line_formats_addr_name_value() {
    let read = |a: u16| match a {
        0xFF40 => 0x91,
        0xFF44 => 0x90,
        _ => 0x00,
    };
    assert_eq!(reg_line(read, r(0xFF40, "LCDC")), "FF40 LCDC 91");
    assert_eq!(reg_line(read, r(0xFF44, "LY")), "FF44 LY   90");
}

#[test]
fn register_groups_cover_the_expected_addresses() {
    assert!(LCD.iter().any(|x| x.addr == 0xFF40 && x.name == "LCDC"));
    assert!(VARIOUS.iter().any(|x| x.addr == 0xFFFF && x.name == "IE"));
    assert!(SOUND.iter().any(|x| x.addr == 0xFF26 && x.name == "NR52"));
    // No FF15/FF1F (the unused sound slots bgb skips).
    assert!(!SOUND.iter().any(|x| x.addr == 0xFF15 || x.addr == 0xFF1F));
}

#[test]
fn bit_states_decode_msb_first() {
    // LCDC = 0x91 = 1001_0001: LCD on(b7), BG tiles(b4), BG on(b0).
    let states = bit_states(0x91, &LCDC_BITS, 7);
    assert_eq!(states.len(), 8);
    assert_eq!(states[0], ("LCD on", true)); // bit 7
    assert_eq!(states[1], ("WIN map", false)); // bit 6
    assert_eq!(states[3], ("BG tiles", true)); // bit 4
    assert_eq!(states[7], ("BG on", true)); // bit 0

    // STAT = 0x81 = 1000_0001: top decoded bit is 6 (LYC int = bit6 -> 0).
    let stat = bit_states(0x81, &STAT_BITS, 6);
    assert_eq!(stat.len(), 5);
    assert_eq!(stat[0], ("LYC int", false)); // bit 6
    assert_eq!(stat[4], ("LY=LYC", false)); // bit 2
}

#[test]
fn wave_row_reads_all_sixteen_bytes() {
    let read = |a: u16| (a - 0xFF30) as u8; // FF30->0, FF31->1, ...
    assert_eq!(wave_row(read), "000102030405060708090A0B0C0D0E0F");
}

#[test]
fn vector_line_decodes_enable_and_pending() {
    // IF = 0x05 (VBlank + Timer pending), IE = 0x01 (VBlank enabled).
    let (vb, vb_en) = vector_line(0, 0x05, 0x01);
    assert_eq!(vb, "40 VBlank *", "VBlank pending marked");
    assert!(vb_en, "VBlank enabled");
    let (timer, timer_en) = vector_line(2, 0x05, 0x01);
    assert_eq!(timer, "50 Timer *", "Timer pending");
    assert!(!timer_en, "Timer not enabled");
    let (lcd, _) = vector_line(1, 0x05, 0x01);
    assert_eq!(lcd, "48 LCD", "LCD neither pending nor marked");
}

#[test]
fn gbc_groups_cover_the_dma_and_palette_ports() {
    assert!(
        GBC_DMA
            .iter()
            .any(|x| x.addr == 0xFF55 && x.name == "HDMA5")
    );
    assert!(GBC_PAL.iter().any(|x| x.addr == 0xFF68 && x.name == "BCPS"));
    assert!(GBC_PAL.iter().any(|x| x.addr == 0xFF6B && x.name == "OCPD"));
}

#[test]
fn render_group_advances_and_draws() {
    let read = |_a: u16| 0x00u8;
    let (w, h) = (120usize, 200usize);
    let mut buf = vec![0u32; w * h];
    let end;
    {
        let mut c = Canvas::new(&mut buf, w, h);
        end = render_group(&mut c, 0, 0, &read, LCD, &T);
    }
    assert_eq!(end, LCD.len() as i32 * line_height());
    // Something was drawn (text ink) in the first row.
    assert!(buf[..w * line_height() as usize].contains(&T.text));
}
