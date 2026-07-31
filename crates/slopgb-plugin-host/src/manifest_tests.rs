use super::*;

#[test]
fn parses_id_name_roles_and_flags() {
    let blob = "id\tmsu1\nname\tMSU-1 Streaming Audio\nprovides\tmsu1-audio\nflag\tmsu1\tdir\tLoad an MSU-1 pack from DIR\t$rom_dir";
    let m = Manifest::parse(blob.as_bytes()).expect("non-empty manifest parses");
    assert_eq!(m.id, "msu1");
    assert_eq!(m.name, "MSU-1 Streaming Audio");
    assert_eq!(m.provides, ["msu1-audio"]);
    assert_eq!(
        m.flags,
        [FlagContribution {
            name: "msu1".into(),
            arg: "dir".into(),
            help: "Load an MSU-1 pack from DIR".into(),
            default: "$rom_dir".into(),
        }]
    );
}

#[test]
fn flag_with_no_default_field_defaults_to_empty() {
    // A manifest written before the `default` field existed still parses; the
    // missing 5th TAB field reads back as "no default", not a parse failure.
    let blob = "flag\tsf2\tpath\tHelp text";
    let m = Manifest::parse(blob.as_bytes()).unwrap();
    assert_eq!(m.flags[0].default, "");
}

#[test]
fn parses_menu_records() {
    let blob = "id\tspc700\nmenu\tExport SPC\tdump_spc\tspc\nmenu\tSecond\tother_export\tbin";
    let m = Manifest::parse(blob.as_bytes()).unwrap();
    assert_eq!(
        m.menus,
        [
            MenuContribution {
                label: "Export SPC".into(),
                export: "dump_spc".into(),
                ext: "spc".into(),
            },
            MenuContribution {
                label: "Second".into(),
                export: "other_export".into(),
                ext: "bin".into(),
            },
        ]
    );
}

#[test]
fn menu_record_with_empty_label_contributes_nothing() {
    let m = Manifest::parse(b"id\tx\nmenu\t\tdump_spc\tspc").unwrap();
    assert!(m.menus.is_empty());
}

#[test]
fn empty_or_blank_blob_is_undeclared() {
    assert_eq!(Manifest::parse(b""), None);
    assert_eq!(Manifest::parse(b"   \n\t\n"), None);
}

#[test]
fn unknown_records_and_blank_lines_are_ignored_forward_compat() {
    // `requires` isn't parsed by this host version (and may never be — the
    // point is any record type this host doesn't recognize yet must still
    // parse (ignored), not fail, so the schema can grow without an ABI bump.
    let blob = "id\tx\n\nrequires\tsnes-cpu\n";
    let m = Manifest::parse(blob.as_bytes()).unwrap();
    assert_eq!(m.id, "x");
    assert!(m.provides.is_empty());
    assert!(m.flags.is_empty());
    assert!(m.menus.is_empty());
}

#[test]
fn empty_field_values_do_not_push_junk() {
    // A `provides`/`flag`/`menu` record with no value contributes nothing.
    let m = Manifest::parse(b"id\ty\nprovides\t\nflag\t\nmenu\t").unwrap();
    assert_eq!(m.id, "y");
    assert!(m.provides.is_empty());
    assert!(m.flags.is_empty());
    assert!(m.menus.is_empty());
}

#[test]
fn non_utf8_is_none() {
    assert_eq!(Manifest::parse(&[0xff, 0xfe]), None);
}
