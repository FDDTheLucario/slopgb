//! CDL (code/data logging) frontend helpers: the per-flag background tint for
//! the memory viewer, and a std-only RLE codec for the compressed save file.
//! The core owns the golden-safe flag store; this is pure display + file glue.

/// Background tint (XRGB) for a byte whose CDL access `flag` is `R=1/W=2/X=4`,
/// or `None` for an unvisited byte (flag 0 → no tint). Each channel maps one
/// access so combos read as distinct blends: X(code)→red, W(write)→green,
/// R(read)→blue (X|R = magenta, X|W = yellow, R|W = cyan, all three = grey).
#[must_use]
pub fn cdl_color(flag: u8) -> Option<u32> {
    match flag & 0x07 {
        0 => None,
        f => {
            let red = u32::from(f & 4 != 0) * 0xC0; // execute
            let green = u32::from(f & 2 != 0) * 0xC0; // write
            let blue = u32::from(f & 1 != 0) * 0xC0; // read
            Some((red << 16) | (green << 8) | blue)
        }
    }
}

/// Run-length encode `data` as `(value, count_le_u16)` triples — tuned for the
/// mostly-zero CDL flag array (a 64 KiB all-zero buffer → 6 bytes). Runs cap at
/// `u16::MAX`; a longer run splits into consecutive triples.
#[must_use]
pub fn rle_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let v = data[i];
        let mut run = 1usize;
        while i + run < data.len() && data[i + run] == v && run < u16::MAX as usize {
            run += 1;
        }
        out.push(v);
        out.extend_from_slice(&(run as u16).to_le_bytes());
        i += run;
    }
    out
}

/// Decode [`rle_encode`] output. A trailing partial triple (corrupt/truncated
/// file) is ignored rather than panicking.
///
/// Output is capped at `RLE_DECODE_CAP`: each triple's count is an untrusted
/// u16 (up to 65535, a ~21845× amplification), so a crafted `.cdl` could
/// otherwise resize to tens of GB and abort the process. The largest real CDL
/// layout is under 9 MiB; a decode that would exceed the cap stops, and the
/// resulting wrong-length buffer is rejected by `load_cdl`'s exact-length gate.
#[must_use]
pub fn rle_decode(bytes: &[u8]) -> Vec<u8> {
    // 16 MiB — safely above any real CDL layout (ROM + 16K VRAM + <=128K SRAM +
    // 32K WRAM), so no legitimate file is ever truncated.
    const RLE_DECODE_CAP: usize = 16 << 20;
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let v = bytes[i];
        let count = u16::from_le_bytes([bytes[i + 1], bytes[i + 2]]) as usize;
        if out.len() + count > RLE_DECODE_CAP {
            break;
        }
        out.resize(out.len() + count, v);
        i += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdl_color_none_for_unvisited_and_distinct_per_combo() {
        assert_eq!(cdl_color(0), None, "unvisited → no tint");
        let colors: Vec<u32> = (1..=7).map(|f| cdl_color(f).unwrap()).collect();
        let mut uniq = colors.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 7, "each r/w/x combo has a distinct color");
        // High bits (bit 3+) are ignored — only r/w/x matter.
        assert_eq!(cdl_color(0xF0), None);
        assert_eq!(cdl_color(0x84), cdl_color(4));
    }

    #[test]
    fn rle_round_trips_and_compresses_zeros() {
        let zero = vec![0u8; 65536];
        let enc = rle_encode(&zero);
        assert!(
            enc.len() < 16,
            "all-zero compresses hard: {} bytes",
            enc.len()
        );
        assert_eq!(rle_decode(&enc), zero);

        let mut sparse = vec![0u8; 65536];
        sparse[0x0100] = 4;
        sparse[0xC000] = 3;
        sparse[0xFFFF] = 1;
        assert_eq!(rle_decode(&rle_encode(&sparse)), sparse);

        let dense: Vec<u8> = (0..2000u32).map(|n| (n * 7 % 8) as u8).collect();
        assert_eq!(rle_decode(&rle_encode(&dense)), dense);

        assert_eq!(rle_decode(&rle_encode(&[])), Vec::<u8>::new());
    }

    #[test]
    fn rle_decode_drops_trailing_partial_triple() {
        // Two runs encode to two 3-byte triples; lopping off the last byte
        // leaves the second triple incomplete. The `i + 3 <= len` guard drops
        // that partial triple (no panic) and the first run still decodes — the
        // documented truncation tolerance.
        let data = [0xAAu8, 0xAA, 0xBB, 0xBB, 0xBB];
        let enc = rle_encode(&data);
        assert_eq!(enc.len(), 6, "two runs -> two 3-byte triples");
        let dec = rle_decode(&enc[..enc.len() - 1]);
        assert_eq!(
            dec,
            vec![0xAA, 0xAA],
            "partial triple dropped, first run kept"
        );
    }

    #[test]
    fn rle_decode_caps_hostile_output() {
        // Each triple claims count=0xFFFF; ~100k of them would resize to ~6.5 GB
        // without the cap. The decode must stay bounded (no OOM) and short
        // enough that load_cdl's exact-length gate then rejects it.
        let hostile = [0u8, 0xFF, 0xFF].repeat(100_000);
        let out = rle_decode(&hostile);
        assert!(
            out.len() <= 16 << 20,
            "decode capped, not {} bytes",
            out.len()
        );
    }

    #[test]
    fn cdl_save_load_pipeline_reconstructs_flags() {
        use slopgb_core::{GameBoy, Model};
        // The full save→load data path (minus fs): encode → decode → load_cdl,
        // with the buffer sized to the machine's physical layout.
        let mut gb = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
        gb.set_cdl(true);
        let mut fixture = gb.cdl_flags().unwrap().to_vec();
        fixture[0x0100] = 4; // X at ROM offset 0x100 (bank 0)
        let dec = rle_decode(&rle_encode(&fixture));
        assert!(gb.load_cdl(&dec), "round-tripped buffer matches the layout");
        assert_eq!(gb.cdl_flag(0x0100), 4);
        assert_eq!(gb.cdl_flags().unwrap(), &fixture[..]);
        // A wrong-length (foreign-cart) buffer is rejected.
        assert!(!gb.load_cdl(&[0u8; 100]));
    }
}
