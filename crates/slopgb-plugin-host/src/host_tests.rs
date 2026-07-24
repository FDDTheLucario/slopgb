//! Load-gating and the host-import round trip, driven by synthetic `.wat`
//! modules so they don't need the wasm32 fixture build.

use slopgb_core::{GameBoy, Model};
use slopgb_plugin_api::ABI_VERSION;

use super::{LoadError, PluginHost};

fn gb() -> GameBoy {
    GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap()
}

fn wasm(src: &str) -> Vec<u8> {
    wat::parse_str(src).unwrap()
}

/// A minimal well-formed introspection plugin, parameterized so tests can bend
/// one field (version / capabilities / body) at a time.
fn plugin_wat(version: i32, caps: i32, on_frame_body: &str) -> String {
    format!(
        r#"(module
          (import "slopgb" "host_read" (func $host_read (param i32) (result i32)))
          (import "slopgb" "host_reg"  (func $host_reg  (param i32) (result i32)))
          (import "slopgb" "host_log"  (func $host_log  (param i32 i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "hi")
          (func (export "slopgb_abi_version")  (result i32) i32.const {version})
          (func (export "slopgb_capabilities") (result i32) i32.const {caps})
          (func (export "slopgb_on_frame")     (result i32) {on_frame_body} i32.const 0)
        )"#
    )
}

#[test]
fn rejects_abi_version_mismatch() {
    let bytes = wasm(&plugin_wat(99, 1, ""));
    let err = PluginHost::load_bytes("bad", &bytes).err().unwrap();
    assert!(
        matches!(err, LoadError::AbiMismatch { found: 99, .. }),
        "{err:?}"
    );
}

#[test]
fn rejects_unsupported_capability() {
    // caps = 3 = INTROSPECTION | MUTATE; MUTATE is not served by this loader.
    let bytes = wasm(&plugin_wat(ABI_VERSION, 0b011, ""));
    let err = PluginHost::load_bytes("greedy", &bytes).err().unwrap();
    assert!(
        matches!(err, LoadError::UnsupportedCapabilities { .. }),
        "{err:?}"
    );
}

#[test]
fn accepts_introspection_plugin() {
    let bytes = wasm(&plugin_wat(ABI_VERSION, 1, ""));
    assert!(PluginHost::load_bytes("ok", &bytes).is_ok());
}

#[test]
fn host_log_reads_guest_memory() {
    // on_frame logs the two bytes "hi" from the data segment at offset 0.
    let bytes = wasm(&plugin_wat(
        ABI_VERSION,
        1,
        "(call $host_log (i32.const 0) (i32.const 2))",
    ));
    let mut host = PluginHost::new();
    host.push(PluginHost::load_bytes("logger", &bytes).unwrap());
    host.pump(&gb());
    assert_eq!(host.take_log(), vec!["[logger] hi".to_string()]);
}

#[test]
fn disabled_plugin_is_skipped_in_pump() {
    // A disabled plugin's on_frame does not fire, so it emits no log; re-enabling
    // resumes it.
    let bytes = wasm(&plugin_wat(
        ABI_VERSION,
        1,
        "(call $host_log (i32.const 0) (i32.const 2))",
    ));
    let mut host = PluginHost::new();
    host.push(PluginHost::load_bytes("logger", &bytes).unwrap());

    host.set_enabled("logger", false);
    host.pump(&gb());
    assert!(host.take_log().is_empty(), "disabled plugin must not log");

    host.set_enabled("logger", true);
    host.pump(&gb());
    assert_eq!(host.take_log(), vec!["[logger] hi".to_string()]);
}

#[test]
fn infos_report_name_caps_and_enabled() {
    let bytes = wasm(&plugin_wat(ABI_VERSION, 1, ""));
    let mut host = PluginHost::new();
    host.push(PluginHost::load_bytes("probe", &bytes).unwrap());
    let infos = host.infos();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].name, "probe");
    assert_eq!(infos[0].capabilities, "introspection");
    assert!(infos[0].enabled);
    host.set_enabled("probe", false);
    assert!(!host.infos()[0].enabled);
}

#[test]
fn load_dir_lists_subsystem_plugins_instead_of_skipping_them() {
    // A SUBSYSTEM plugin (caps 0b100) is a valid, higher-tier plugin the per-frame
    // host doesn't drive — it must be discovered + listed (for the UI), not
    // dropped. An introspection plugin in the same dir is still driven.
    let dir = std::env::temp_dir().join(format!("slopgb-plugin-subsys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("frame.wasm"),
        wasm(&plugin_wat(ABI_VERSION, 0b001, "")),
    )
    .unwrap();
    std::fs::write(
        dir.join("chip.wasm"),
        wasm(&plugin_wat(ABI_VERSION, 0b100, "")), // SUBSYSTEM
    )
    .unwrap();

    let host = PluginHost::load_dir(&dir).unwrap();
    let infos = host.infos();
    assert_eq!(infos.len(), 2, "both plugins listed, none skipped");
    let frame = infos.iter().find(|i| i.name == "frame").unwrap();
    assert_eq!(frame.capabilities, "introspection");
    assert!(frame.enabled, "the per-frame plugin is driven");
    let chip = infos.iter().find(|i| i.name == "chip").unwrap();
    assert_eq!(chip.capabilities, "subsystem");
    assert!(
        chip.enabled,
        "a subsystem plugin starts enabled, like a per-frame one"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn subsystem_plugins_are_togglable_and_survive_a_reload() {
    // The UI's per-plugin checkbox must reach a SUBSYSTEM plugin too: this host
    // does not drive one, but it records the flag so the owning seam (the SGB
    // coprocessor) can read it out of `infos()` when it next builds a machine —
    // and a re-scan preserves it by name, exactly as for a per-frame plugin.
    let dir = std::env::temp_dir().join(format!("slopgb-plugin-subtog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("chip.wasm"),
        wasm(&plugin_wat(ABI_VERSION, 0b100, "")), // SUBSYSTEM
    )
    .unwrap();

    let mut host = PluginHost::load_dir(&dir).unwrap();
    assert!(host.infos()[0].enabled);
    host.set_enabled("chip", false);
    assert!(
        !host.infos()[0].enabled,
        "the toggle reaches a subsystem plugin"
    );
    host.reload();
    assert_eq!(host.infos().len(), 1);
    assert!(
        !host.infos()[0].enabled,
        "the off flag survives a re-scan of the same dir"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reload_rescans_dir_and_preserves_enabled() {
    // load_dir remembers its directory; reload picks up a newly-dropped .wasm and
    // keeps the per-plugin enabled flag across the re-scan.
    let dir = std::env::temp_dir().join(format!("slopgb-plugin-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let one = wasm(&plugin_wat(ABI_VERSION, 1, ""));
    std::fs::write(dir.join("one.wasm"), &one).unwrap();
    let mut host = PluginHost::load_dir(&dir).unwrap();
    assert_eq!(host.infos().len(), 1);

    // Disable it, then drop a second plugin and reload.
    host.set_enabled("one", false);
    std::fs::write(dir.join("two.wasm"), &one).unwrap();
    host.reload();

    let infos = host.infos();
    assert_eq!(infos.len(), 2, "reload must pick up the new plugin");
    let one_info = infos.iter().find(|i| i.name == "one").unwrap();
    assert!(!one_info.enabled, "disabled flag must survive the re-scan");
    assert!(infos.iter().find(|i| i.name == "two").unwrap().enabled);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runaway_on_frame_traps_instead_of_hanging() {
    // A plugin whose on_frame is an infinite loop must NOT hang the host: the
    // metered engine exhausts its per-frame fuel and traps, which pump logs and
    // skips. If fuel weren't enabled this test would hang forever.
    let bytes = wasm(&plugin_wat(ABI_VERSION, 1, "(loop $l br $l)"));
    let mut host = PluginHost::new();
    host.push(PluginHost::load_bytes("runaway", &bytes).unwrap());
    host.pump(&gb()); // returns (traps) rather than spinning
    let log = host.take_log();
    assert_eq!(log.len(), 1, "the trap is logged: {log:?}");
    assert!(log[0].contains("trapped"), "{log:?}");
}

#[test]
fn pump_survives_a_trapping_plugin_and_runs_the_next() {
    // One plugin trapping (here: fuel exhaustion) must not stop the others —
    // pump logs the trap and carries on to the next plugin's on_frame.
    let runaway = wasm(&plugin_wat(ABI_VERSION, 1, "(loop $l br $l)"));
    let good = wasm(&plugin_wat(
        ABI_VERSION,
        1,
        "(call $host_log (i32.const 0) (i32.const 2))",
    ));
    let mut host = PluginHost::new();
    host.push(PluginHost::load_bytes("runaway", &runaway).unwrap());
    host.push(PluginHost::load_bytes("good", &good).unwrap());
    host.pump(&gb());
    let log = host.take_log();
    assert!(
        log.iter().any(|l| l.contains("[runaway] trapped")),
        "{log:?}"
    );
    assert!(log.iter().any(|l| l == "[good] hi"), "{log:?}");
}

#[test]
fn oversized_host_log_len_does_not_overallocate() {
    // A guest asking host_log for i32::MAX bytes must not trigger a ~2 GiB
    // allocation: the read length is clamped to the guest's actual memory size.
    // Reaching the assertion at all proves no OOM/panic on the huge len.
    let bytes = wasm(&plugin_wat(
        ABI_VERSION,
        1,
        "(call $host_log (i32.const 0) (i32.const 2147483647))",
    ));
    let mut host = PluginHost::new();
    host.push(PluginHost::load_bytes("greedy", &bytes).unwrap());
    host.pump(&gb()); // must not panic / OOM
    assert_eq!(host.take_log().len(), 1, "the clamped read still logs once");
}

#[test]
fn rejects_malformed_wasm() {
    let err = PluginHost::load_bytes("junk", b"this is not wasm")
        .err()
        .unwrap();
    assert!(matches!(err, LoadError::Wasm(_)), "{err:?}");
}

#[test]
fn load_dir_skips_a_malformed_wasm_and_keeps_the_good_one() {
    // A garbage `.wasm` in the dir is logged and skipped; a valid plugin beside
    // it still loads (one bad file can't wedge the whole scan).
    let dir = std::env::temp_dir().join(format!("slopgb-plugin-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("broken.wasm"), b"not wasm at all").unwrap();
    std::fs::write(dir.join("ok.wasm"), wasm(&plugin_wat(ABI_VERSION, 1, ""))).unwrap();

    let host = PluginHost::load_dir(&dir).unwrap();
    let infos = host.infos();
    assert_eq!(infos.len(), 1, "only the valid plugin is loaded: {infos:?}");
    assert_eq!(infos[0].name, "ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rejects_module_missing_abi_export() {
    // A module with no `slopgb_abi_version` export is rejected as MissingExport,
    // not accepted or panicked on.
    let src = r#"(module
      (memory (export "memory") 1)
      (func (export "slopgb_capabilities") (result i32) i32.const 1)
      (func (export "slopgb_on_frame")     (result i32) i32.const 0)
    )"#;
    let err = PluginHost::load_bytes("noabi", &wasm(src)).err().unwrap();
    assert!(
        matches!(err, LoadError::MissingExport("slopgb_abi_version")),
        "{err:?}"
    );
}

#[test]
fn host_read_sees_snapshot() {
    // on_frame reads byte $0147 (cartridge type) and logs it back via host_read
    // → store at mem[8] → log 1 byte. Simplest observable path: read then drop,
    // asserting no trap. Value correctness is covered by the fixture round trip.
    let body = "(drop (call $host_read (i32.const 0x0147)))";
    let bytes = wasm(&plugin_wat(ABI_VERSION, 1, body));
    let mut host = PluginHost::new();
    host.push(PluginHost::load_bytes("reader", &bytes).unwrap());
    host.pump(&gb()); // must not trap
    assert!(host.take_log().is_empty());
}
