use super::*;

#[test]
fn parses_id_name_roles_and_flags() {
    let blob = "id\tmsu1\nname\tMSU-1 Streaming Audio\nprovides\tmsu1-audio\nflag\tmsu1\tdir\tLoad an MSU-1 pack from DIR";
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
        }]
    );
}

#[test]
fn empty_or_blank_blob_is_undeclared() {
    assert_eq!(Manifest::parse(b""), None);
    assert_eq!(Manifest::parse(b"   \n\t\n"), None);
}

#[test]
fn unknown_records_and_blank_lines_are_ignored_forward_compat() {
    // `menu` / `requires` aren't parsed by this host version yet; a future
    // plugin declaring them must still parse (ignored), not fail.
    let blob = "id\tx\n\nmenu\tExport SPC\tdump_spc\tspc\nrequires\tsnes-cpu\n";
    let m = Manifest::parse(blob.as_bytes()).unwrap();
    assert_eq!(m.id, "x");
    assert!(m.provides.is_empty());
    assert!(m.flags.is_empty());
}

#[test]
fn empty_field_values_do_not_push_junk() {
    // A `provides`/`flag` record with no value contributes nothing.
    let m = Manifest::parse(b"id\ty\nprovides\t\nflag\t").unwrap();
    assert_eq!(m.id, "y");
    assert!(m.provides.is_empty());
    assert!(m.flags.is_empty());
}

#[test]
fn non_utf8_is_none() {
    assert_eq!(Manifest::parse(&[0xff, 0xfe]), None);
}
