//! Startup-resource reconciliation tests.

use super::*;
use crate::windows::options::{PluginConfig, PluginEntry};

fn cfg(entries: Vec<PluginEntry>) -> PluginConfig {
    PluginConfig {
        dir: String::new(),
        allow_mutation: false,
        entries,
    }
}

fn off(name: &str, capabilities: &str) -> PluginEntry {
    PluginEntry {
        name: name.to_owned(),
        capabilities: capabilities.to_owned(),
        enabled: false,
    }
}

/// The guard: a name read back from the **tier-1** `disabled` key must not be
/// able to turn a subsystem plugin off by name collision. Builds before the
/// subsystem toggle wrote every discovered subsystem plugin into that key, so
/// honouring it would silently kill SGB audio on upgrade. Only the tier-3
/// `disabled_subsystems` key speaks for a subsystem plugin.
#[test]
fn a_stale_tier1_disabled_name_cannot_disable_a_subsystem_plugin() {
    // The stale shape a pre-toggle build wrote: every discovered subsystem
    // plugin in the tier-1 key, as a capability-less placeholder.
    let stale = cfg(vec![
        off("tracer", ""),
        off("spc700", ""),
        off("w65c816", ""),
    ]);
    let subsystem = ["spc700".to_owned(), "w65c816".to_owned()];
    assert_eq!(
        disabled_to_apply(&stale, &subsystem),
        vec!["tracer".to_owned()],
        "only the real tier-1 disable is applied"
    );
}

/// The tier-3 key does turn a subsystem plugin off, and leaves the per-frame
/// one alone.
#[test]
fn the_tier3_key_disables_a_subsystem_plugin() {
    let c = cfg(vec![off("spc700", "subsystem")]);
    assert_eq!(c.disabled_subsystem_names(), vec!["spc700".to_owned()]);
    assert!(c.disabled_names().is_empty());
    assert_eq!(
        disabled_to_apply(&c, &["spc700".to_owned()]),
        vec!["spc700".to_owned()]
    );
}

/// A subsystem plugin named in BOTH keys (the tier-1 one stale, the tier-3 one
/// a real choice) is still disabled exactly once — the stale name is dropped,
/// the real one applies.
#[test]
fn both_keys_naming_a_subsystem_plugin_disables_it_once() {
    let c = cfg(vec![off("spc700", ""), off("spc700", "subsystem")]);
    assert_eq!(
        disabled_to_apply(&c, &["spc700".to_owned()]),
        vec!["spc700".to_owned()]
    );
}
