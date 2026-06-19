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

/// Lockstep task 5: `poll_blocking` waits up to the timeout for a packet,
/// returning it when one arrives and `None` on timeout.
#[test]
fn poll_blocking_returns_packet_or_times_out() {
    let server = LinkSocket::listen(0).expect("bind");
    let port = server.port();
    let client = LinkSocket::connect("127.0.0.1".into(), port).expect("connect");
    wait_until("both connected", || {
        server.is_connected() && client.is_connected()
    });
    // Idle: blocks for ~the timeout, then None.
    let t0 = Instant::now();
    assert!(client.poll_blocking(Duration::from_millis(40)).is_none());
    assert!(t0.elapsed() >= Duration::from_millis(25), "actually waited");
    // A sent packet is delivered.
    let sent = Packet::new(cmd::SYNC2, 0x77, 0x80, 0);
    server.send(sent);
    let mut got = None;
    wait_until("client receives via blocking poll", || {
        got = client.poll_blocking(Duration::from_millis(50));
        got.is_some()
    });
    assert_eq!(got.unwrap(), sent);
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

/// A ROM that runs a master (internal-clock) transfer: SB <- 0, SC <- 0x81,
/// then self-loops. The transfer shifts in whatever the link feeds it.
fn master_xfer_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x010A].copy_from_slice(&[
        0x3E, 0x00, // ld a, 0
        0xE0, 0x01, // ldh (FF01), a   ; SB = 0
        0x3E, 0x81, // ld a, 0x81
        0xE0, 0x02, // ldh (FF02), a   ; SC = transfer + internal clock
        0x18, 0xFE, // jr -2
    ]);
    rom
}

/// Lockstep task 6: a SYNC1 received while we are NOT an armed slave is buffered
/// in the frontend (NOT pushed into the core master queue — no
/// cross-contamination); a stalled master is then fed the buffered byte by the
/// drain, completing without a SYNC2 reply (both-master exchange).
#[test]
fn buffered_byte_feeds_a_stalled_master() {
    let mut link = Link::new();
    let mut gb = GameBoy::new(Model::Dmg, master_xfer_rom()).unwrap();
    gb.link_connect(true);
    let reply = link.apply_packet(&mut gb, Packet::new(cmd::SYNC1, 0x12, 0x80, 0));
    assert!(reply.is_none(), "non-slave port buffers, sends no reply");
    // Our master transfer runs and stalls (lockstep) awaiting a peer byte —
    // proving the buffered byte did NOT leak into the core master queue.
    gb.run_frame();
    assert!(gb.link_stalled(), "master stalls without a buffered core byte");
    let replies = link.drain_pending(&mut gb);
    assert!(replies.is_empty(), "feeding a master emits no SYNC2");
    assert!(!gb.link_stalled(), "the buffered byte completed the stall");
    assert_eq!(gb.debug_read(0xFF01), 0x12, "master received the peer byte");
}

/// Lockstep task 6: a SYNC1 received before our slave is armed is buffered, then
/// completed + replied (SYNC2) by the drain once the slave arms.
#[test]
fn buffered_master_byte_completes_when_slave_arms() {
    let mut link = Link::new();
    // Connected, but the slave is not armed yet (setup not run).
    let mut gb = GameBoy::new(Model::Dmg, slave_arm_rom(0x34)).unwrap();
    gb.link_connect(true);
    let reply = link.apply_packet(&mut gb, Packet::new(cmd::SYNC1, 0x12, 0x80, 0));
    assert!(reply.is_none(), "unarmed port buffers, no immediate reply");
    assert_eq!(gb.debug_read(0xFF01), 0x00, "core SB untouched while buffered");
    // Arm the slave (run the four setup instructions).
    for _ in 0..6 {
        gb.step();
    }
    assert_eq!(gb.debug_read(0xFF02) & 0x80, 0x80, "slave armed");
    // Drain: the buffered byte completes the slave and yields a SYNC2 reply.
    let replies = link.drain_pending(&mut gb);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].cmd, cmd::SYNC2);
    assert_eq!(replies[0].b2, 0x34, "reply carries our outgoing byte");
    assert_eq!(gb.debug_read(0xFF01), 0x12, "slave received the master byte");
    assert_eq!(gb.debug_read(0xFF0F) & 0x08, 0x08, "serial IF raised");
}

/// A serial-exchange ROM: run `count` transfers sending `start`, `start+1`, …,
/// storing each received byte at WRAM 0xC000.. , then self-loop. `sc` selects
/// internal-clock master (0x81) or external-clock slave (0x80).
fn multi_xfer_rom(count: u8, start: u8, sc: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0100..0x011D].copy_from_slice(&[
        0x06, count, // ld b,count
        0x21, 0x00, 0xC0, // ld hl,C000
        0x0E, start, // ld c,start
        0x79, // .loop: ld a,c
        0xE0, 0x01, // ldh (01),a   ; SB
        0x3E, sc, // ld a,sc
        0xE0, 0x02, // ldh (02),a   ; SC
        0xF0, 0x02, // .wait: ldh a,(02)
        0xCB, 0x7F, // bit 7,a
        0x20, 0xFA, // jr nz,.wait
        0xF0, 0x01, // ldh a,(01)   ; received
        0x22, // ld (hl+),a
        0x0C, // inc c
        0x05, // dec b
        0x20, 0xEC, // jr nz,.loop
        0x18, 0xFE, // jr -2
    ]);
    rom
}

fn multi_master_rom() -> Vec<u8> {
    multi_xfer_rom(8, 0xA0, 0x81)
}

fn multi_slave_rom() -> Vec<u8> {
    multi_xfer_rom(8, 0xB0, 0x80)
}

/// Lockstep task 7 (acceptance): two real `Link`s over a localhost socket pair
/// trade an 8-byte block master↔slave. Each side must receive the EXACT bytes
/// the other sent, in order — the old per-frame model corrupted this to 0xFF.
#[test]
fn multi_byte_exchange_over_socket_has_no_corruption() {
    // Master listens on an OS-chosen port; slave dials it.
    let mut link_m = Link::new();
    link_m.listen_on(0).expect("listen");
    let m_port = link_m.port().expect("port");
    let mut link_s = Link::new();
    link_s.connect("127.0.0.1".into(), m_port).expect("connect");

    let mut gb_m = GameBoy::new(Model::Dmg, multi_master_rom()).unwrap();
    let mut gb_s = GameBoy::new(Model::Dmg, multi_slave_rom()).unwrap();

    // Attach both cores BEFORE running — a master that clocks while the link is
    // still dialing would complete disconnected (0xFF) and race ahead.
    wait_until("both connected", || {
        link_m.is_connected() && link_s.is_connected()
    });
    link_m.pump(&mut gb_m);
    link_s.pump(&mut gb_s);
    assert!(gb_m.link_connected() && gb_s.link_connected(), "cores attached");

    let done = |gb: &GameBoy| (0..8).all(|i| gb.debug_read(0xC000 + i) != 0x00);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        gb_m.run_frame(); // stalls on each master transfer
        link_m.pump(&mut gb_m);
        gb_s.run_frame();
        link_s.pump(&mut gb_s);
        if done(&gb_m) && done(&gb_s) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    let recv_m: Vec<u8> = (0..8).map(|i| gb_m.debug_read(0xC000 + i)).collect();
    let recv_s: Vec<u8> = (0..8).map(|i| gb_s.debug_read(0xC000 + i)).collect();
    assert_eq!(
        recv_m,
        (0xB0..=0xB7).collect::<Vec<u8>>(),
        "master received the slave's exact bytes (no 0xFF corruption)"
    );
    assert_eq!(
        recv_s,
        (0xA0..=0xA7).collect::<Vec<u8>>(),
        "slave received the master's exact bytes"
    );
}

/// Lockstep task 7: `pump_blocking` delivers a peer reply that resumes a
/// stalled master (the single-process responsiveness path).
#[test]
fn pump_blocking_resumes_a_stalled_master() {
    let mut link = Link::new();
    link.listen_on(0).expect("listen");
    let port = link.port().unwrap();
    let peer = LinkSocket::connect("127.0.0.1".into(), port).expect("peer");
    let mut gb = GameBoy::new(Model::Dmg, master_xfer_rom()).unwrap();
    wait_until("connected", || link.is_connected() && peer.is_connected());
    link.pump(&mut gb); // attach the core
    // Run the master until it stalls awaiting the peer byte.
    gb.run_frame();
    link.pump(&mut gb);
    assert!(gb.link_stalled(), "master stalled");
    // The peer (acting as the slave) replies; pump_blocking delivers + resumes.
    peer.send(Packet::new(cmd::SYNC2, 0x77, 0x80, 0));
    wait_until("master resumes", || link.pump_blocking(&mut gb));
    assert!(!gb.link_stalled());
    gb.run_frame();
    assert_eq!(gb.debug_read(0xFF01), 0x77, "master received the peer byte");
}

/// Lockstep task 10 (deterministic, no socket): two cores bridged directly via
/// `apply_packet`/`drain_pending` trade a 16-byte block. No threads/timing — a
/// pure proof that the protocol routing + core lockstep exchange every byte
/// correctly in both directions.
#[test]
fn loopback_16_byte_block_no_socket() {
    const N: u16 = 16;
    let mut link_m = Link::new();
    let mut link_s = Link::new();
    let mut gb_m = GameBoy::new(Model::Dmg, multi_xfer_rom(N as u8, 0xA0, 0x81)).unwrap();
    let mut gb_s = GameBoy::new(Model::Dmg, multi_xfer_rom(N as u8, 0xB0, 0x80)).unwrap();
    gb_m.link_connect(true);
    gb_s.link_connect(true);

    let done = |gb: &GameBoy| (0..N).all(|i| gb.debug_read(0xC000 + i) != 0x00);
    for _ in 0..100_000 {
        gb_m.run_frame(); // master stalls per transfer
        gb_s.run_frame(); // slave spins armed
        // Shuttle the master's outgoing byte to the slave; route its SYNC2 back.
        while let Some(byte) = gb_m.link_take_send() {
            if let Some(reply) =
                link_s.apply_packet(&mut gb_s, Packet::new(cmd::SYNC1, byte, 0x80, 0))
            {
                link_m.apply_packet(&mut gb_m, reply);
            }
        }
        // Dispatch anything the slave buffered (armed since), routing replies.
        for reply in link_s.drain_pending(&mut gb_s) {
            link_m.apply_packet(&mut gb_m, reply);
        }
        if done(&gb_m) && done(&gb_s) {
            break;
        }
    }

    let recv_m: Vec<u8> = (0..N).map(|i| gb_m.debug_read(0xC000 + i)).collect();
    let recv_s: Vec<u8> = (0..N).map(|i| gb_s.debug_read(0xC000 + i)).collect();
    assert_eq!(
        recv_m,
        (0xB0..0xC0).collect::<Vec<u8>>(),
        "master got the slave's 16 bytes 0xB0..=0xBF"
    );
    assert_eq!(
        recv_s,
        (0xA0..0xB0).collect::<Vec<u8>>(),
        "slave got the master's 16 bytes 0xA0..=0xAF"
    );
}

/// A SYNC2 arriving with no master stalled (a stale reply after an abort, or a
/// spurious packet) is dropped — it must not poison the core incoming queue and
/// desync the next master transfer.
#[test]
fn stale_sync2_without_stall_is_dropped() {
    let mut link = Link::new();
    let mut gb = GameBoy::new(Model::Dmg, master_xfer_rom()).unwrap();
    gb.link_connect(true);
    assert!(!gb.link_stalled());
    let reply = link.apply_packet(&mut gb, Packet::new(cmd::SYNC2, 0x99, 0, 0));
    assert!(reply.is_none());
    // The stale byte was NOT enqueued: the next master transfer stalls (it would
    // have completed with 0x99 had the byte poisoned link_in).
    gb.run_frame();
    assert!(gb.link_stalled(), "no stale byte fed the master");
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
fn pump_reaps_a_listener_whose_worker_died_without_attaching() {
    // A listener accepts one peer that connects then closes immediately — the
    // worker runs and exits (finished) but pump may never observe it connected,
    // so the attached-edge alone can't reap it. pump must still reap the dead
    // worker (no zombie "listening :port" with a dead accept thread).
    let mut link = Link::new();
    link.listen_on(0).expect("listen");
    let port = link.port().expect("bound port");
    let peer = std::net::TcpStream::connect(("127.0.0.1", port)).expect("dial");
    drop(peer); // connect then close immediately
    let mut gb = GameBoy::new(Model::Dmg, slave_arm_rom(0x00)).unwrap();
    wait_until("dead worker reaped", || {
        link.pump(&mut gb);
        link.status_label().is_none()
    });
    assert!(!link.is_active() && !link.is_listening() && !link.is_connected());
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
