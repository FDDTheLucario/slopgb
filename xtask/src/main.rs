//! Dev-only workspace tasks, invoked via the `cargo xtask` alias (std only, no
//! deps). Today it has one job: build the coprocessor plugin crates to wasm and
//! stage them under the fixed filenames the loaders look up.
//!
//! ```text
//! cargo xtask stage-plugins <dir>
//! ```
//!
//! A plain `cargo build` produces native cdylibs (`.so`/`.dylib`/`.dll`) the
//! wasm host can't load; and even the wasm build lands under crate-prefixed
//! names (`slopgb_spc700_plugin.wasm`) that don't match the names the SGB
//! coprocessor / MSU-1 seams open. This does both: `--target
//! wasm32-unknown-unknown` build, then copy to `spc700.wasm` / `w65c816.wasm` /
//! `msu1.wasm`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Each plugin: its cargo package, the wasm artifact cargo emits, and the
/// filename the loader opens (`msu1.rs` `msu1.wasm`; `session.rs` `spc700.wasm`
/// and `w65c816.wasm`). Staging all three into one dir serves both seams: the
/// SGB coprocessor reads the first two, and MSU-1 reads the third.
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
        Some((cmd, _)) => {
            Err(format!("unknown task {cmd:?}; expected: stage-plugins <dir>").into())
        }
        None => Err("usage: cargo xtask stage-plugins <dir>".into()),
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
            ["spc700.wasm", "w65c816.wasm", "msu1.wasm", "snes-ppu.wasm"]
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
    }
}
