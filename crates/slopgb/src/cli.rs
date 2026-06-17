//! Command-line parsing for the slopgb frontend. Kept pure (no I/O, no exit)
//! so [`Options::parse`] is unit-testable; `main` prints the help / errors.

use std::path::PathBuf;

use slopgb_core::Model;

pub(crate) const USAGE: &str = "\
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
    -h, --help        Print this help

KEYS:
    Z = A        X = B        Enter = Start    RShift/Backspace = Select
    Arrow keys = D-pad        Tab (hold) = turbo
    P = pause    R = reset    Esc = quit       F9 = break/resume
    F2 = debugger    F3 = VRAM viewer    F4 = I/O map  (bgb-style debug windows)

When the debugger window is focused its keys follow bgb: F2 toggle breakpoint,
F3 step over, F7 trace (step), F4 run to cursor, Ctrl+G go to, F5/F10 open the
VRAM viewer / I/O map. Right-click a debugger pane for its context menu.

A ROM file dropped onto the window is loaded in place of the current one.
Set SLOPGB_OPEN_TOOLS=debugger,vram,iomap to open debug windows at startup.
Serial link cable: open the game-window right-click Link menu (Listen / Connect),
or set SLOPGB_LINK_LISTEN=1 / SLOPGB_LINK_CONNECT=host:port to link at startup.
";

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
}

/// What a successful argument parse asks the program to do. Printing the
/// help text (and exiting) is the caller's job, keeping `parse` pure and
/// testable.
pub(crate) enum ParseOutcome {
    Run(Options),
    Help,
}

impl Options {
    pub(crate) fn parse(mut args: impl Iterator<Item = String>) -> Result<ParseOutcome, String> {
        let mut rom = None;
        let mut model = None;
        let mut scale = 3u32;
        let mut mute = false;
        let mut boot = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(ParseOutcome::Help),
                "--mute" => mute = true,
                "--boot" => {
                    let v = args.next().ok_or("--boot requires a path")?;
                    boot = Some(PathBuf::from(v));
                }
                "--model" => {
                    let v = args.next().ok_or("--model requires a value")?;
                    model = Some(parse_model(&v)?);
                }
                "--scale" => {
                    let v = args.next().ok_or("--scale requires a value")?;
                    scale = v
                        .parse::<u32>()
                        .ok()
                        .filter(|n| (1..=16).contains(n))
                        .ok_or_else(|| format!("invalid --scale '{v}' (expected 1-16)"))?;
                }
                s if s.starts_with('-') => return Err(format!("unknown option '{s}'")),
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
        }))
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
