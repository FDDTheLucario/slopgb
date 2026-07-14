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
    // caps = 3 = INTROSPECTION | MUTATE; MUTATE is not served in phase 1.
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
        1,
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
    let bytes = wasm(&plugin_wat(1, 1, ""));
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
fn reload_rescans_dir_and_preserves_enabled() {
    // load_dir remembers its directory; reload picks up a newly-dropped .wasm and
    // keeps the per-plugin enabled flag across the re-scan.
    let dir = std::env::temp_dir().join(format!("slopgb-plugin-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let one = wasm(&plugin_wat(1, 1, ""));
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
