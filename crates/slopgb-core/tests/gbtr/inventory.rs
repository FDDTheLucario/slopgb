//! Whole-collection inventory guard: every `.gb`/`.gbc` file in the
//! game-boy-test-roms checkout must be claimed (≥1 rom×model case) or
//! exempted (documented never-run) by exactly one suite module, so adding a
//! ROM to the collection — or re-pinning it — can never silently fall
//! through the harness. The per-suite `inventory()` hooks partition each
//! suite's own directory; this test stitches them together and walks disk.

use std::collections::{BTreeMap, BTreeSet};

use crate::common;
use crate::harness;

#[test]
fn every_collection_rom_is_claimed_or_exempted_exactly_once() {
    let Some(root) = common::gbtr_root() else {
        common::skip_or_fail_gbtr(
            "collection inventory guard",
            "game-boy-test-roms collection not present",
        );
        return;
    };

    type Inventory = fn() -> (Vec<String>, Vec<String>);
    let suites: [(&str, Inventory); 10] = [
        ("acid", crate::acid::inventory),
        ("age", crate::age::inventory),
        ("blargg", crate::blargg::inventory),
        ("gambatte", crate::gambatte::inventory),
        ("gbmicrotest", crate::gbmicrotest::inventory),
        ("mealybug", crate::mealybug::inventory),
        ("mooneye2022", crate::mooneye2022::inventory),
        ("same_suite", crate::same_suite::inventory),
        ("smallsuites", crate::smallsuites::inventory),
        ("wilbertpol", crate::wilbertpol::inventory),
    ];

    // path -> "suite (claimed|exempted)"; duplicates within or across
    // suites are reported, not silently overwritten.
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    let mut duplicates = Vec::new();
    for (suite, inventory) in suites {
        let (claimed, exempted) = inventory();
        for (kind, paths) in [("claimed", claimed), ("exempted", exempted)] {
            for p in paths {
                let tag = format!("{suite} ({kind})");
                if let Some(prev) = owner.insert(p.clone(), tag.clone()) {
                    duplicates.push(format!("{p}: {prev} AND {tag}"));
                }
            }
        }
    }

    let mut on_disk = Vec::new();
    common::collect_roms(&root, true, &mut on_disk).expect("collection walk failed");
    let on_disk: BTreeSet<String> = on_disk
        .iter()
        .map(|p| harness::rel_unix(&root, p))
        .collect();

    let inventoried: BTreeSet<String> = owner.keys().cloned().collect();
    let unclaimed: Vec<&String> = on_disk.difference(&inventoried).collect();
    let phantom: Vec<String> = inventoried
        .difference(&on_disk)
        .map(|p| format!("{p} ({})", owner[p]))
        .collect();

    assert!(
        duplicates.is_empty() && unclaimed.is_empty() && phantom.is_empty(),
        "collection inventory mismatch\n\
         {} duplicate ownership(s):\n  {}\n\
         {} unclaimed ROM(s) on disk:\n  {}\n\
         {} inventoried path(s) missing from disk:\n  {}",
        duplicates.len(),
        duplicates.join("\n  "),
        unclaimed.len(),
        unclaimed
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  "),
        phantom.len(),
        phantom.join("\n  "),
    );
}
