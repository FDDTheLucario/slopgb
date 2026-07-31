use super::*;
use crate::manifest::{FlagContribution, MenuContribution};

fn manifest_with_role(role: &str) -> Manifest {
    Manifest {
        id: role.into(),
        provides: vec![role.into()],
        ..Default::default()
    }
}

#[test]
fn duplicate_role_registration_is_a_hard_error() {
    let mut reg = PluginRegistry::new();
    reg.register("msu1.wasm", manifest_with_role("audio-coprocessor"))
        .unwrap();
    let err = reg
        .register("msu1-old.wasm", manifest_with_role("audio-coprocessor"))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "two plugins provide role 'audio-coprocessor': msu1.wasm, msu1-old.wasm"
    );
}

#[test]
fn empty_registry_is_empty_and_inert() {
    let reg = PluginRegistry::new();
    assert!(reg.is_empty());
    assert!(reg.units().is_empty());
    assert!(reg.flags().is_empty());
    assert!(reg.menus().is_empty());
    assert!(reg.unit_for_role("anything").is_none());
}

#[test]
fn unit_for_role_finds_the_registering_source() {
    let mut reg = PluginRegistry::new();
    reg.register("w65c816.wasm", manifest_with_role("snes-cpu"))
        .unwrap();
    assert_eq!(
        reg.unit_for_role("snes-cpu").unwrap().source,
        "w65c816.wasm"
    );
    assert!(reg.unit_for_role("snes-video").is_none());
}

#[test]
fn flag_prefers_explicit_over_default() {
    let mut reg = PluginRegistry::new();
    let m = Manifest {
        id: "msu1".into(),
        flags: vec![FlagContribution {
            name: "msu1".into(),
            arg: "dir".into(),
            help: "h".into(),
            default: "$rom_dir".into(),
        }],
        ..Default::default()
    };
    reg.register("msu1.wasm", m).unwrap();
    reg.set_context(Context {
        rom_dir: Some("/roms".into()),
        ..Default::default()
    });
    assert_eq!(reg.flag("msu1"), Some("/roms".to_string()));
    reg.set_flag("msu1", "/explicit");
    assert_eq!(reg.flag("msu1"), Some("/explicit".to_string()));
}

#[test]
fn flag_default_ambient_token_is_none_without_context() {
    let mut reg = PluginRegistry::new();
    let m = Manifest {
        id: "msu1".into(),
        flags: vec![FlagContribution {
            name: "msu1".into(),
            arg: "dir".into(),
            help: "h".into(),
            default: "$rom_dir".into(),
        }],
        ..Default::default()
    };
    reg.register("msu1.wasm", m).unwrap();
    assert_eq!(reg.flag("msu1"), None);
}

#[test]
fn flag_empty_default_is_none() {
    let mut reg = PluginRegistry::new();
    let m = Manifest {
        id: "sf2".into(),
        flags: vec![FlagContribution {
            name: "sf2".into(),
            arg: "path".into(),
            help: "h".into(),
            default: String::new(),
        }],
        ..Default::default()
    };
    reg.register("sf2.wasm", m).unwrap();
    assert_eq!(reg.flag("sf2"), None);
}

#[test]
fn flag_literal_default_passes_through() {
    let mut reg = PluginRegistry::new();
    let m = Manifest {
        id: "x".into(),
        flags: vec![FlagContribution {
            name: "x".into(),
            arg: "string".into(),
            help: "h".into(),
            default: "42".into(),
        }],
        ..Default::default()
    };
    reg.register("x.wasm", m).unwrap();
    assert_eq!(reg.flag("x"), Some("42".to_string()));
}

#[test]
fn unset_flag_is_none() {
    let reg = PluginRegistry::new();
    assert_eq!(reg.flag("nope"), None);
}

#[test]
fn late_set_context_is_reflected_without_reregistering() {
    let mut reg = PluginRegistry::new();
    let m = Manifest {
        id: "msu1".into(),
        flags: vec![FlagContribution {
            name: "msu1".into(),
            arg: "dir".into(),
            help: "h".into(),
            default: "$rom_dir".into(),
        }],
        ..Default::default()
    };
    reg.register("msu1.wasm", m).unwrap();
    assert_eq!(reg.flag("msu1"), None);
    reg.set_context(Context {
        rom_dir: Some("/roms".into()),
        ..Default::default()
    });
    assert_eq!(reg.flag("msu1"), Some("/roms".to_string()));
}

#[test]
fn flags_and_menus_pair_with_declaring_source() {
    let mut reg = PluginRegistry::new();
    let m = Manifest {
        id: "spc700".into(),
        flags: vec![FlagContribution {
            name: "f".into(),
            arg: "none".into(),
            help: "h".into(),
            default: String::new(),
        }],
        menus: vec![MenuContribution {
            label: "Export".into(),
            export: "dump".into(),
            ext: "spc".into(),
        }],
        ..Default::default()
    };
    reg.register("spc700.wasm", m).unwrap();

    let flags = reg.flags();
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].0, "spc700.wasm");
    assert_eq!(flags[0].1.name, "f");

    let menus = reg.menus();
    assert_eq!(menus.len(), 1);
    assert_eq!(menus[0].0, "spc700.wasm");
    assert_eq!(menus[0].1.label, "Export");
}

#[test]
fn scan_of_missing_dir_is_io_error() {
    let err = PluginRegistry::scan(Path::new("/definitely/does/not/exist/slopgb-registry-test"))
        .unwrap_err();
    assert!(matches!(err, RegistryError::Io(_)));
}

#[test]
fn scan_of_empty_dir_is_empty_registry() {
    let dir =
        std::env::temp_dir().join(format!("slopgb-registry-scan-empty-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let reg = PluginRegistry::scan(&dir).unwrap();
    assert!(reg.is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn scan_skips_a_malformed_wasm_file_silently() {
    let dir = std::env::temp_dir().join(format!("slopgb-registry-scan-bad-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("bad.wasm"), b"not a real wasm module").unwrap();
    let reg = PluginRegistry::scan(&dir).unwrap();
    assert!(
        reg.is_empty(),
        "a file that fails to load is skipped, not an error"
    );
    fs::remove_dir_all(&dir).ok();
}

/// A coprocessor-shaped module declaring ABI `version` — enough to reach the
/// version check, which `LoadedCoprocessor::load` runs before any other export
/// lookup (so a stale plugin fails here, exactly as in the field).
fn abi_wat(version: i32) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
          (memory (export "memory") 1)
          (func (export "slopgb_abi_version") (result i32) i32.const {version})
        )"#
    ))
    .unwrap()
}

#[test]
fn scan_reports_a_plugin_skipped_for_a_stale_abi() {
    // A plugin left over from an older build still loads as a file but declares
    // the wrong ABI, so it contributes no flags — and the user sees only
    // `unknown option '--sf2'`, which blames the flag rather than the plugin.
    // The skip has to be diagnosable.
    let dir = std::env::temp_dir().join(format!("slopgb-registry-stale-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("sf2.wasm"),
        abi_wat(slopgb_plugin_api::ABI_VERSION - 1),
    )
    .unwrap();
    let reg = PluginRegistry::scan(&dir).unwrap();
    assert!(reg.is_empty(), "a stale plugin contributes nothing");
    let skipped = reg.skipped();
    assert_eq!(skipped.len(), 1, "the stale plugin must be reported");
    assert!(
        skipped[0].contains("sf2.wasm"),
        "the report names the file: {:?}",
        skipped[0]
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn scan_stays_silent_for_a_module_that_is_not_a_coprocessor() {
    // Tier-1 plugins live in the same directory and never export the
    // coprocessor ABI. Reporting those would print a line per plugin on every
    // launch, so a loader mismatch stays silent — only a real coprocessor that
    // cannot be used is worth a word.
    let dir = std::env::temp_dir().join(format!("slopgb-registry-quiet-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("bad.wasm"), b"not a real wasm module").unwrap();
    fs::write(
        dir.join("tier1.wasm"),
        wat::parse_str(r#"(module (memory (export "memory") 1))"#).unwrap(),
    )
    .unwrap();
    let reg = PluginRegistry::scan(&dir).unwrap();
    assert!(
        reg.skipped().is_empty(),
        "a loader mismatch is not a diagnosable skip: {:?}",
        reg.skipped()
    );
    fs::remove_dir_all(&dir).ok();
}
