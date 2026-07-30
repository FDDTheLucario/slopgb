//! Dev-only workspace tasks, invoked via the `cargo xtask` alias (std only, plus
//! the std-only/zero-dep `slopgb-sf2` for `gen-sf2`). Two jobs: build the
//! coprocessor plugin crates to wasm and stage them under the fixed filenames
//! the loaders look up, and export the SGB system ROM's N-SPC sample bank to a
//! standard SF2 file.
//!
//! ```text
//! cargo xtask stage-plugins <dir>
//! cargo xtask gen-sf2 <program.rom> <out.sf2>
//! ```
//!
//! A plain `cargo build` produces native cdylibs (`.so`/`.dylib`/`.dll`) the
//! wasm host can't load; and even the wasm build lands under crate-prefixed
//! names (`slopgb_spc700_plugin.wasm`) that don't match the names the SGB
//! coprocessor / MSU-1 / SF2 seams open. This does both: `--target
//! wasm32-unknown-unknown` build, then copy to `spc700.wasm` / `w65c816.wasm` /
//! `msu1.wasm` / `snes-ppu.wasm` / `sf2.wasm`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Each plugin: its cargo package, the wasm artifact cargo emits, and the
/// filename the loader opens (`msu1.rs` `msu1.wasm`; `session.rs`
/// `spc700.wasm`/`w65c816.wasm`/`snes-ppu.wasm`; the frontend's `--sf2` path
/// `sf2.wasm`). Staging them all into one dir serves every seam: the SGB
/// coprocessor auto-loads `spc700.wasm`/`w65c816.wasm`/`snes-ppu.wasm` from the
/// `--plugins` dir on SGB models, `--sf2` reads `sf2.wasm` from that same dir
/// on an SF2 cache miss, and MSU-1 reads `msu1.wasm` from its own `--msu1`
/// pack — each seam looks up only the file(s) it needs.
const PLUGINS: &[Plugin] = &[
    Plugin {
        pkg: "slopgb-spc700-plugin",
        artifact: "slopgb_spc700_plugin.wasm",
        staged: "spc700.wasm",
    },
    Plugin {
        pkg: "slopgb-w65c816-plugin",
        artifact: "slopgb_w65c816_plugin.wasm",
        staged: "w65c816.wasm",
    },
    Plugin {
        pkg: "slopgb-msu1-plugin",
        artifact: "slopgb_msu1_plugin.wasm",
        staged: "msu1.wasm",
    },
    Plugin {
        pkg: "slopgb-snes-ppu-plugin",
        artifact: "slopgb_snes_ppu_plugin.wasm",
        staged: "snes-ppu.wasm",
    },
    Plugin {
        pkg: "slopgb-sf2-plugin",
        artifact: "slopgb_sf2_plugin.wasm",
        staged: "sf2.wasm",
    },
];

struct Plugin {
    pkg: &'static str,
    artifact: &'static str,
    staged: &'static str,
}

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn main() {
    if let Err(e) = run(std::env::args().skip(1).collect()) {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

/// Dispatch a subcommand. Split from `main` so the arg handling is testable.
fn run(args: Vec<String>) -> Fallible {
    match args.split_first() {
        Some((cmd, rest)) if cmd == "stage-plugins" => {
            let dir = rest
                .first()
                .ok_or("usage: cargo xtask stage-plugins <dir>")?;
            stage_plugins(Path::new(dir))
        }
        Some((cmd, rest)) if cmd == "gen-sf2" => match rest {
            [rom, out] => gen_sf2(Path::new(rom), Path::new(out)),
            _ => Err("usage: cargo xtask gen-sf2 <program.rom> <out.sf2>".into()),
        },
        Some((cmd, _)) => Err(format!(
            "unknown task {cmd:?}; expected: stage-plugins <dir> | gen-sf2 <program.rom> <out.sf2>"
        )
        .into()),
        None => {
            Err("usage: cargo xtask stage-plugins <dir> | gen-sf2 <program.rom> <out.sf2>".into())
        }
    }
}

/// The workspace root: xtask's manifest dir is `<root>/xtask`, so its parent is
/// the root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives under the workspace root")
        .to_path_buf()
}

/// The cargo target dir, honouring `CARGO_TARGET_DIR`, else `<root>/target`.
fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

fn stage_plugins(dest: &Path) -> Fallible {
    let root = workspace_root();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    // One cargo invocation builds every plugin crate to wasm.
    let mut build = Command::new(&cargo);
    build
        .current_dir(&root)
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"]);
    for p in PLUGINS {
        build.args(["-p", p.pkg]);
    }
    if !build.status()?.success() {
        return Err("plugin wasm build failed".into());
    }

    let release = target_dir(&root).join("wasm32-unknown-unknown/release");
    std::fs::create_dir_all(dest)?;
    for p in PLUGINS {
        let src = release.join(p.artifact);
        let dst = dest.join(p.staged);
        std::fs::copy(&src, &dst)
            .map_err(|e| format!("copy {} -> {}: {e}", src.display(), dst.display()))?;
        println!("staged {} -> {}", p.artifact, dst.display());
    }
    println!("staged {} plugins into {}", PLUGINS.len(), dest.display());
    Ok(())
}

/// Offset of the SGB system ROM's SPC700 APU upload table (LoROM `$06:8000`),
/// same fixed offset as `slopgb-sgb-coprocessor`'s `samples::TABLE_OFF`.
const TABLE_OFF: usize = 0x3_0000;

/// Parse a standard SNES APU upload table (`[u16 len, u16 dest, len bytes]*`
/// terminated by `[0000, entry]`) starting at `off`, returning `(entry,
/// blocks)`. Rejects a malformed table (out-of-bounds length, no terminator, or
/// no block loading the N-SPC engine entry `$0400`). Replicated (not
/// depended-on, to keep xtask off the wasm-runtime-carrying coprocessor crate)
/// from `crates/slopgb-sgb-coprocessor/src/lib.rs`'s `parse_apu_blocks`.
type ApuBlocks = Vec<(u16, Vec<u8>)>;

fn parse_apu_blocks(rom: &[u8], mut off: usize) -> Option<(u16, ApuBlocks)> {
    let mut blocks: ApuBlocks = Vec::new();
    loop {
        let len = u16::from_le_bytes([*rom.get(off)?, *rom.get(off + 1)?]);
        let dest = u16::from_le_bytes([*rom.get(off + 2)?, *rom.get(off + 3)?]);
        off += 4;
        if len == 0 {
            // Terminator: `dest` is the SPC700 execution entry. A valid driver
            // table loads the engine to $0400 and jumps there.
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

/// Export the SGB system ROM's N-SPC sample bank (dir + instrument table +
/// BRR waveforms, uploaded as part of its SPC700 APU block table) to a
/// standard, playable SF2 file.
fn gen_sf2(rom_path: &Path, out_path: &Path) -> Fallible {
    let rom = std::fs::read(rom_path).map_err(|e| format!("read {}: {e}", rom_path.display()))?;
    let (_entry, blocks) = parse_apu_blocks(&rom, TABLE_OFF).ok_or(
        "gen-sf2: no valid SPC700 APU upload table found at LoROM $06:8000 (file offset 0x30000)",
    )?;

    // Assemble the 64 KiB APU RAM image the real SPC700 sees after the
    // upload, so the directory's absolute pointers resolve.
    let mut apu_ram = [0u8; 0x1_0000];
    for (dest, data) in &blocks {
        println!("block: dest=${:04X} len={}", dest, data.len());
        let start = usize::from(*dest);
        let end = start
            .checked_add(data.len())
            .filter(|&e| e <= apu_ram.len())
            .ok_or_else(|| {
                format!(
                    "gen-sf2: block at ${dest:04X} (len {}) runs off the end of APU RAM",
                    data.len()
                )
            })?;
        apu_ram[start..end].copy_from_slice(data);
    }

    // N-SPC fixed layout: dir $4B00..$4C10 (64 x 4B), instrument table
    // $4C30..$4DB0 (64 x 6B). If the ROM uploads either as its own block, its
    // length gives the entry count; if bundled into a larger block, fall back
    // to the fixed maxima.
    let dir_block_len = blocks
        .iter()
        .find(|(d, _)| *d == slopgb_sf2::DIR_DEST)
        .map(|(_, data)| data.len());
    let instr_block_len = blocks
        .iter()
        .find(|(d, _)| *d == slopgb_sf2::INSTR_DEST)
        .map(|(_, data)| data.len());
    let n_dir_max = dir_block_len.map_or(64, |len| len / 4);
    let n_instr_max = instr_block_len.map_or(64, |len| len / 6);

    // Clamp to the leading run of VALID entries so `export_sf2` doesn't error
    // on trailing garbage: a dir entry is valid if its start pointer is a
    // plausible APU address into the BRR region (non-zero, >= BRR_DEST); an
    // instrument entry is valid if its SRCN is in range (after the dir clamp)
    // and it isn't an all-zero/all-$FF empty slot.
    let n_dir = (0..n_dir_max)
        .take_while(|&i| {
            let e = usize::from(slopgb_sf2::DIR_DEST) + i * 4;
            let start = u16::from_le_bytes([apu_ram[e], apu_ram[e + 1]]);
            start != 0 && start >= slopgb_sf2::BRR_DEST
        })
        .count();
    let n_instr = (0..n_instr_max)
        .take_while(|&i| {
            let e = usize::from(slopgb_sf2::INSTR_DEST) + i * 6;
            let entry = &apu_ram[e..e + 6];
            usize::from(entry[0]) < n_dir
                && !entry.iter().all(|&b| b == 0x00)
                && !entry.iter().all(|&b| b == 0xFF)
        })
        .count();

    println!(
        "dir: {} -> n_dir={n_dir}",
        dir_block_len.map_or("bundled (64-max fallback)".to_string(), |l| format!(
            "own block, {l} bytes ({} entries)",
            l / 4
        ))
    );
    println!(
        "instr: {} -> n_instr={n_instr}",
        instr_block_len.map_or("bundled (64-max fallback)".to_string(), |l| format!(
            "own block, {l} bytes ({} entries)",
            l / 6
        ))
    );

    let sf2_bytes = slopgb_sf2::export_sf2(
        &apu_ram,
        slopgb_sf2::DIR_DEST,
        slopgb_sf2::INSTR_DEST,
        n_dir,
        n_instr,
    )
    .map_err(|e| format!("gen-sf2: {e}"))?;
    std::fs::write(out_path, &sf2_bytes)
        .map_err(|e| format!("write {}: {e}", out_path.display()))?;
    println!("wrote {} ({} bytes)", out_path.display(), sf2_bytes.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_names_match_the_loader_contract() {
        // The seams open these exact filenames (msu1.rs PLUGIN_WASM, session.rs
        // coprocessor pair). A typo here would silently stage unusable plugins.
        let staged: Vec<&str> = PLUGINS.iter().map(|p| p.staged).collect();
        assert_eq!(
            staged,
            [
                "spc700.wasm",
                "w65c816.wasm",
                "msu1.wasm",
                "snes-ppu.wasm",
                "sf2.wasm"
            ]
        );
    }

    #[test]
    fn run_rejects_bad_args_without_building() {
        assert!(run(vec![]).is_err(), "no subcommand");
        assert!(run(vec!["bogus".into()]).is_err(), "unknown subcommand");
        assert!(
            run(vec!["stage-plugins".into()]).is_err(),
            "stage-plugins needs a dir"
        );
        assert!(
            run(vec!["gen-sf2".into()]).is_err(),
            "gen-sf2 needs rom + out args"
        );
        assert!(
            run(vec!["gen-sf2".into(), "only-one".into()]).is_err(),
            "gen-sf2 needs both args, not just one"
        );
        // Bogus paths that don't exist: the arg-count check above returns
        // before any I/O, and a nonexistent rom path errors on read rather
        // than doing any building/writing either.
        assert!(
            run(vec![
                "gen-sf2".into(),
                "/nonexistent/no.rom".into(),
                "/nonexistent/no.sf2".into()
            ])
            .is_err(),
            "gen-sf2 with an unreadable rom errors cleanly"
        );
    }
}
