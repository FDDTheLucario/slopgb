use super::*;

fn parse(args: &[&str]) -> Result<ParseOutcome, String> {
    Options::parse(args.iter().map(ToString::to_string))
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
    assert!(matches!(
        parse_run(&["--ram-init", "random", "g.gb"])
            .unwrap()
            .ram_init,
        Some(RamInit::Random(_))
    ));
    assert!(parse(&["--ram-init", "bogus", "g.gb"]).is_err());
    assert!(parse(&["--ram-init", "fill:zz", "g.gb"]).is_err());
    assert!(parse(&["--ram-init"]).is_err(), "missing value");
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
