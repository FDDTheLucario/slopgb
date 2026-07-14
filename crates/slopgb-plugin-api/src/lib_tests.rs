//! Host-target tests for the pieces that don't need a wasm runtime.

use super::*;

#[test]
fn capabilities_union_and_contains() {
    let caps = Capabilities::INTROSPECTION.union(Capabilities::MUTATE);
    assert!(caps.contains(Capabilities::INTROSPECTION));
    assert!(caps.contains(Capabilities::MUTATE));
    assert!(!caps.contains(Capabilities::SUBSYSTEM));
    assert!(caps.contains(Capabilities::INTROSPECTION.union(Capabilities::MUTATE)));
}

#[test]
fn capabilities_bits_roundtrip() {
    let caps = Capabilities::INTROSPECTION.union(Capabilities::SUBSYSTEM);
    assert_eq!(Capabilities::from_bits(caps.bits()), caps);
    assert_eq!(caps.bits(), 0b101);
}

#[test]
fn reg_indices_are_dense_and_ordered() {
    for (i, reg) in Reg::ALL.iter().enumerate() {
        assert_eq!(reg.index(), i as i32);
    }
}
