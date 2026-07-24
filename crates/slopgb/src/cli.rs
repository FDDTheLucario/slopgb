//! Command-line parsing for the slopgb frontend. Kept pure (no I/O, no exit)
//! so [`Options::parse`] is unit-testable; `main` prints the help / errors.
//!
//! Plugin-contributed flags (declared in a coprocessor's manifest — see
//! `slopgb_plugin_host::FlagContribution`) are threaded in as `declared`
//! rather than hardcoded here: a flag exists **iff** its plugin is present in
//! the resolved plugins dir (present-iff rule), so the same `--foo` can be a
//! hard `unknown option` error on one run and a valid flag on the next,
//! depending only on what's in the plugins dir. `main` builds `declared` by
//! pre-scanning the plugins dir before this parse (see `app_boot`).

use std::path::PathBuf;

use slopgb_core::{Model, RamInit};
use slopgb_plugin_host::FlagContribution;

const USAGE_HEAD: &str = "\
slopgb — Game Boy / Game Boy Color emulator

USAGE:
    slopgb [rom.gb|.gbc] [OPTIONS]

Launched without a ROM, slopgb opens to a blank LCD (like bgb); load a ROM
later by dropping a file on the window or via the right-click Load ROM... menu.

OPTIONS:
    --model <MODEL>   Hardware model: dmg, dmg0, mgb, sgb, sgb2, cgb, agb
                      (default: auto-detect from the ROM header)
    --scale <N>       Initial window scale factor, 1-16 (default: 3)
    --mute            Disable audio output
    --boot <PATH>     Execute a boot ROM from power-on (logo + chime); 256 B for
                      DMG/MGB/SGB, 2304 B for CGB/AGB. Also via SLOPGB_BOOT=<path>
    --sgb-bios <PATH> Your own SGB cartridge SNES-side ROM image (SGB models
                      only). Feeds the SGB audio driver; the Nintendo border and
                      title palette are NOT extracted (slopgb never runs the SNES
                      CPU) — the default border stands. Also via SLOPGB_SGB_BIOS
    --mcp-port <N>    Host an MCP server on 127.0.0.1:<N> so an LLM agent can
                      drive the debugger (disassemble/peek/cdl/vram/breakpoint/
                      registers/expr). Also via SLOPGB_MCP_PORT=<N>
    --plugins <DIR>   Load every *.wasm plugin in <DIR>. Tier-1 introspection
                      plugins run in the per-frame pump; the SGB coprocessor
                      (spc700.wasm + w65c816.wasm) auto-loads from here on SGB
                      models. See docs/ui-state/plugin-api.md. Also via
                      SLOPGB_PLUGINS_DIR
";

// No `"\` opening continuation here (unlike `USAGE_HEAD`): that continuation
// trims ALL leading whitespace off the line right after it, and this tail's
// very first line legitimately needs its 4-space `OPTIONS` indent kept.
const USAGE_TAIL: &str =
    "    --ram-init <SPEC> Power-on RAM: fill:HH sets cart SRAM to a byte (default
                      fill:FF); random[:seed] fills all RAM with seeded xorshift
                      garbage (authentic power-on). A .sav still overrides SRAM.
    -h, --help        Print this help

KEYS:
    Z = A        X = B        Enter = Start    RShift/Backspace = Select
    Arrow keys = D-pad        Tab (hold) = turbo
    P = pause    R = reset    Esc = debugger   F9 = break/resume
    F2 = debugger    F3 = VRAM viewer    F4 = I/O map  (bgb-style debug windows)

When the debugger window is focused its keys follow bgb: F2 toggle breakpoint,
F3 step over, F7 trace (step), F4 run to cursor, Ctrl+G go to, F5/F10 open the
VRAM viewer / I/O map. Right-click a debugger pane for its context menu.

A ROM file dropped onto the window is loaded in place of the current one.
Set SLOPGB_OPEN_TOOLS=debugger,vram,iomap to open debug windows at startup.
Serial link cable: open the game-window right-click Link menu (Listen / Connect),
or set SLOPGB_LINK_LISTEN=1 / SLOPGB_LINK_CONNECT=host:port to link at startup.
";

/// The full `--help` text: the built-in `OPTIONS` block plus whatever the
/// current plugins dir's manifests declared (`declared`, in scan order),
/// spliced in where the flags they replaced (`--sf2` / `--msu1`) used to be
/// documented. With no plugins scanned, byte-identical to the old fixed
/// `USAGE` constant.
pub(crate) fn usage(declared: &[FlagContribution]) -> String {
    let mut extra = String::new();
    for f in declared {
        extra.push_str(&flag_usage_block(f));
    }
    format!("{USAGE_HEAD}{extra}{USAGE_TAIL}")
}

/// Column the right (help) side of an `OPTIONS` row starts at — 4-space
/// indent + an 18-wide left field, matching the hand-formatted built-ins.
const HELP_COLUMN: usize = 22;
/// Wrapped-help content width (so the full line stays close to the built-ins'
/// ~80-column wrap).
const HELP_WIDTH: usize = 80 - HELP_COLUMN;

/// Format one declared flag as an `OPTIONS` row: `--name <ARG>` (or bare
/// `--name` for `arg == "none"`, matching `--mute`) in the left column, its
/// help word-wrapped into the right column at [`HELP_COLUMN`].
fn flag_usage_block(f: &FlagContribution) -> String {
    let left = if f.arg == "none" || f.arg.is_empty() {
        format!("    --{}", f.name)
    } else {
        format!("    --{} <{}>", f.name, f.arg.to_ascii_uppercase())
    };
    let pad = " ".repeat(HELP_COLUMN.saturating_sub(left.len()).max(1));
    let mut out = String::new();
    let lines = wrap_help(&f.help, HELP_WIDTH);
    if lines.is_empty() {
        out.push_str(&left);
        out.push('\n');
        return out;
    }
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            out.push_str(&left);
            out.push_str(&pad);
        } else {
            out.push_str(&" ".repeat(HELP_COLUMN));
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Greedy word-wrap of `text` into lines at most `width` columns wide.
fn wrap_help(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[derive(Debug)]
pub(crate) struct Options {
    /// ROM to load at startup, or `None` to boot to a blank LCD (bgb-style) and
    /// load one later via drag-drop / the Load ROM... menu.
    pub(crate) rom: Option<PathBuf>,
    pub(crate) model: Option<Model>,
    pub(crate) scale: u32,
    pub(crate) mute: bool,
    /// Optional boot ROM to execute from power-on (bgb's boot ROM: logo + chime).
    /// `--boot <path>`; falls back to the `SLOPGB_BOOT` env var (resolved in
    /// `main`). `None` = the direct post-boot install (default).
    pub(crate) boot: Option<PathBuf>,
    /// Optional user-supplied SGB BIOS (the SGB cartridge's SNES-side ROM
    /// image). `--sgb-bios <path>`; falls back to `SLOPGB_SGB_BIOS` (resolved in
    /// `main`). `None` = SGB audio silent for the default bank, default border.
    pub(crate) sgb_bios: Option<PathBuf>,
    /// Port for the opt-in MCP debug server (`--mcp-port`; falls back to
    /// `SLOPGB_MCP_PORT`, resolved in `main`). `None` = no server (default).
    pub(crate) mcp_port: Option<u16>,
    /// Directory of `*.wasm` plugins to load (`--plugins`; falls back to
    /// `SLOPGB_PLUGINS_DIR`, resolved in `main`). `None` = no plugins (default,
    /// golden path untouched).
    pub(crate) plugins_dir: Option<PathBuf>,
    /// Power-on RAM initialisation (`--ram-init fill:HH` / `--ram-init
    /// random[:seed]`). `None` = the deterministic 0xFF cart-SRAM default (leaves
    /// the machine byte-identical to `GameBoy::new`).
    pub(crate) ram_init: Option<RamInit>,
    /// Values for CLI flags a scanned plugin's manifest declared (present iff
    /// that plugin is in the resolved plugins dir — see the module doc), keyed
    /// by the flag's declared `name` (no dashes). The plugin consumes its own
    /// value (`Session::set_plugin_flags`); the frontend keeps no typed field.
    /// A flag with `arg == "none"` is recorded with an empty value.
    pub(crate) plugin_flags: Vec<(String, String)>,
}

/// What a successful argument parse asks the program to do. Printing the
/// help text (and exiting) is the caller's job, keeping `parse` pure and
/// testable.
#[derive(Debug)]
pub(crate) enum ParseOutcome {
    Run(Options),
    Help,
}

impl Options {
    /// Parse `args` against the built-in flags plus whatever `declared`
    /// plugin flags the current plugins dir contributed. An unmatched `--foo`
    /// not named in `declared` is a hard `unknown option` error — a flag
    /// exists only while its declaring plugin is present (see module doc).
    pub(crate) fn parse(
        mut args: impl Iterator<Item = String>,
        declared: &[FlagContribution],
    ) -> Result<ParseOutcome, String> {
        let mut rom = None;
        let mut model = None;
        let mut scale = 3u32;
        let mut mute = false;
        let mut boot = None;
        let mut sgb_bios = None;
        let mut mcp_port = None;
        let mut plugins_dir = None;
        let mut ram_init = None;
        let mut plugin_flags = Vec::new();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(ParseOutcome::Help),
                "--mute" => mute = true,
                "--boot" => {
                    let v = args.next().ok_or("--boot requires a path")?;
                    boot = Some(PathBuf::from(v));
                }
                "--sgb-bios" => {
                    let v = args.next().ok_or("--sgb-bios requires a path")?;
                    sgb_bios = Some(PathBuf::from(v));
                }
                "--mcp-port" => {
                    let v = args.next().ok_or("--mcp-port requires a port number")?;
                    mcp_port = Some(
                        v.parse::<u16>()
                            .map_err(|_| format!("invalid --mcp-port '{v}' (expected 0-65535)"))?,
                    );
                }
                "--plugins" => {
                    let v = args.next().ok_or("--plugins requires a directory")?;
                    plugins_dir = Some(PathBuf::from(v));
                }
                "--model" => {
                    let v = args.next().ok_or("--model requires a value")?;
                    model = Some(parse_model(&v)?);
                }
                "--ram-init" => {
                    let v = args.next().ok_or("--ram-init requires a value")?;
                    ram_init = Some(parse_ram_init(&v)?);
                }
                "--scale" => {
                    let v = args.next().ok_or("--scale requires a value")?;
                    scale = v
                        .parse::<u32>()
                        .ok()
                        .filter(|n| (1..=16).contains(n))
                        .ok_or_else(|| format!("invalid --scale '{v}' (expected 1-16)"))?;
                }
                s if s.starts_with('-') => {
                    let found = s
                        .strip_prefix("--")
                        .and_then(|name| declared.iter().find(|f| f.name == name));
                    match found {
                        Some(f) if f.arg == "none" => {
                            plugin_flags.push((f.name.clone(), String::new()));
                        }
                        Some(f) => {
                            let v = args
                                .next()
                                .ok_or_else(|| format!("--{} requires a value", f.name))?;
                            plugin_flags.push((f.name.clone(), v));
                        }
                        None => return Err(format!("unknown option '{s}'")),
                    }
                }
                _ => {
                    if rom.is_some() {
                        return Err(format!("unexpected extra argument '{arg}'"));
                    }
                    rom = Some(PathBuf::from(arg));
                }
            }
        }
        // A missing ROM is no longer an error: slopgb boots to a blank LCD and
        // loads one later (bgb behaviour — the CLI execution dependency is gone).
        Ok(ParseOutcome::Run(Self {
            rom,
            model,
            scale,
            mute,
            boot,
            sgb_bios,
            mcp_port,
            plugins_dir,
            ram_init,
            plugin_flags,
        }))
    }
}

/// Default xorshift seed for a bare `--ram-init random` and for bgb's persisted
/// `UninitedWRAM` toggle — a fixed value so "random" RAM stays reproducible.
pub(crate) const DEFAULT_RAM_SEED: u64 = 0xA5A5_A5A5_A5A5_A5A5;

/// The effective power-on RAM init: an explicit `--ram-init` (CLI) wins; else
/// bgb's persisted `UninitedWRAM` maps to seeded-random RAM; else the default.
pub(crate) fn effective_ram_init(cli: Option<RamInit>, uninited_wram: bool) -> Option<RamInit> {
    cli.or_else(|| uninited_wram.then_some(RamInit::Random(DEFAULT_RAM_SEED)))
}

/// Parse `--ram-init`: `fill:HH` (a hex byte for cart SRAM) or `random[:seed]`
/// (a seeded xorshift over all RAM; bare `random` uses a fixed default seed).
pub(crate) fn parse_ram_init(s: &str) -> Result<RamInit, String> {
    let (kind, arg) = s.split_once(':').map_or((s, None), |(k, v)| (k, Some(v)));
    match kind.to_ascii_lowercase().as_str() {
        "fill" => {
            let v = arg.ok_or("--ram-init fill requires a byte, e.g. fill:0xFF")?;
            Ok(RamInit::Fill(parse_hex_u8(v)?))
        }
        "random" => {
            let seed = match arg {
                Some(v) => {
                    parse_u64(v).ok_or_else(|| format!("invalid --ram-init random seed '{v}'"))?
                }
                None => DEFAULT_RAM_SEED,
            };
            Ok(RamInit::Random(seed))
        }
        _ => Err(format!(
            "unknown --ram-init '{s}' (expected fill:HH or random[:seed])"
        )),
    }
}

/// Parse a byte written as `0xHH` or `HH` (two hex digits — the `fill:HH` form).
fn parse_hex_u8(s: &str) -> Result<u8, String> {
    let h = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u8::from_str_radix(h, 16).map_err(|_| format!("invalid byte '{s}' (expected hex 00-FF)"))
}

/// Parse a u64 seed as `0x…` hex or plain decimal. `None` if malformed.
fn parse_u64(s: &str) -> Option<u64> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse::<u64>().ok(),
    }
}

pub(crate) fn parse_model(s: &str) -> Result<Model, String> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "dmg" => Model::Dmg,
        "dmg0" => Model::Dmg0,
        "mgb" => Model::Mgb,
        "sgb" => Model::Sgb,
        "sgb2" => Model::Sgb2,
        "cgb" => Model::Cgb,
        "agb" => Model::Agb,
        _ => {
            return Err(format!(
                "unknown model '{s}' (expected dmg, dmg0, mgb, sgb, sgb2, cgb or agb)"
            ));
        }
    })
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
