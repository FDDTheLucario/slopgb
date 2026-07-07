use super::*;
use slopgb_core::{GameBoy, Model};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn http_post(port: u16, body: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    s.write_all(req.as_bytes()).unwrap();
    s.flush().unwrap();
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        assert_ne!(s.read(&mut byte).unwrap(), 0, "server closed early");
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let headers = String::from_utf8_lossy(&buf).to_lowercase();
    let len: usize = headers
        .split("content-length:")
        .nth(1)
        .and_then(|s| s.split("\r\n").next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let mut body_buf = vec![0u8; len];
    s.read_exact(&mut body_buf).unwrap();
    String::from_utf8_lossy(&body_buf).into_owned()
}

#[test]
fn parse_port_defaults_on_blank_or_garbage() {
    assert_eq!(parse_port("8123"), 8123);
    assert_eq!(parse_port("  40000 "), 40000);
    assert_eq!(parse_port(""), DEFAULT_PORT);
    assert_eq!(parse_port("notaport"), DEFAULT_PORT);
    assert_eq!(parse_port("99999"), DEFAULT_PORT); // out of u16 range → default
}

#[test]
fn status_label_reflects_the_server() {
    let mut mcp = Mcp::new();
    assert_eq!(mcp.status_label(), None);
    mcp.start(0).unwrap();
    let label = mcp.status_label().unwrap();
    assert!(label.starts_with("MCP :"), "{label}");
    mcp.stop();
    assert!(!mcp.is_active());
    assert_eq!(mcp.status_label(), None);
}

#[test]
fn pump_without_server_is_a_noop() {
    let mut mcp = Mcp::new();
    assert!(!mcp.is_active());
    let gb = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    let mut dbg = Debugger::default();
    let syms = SymbolTable::default();
    mcp.pump(&gb, &mut dbg, &syms); // must not panic without a server
}

#[test]
fn start_then_pump_serves_a_live_tool_call() {
    let mut mcp = Mcp::new();
    mcp.start(0).unwrap();
    assert!(mcp.is_active());
    let port = mcp.port().unwrap();

    let gb = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
    let mut dbg = Debugger::default();
    let syms = SymbolTable::default();

    // Client on another thread; this thread is the "UI" and pumps.
    let client = std::thread::spawn(move || {
        http_post(
            port,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"breakpoint","arguments":{"address":"0150"}}}"#,
        )
    });

    let start = Instant::now();
    while !client.is_finished() && start.elapsed() < Duration::from_secs(5) {
        mcp.pump(&gb, &mut dbg, &syms);
        std::thread::sleep(Duration::from_millis(2));
    }
    let resp = client.join().unwrap();
    assert!(resp.contains("breakpoint set at"), "tool ran: {resp}");
    // The breakpoint tool's mutation landed in the App-owned set (round-trip).
    assert!(dbg.breakpoints().contains(0x0150));
}
