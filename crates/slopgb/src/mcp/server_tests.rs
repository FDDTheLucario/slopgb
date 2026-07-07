use super::*;
use crate::dbg::Breakpoints;
use crate::mcp::Job;
use crate::symbols::SymbolTable;
use slopgb_core::{GameBoy, Model};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[test]
fn base64_known_vectors() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

#[test]
fn tool_defs_lists_every_named_tool() {
    let Json::Arr(tools) = tool_defs() else {
        panic!("tools is an array")
    };
    assert_eq!(tools.len(), 8);
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Json::as_str))
        .collect();
    for want in [
        "disassemble", "peek", "cdl", "vram", "screencap", "breakpoint", "registers", "expr",
    ] {
        assert!(names.contains(&want), "missing tool {want}");
    }
}

#[test]
fn build_call_validates_arguments() {
    let args = Json::obj([("from", Json::str("C000")), ("to", Json::str("C00F"))]);
    assert!(matches!(build_call("peek", Some(&args)), Ok(Call::Peek { .. })));
    assert!(matches!(build_call("registers", None), Ok(Call::Registers)));
    assert!(matches!(build_call("screencap", None), Ok(Call::Screencap)));
    // Missing argument and unknown tool are errors, not panics.
    assert!(build_call("peek", None).is_err());
    assert!(build_call("frobnicate", None).is_err());
}

#[test]
fn process_handles_handshake_methods() {
    let (tx, _rx) = std::sync::mpsc::channel::<Job>();
    let init = super::super::json::parse(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    )
    .unwrap();
    let r = process(&init, &tx).unwrap().render();
    assert!(r.contains("\"result\"") && r.contains("serverInfo"));
    assert!(r.contains("2025-06-18"), "echoes the client protocol version");

    let list = super::super::json::parse(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
    let r = process(&list, &tx).unwrap().render();
    assert!(r.contains("disassemble") && r.contains("registers"));

    // A notification (no id) gets no response.
    let note = super::super::json::parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
    assert!(process(&note, &tx).is_none());

    // Unknown method → JSON-RPC method-not-found.
    let bad = super::super::json::parse(r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#).unwrap();
    assert!(process(&bad, &tx).unwrap().render().contains("-32601"));
}

/// Write an HTTP POST and read back the JSON body (parsing Content-Length).
fn http_post(stream: &mut TcpStream, body: &str) -> String {
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    // Read headers up to the blank line, then the exact body.
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    let header_end = loop {
        let n = stream.read(&mut byte).unwrap();
        assert_ne!(n, 0, "server closed early");
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break buf.len();
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
    let len: usize = headers
        .split("content-length:")
        .nth(1)
        .and_then(|s| s.split("\r\n").next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let mut body_buf = vec![0u8; len];
    stream.read_exact(&mut body_buf).unwrap();
    String::from_utf8_lossy(&body_buf).into_owned()
}

#[test]
fn end_to_end_over_a_socket() {
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    let server = Server::start(0, tx).unwrap();
    let port = server.port();
    assert_ne!(port, 0);

    // Fake UI thread: serve one job (the tools/call) against a real machine.
    let ui = std::thread::spawn(move || {
        let gb = GameBoy::new(Model::Dmg, vec![0u8; 0x8000]).unwrap();
        let mut bps = Breakpoints::default();
        let syms = SymbolTable::default();
        if let Ok(job) = rx.recv_timeout(Duration::from_secs(5)) {
            let r = crate::mcp::tools::dispatch(&job.call, &gb, &mut bps, &syms);
            let _ = job.reply.send(r);
        }
    });

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // initialize (no emulator needed) then a tools/call registers (round-trips).
    let init = http_post(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    );
    assert!(init.contains("serverInfo"), "initialize: {init}");
    let regs = http_post(
        &mut stream,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"registers","arguments":{}}}"#,
    );
    assert!(regs.contains("af="), "registers via tools/call: {regs}");
    assert!(regs.contains("\"isError\":false"));

    ui.join().unwrap();
    drop(stream);
    drop(server);
}

#[test]
fn get_is_method_not_allowed() {
    let (tx, _rx) = std::sync::mpsc::channel::<Job>();
    let server = Server::start(0, tx).unwrap();
    let port = server.port();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /mcp HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).unwrap();
    let resp = String::from_utf8_lossy(&buf[..n]);
    assert!(resp.starts_with("HTTP/1.1 405"), "GET → 405: {resp}");
    drop(server);
}
