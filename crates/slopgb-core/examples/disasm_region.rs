//! Disassemble a region of a raw binary (e.g. a boot ROM) for analysis.
//! `cargo run -p slopgb-core --example disasm_region -- <bin> <start_hex> <end_hex>`
use slopgb_core::debug::decode;

fn main() {
    let mut a = std::env::args().skip(1);
    let bin = std::fs::read(a.next().expect("bin")).expect("read");
    let start = u16::from_str_radix(a.next().expect("start").trim_start_matches("0x"), 16).unwrap();
    let end = u16::from_str_radix(a.next().expect("end").trim_start_matches("0x"), 16).unwrap();
    let mut pc = start;
    while pc < end {
        let ins = decode(&bin[pc as usize..], pc);
        let raw: Vec<String> = (0..ins.len)
            .map(|i| format!("{:02X}", bin[pc as usize + i as usize]))
            .collect();
        println!("{:04X}  {:<8}  {}", pc, raw.join(" "), ins.text);
        pc = pc.wrapping_add(u16::from(ins.len.max(1)));
    }
}
