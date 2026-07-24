use super::*;

/// Declared-flag table matching one real manifest (`msu1.wasm`'s), for tests
/// exercising the plugin-flag path without a live scan.
fn declared_msu1() -> Vec<FlagContribution> {
    vec![FlagContribution {
        name: "msu1".into(),
        arg: "dir".into(),
        help: "Load an MSU-1 streaming-audio pack from DIR".into(),
        default: "$rom_dir".into(),
    }]
}

fn declared_sf2() -> Vec<FlagContribution> {
    vec![FlagContribution {
        name: "sf2".into(),
        arg: "path".into(),
        help: "Supply the SGB N-SPC sample bank from a standard SoundFont-2 file".into(),
        default: String::new(),
    }]
}

fn parse(args: &[&str]) -> Result<ParseOutcome, String> {
    Options::parse(args.iter().map(ToString::to_string), &[])
}

fn parse_declared(args: &[&str], declared: &[FlagContribution]) -> Result<ParseOutcome, String> {
    Options::parse(args.iter().map(ToString::to_string), declared)
}

/// Parse args expected to yield a run (not help).
fn parse_run(args: &[&str]) -> Result<Options, String> {
    match parse(args)? {
        ParseOutcome::Run(opts) => Ok(opts),
        ParseOutcome::Help => panic!("unexpected help outcome for {args:?}"),
    }
}

#[test]
fn ram_init_parses_fill_and_random() {
    use slopgb_core::RamInit;
    assert_eq!(
        parse_run(&["g.gb"]).unwrap().ram_init,
        None,
        "default: none"
    );
    assert_eq!(
        parse_run(&["--ram-init", "fill:0xA5", "g.gb"])
            .unwrap()
            .ram_init,
        Some(RamInit::Fill(0xA5))
    );
    assert_eq!(
        parse_run(&["--ram-init", "fill:ff", "g.gb"])
            .unwrap()
            .ram_init,
        Some(RamInit::Fill(0xFF)),
        "bare hex byte"
    );
    assert_eq!(
        parse_run(&["--ram-init", "random:42", "g.gb"])
            .unwrap()
            .ram_init,
        Some(RamInit::Random(42))
    );
    // Bare `random` must resolve to the fixed DEFAULT_RAM_SEED, not a wildcard:
    // the documented contract (cli.rs) is cross-run reproducibility, so a
    // regression to an entropy/time seed must fail here.
    assert_eq!(
        parse_run(&["--ram-init", "random", "g.gb"])
            .unwrap()
            .ram_init,
        Some(RamInit::Random(DEFAULT_RAM_SEED))
    );
    assert!(parse(&["--ram-init", "bogus", "g.gb"]).is_err());
    assert!(parse(&["--ram-init", "fill:zz", "g.gb"]).is_err());
    assert!(parse(&["--ram-init"]).is_err(), "missing value");
}

#[test]
fn effective_ram_init_cli_beats_uninited_wram_setting() {
    use slopgb_core::RamInit;
    // No CLI, setting off → default (None).
    assert_eq!(effective_ram_init(None, false), None);
    // No CLI, bgb UninitedWRAM on → seeded-random RAM.
    assert_eq!(
        effective_ram_init(None, true),
        Some(RamInit::Random(DEFAULT_RAM_SEED))
    );
    // An explicit --ram-init overrides the persisted toggle.
    assert_eq!(
        effective_ram_init(Some(RamInit::Fill(0)), true),
        Some(RamInit::Fill(0)),
        "CLI wins over UninitedWRAM"
    );
}

#[test]
fn parse_rom_only_defaults() {
    let opts = parse_run(&["game.gb"]).unwrap();
    assert_eq!(opts.rom, Some(PathBuf::from("game.gb")));
    assert_eq!(opts.model, None);
    assert_eq!(opts.scale, 3);
    assert!(!opts.mute);
}

#[test]
fn parse_no_rom_starts_blank() {
    // bgb starts with no ROM: a bare invocation is a valid run, not an error
    // (the CLI execution dependency is removed — a ROM loads later via the menu).
    let opts = parse_run(&[]).unwrap();
    assert_eq!(opts.rom, None);
    assert_eq!(opts.scale, 3);
    // Options without a ROM still parse — only the positional is optional.
    let opts = parse_run(&["--scale", "4", "--mute"]).unwrap();
    assert_eq!(opts.rom, None);
    assert_eq!(opts.scale, 4);
    assert!(opts.mute);
}

#[test]
fn parse_all_options() {
    let opts = parse_run(&[
        "--model", "cgb", "--scale", "5", "--mute", "--boot", "boot.bin", "x.gbc",
    ])
    .unwrap();
    assert_eq!(opts.rom, Some(PathBuf::from("x.gbc")));
    assert_eq!(opts.model, Some(Model::Cgb));
    assert_eq!(opts.scale, 5);
    assert!(opts.mute);
    assert_eq!(opts.boot, Some(PathBuf::from("boot.bin")));
}

#[test]
fn parse_boot_path_and_default() {
    // `--boot <path>` records the boot ROM; absent, it defaults to None.
    let opts = parse_run(&["--boot", "/roms/dmg_boot.bin", "game.gb"]).unwrap();
    assert_eq!(opts.boot, Some(PathBuf::from("/roms/dmg_boot.bin")));
    assert_eq!(parse_run(&["game.gb"]).unwrap().boot, None);
    // A missing value is an error (like --model/--scale).
    assert!(parse(&["--boot"]).is_err());
}

#[test]
fn parse_sgb_bios_path_and_default() {
    // `--sgb-bios <path>` records the SGB BIOS image; absent, it defaults to None.
    let opts = parse_run(&["--sgb-bios", "/roms/sgb.bin", "game.gb"]).unwrap();
    assert_eq!(opts.sgb_bios, Some(PathBuf::from("/roms/sgb.bin")));
    assert_eq!(parse_run(&["game.gb"]).unwrap().sgb_bios, None);
    // A missing value is an error (like --boot).
    assert!(parse(&["--sgb-bios"]).is_err());
}

/// The present-iff rule: `--sf2` is not a built-in flag — it is manifest-declared
/// (`sf2.wasm`) and present iff that plugin is in the scanned plugins dir. This is
/// an ACCEPTED regression for a user with a valid `<hash>.smpl` cache and no
/// plugins dir: their cache hit used to need no plugin at all
/// (`session::load_or_import_sf2`); now `--sf2` itself hard-errors with no
/// `sf2.wasm` present. Do not "fix" this back to a built-in flag — it's the locked
/// contract, not a bug.
#[test]
fn parse_sf2_is_declared_not_builtin() {
    // Empty declared table (no plugins scanned, or sf2.wasm absent): unknown
    // option, same as any other unrecognized flag.
    assert_eq!(
        parse(&["--sf2", "/roms/bank.sf2", "game.gb"]).unwrap_err(),
        "unknown option '--sf2'"
    );
    // With sf2.wasm's manifest declared: the value lands in `plugin_flags`,
    // keyed by the flag's name, not a typed `Options` field.
    let declared = declared_sf2();
    let opts = match parse_declared(&["--sf2", "/roms/bank.sf2", "game.gb"], &declared).unwrap() {
        ParseOutcome::Run(o) => o,
        ParseOutcome::Help => panic!("unexpected help"),
    };
    assert_eq!(
        opts.plugin_flags,
        vec![("sf2".to_string(), "/roms/bank.sf2".to_string())]
    );
    // A missing value is still an error.
    assert_eq!(
        parse_declared(&["--sf2"], &declared).unwrap_err(),
        "--sf2 requires a value"
    );
}

#[test]
fn parse_help_returns_outcome_instead_of_exiting() {
    assert!(matches!(parse(&["-h"]), Ok(ParseOutcome::Help)));
    assert!(matches!(parse(&["--help"]), Ok(ParseOutcome::Help)));
    // Help wins even when mixed with other (even bogus) arguments.
    assert!(matches!(parse(&["x.gb", "--help"]), Ok(ParseOutcome::Help)));
}

#[test]
fn parse_mcp_port() {
    assert_eq!(
        parse_run(&["--mcp-port", "8123", "g.gb"]).unwrap().mcp_port,
        Some(8123)
    );
    assert_eq!(parse_run(&["g.gb"]).unwrap().mcp_port, None);
    assert!(parse(&["--mcp-port"]).is_err()); // value missing
    assert!(parse(&["--mcp-port", "notaport"]).is_err());
}

#[test]
fn parse_plugins_dir() {
    use std::path::PathBuf;
    assert_eq!(
        parse_run(&["--plugins", "/opt/plugins", "g.gb"])
            .unwrap()
            .plugins_dir,
        Some(PathBuf::from("/opt/plugins"))
    );
    assert_eq!(parse_run(&["g.gb"]).unwrap().plugins_dir, None);
    assert!(parse(&["--plugins"]).is_err()); // value missing
}

#[test]
fn parse_rejects_bad_input() {
    assert!(parse(&["--model", "snes", "x.gb"]).is_err());
    assert!(parse(&["--scale", "0", "x.gb"]).is_err());
    assert!(parse(&["--scale", "huge", "x.gb"]).is_err());
    assert!(parse(&["--frobnicate", "x.gb"]).is_err());
    assert!(parse(&["a.gb", "b.gb"]).is_err());
    assert!(parse(&["--model"]).is_err()); // value missing
}

#[test]
fn parse_model_accepts_every_variant() {
    for (s, m) in [
        ("dmg", Model::Dmg),
        ("dmg0", Model::Dmg0),
        ("mgb", Model::Mgb),
        ("sgb", Model::Sgb),
        ("sgb2", Model::Sgb2),
        ("cgb", Model::Cgb),
        ("agb", Model::Agb),
    ] {
        assert_eq!(parse_model(s).unwrap(), m);
    }
}

/// The present-iff rule: a plugin-contributed flag exists only while its plugin
/// is present in the resolved plugins dir. With an empty declared table (no
/// plugins dir, or the plugin absent from it), `--msu1` is a hard `unknown option`
/// error — never a soft warning, and never silently accepted.
#[test]
fn undeclared_plugin_flag_is_a_hard_unknown_option_error() {
    assert_eq!(
        parse(&["--msu1", "/roms", "game.gb"]).unwrap_err(),
        "unknown option '--msu1'"
    );
}

#[test]
fn declared_plugin_flag_parses_into_plugin_flags() {
    let declared = declared_msu1();
    let opts = match parse_declared(&["--msu1", "/roms/pack", "game.gb"], &declared).unwrap() {
        ParseOutcome::Run(o) => o,
        ParseOutcome::Help => panic!("unexpected help"),
    };
    assert_eq!(opts.rom, Some(PathBuf::from("game.gb")));
    assert_eq!(
        opts.plugin_flags,
        vec![("msu1".to_string(), "/roms/pack".to_string())]
    );
    // A missing value is still an error, same as a built-in flag.
    assert_eq!(
        parse_declared(&["--msu1"], &declared).unwrap_err(),
        "--msu1 requires a value"
    );
}

#[test]
fn declared_flag_with_arg_none_needs_no_value() {
    let declared = vec![FlagContribution {
        name: "verbose-chip".into(),
        arg: "none".into(),
        help: "h".into(),
        default: String::new(),
    }];
    let opts = match parse_declared(&["--verbose-chip", "game.gb"], &declared).unwrap() {
        ParseOutcome::Run(o) => o,
        ParseOutcome::Help => panic!("unexpected help"),
    };
    assert_eq!(opts.rom, Some(PathBuf::from("game.gb")));
    assert_eq!(
        opts.plugin_flags,
        vec![("verbose-chip".to_string(), String::new())]
    );
}

/// `--help` splices in declared plugin flags' help lines and omits them when
/// the table is empty — the built-in text otherwise stays byte-identical.
#[test]
fn help_includes_declared_flags_and_excludes_them_when_empty() {
    let bare = usage(&[]);
    assert!(!bare.contains("--msu1"));
    assert!(!bare.contains("--sf2"));
    // The built-in OPTIONS/KEYS text survives untouched around the splice
    // point (where --sf2/--msu1 used to be hardcoded).
    assert!(bare.contains("--plugins <DIR>"));
    // The exact 4-space `OPTIONS`-column indent survives the head/tail splice
    // (regression check: a stray `"\` continuation on `USAGE_TAIL` once ate it).
    assert!(bare.contains("\n    --ram-init <SPEC>"));

    let declared = declared_msu1();
    let with_msu1 = usage(&declared);
    assert!(with_msu1.contains("--msu1 <DIR>"));
    assert!(with_msu1.contains("Load an MSU-1 streaming-audio pack from DIR"));
    // Still doesn't invent a flag that wasn't declared.
    assert!(!with_msu1.contains("--sf2"));
}
