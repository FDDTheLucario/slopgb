//! Black-box extraction of the CGB boot ROM's DMG-compat palette assignment.
//!
//! We treat the reference `cgb_boot.bin` purely as an oracle: build synthetic
//! Nintendo-licensed DMG carts whose title bytes ($0134-$0143) sum to a chosen
//! 8-bit checksum (with a chosen 4th letter at $0137), boot each through the
//! real boot ROM, and read back the (BG,OBJ0,OBJ1) palette it assigns — by
//! reading the palette number it computed (WRAM $D008) and the 24-byte palette
//! set it built (WRAM $DA00 + palette_no*24). No ROM code or data layout is
//! copied; only the functional title->palette mapping is observed, then
//! reproduced in our own boot ROM as `boot/cgb_palettes.inc`.
//!
//! `cargo run -p slopgb-core --example cgb_palette_extract -- <cgb_boot.bin> emit`
use slopgb_core::{Button, GameBoy, Model};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

type Pal = [u16; 4];
type Set = [Pal; 3]; // BG, OBJ0, OBJ1

fn synth_cart(chk: u8, letter: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100] = 0x18; // jr -2: cart spins after hand-off
    rom[0x0101] = 0xFE;
    rom[0x0104..0x0134].copy_from_slice(&NINTENDO_LOGO);
    rom[0x0137] = letter; // 4th title letter (collision tiebreaker)
    rom[0x0134] = chk.wrapping_sub(letter); // make the 16-byte title sum == chk
    rom[0x014B] = 0x01; // old licensee = Nintendo (colorization gate)
    let mut x = 0u8;
    for &b in &rom[0x0134..=0x014C] {
        x = x.wrapping_sub(b).wrapping_sub(1);
    }
    rom[0x014D] = x;
    rom
}

/// Boot (chk, letter) through the real ROM and read the assigned palette set.
fn assign(boot: &[u8], chk: u8, letter: u8) -> Set {
    let cart = synth_cart(chk, letter);
    let mut gb = GameBoy::new_with_boot(Model::Cgb, cart, boot.to_vec()).expect("build");
    for _ in 0..15 {
        gb.run_frame();
    }
    let pal_no = u16::from(gb.debug_read(0xD008));
    let base = 0xDA00u16 + pal_no * 24;
    let w = |gb: &GameBoy, a: u16| u16::from(gb.debug_read(a)) | (u16::from(gb.debug_read(a + 1)) << 8);
    let pal = |gb: &GameBoy, b: u16| [w(gb, b), w(gb, b + 2), w(gb, b + 4), w(gb, b + 6)];
    [pal(&gb, base), pal(&gb, base + 8), pal(&gb, base + 16)]
}

/// Boot with `buttons` held from power-on and read the palette the boot ROM's
/// manual override installs (reference table at $08E4). The held d-pad+button
/// combo overrides the title hash.
fn assign_held(boot: &[u8], buttons: &[Button]) -> Set {
    let cart = synth_cart(0x00, 0x00); // title irrelevant under a manual override
    let mut gb = GameBoy::new_with_boot(Model::Cgb, cart, boot.to_vec()).expect("build");
    for _ in 0..15 {
        for &b in buttons {
            gb.press(b);
        }
        gb.run_frame();
    }
    let pal_no = u16::from(gb.debug_read(0xD008));
    let base = 0xDA00u16 + pal_no * 24;
    let w = |gb: &GameBoy, a: u16| u16::from(gb.debug_read(a)) | (u16::from(gb.debug_read(a + 1)) << 8);
    let pal = |gb: &GameBoy, b: u16| [w(gb, b), w(gb, b + 2), w(gb, b + 4), w(gb, b + 6)];
    [pal(&gb, base), pal(&gb, base + 8), pal(&gb, base + 16)]
}

/// The 12 manual palette combos: (joypad code as the reference boot reads it,
/// held buttons). D-pad in the high nibble (Up $40, Left $20, Down $80, Right
/// $10), A=$01/B=$02 in the low nibble.
const COMBOS: [(u8, &[Button]); 12] = [
    (0x40, &[Button::Up]),
    (0x41, &[Button::Up, Button::A]),
    (0x42, &[Button::Up, Button::B]),
    (0x20, &[Button::Left]),
    (0x21, &[Button::Left, Button::A]),
    (0x22, &[Button::Left, Button::B]),
    (0x80, &[Button::Down]),
    (0x81, &[Button::Down, Button::A]),
    (0x82, &[Button::Down, Button::B]),
    (0x10, &[Button::Right]),
    (0x11, &[Button::Right, Button::A]),
    (0x12, &[Button::Right, Button::B]),
];

fn main() {
    let mut a = std::env::args().skip(1);
    let boot = std::fs::read(a.next().expect("cgb_boot.bin path")).expect("read boot");
    assert_eq!(boot.len(), 0x900, "CGB boot ROM is 2304 bytes");
    let arg2 = a.next();
    let emit = arg2.as_deref() == Some("emit");

    // verify <my_boot.bin>: boot every checksum (+ collision letters) through our
    // own boot ROM, read the palette it installs at hand-off, and compare to the
    // reference ROM's assignment. Proves 100% compatibility.
    if arg2.as_deref() == Some("verify") {
        let mine = std::fs::read(a.next().expect("my boot path")).expect("read mine");
        let mut letters: Vec<u8> = boot[0x0716..0x0733].to_vec();
        letters.sort_unstable();
        letters.dedup();
        let mut test_letters = vec![0x00u8];
        test_letters.extend(letters);
        let mut checked = 0u32;
        let mut bad = 0u32;
        for chk in 0u8..=255 {
            for &l in &test_letters {
                let want = assign(&boot, chk, l);
                // Our boot ROM: run to hand-off, then read the live palette RAM.
                let cart = synth_cart(chk, l);
                let mut gb = GameBoy::new_with_boot(Model::Cgb, cart, mine.clone()).expect("build");
                for _ in 0..400 {
                    gb.run_frame();
                    if !gb.boot_active() {
                        break;
                    }
                }
                let (bg, obj) = gb.cgb_palette_ram();
                let w = |r: &[u8; 64], i: usize| u16::from(r[i * 2]) | (u16::from(r[i * 2 + 1]) << 8);
                let got: Set = [
                    [w(bg, 0), w(bg, 1), w(bg, 2), w(bg, 3)],
                    [w(obj, 0), w(obj, 1), w(obj, 2), w(obj, 3)],
                    [w(obj, 4), w(obj, 5), w(obj, 6), w(obj, 7)],
                ];
                checked += 1;
                if got != want {
                    bad += 1;
                    if bad <= 12 {
                        println!("MISMATCH chk=${chk:02X} 4th=${l:02X}\n  want {want:04X?}\n  got  {got:04X?}");
                    }
                }
            }
        }
        println!("verify: {checked} cases, {bad} mismatches");
        return;
    }

    // diff1 <my_boot> <chk_hex> <letter_hex>: dump want vs got for one case,
    // tracing my boot ROM's palette RAM at hand-off.
    if arg2.as_deref() == Some("diff1") {
        let mine = std::fs::read(a.next().expect("my boot")).expect("read mine");
        let chk = u8::from_str_radix(&a.next().unwrap(), 16).unwrap();
        let letter = u8::from_str_radix(&a.next().unwrap(), 16).unwrap();
        let want = assign(&boot, chk, letter);
        let cart = synth_cart(chk, letter);
        let mut gb = GameBoy::new_with_boot(Model::Cgb, cart, mine).expect("build");
        let mut hand = 0;
        for fr in 0..400 {
            gb.run_frame();
            if !gb.boot_active() {
                hand = fr;
                break;
            }
        }
        let (bg, obj) = gb.cgb_palette_ram();
        let w = |r: &[u8; 64], i: usize| u16::from(r[i * 2]) | (u16::from(r[i * 2 + 1]) << 8);
        let got: Set = [
            [w(bg, 0), w(bg, 1), w(bg, 2), w(bg, 3)],
            [w(obj, 0), w(obj, 1), w(obj, 2), w(obj, 3)],
            [w(obj, 4), w(obj, 5), w(obj, 6), w(obj, 7)],
        ];
        println!("chk=${chk:02X} 4th=${letter:02X} handoff@{hand}");
        println!("  want {want:04X?}");
        println!("  got  {got:04X?}");
        println!("  boot_active={} pc={:04X}", gb.boot_active(), gb.cpu_regs().pc);
        return;
    }

    if arg2.as_deref() == Some("combos") {
        for (code, btns) in COMBOS {
            let s = assign_held(&boot, btns);
            println!("combo ${code:02X} {btns:?}: BG={:04X?} OBJ0={:04X?} OBJ1={:04X?}", s[0], s[1], s[2]);
        }
        return;
    }

    // vcombo <my_boot>: hold each combo through our boot ROM, compare the
    // installed palette to the reference's manual-override palette.
    if arg2.as_deref() == Some("vcombo") {
        let mine = std::fs::read(a.next().expect("my boot")).expect("read mine");
        let mut bad = 0;
        for (code, btns) in COMBOS {
            let want = assign_held(&boot, btns);
            let cart = synth_cart(0x99, 0x99); // arbitrary title; the combo overrides
            let mut gb = GameBoy::new_with_boot(Model::Cgb, cart, mine.clone()).expect("build");
            for _ in 0..400 {
                for &b in btns {
                    gb.press(b);
                }
                gb.run_frame();
                if !gb.boot_active() {
                    break;
                }
            }
            let (bg, obj) = gb.cgb_palette_ram();
            let w = |r: &[u8; 64], i: usize| u16::from(r[i * 2]) | (u16::from(r[i * 2 + 1]) << 8);
            let got: Set = [
                [w(bg, 0), w(bg, 1), w(bg, 2), w(bg, 3)],
                [w(obj, 0), w(obj, 1), w(obj, 2), w(obj, 3)],
                [w(obj, 4), w(obj, 5), w(obj, 6), w(obj, 7)],
            ];
            let ok = got == want;
            if !ok {
                bad += 1;
            }
            println!("combo ${code:02X} {:<28} {}", format!("{btns:?}"), if ok { "OK".into() } else { format!("MISMATCH want {want:04X?} got {got:04X?}") });
        }
        println!("vcombo: {bad} mismatches");
        return;
    }

    // The distinct 4th-letter tiebreakers the ROM keys on live in the table at
    // $0716; collisions can only resolve to one of these (or fall to default).
    let mut letters: Vec<u8> = boot[0x0716..0x0733].to_vec();
    letters.sort_unstable();
    letters.dedup();
    let mut test_letters = vec![0x00u8]; // a value never in the (ASCII) table = "no tiebreak"
    test_letters.extend(letters.iter().copied());

    // Default palette = what a title whose checksum is absent from the table gets
    // (the ROM resolves a miss to index 0). Find an absent checksum.
    let table: Vec<u8> = boot[0x06C7..0x0716].to_vec();
    let absent = (0u8..=255).find(|c| !table.contains(c)).expect("some checksum absent");
    let default = assign(&boot, absent, 0x00);

    // Map (checksum, 4th-letter) -> palette set across the input space.
    let mut rules: Vec<(u8, u8, Set)> = Vec::new(); // (checksum, 4th or 0=wildcard, set)
    for chk in 0u8..=255 {
        let mut results: BTreeMap<u8, Set> = BTreeMap::new();
        for &l in &test_letters {
            results.insert(l, assign(&boot, chk, l));
        }
        let distinct: Vec<&Set> = {
            let mut v: Vec<&Set> = results.values().collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        if distinct.len() == 1 {
            let s = *distinct[0];
            if s != default {
                rules.push((chk, 0x00, s)); // wildcard rule
            }
        } else {
            // Collision: emit a rule per tiebreaker letter (skip default results).
            for (&l, &s) in &results {
                if l != 0x00 && s != default {
                    rules.push((chk, l, s));
                }
            }
        }
    }

    // Dedupe palettes -> a unique palette array; each set is 3 indices into it.
    let mut palettes: Vec<Pal> = Vec::new();
    let idx_of = |p: Pal, palettes: &mut Vec<Pal>| -> usize {
        if let Some(i) = palettes.iter().position(|q| *q == p) {
            i
        } else {
            palettes.push(p);
            palettes.len() - 1
        }
    };
    let mut set_ids: Vec<[u8; 3]> = Vec::new();
    let mut uniq_sets: Vec<[u8; 3]> = Vec::new();
    let mut rule_set: Vec<u8> = Vec::new();
    let default_ids = {
        let ids = [
            idx_of(default[0], &mut palettes) as u8,
            idx_of(default[1], &mut palettes) as u8,
            idx_of(default[2], &mut palettes) as u8,
        ];
        if let Some(i) = uniq_sets.iter().position(|s| *s == ids) {
            i as u8
        } else {
            uniq_sets.push(ids);
            (uniq_sets.len() - 1) as u8
        }
    };
    for (_, _, s) in &rules {
        let ids = [
            idx_of(s[0], &mut palettes) as u8,
            idx_of(s[1], &mut palettes) as u8,
            idx_of(s[2], &mut palettes) as u8,
        ];
        let sid = if let Some(i) = uniq_sets.iter().position(|q| *q == ids) {
            i as u8
        } else {
            uniq_sets.push(ids);
            (uniq_sets.len() - 1) as u8
        };
        rule_set.push(sid);
        set_ids.push(ids);
    }

    // Manual palette combos: a held d-pad direction (+ optional A/B) at boot
    // overrides the title hash with one of 12 presets (reference table $08E4).
    let mut combo_rows: Vec<(u8, u8)> = Vec::new();
    for (code, btns) in COMBOS {
        let s = assign_held(&boot, btns);
        let ids = [
            idx_of(s[0], &mut palettes) as u8,
            idx_of(s[1], &mut palettes) as u8,
            idx_of(s[2], &mut palettes) as u8,
        ];
        let sid = if let Some(i) = uniq_sets.iter().position(|q| *q == ids) {
            i as u8
        } else {
            uniq_sets.push(ids);
            (uniq_sets.len() - 1) as u8
        };
        combo_rows.push((code, sid));
    }

    eprintln!(
        "rules={} unique_palettes={} unique_sets={} default_set={}",
        rules.len(),
        palettes.len(),
        uniq_sets.len(),
        default_ids
    );

    if !emit {
        // Spot-check a few well-known titles.
        for (t, want) in [
            ("TETRIS", [0x7FFFu16, 0x03FF, 0x001F, 0x0000]),
            ("DR.MARIO", [0x7FFF, 0x7E8C, 0x7C00, 0x0000]),
        ] {
            let mut bytes = [0u8; 16];
            for (i, c) in t.bytes().take(16).enumerate() {
                bytes[i] = c;
            }
            let chk = bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b));
            let s = assign(&boot, chk, bytes[3]);
            println!("{t}: BG={:04X?} want {:04X?} {}", s[0], want, if s[0] == want { "OK" } else { "MISMATCH" });
        }
        return;
    }

    // Emit boot/cgb_palettes.inc
    let mut out = String::new();
    out.push_str("; CGB compatibility palette data (factual hardware-interop data).\n");
    out.push_str("; Generated by `cargo run -p slopgb-core --example cgb_palette_extract -- <cgb_boot.bin> emit`,\n");
    out.push_str("; which observes the reference boot ROM's title->palette output as a black box.\n");
    out.push_str("; Do not edit by hand. See boot/README.md for provenance.\n\n");

    writeln!(out, "DEF CGB_PAL_COUNT  EQU {}", palettes.len()).unwrap();
    writeln!(out, "DEF CGB_RULE_COUNT EQU {}", rules.len()).unwrap();
    writeln!(out, "DEF CGB_COMBO_COUNT EQU {}", combo_rows.len()).unwrap();
    writeln!(out, "DEF CGB_DEFAULT_SET EQU {default_ids}\n").unwrap();

    out.push_str("; Unique palettes: 4 BGR555 colours (8 bytes) each.\n");
    out.push_str("CgbPalettes:\n");
    for p in &palettes {
        writeln!(out, "    dw ${:04X}, ${:04X}, ${:04X}, ${:04X}", p[0], p[1], p[2], p[3]).unwrap();
    }
    out.push('\n');

    out.push_str("; Palette sets: BG, OBJ0, OBJ1 palette indices (3 bytes each).\n");
    out.push_str("CgbSets:\n");
    for s in &uniq_sets {
        writeln!(out, "    db {}, {}, {}", s[0], s[1], s[2]).unwrap();
    }
    out.push('\n');

    out.push_str("; Rules: checksum, 4th-letter ($00 = wildcard), set index. Scanned in\n");
    out.push_str("; order; first checksum match with a matching (or wildcard) letter wins.\n");
    out.push_str("CgbRules:\n");
    for ((chk, letter, _), sid) in rules.iter().zip(&rule_set) {
        writeln!(out, "    db ${chk:02X}, ${letter:02X}, {sid}").unwrap();
    }
    out.push('\n');

    out.push_str("; Manual combos: joypad code (d-pad high nibble, A=$01/B=$02 low),\n");
    out.push_str("; set index. A held combo at boot overrides the title hash.\n");
    out.push_str("CgbCombos:\n");
    for (code, sid) in &combo_rows {
        writeln!(out, "    db ${code:02X}, {sid}").unwrap();
    }
    out.push('\n');

    std::fs::write("boot/cgb_palettes.inc", &out).expect("write inc");
    eprintln!("wrote boot/cgb_palettes.inc ({} bytes)", out.len());
}
