use super::*;

// Headless: stand-in u64 window ids.
type Reg = WindowRegistry<u64>;

#[test]
fn new_registry_is_empty() {
    let r = Reg::new();
    assert!(!r.is_open(ToolWindow::Debugger));
    assert_eq!(r.kind_of(1), None);
    assert_eq!(r.ids().count(), 0);
}

#[test]
fn register_then_route_then_forget() {
    let mut r = Reg::new();
    r.register(10, ToolWindow::Debugger);
    r.register(20, ToolWindow::Vram);
    assert!(r.is_open(ToolWindow::Debugger));
    assert!(r.is_open(ToolWindow::Vram));
    assert!(!r.is_open(ToolWindow::IoMap));
    assert_eq!(r.kind_of(10), Some(ToolWindow::Debugger));
    assert_eq!(r.kind_of(20), Some(ToolWindow::Vram));
    assert_eq!(r.id_of(ToolWindow::Vram), Some(20));
    assert_eq!(r.ids().count(), 2);

    assert_eq!(r.forget(10), Some(ToolWindow::Debugger));
    assert!(!r.is_open(ToolWindow::Debugger));
    assert_eq!(r.kind_of(10), None);
    assert_eq!(r.ids().count(), 1);
}

#[test]
fn forget_unknown_id_is_none() {
    let mut r = Reg::new();
    assert_eq!(r.forget(999), None);
}

#[test]
fn toggle_opens_when_closed_closes_when_open() {
    let mut r = Reg::new();
    assert!(r.toggle_opens(ToolWindow::IoMap), "closed -> should open");
    r.register(5, ToolWindow::IoMap); // caller opened it
    assert!(!r.toggle_opens(ToolWindow::IoMap), "open -> should close");
}

#[test]
fn default_matches_new() {
    let r: Reg = WindowRegistry::default();
    assert_eq!(r.ids().count(), 0);
}
