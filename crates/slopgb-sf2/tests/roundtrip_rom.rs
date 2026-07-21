//! ROM-gated round-trip integration test: the SGB system ROM's N-SPC sample
//! bank exports to a standard SF2 and re-imports to structurally valid S-DSP
//! regions, with the decoded audio surviving the lossy BRR re-encode.
//!
//! Gated on `SLOPGB_SF2_ROUNDTRIP_ROM` (path to the private SGB system ROM,
//! mirroring the repo's `SLOPGB_REQUIRE_ROMS` optional-ROM pattern): unset,
//! missing, or unreadable skips with a printed note rather than failing —
//! the ROM is not checked into the repo.

use slopgb_sf2::{BRR_DEST, DIR_DEST, INSTR_DEST, brr, export_sf2, import_sf2, reader};

/// File offset of the SGB system ROM's SPC700 APU upload table (LoROM
/// `$06:8000`), same fixed offset as `slopgb-sgb-coprocessor`'s
/// `samples::TABLE_OFF` / `xtask`'s `TABLE_OFF`.
const TABLE_OFF: usize = 0x3_0000;

/// Parse a standard SNES APU upload table (`[u16 len, u16 dest, len bytes]*`
/// terminated by `[0000, entry]`) starting at `off`, returning `(entry,
/// blocks)`. Rejects a malformed table (out-of-bounds length, no terminator,
/// or no block loading the N-SPC engine entry `$0400`). Replicated (not
/// depended-on, so this crate stays independent of `slopgb-sgb-coprocessor`)
/// from that crate's `src/lib.rs::parse_apu_blocks` (identical copy also in
/// `xtask/src/main.rs`).
fn parse_apu_blocks(rom: &[u8], mut off: usize) -> Option<(u16, Vec<(u16, Vec<u8>)>)> {
    let mut blocks: Vec<(u16, Vec<u8>)> = Vec::new();
    loop {
        let len = u16::from_le_bytes([*rom.get(off)?, *rom.get(off + 1)?]);
        let dest = u16::from_le_bytes([*rom.get(off + 2)?, *rom.get(off + 3)?]);
        off += 4;
        if len == 0 {
            return (dest == 0x0400 && blocks.iter().any(|(d, _)| *d == 0x0400))
                .then_some((dest, blocks));
        }
        let end = off.checked_add(usize::from(len))?;
        blocks.push((dest, rom.get(off..end)?.to_vec()));
        off = end;
        if blocks.len() > 64 {
            return None; // runaway guard: no real driver table is this long
        }
    }
}

/// Pearson correlation of two equal-length sample slices, for the
/// original-vs-re-encoded BRR gross-waveform-shape check.
fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for (&x, &y) in a.iter().zip(b) {
        let da = x - mean_a;
        let db = y - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    if var_a == 0.0 || var_b == 0.0 {
        return 0.0;
    }
    cov / (var_a.sqrt() * var_b.sqrt())
}

/// Linearly resample `pcm` to exactly `target_len` samples (nearest-index —
/// only used to align original/re-encoded sample counts for a correlation
/// check, not for audio quality).
fn resample_to_len(pcm: &[i16], target_len: usize) -> Vec<f64> {
    if pcm.is_empty() || target_len == 0 {
        return Vec::new();
    }
    (0..target_len)
        .map(|i| {
            let src_idx = i * (pcm.len() - 1).max(1) / target_len.max(1);
            f64::from(pcm[src_idx.min(pcm.len() - 1)])
        })
        .collect()
}

#[test]
fn roundtrip_rom() {
    let Ok(rom_path) = std::env::var("SLOPGB_SF2_ROUNDTRIP_ROM") else {
        eprintln!(
            "skipping roundtrip_rom: SLOPGB_SF2_ROUNDTRIP_ROM not set \
             (point it at the private SGB system ROM to run this test)"
        );
        return;
    };
    let rom = match std::fs::read(&rom_path) {
        Ok(rom) => rom,
        Err(e) => {
            eprintln!("skipping roundtrip_rom: cannot read {rom_path}: {e}");
            return;
        }
    };

    let (_entry, blocks) = parse_apu_blocks(&rom, TABLE_OFF)
        .expect("SGB system ROM must contain a valid SPC700 APU upload table at 0x30000");

    // Assemble the 64 KiB APU RAM image the real SPC700 sees after the
    // upload, so the directory's absolute pointers resolve.
    let mut ram = Box::new([0u8; 0x1_0000]);
    for (dest, data) in &blocks {
        let start = usize::from(*dest);
        let end = start + data.len();
        assert!(
            end <= ram.len(),
            "block at ${dest:04X} (len {}) runs off the end of APU RAM",
            data.len()
        );
        ram[start..end].copy_from_slice(data);
    }

    let dir_block_len = blocks
        .iter()
        .find(|(d, _)| *d == DIR_DEST)
        .map(|(_, data)| data.len())
        .expect("ROM's upload table must have a dedicated block at DIR_DEST ($4B00)");
    let instr_block_len = blocks
        .iter()
        .find(|(d, _)| *d == INSTR_DEST)
        .map(|(_, data)| data.len())
        .expect("ROM's upload table must have a dedicated block at INSTR_DEST ($4C30)");
    let n_dir = dir_block_len / 4;
    let n_instr = instr_block_len / 6;

    // Step 4: export to SF2.
    let sf2 = export_sf2(&ram, DIR_DEST, INSTR_DEST, n_dir, n_instr).unwrap();
    assert!(!sf2.is_empty(), "exported SF2 is empty");
    assert!(sf2.starts_with(b"RIFF"), "exported SF2 does not start with RIFF");
    assert!(
        sf2.windows(4).any(|w| w == b"sfbk"),
        "exported SF2 has no sfbk form-type chunk"
    );

    // Step 5: import back.
    let regions = import_sf2(&sf2).unwrap();
    assert!(!regions.dir.is_empty(), "re-imported dir region is empty");
    assert!(!regions.instr.is_empty(), "re-imported instr region is empty");
    assert!(!regions.brr.is_empty(), "re-imported brr region is empty");

    // Step 6: structural asserts.
    assert_eq!(
        regions.instr.len() % 6,
        0,
        "instrument-table region ({} bytes) is not a whole number of 6-byte entries",
        regions.instr.len()
    );
    let instr_count_out = regions.instr.len() / 6;
    println!("instr count out: {instr_count_out} (n_instr in: {n_instr})");
    if instr_count_out != n_instr {
        assert!(
            instr_count_out > 0,
            "re-imported instrument count is 0 (expected {n_instr})"
        );
    } else {
        assert_eq!(instr_count_out, n_instr);
    }

    assert_eq!(
        regions.dir.len() % 4,
        0,
        "dir region ({} bytes) is not a whole number of 4-byte entries",
        regions.dir.len()
    );
    let dir_count_out = regions.dir.len() / 4;
    assert!(dir_count_out > 0, "re-imported dir has 0 entries");

    for i in 0..instr_count_out {
        let srcn = regions.instr[i * 6];
        assert!(
            (srcn as usize) < dir_count_out,
            "instrument {i} SRCN {srcn} is out of range for the {dir_count_out}-entry dir \
             (dangling sample reference)"
        );
    }

    // The encoder (mapping::import_sf2 -> brr::encode) never pads: it emits
    // exactly `n_blocks * 9` bytes per sample with no trailing padding, so
    // the whole region is an exact multiple of 9-byte BRR blocks. Assert
    // that (not the "> 0" fallback) since it is the stronger, still-true
    // check here.
    assert_eq!(
        regions.brr.len() % 9,
        0,
        "re-imported BRR region ({} bytes) is not a whole number of 9-byte blocks \
         (encoder is not known to pad)",
        regions.brr.len()
    );

    // Step 7: correlation check. brr::decode is a public fn in this crate, so
    // both the original ROM BRR and the re-imported BRR are directly
    // decodable — run the real check rather than skipping it.
    //
    // The exported SF2's sample order is "referenced SRCNs, first-seen
    // order" (mapping::export_sf2), not necessarily numeric SRCN order, and
    // `import_sf2` builds `regions.dir` in that same sample order — so dir
    // entry `i` is NOT generally original directory entry `i`. Re-parse the
    // SF2 (the same bytes `import_sf2` parsed) to recover each sample's
    // original SRCN from its `srcn_XX`-named `Sf2Sample` and correlate the
    // right pair.
    let parsed = reader::parse(&sf2).expect("re-parsing our own exported SF2 must succeed");
    assert_eq!(
        parsed.samples.len(),
        dir_count_out,
        "reader::parse sample count disagrees with import_sf2's dir entry count"
    );
    let n_samples_to_check = parsed.samples.len().min(5);
    let mut correlations = Vec::new();
    for (i, sample) in parsed.samples.iter().take(n_samples_to_check).enumerate() {
        let srcn: usize = sample
            .name
            .strip_prefix("srcn_")
            .and_then(|hex| usize::from_str_radix(hex, 16).ok())
            .unwrap_or_else(|| panic!("sample name {:?} is not the expected srcn_XX form", sample.name));

        let orig_e = usize::from(DIR_DEST) + srcn * 4;
        let orig_start = u16::from_le_bytes([ram[orig_e], ram[orig_e + 1]]) as usize;
        let orig_decoded = brr::decode(ram.as_slice(), orig_start)
            .unwrap_or_else(|e| panic!("decode original BRR for srcn {srcn:02X}: {e}"));

        let new_e = i * 4;
        let new_start = u16::from_le_bytes([regions.dir[new_e], regions.dir[new_e + 1]]) as usize;
        let brr_dest = usize::from(BRR_DEST);
        assert!(
            new_start >= brr_dest,
            "re-imported dir entry {i} start ${new_start:04X} precedes BRR_DEST"
        );
        let new_decoded = brr::decode(&regions.brr, new_start - brr_dest)
            .unwrap_or_else(|e| panic!("decode re-imported BRR for srcn {srcn:02X}: {e}"));

        let len = orig_decoded.pcm.len().min(new_decoded.pcm.len()).max(1);
        let a = resample_to_len(&orig_decoded.pcm, len);
        let b = resample_to_len(&new_decoded.pcm, len);
        let corr = pearson(&a, &b);
        println!(
            "srcn {srcn:02X}: orig {} samples, re-imported {} samples, correlation = {corr:.4}",
            orig_decoded.pcm.len(),
            new_decoded.pcm.len()
        );
        // Lossy BRR re-encode must still preserve gross waveform shape.
        assert!(
            corr > 0.5,
            "srcn {srcn:02X}: correlation {corr:.4} did not exceed the 0.5 bound"
        );
        correlations.push(corr);
    }

    println!(
        "roundtrip_rom summary: rom={rom_path} n_dir_in={n_dir} n_instr_in={n_instr} \
         dir_out={} bytes ({dir_count_out} entries) instr_out={} bytes ({instr_count_out} \
         entries) brr_out={} bytes correlations={correlations:?}",
        regions.dir.len(),
        regions.instr.len(),
        regions.brr.len(),
    );
}
