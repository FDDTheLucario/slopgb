//! Tests for the serial Link transport (`link.rs`).

use super::*;
use slopgb_core::{GameBoy, Model};
use std::time::{Duration, Instant};

// ---- Task 8: packet framing (pure) ----

#[test]
fn packet_encode_byte_layout() {
    let p = Packet {
        cmd: cmd::SYNC1,
        b2: 0x12,
        b3: 0x80,
        b4: 0x00,
        timestamp: 0x0403_0201,
    };
    // cmd, b2, b3, b4, then the timestamp little-endian.
    assert_eq!(p.encode(), [104, 0x12, 0x80, 0x00, 0x01, 0x02, 0x03, 0x04]);
}

#[test]
fn packet_roundtrips() {
    for p in [
        Packet::new(cmd::VERSION, 1, 4, 0),
        Packet::new(cmd::SYNC1, 0xAB, 0x80, 0),
        Packet {
            cmd: cmd::SYNC2,
            b2: 0xFF,
            b3: 0,
            b4: 0,
            timestamp: u32::MAX,
        },
        Packet::new(cmd::DISCONNECT, 0, 0, 0),
    ] {
        assert_eq!(Packet::decode(&p.encode()), p);
    }
}

// ---- Task 9: real localhost TCP ----

/// Spin-wait (bounded) for `f` to hold; fail the test on timeout.
fn wait_until(label: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for: {label}");
}

#[test]
fn localhost_self_connect_exchanges_packet() {
    // Listen on an OS-chosen ephemeral port, then dial it.
    let server = LinkSocket::listen(0).expect("bind");
    let port = server.port();
    assert_ne!(port, 0, "ephemeral port chosen");
    let client = LinkSocket::connect("127.0.0.1".into(), port).expect("connect spawn");

    wait_until("both connected", || {
        server.is_connected() && client.is_connected()
    });

    // Client → server.
    let sent = Packet::new(cmd::SYNC1, 0x5A, 0x80, 0);
    client.send(sent);
    let mut got = None;
    wait_until("server receives", || {
        got = server.poll();
        got.is_some()
    });
    assert_eq!(got.unwrap(), sent);

    // Server → client (the other direction).
    let back = Packet::new(cmd::SYNC2, 0x3C, 0x80, 0);
    server.send(back);
    let mut got = None;
    wait_until("client receives", || {
        got = client.poll();
        got.is_some()
    });
    assert_eq!(got.unwrap(), back);
}

#[test]
fn connect_to_dead_peer_then_drop_returns_promptly() {
    // Dial a refused port; the dial retry loop honors the stop flag, so a Drop
    // (Disconnect) joins within one bounded connect attempt — not the OS's
    // multi-minute connect timeout. The test framework would hang on regression.
    let s = LinkSocket::connect("127.0.0.1".into(), 1).expect("spawn");
    assert!(!s.is_connected(), "nobody listening on port 1");
    drop(s); // must not block on a wedged TcpStream::connect
}

#[test]
fn listen_then_disconnect_releases_the_port() {
    // Dropping a listening socket must join its thread and free the port so a
    // re-listen on the same port succeeds (no leaked accept loop). The OS can
    // take a brief moment to release the port after close (std TcpListener has
    // no SO_REUSEADDR without a dep), so retry within a bounded window — the
    // point under test is that the listener is released at all, not instantly.
    let s = LinkSocket::listen(0).expect("bind");
    let port = s.port();
    drop(s); // Drop sets stop + joins the accept thread.
    let mut last_err = None;
    for _ in 0..100 {
        match LinkSocket::listen(port) {
            Ok(_) => return,
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    panic!("port not freed within 1s after drop: {last_err:?}");
}

// ---- Task 10: per-frame core pump (packet routing) ----

/// A ROM that arms an external-clock (slave) transfer: SB <- `byte`, SC <- 0x80,
/// then self-loops. Run a few steps to execute the four setup instructions.
fn slave_arm_rom(byte: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x010A].copy_from_slice(&[
        0x3E, byte, // ld a, byte
        0xE0, 0x01, // ldh (FF01), a   ; SB
        0x3E, 0x80, // ld a, 0x80
        0xE0, 0x02, // ldh (FF02), a   ; SC = transfer + external clock
        0x18, 0xFE, // jr -2           ; self-loop
    ]);
    rom
}

fn armed_slave_gb(byte: u8) -> GameBoy {
    let mut gb = GameBoy::new(Model::Dmg, slave_arm_rom(byte)).unwrap();
    for _ in 0..6 {
        gb.step();
    }
    assert_eq!(gb.debug_read(0xFF02) & 0x80, 0x80, "slave armed");
    gb
}

#[test]
fn apply_sync1_completes_armed_slave_and_replies() {
    let mut link = Link::new();
    let mut gb = armed_slave_gb(0x34); // our outgoing byte
    let reply = link.apply_packet(&mut gb, Packet::new(cmd::SYNC1, 0x12, 0x80, 0));
    let reply = reply.expect("armed slave replies with SYNC2");
    assert_eq!(reply.cmd, cmd::SYNC2);
    assert_eq!(reply.b2, 0x34, "reply carries our outgoing byte");
    assert_eq!(
        gb.debug_read(0xFF01),
        0x12,
        "slave received the master byte"
    );
    assert_eq!(gb.debug_read(0xFF0F) & 0x08, 0x08, "serial IF raised");
}

#[test]
fn apply_sync1_with_no_pending_transfer_stashes_byte_no_reply() {
    let mut link = Link::new();
    let mut gb = GameBoy::new(Model::Dmg, slave_arm_rom(0x00)).unwrap(); // not stepped: idle
    let reply = link.apply_packet(&mut gb, Packet::new(cmd::SYNC1, 0x12, 0x80, 0));
    assert!(reply.is_none(), "idle port sends no reply");
}

#[test]
fn apply_disconnect_tears_down_and_detaches_core() {
    let mut link = Link::new();
    let mut gb = GameBoy::new(Model::Dmg, slave_arm_rom(0x00)).unwrap();
    gb.link_connect(true);
    let reply = link.apply_packet(&mut gb, Packet::new(cmd::DISCONNECT, 0, 0, 0));
    assert!(reply.is_none());
    assert!(link.status_label().is_none(), "socket torn down");
    assert!(!link.is_connected() && !link.is_listening());
    assert!(!gb.link_connected(), "core peer detached");
}

#[test]
fn pump_is_a_noop_when_not_connected() {
    let mut link = Link::new();
    let mut gb = GameBoy::new(Model::Dmg, slave_arm_rom(0x00)).unwrap();
    link.pump(&mut gb); // no socket
    assert!(
        !gb.link_connected(),
        "pump never attaches the core without a peer"
    );
    assert!(link.status_label().is_none());
}

#[test]
fn pump_tears_down_on_peer_disconnect() {
    // Connect a Link to a peer, attach the core, then have the peer drop. The
    // next pumps must detect the dead connection and tear the link down (detach
    // the core + drop the socket) — no zombie connected/listening state.
    let server = LinkSocket::listen(0).expect("bind");
    let port = server.port();
    let mut link = Link::new();
    link.connect("127.0.0.1".into(), port).expect("connect");
    let mut gb = GameBoy::new(Model::Dmg, slave_arm_rom(0x00)).unwrap();

    wait_until("link connects", || link.is_connected());
    link.pump(&mut gb);
    assert!(
        gb.link_connected(),
        "core attached after the first connected pump"
    );

    drop(server); // peer closes the TCP connection
    wait_until("pump tears down after peer drop", || {
        link.pump(&mut gb);
        !gb.link_connected()
    });
    assert!(!link.is_active(), "dead socket dropped");
    assert!(!link.is_connected() && !link.is_listening());
}

#[test]
fn status_predicates_reflect_socket_state() {
    let mut link = Link::new();
    assert!(!link.is_listening() && !link.is_connected());
    assert!(link.status_label().is_none(), "no label when idle");
    // Ephemeral port (not the fixed default) so the test is isolated — robust to
    // anything (a parallel run, a live instance) already holding port 8765.
    link.listen_on(0).expect("listen");
    assert!(link.is_listening(), "listening until a peer connects");
    assert!(!link.is_connected());
    assert!(
        link.status_label()
            .is_some_and(|s| s.starts_with("listening")),
        "title shows the listening state"
    );
    let mut gb = GameBoy::new(Model::Dmg, slave_arm_rom(0x00)).unwrap();
    link.disconnect(&mut gb);
    assert!(!link.is_listening() && link.status_label().is_none());
}
