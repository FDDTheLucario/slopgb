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
    assert_eq!(tools.len(), 9);
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Json::as_str))
        .collect();
    for want in [
        "disassemble",
        "peek",
        "cdl",
        "cdl-ranges",
        "vram",
        "screencap",
        "breakpoint",
        "registers",
        "expr",
    ] {
        assert!(names.contains(&want), "missing tool {want}");
    }
}

#[test]
fn build_call_validates_arguments() {
    let args = Json::obj([("from", Json::str("C000")), ("to", Json::str("C00F"))]);
    assert!(matches!(
        build_call("peek", Some(&args)),
        Ok(Call::Peek { .. })
    ));
    assert!(matches!(build_call("registers", None), Ok(Call::Registers)));
    assert!(matches!(
        build_call("screencap", None),
        Ok(Call::Screencap { scale: 1 })
    ));
    assert!(matches!(
        build_call("cdl-ranges", None),
        Ok(Call::CdlRanges)
    ));
    // Missing argument and unknown tool are errors, not panics.
    assert!(build_call("peek", None).is_err());
    assert!(build_call("frobnicate", None).is_err());
    // Optional scale is parsed for the image tools; a bad one is an error.
    let s = Json::obj([("scale", Json::str("5x"))]);
    assert!(matches!(
        build_call("screencap", Some(&s)),
        Ok(Call::Screencap { scale: 5 })
    ));
    assert!(build_call("screencap", Some(&Json::obj([("scale", Json::str("9x"))]))).is_err());
}

#[test]
fn image_tool_schemas_mark_scale_optional() {
    let Json::Arr(tools) = tool_defs() else {
        panic!("array")
    };
    for name in ["vram", "screencap"] {
        let t = tools
            .iter()
            .find(|t| t.get("name").and_then(Json::as_str) == Some(name))
            .unwrap();
        let schema = t.get("inputSchema").unwrap();
        assert!(
            schema
                .get("properties")
                .and_then(|p| p.get("scale"))
                .is_some(),
            "{name} advertises a scale property"
        );
        let Json::Arr(req) = schema.get("required").unwrap() else {
            panic!("required is an array")
        };
        assert!(
            !req.iter().any(|r| r.as_str() == Some("scale")),
            "{name} scale is optional (not in required)"
        );
    }
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
    assert!(
        r.contains("2025-06-18"),
        "echoes the client protocol version"
    );

    let list =
        super::super::json::parse(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
    let r = process(&list, &tx).unwrap().render();
    assert!(r.contains("disassemble") && r.contains("registers"));

    // A notification (no id) gets no response.
    let note =
        super::super::json::parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .unwrap();
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

// --- Abuse guards on the untrusted-net request parser (`read_request`). ---
// These drive `read_request` directly (the exact path `handle_conn` uses) with
// crafted byte streams. Each asserts the guard's *observable* effect so it fails
// the instant that guard is removed — see the per-test comment for what the
// un-guarded code would return instead.

/// Feed crafted request bytes straight into [`read_request`] over a loopback
/// socket pair and return its outcome. A writer thread sends `request` and then
/// closes its write half, so a body larger than the socket buffer can't deadlock
/// the send and a truncated request still reaches EOF. Broken-pipe write errors
/// are ignored: the MAX_BODY guard drops the socket before draining an oversized
/// body, so the writer's remaining bytes are expected to fail.
fn drive_read_request(request: Vec<u8>) -> Result<Option<Request>, RequestError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let writer = std::thread::spawn(move || {
        if let Ok(mut c) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
            let _ = c.write_all(&request);
            let _ = c.flush();
            let _ = c.shutdown(std::net::Shutdown::Write);
        }
    });
    let (server, _) = listener.accept().unwrap();
    // Generous timeout: no test path here legitimately waits on it (data or EOF
    // always arrives), it only defeats a writer-vs-reader startup race.
    server
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(server);
    let stop = AtomicBool::new(false);
    let outcome = read_request(&mut reader, &stop);
    drop(reader); // close the server side so a still-blocked writer unblocks
    writer.join().unwrap();
    outcome
}

#[test]
fn oversized_content_length_is_rejected_before_the_body() {
    let len = MAX_BODY + 1;
    let mut req = format!("POST /mcp HTTP/1.1\r\nContent-Length: {len}\r\n\r\n").into_bytes();
    req.resize(req.len() + len, 0); // a full body the size the header claims
    let outcome = drive_read_request(req);
    // Guard present: Fatal right after the headers, the `len`-byte body never
    // allocated or read. Guard removed: it would allocate `len` bytes, read the
    // whole body, and return Ok(Some(..)) — so this Fatal assert fails.
    assert!(
        matches!(outcome, Err(RequestError::Fatal)),
        "Content-Length exceeding MAX_BODY must be rejected before the body"
    );
}

#[test]
fn oversized_headers_overflow_reject() {
    // One header line whose length alone blows past MAX_HEADERS.
    let mut req = b"POST /mcp HTTP/1.1\r\nX: ".to_vec();
    req.resize(req.len() + MAX_HEADERS + 8, b'a');
    req.extend_from_slice(b"\r\n\r\n");
    let outcome = drive_read_request(req);
    // Guard present: header_bytes crosses MAX_HEADERS -> Fatal. Guard removed:
    // the giant header is accepted, the blank line ends the headers, and it
    // returns Ok(Some(..)) with an empty body — so this Fatal assert fails.
    assert!(
        matches!(outcome, Err(RequestError::Fatal)),
        "headers exceeding MAX_HEADERS must overflow-reject"
    );
}

#[test]
fn truncated_headers_terminate_not_hang() {
    // Request line + one header, then EOF with no blank line.
    let req = b"POST /mcp HTTP/1.1\r\nHost: localhost\r\n".to_vec();
    let outcome = drive_read_request(req);
    // The header loop hits read_line -> Ok(0) (EOF) and returns Fatal. This
    // returning at all proves "terminates, not hangs": were that arm to loop or
    // continue instead of returning, `read_request` would spin forever.
    assert!(
        matches!(outcome, Err(RequestError::Fatal)),
        "truncated headers must terminate with Fatal"
    );
}

#[test]
fn unparseable_content_length_defaults_to_empty_body() {
    let req = b"POST /mcp HTTP/1.1\r\nContent-Length: not-a-number\r\n\r\n".to_vec();
    let outcome = drive_read_request(req);
    // `.parse().unwrap_or(0)` -> a 0-length body, not a panic. Were it `.unwrap()`
    // this would panic (aborting the read) instead of yielding an empty-body
    // request, so matching Ok(Some { body empty }) pins the defaulting.
    assert!(
        matches!(outcome, Ok(Some(ref r)) if r.method == "POST" && r.body.is_empty()),
        "unparseable Content-Length must default to an empty body"
    );
}
