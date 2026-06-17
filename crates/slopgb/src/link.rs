//! Serial **Link cable** over TCP — bgb-compatible 8-byte packet framing,
//! driven by a background socket thread so the paced UI never blocks. Uses
//! `std::net` + `std::thread` + `std::sync::mpsc` only (respects the frontend's
//! winit/softbuffer/cpal-only, no-Cargo-dep rule).
//!
//! The core serial port exposes a golden-safe byte-exchange hook
//! (`GameBoy::link_connect`/`link_take_send`/`link_push_recv`/
//! `link_slave_transfer`, all inert when disconnected). This module is the
//! transport + protocol glue: it ships the byte a completed master transfer
//! produced to the peer (a `SYNC1` packet) and feeds a peer's byte back into
//! the core. The model is a per-byte swap with one transfer of latency — what
//! bgb/SameBoy/gambatte do over TCP; games handshake around it.
//!
//! Scope (this milestone "starts" Link): slopgb↔slopgb linking with the bgb
//! packet *format* so a future session can complete bgb-wire interop
//! (timestamp-precise lockstep) without reshaping the transport.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use slopgb_core::GameBoy;

/// bgb's default link port.
pub const DEFAULT_PORT: u16 = 8765;

/// How long the socket thread blocks on a read before looping to drain the
/// outgoing queue and re-check the stop flag. Small enough to stay responsive
/// to the paced loop, large enough not to spin.
const READ_POLL: Duration = Duration::from_millis(2);
/// Poll interval while a listener waits for its one peer to connect, and
/// between connect retries.
const ACCEPT_POLL: Duration = Duration::from_millis(5);
/// Per-attempt connect timeout. Bounded so a dial to a black-holed host can't
/// wedge the socket thread (and thus a [`LinkSocket::drop`] join) for minutes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
/// Write timeout: a blocked `write_all` to a frozen peer is treated as a
/// disconnect rather than wedging the socket thread (and a drop join).
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Bound on the inbound packet queue. The UI drains it every emulated frame, so
/// this is only reached if emulation is paused (or a peer floods) — past it
/// packets are dropped rather than grown without bound (the link desyncs, but
/// no OOM). Far more than a frame's worth of normal serial traffic.
const IN_QUEUE_CAP: usize = 1024;

/// bgb link protocol command bytes (bgb.bircd.org/bgblink.html).
pub mod cmd {
    /// Protocol-version handshake (`b2`=major, `b3`=minor).
    pub const VERSION: u8 = 1;
    /// Master sends a data byte (`b2`=data, `b3`=control bits).
    pub const SYNC1: u8 = 104;
    /// Slave replies with its data byte (`b2`=data).
    pub const SYNC2: u8 = 105;
    /// Peer is closing the link.
    pub const DISCONNECT: u8 = 109;
    // bgb also defines SYNC3 (106, idle keep-alive) and STATUS (108, run/pause)
    // — added when timestamp-precise bgb-wire interop lands (see docs/bgb-link-plan.md).
}

/// An 8-byte bgb link packet: a command, three data bytes, and a little-endian
/// 4-byte timestamp (a cycle counter; informational for slopgb↔slopgb).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet {
    pub cmd: u8,
    pub b2: u8,
    pub b3: u8,
    pub b4: u8,
    pub timestamp: u32,
}

impl Packet {
    #[must_use]
    pub fn new(cmd: u8, b2: u8, b3: u8, b4: u8) -> Self {
        Self {
            cmd,
            b2,
            b3,
            b4,
            timestamp: 0,
        }
    }

    /// Serialize to the wire: `cmd b2 b3 b4` then the timestamp little-endian.
    #[must_use]
    pub fn encode(&self) -> [u8; 8] {
        let t = self.timestamp.to_le_bytes();
        [self.cmd, self.b2, self.b3, self.b4, t[0], t[1], t[2], t[3]]
    }

    /// Parse a complete 8-byte frame (the caller guarantees the length).
    #[must_use]
    pub fn decode(buf: &[u8; 8]) -> Self {
        Self {
            cmd: buf[0],
            b2: buf[1],
            b3: buf[2],
            b4: buf[3],
            timestamp: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
        }
    }
}

/// A live TCP link endpoint: a background thread owns the socket and exchanges
/// [`Packet`]s with the UI thread over channels, so neither blocks the other.
pub struct LinkSocket {
    out_tx: Sender<Packet>,
    in_rx: Receiver<Packet>,
    connected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    /// Set by the worker on every exit path. Lets the UI reap a socket whose
    /// thread died *without* ever connecting (dial/accept failed, or a peer
    /// connected then closed between two pumps) — which the connected-edge
    /// teardown alone can't see.
    finished: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    /// The bound (listen) or target (connect) port — shown by the UI.
    port: u16,
}

impl LinkSocket {
    /// Bind `port` and accept one peer on a background thread. Binding happens
    /// here so a taken port surfaces synchronously; accepting is async. Pass
    /// port 0 to let the OS pick an ephemeral port (see [`Self::port`]).
    pub fn listen(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let bound = listener.local_addr().map_or(port, |a| a.port());
        let mut sock = Self::spawn(move |connected, stop, out_rx, in_tx| {
            if let Some(stream) = accept_one(&listener, &stop) {
                run_stream(stream, &connected, &stop, &out_rx, &in_tx);
            }
        });
        sock.port = bound;
        Ok(sock)
    }

    /// Dial `host:port` on a background thread (the dial never blocks the UI).
    /// Retries (bounded per-attempt) until the peer's listener accepts or the
    /// socket is dropped, so connecting before the peer clicks "Listen" still
    /// links. A failed/unresolvable host leaves [`Self::is_connected`] false.
    pub fn connect(host: String, port: u16) -> std::io::Result<Self> {
        let mut sock = Self::spawn(move |connected, stop, out_rx, in_tx| {
            if let Some(stream) = dial(&host, port, &stop) {
                run_stream(stream, &connected, &stop, &out_rx, &in_tx);
            }
        });
        sock.port = port;
        Ok(sock)
    }

    /// Spawn the socket thread with fresh channels + flags.
    fn spawn<F>(body: F) -> Self
    where
        F: FnOnce(Arc<AtomicBool>, Arc<AtomicBool>, Receiver<Packet>, SyncSender<Packet>)
            + Send
            + 'static,
    {
        let (out_tx, out_rx) = mpsc::channel::<Packet>();
        // Inbound is bounded (try_send) so a paused UI / flooding peer can't grow
        // it without limit; outbound is unbounded so the UI never blocks on send.
        let (in_tx, in_rx) = mpsc::sync_channel::<Packet>(IN_QUEUE_CAP);
        let connected = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (tc, ts, tf) = (
            Arc::clone(&connected),
            Arc::clone(&stop),
            Arc::clone(&finished),
        );
        let handle = thread::spawn(move || {
            // A drop guard marks the worker finished on *every* exit — normal
            // return OR a panic unwind — so a panicked worker is still reapable
            // (the UI can't otherwise see it died). The thread sets `connected`
            // false on its own exit paths, so reap only needs `finished`.
            struct FinishGuard(Arc<AtomicBool>);
            impl Drop for FinishGuard {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            let _guard = FinishGuard(tf);
            body(tc, ts, out_rx, in_tx);
        });
        Self {
            out_tx,
            in_rx,
            connected,
            stop,
            finished,
            handle: Some(handle),
            port: 0,
        }
    }

    /// The bound (listen) or target (connect) port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Whether the TCP connection is currently established.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Whether the worker thread has exited (success or failure).
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    /// Non-blocking: the next packet the peer sent, if any.
    #[must_use]
    pub fn poll(&self) -> Option<Packet> {
        self.in_rx.try_recv().ok()
    }

    /// Queue a packet for the socket thread to write (never blocks the UI).
    pub fn send(&self, p: Packet) {
        let _ = self.out_tx.send(p);
    }
}

impl Drop for LinkSocket {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Dial `host:port`, retrying until connected or the stop flag is set. Each
/// attempt is bounded by [`CONNECT_TIMEOUT`] so a black-holed host can't wedge
/// the thread (and thus a `drop` join). Returns the stream, or `None` if the
/// host is unresolvable or the socket was dropped first.
fn dial(host: &str, port: u16, stop: &AtomicBool) -> Option<TcpStream> {
    use std::net::ToSocketAddrs;
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        // Re-resolve each round so a peer that comes up later (DNS/host) is
        // still reachable; an unresolvable host gives no addrs → keep waiting.
        let addrs: Vec<_> = (host, port)
            .to_socket_addrs()
            .map(Iterator::collect)
            .unwrap_or_default();
        for addr in &addrs {
            if stop.load(Ordering::Relaxed) {
                return None;
            }
            if let Ok(stream) = TcpStream::connect_timeout(addr, CONNECT_TIMEOUT) {
                return Some(stream);
            }
        }
        thread::sleep(ACCEPT_POLL);
    }
}

/// Poll-accept one peer, bailing if the stop flag is set (so a listening socket
/// can be cancelled). Returns the accepted stream, or `None` if cancelled.
fn accept_one(listener: &TcpListener, stop: &AtomicBool) -> Option<TcpStream> {
    // Must be non-blocking to honor the stop flag; if that fails, a blocking
    // accept would never re-check stop and a drop join would hang — so bail.
    if listener.set_nonblocking(true).is_err() {
        return None;
    }
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => return None,
        }
    }
}

/// The connected read/write loop: drain queued outgoing packets, then read with
/// a short timeout (partial-read-safe), forwarding complete frames. Exits on
/// stop, peer close, or a socket error, clearing the connected flag.
fn run_stream(
    stream: TcpStream,
    connected: &AtomicBool,
    stop: &AtomicBool,
    out_rx: &Receiver<Packet>,
    in_tx: &SyncSender<Packet>,
) {
    // A read timeout is what lets the loop re-check the stop flag; without it a
    // read blocks forever on an idle peer and a drop join hangs — so bail if it
    // can't be set. A write timeout likewise bounds a write_all to a frozen /
    // partitioned peer (else it blocks for the OS retransmit timeout, wedging a
    // drop join for minutes); a timed-out write is treated as a disconnect
    // below. nodelay failing is harmless (latency only).
    if stream.set_read_timeout(Some(READ_POLL)).is_err()
        || stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err()
    {
        return;
    }
    stream.set_nodelay(true).ok();
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut reader = stream;
    connected.store(true, Ordering::Relaxed);
    let mut buf = [0u8; 8];
    let mut filled = 0usize;
    'outer: loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // Drain everything the UI queued.
        loop {
            match out_rx.try_recv() {
                Ok(p) => {
                    if writer.write_all(&p.encode()).is_err() {
                        break 'outer;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break 'outer,
            }
        }
        // Read whatever is available; assemble 8-byte frames across reads.
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break, // peer closed
            Ok(n) => {
                filled += n;
                if filled == 8 {
                    // try_send (never blocks): a full queue drops the packet
                    // rather than wedging the read loop or growing unbounded.
                    let _ = in_tx.try_send(Packet::decode(&buf));
                    filled = 0;
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    connected.store(false, Ordering::Relaxed);
}

/// High-level link state for the menu + the per-frame core pump. Owns the
/// optional [`LinkSocket`] and the protocol handshake.
#[derive(Default)]
pub struct Link {
    socket: Option<LinkSocket>,
    listening: bool,
    /// Whether we have completed the connect handshake with the peer + told the
    /// core a peer is attached.
    attached: bool,
    /// Outgoing-packet timestamp counter (informational for slopgb↔slopgb).
    seq: u32,
}

impl Link {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Listen for an incoming peer on the default port (bgb "Listen").
    pub fn listen(&mut self) -> std::io::Result<()> {
        self.listen_on(DEFAULT_PORT)
    }

    /// Listen on a specific port (port 0 = OS-chosen ephemeral). Replaces any
    /// existing socket. Errors if the port is already in use.
    pub fn listen_on(&mut self, port: u16) -> std::io::Result<()> {
        let sock = LinkSocket::listen(port)?;
        self.reset_to(Some(sock));
        self.listening = true;
        Ok(())
    }

    /// Dial a peer (bgb "Connect"). Replaces any existing socket.
    pub fn connect(&mut self, host: String, port: u16) -> std::io::Result<()> {
        let sock = LinkSocket::connect(host, port)?;
        self.reset_to(Some(sock));
        Ok(())
    }

    /// Tear the link down (bgb "Disconnect" / "Cancel listen") and detach the
    /// core peer so the serial port returns to standalone (golden) behavior.
    pub fn disconnect(&mut self, gb: &mut GameBoy) {
        if self.attached {
            // Best-effort notify the peer before dropping the socket.
            if let Some(s) = &self.socket {
                s.send(Packet::new(cmd::DISCONNECT, 0, 0, 0));
            }
        }
        self.reset_to(None);
        gb.link_connect(false);
    }

    fn reset_to(&mut self, sock: Option<LinkSocket>) {
        self.socket = sock;
        self.listening = false;
        self.attached = false;
        self.seq = 0;
    }

    /// Whether a listener is waiting for its peer (and not yet connected).
    #[must_use]
    pub fn is_listening(&self) -> bool {
        self.listening && !self.is_connected()
    }

    /// Whether a peer is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.socket.as_ref().is_some_and(LinkSocket::is_connected)
    }

    /// Whether any socket is alive (dialing, listening, or connected). A
    /// pending dial is "active" but neither connected nor listening, so the
    /// menu uses this to keep Disconnect live (to abort the dial).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.socket.is_some()
    }

    /// The bound (listen) or target (connect) port of the active socket, if any.
    #[must_use]
    pub fn port(&self) -> Option<u16> {
        self.socket.as_ref().map(LinkSocket::port)
    }

    /// A short status label for the window title (bgb shows the link state),
    /// or `None` when no link is up: `"linked"` once connected, else
    /// `"listening :<port>"` (we bound the port) or `"connecting :<port>"`
    /// (we are dialing a peer) while the link is coming up.
    #[must_use]
    pub fn status_label(&self) -> Option<String> {
        let socket = self.socket.as_ref()?;
        if socket.is_connected() {
            Some("linked".to_owned())
        } else if self.listening {
            Some(format!("listening :{}", socket.port()))
        } else {
            Some(format!("connecting :{}", socket.port()))
        }
    }

    /// Per-frame pump: complete the handshake, ship the core's outgoing serial
    /// byte to the peer, and feed the peer's bytes back into the core. A no-op
    /// when no socket is connected, so it is safe to call every frame.
    pub fn pump(&mut self, gb: &mut GameBoy) {
        // Reap a dead link instead of leaving a zombie "connected"/"listening"/
        // "connecting" state behind: either a peer-initiated disconnect after we
        // had attached (the connection flag dropped), OR a worker thread that
        // exited without ever connecting — a failed dial/accept, or a peer that
        // connected then closed between two pumps (e.g. while paused), which the
        // attached-edge alone can't see.
        let worker_died = self
            .socket
            .as_ref()
            .is_some_and(|s| s.is_finished() && !s.is_connected());
        if (self.attached && !self.is_connected()) || worker_died {
            self.disconnect(gb);
            return;
        }
        if !self.is_connected() {
            return;
        }
        if !self.attached {
            self.attached = true;
            gb.link_connect(true);
            self.emit(Packet::new(cmd::VERSION, 1, 4, 0));
        }
        // Ship our master transfer's outgoing byte(s).
        while let Some(byte) = gb.link_take_send() {
            self.emit(Packet::new(cmd::SYNC1, byte, 0x80, 0));
        }
        // Feed in whatever the peer sent.
        while let Some(p) = self.poll_socket() {
            if let Some(reply) = self.apply_packet(gb, p) {
                self.emit(reply);
            }
        }
    }

    fn poll_socket(&self) -> Option<Packet> {
        self.socket.as_ref().and_then(LinkSocket::poll)
    }

    /// Apply one received packet to the core, returning a reply to send if any.
    /// Pure routing (no socket I/O) so it is unit-testable with a real core.
    pub fn apply_packet(&mut self, gb: &mut GameBoy, p: Packet) -> Option<Packet> {
        match p.cmd {
            cmd::SYNC1 => {
                // Peer is the master and sent its byte. If we are an armed slave
                // (external clock, transfer pending), complete and reply with our
                // byte; otherwise stash it for our next master transfer to read.
                match gb.link_slave_transfer(p.b2) {
                    Some(out) => Some(Packet::new(cmd::SYNC2, out, 0x80, 0)),
                    None => {
                        gb.link_push_recv(p.b2);
                        None
                    }
                }
            }
            cmd::SYNC2 => {
                // Peer (slave) replied with its byte → our next master read.
                gb.link_push_recv(p.b2);
                None
            }
            cmd::DISCONNECT => {
                self.disconnect(gb);
                None
            }
            // VERSION / SYNC3 / STATUS: no behavior yet (kept for bgb interop).
            _ => None,
        }
    }

    /// Send a packet with the next sequence timestamp.
    fn emit(&mut self, mut p: Packet) {
        if let Some(s) = &self.socket {
            p.timestamp = self.seq;
            self.seq = self.seq.wrapping_add(1);
            s.send(p);
        }
    }
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
