//! The MCP transport: a background thread owns a `TcpListener` and speaks the
//! Model Context Protocol **streamable-HTTP** profile — a client POSTs a
//! JSON-RPC request and gets a JSON response (`claude mcp add --transport http`).
//! Handshake + tool metadata are answered here; a `tools/call` is forwarded to
//! the UI thread over a channel (a [`Job`] with a one-shot reply) and executed
//! against the live machine, so the socket never touches emulator state directly.
//!
//! Std-only (no serde, no HTTP crate) like [`crate::link`]. One connection is
//! handled at a time — an agent uses a single keep-alive connection; a socket
//! read timeout keeps a silent peer from wedging the loop.
//
// ponytail: single-threaded accept+handle. If concurrent MCP clients ever
// matter, spawn a bounded worker per connection — a debug endpoint doesn't.

use std::io::{BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::Job;
use super::json::Json;
use super::tools::{Call, ToolResult};

/// The MCP protocol revision we advertise if the client doesn't ask for one.
const PROTOCOL_VERSION: &str = "2024-11-05";
/// How long a tool call waits for the UI thread to answer before returning an
/// error result (the UI pumps every wake, so this only bites if it's wedged).
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// Socket read timeout — bounds a silent peer so the accept loop stays live and
/// a `Drop` join can't hang.
const READ_TIMEOUT: Duration = Duration::from_millis(200);
const ACCEPT_POLL: Duration = Duration::from_millis(5);
/// Reject an over-large request body rather than allocating it (untrusted net).
const MAX_BODY: usize = 8 << 20;
/// Bound the header section too.
const MAX_HEADERS: usize = 64 << 10;

/// A running MCP server: the socket thread + its stop/finished flags, reaped on
/// drop (mirrors `link::LinkSocket`).
pub struct Server {
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    port: u16,
}

impl Server {
    /// Bind `127.0.0.1:port` (a taken port errors synchronously) and serve on a
    /// background thread, forwarding tool calls over `tx`. Port 0 → an OS-chosen
    /// ephemeral port (see [`Self::port`]).
    pub fn start(port: u16, tx: Sender<Job>) -> std::io::Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let bound = listener.local_addr().map_or(port, |a| a.port());
        let stop = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (ts, tf) = (Arc::clone(&stop), Arc::clone(&finished));
        let handle = thread::spawn(move || {
            struct FinishGuard(Arc<AtomicBool>);
            impl Drop for FinishGuard {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            let _guard = FinishGuard(tf);
            serve(&listener, &ts, &tx);
        });
        Ok(Server {
            stop,
            finished,
            handle: Some(handle),
            port: bound,
        })
    }

    /// The bound port (useful when started on port 0).
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Whether the socket thread has exited.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Poll-accept connections until stopped, handling each to completion.
fn serve(listener: &TcpListener, stop: &AtomicBool, tx: &Sender<Job>) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => handle_conn(stream, stop, tx),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => return,
        }
    }
}

/// Serve requests on one connection (keep-alive) until it closes or stops.
fn handle_conn(stream: TcpStream, stop: &AtomicBool, tx: &Sender<Job>) {
    if stream.set_read_timeout(Some(READ_TIMEOUT)).is_err() {
        return;
    }
    let write_stream = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    let mut writer = write_stream;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match read_request(&mut reader, stop) {
            Ok(Some(req)) => {
                let (status, ctype, body) = respond(&req, tx);
                if write_response(&mut writer, status, ctype, &body).is_err() {
                    return;
                }
            }
            Ok(None) => return,                 // clean EOF (peer closed)
            Err(RequestError::WouldBlock) => {} // idle keep-alive; re-check stop
            Err(RequestError::Fatal) => return,
        }
    }
}

struct Request {
    method: String,
    body: Vec<u8>,
}

enum RequestError {
    WouldBlock,
    Fatal,
}

/// Read one HTTP request (request line, headers, Content-Length body). `Ok(None)`
/// on a clean EOF; `WouldBlock` when the read timed out mid-idle (no bytes yet).
fn read_request(
    reader: &mut BufReader<TcpStream>,
    stop: &AtomicBool,
) -> Result<Option<Request>, RequestError> {
    let mut line = String::new();
    match read_line(reader, &mut line, stop) {
        Ok(0) => return Ok(None), // EOF
        Ok(_) => {}
        Err(e) if is_timeout(&e) => return Err(RequestError::WouldBlock),
        Err(_) => return Err(RequestError::Fatal),
    }
    let method = line.split_whitespace().next().unwrap_or("").to_owned();

    // Headers until a blank line; capture Content-Length.
    let mut content_len = 0usize;
    let mut header_bytes = line.len();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(RequestError::Fatal);
        }
        let mut h = String::new();
        match read_line(reader, &mut h, stop) {
            Ok(0) => return Err(RequestError::Fatal), // truncated headers
            Ok(n) => header_bytes += n,
            Err(e) if is_timeout(&e) => continue, // a slow header; keep reading
            Err(_) => return Err(RequestError::Fatal),
        }
        if header_bytes > MAX_HEADERS {
            return Err(RequestError::Fatal);
        }
        let t = h.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some(v) = t.split_once(':') {
            if v.0.eq_ignore_ascii_case("content-length") {
                content_len = v.1.trim().parse().unwrap_or(0);
            }
        }
    }
    if content_len > MAX_BODY {
        return Err(RequestError::Fatal);
    }
    let mut body = vec![0u8; content_len];
    read_exact_timeout(reader, &mut body, stop)?;
    Ok(Some(Request { method, body }))
}

/// Read a single `\n`-terminated line into `buf`. Retries on a read timeout so a
/// header split across timeouts still assembles — but bails on `stop` so a peer
/// that stalls mid-line can't wedge the thread (and a `Drop` join can't hang).
fn read_line(
    reader: &mut BufReader<TcpStream>,
    buf: &mut String,
    stop: &AtomicBool,
) -> std::io::Result<usize> {
    let mut bytes = Vec::new();
    loop {
        let mut b = [0u8; 1];
        match reader.read(&mut b) {
            Ok(0) => break,
            Ok(_) => {
                bytes.push(b[0]);
                if b[0] == b'\n' {
                    break;
                }
            }
            Err(e) => {
                if bytes.is_empty() {
                    return Err(e);
                }
                if is_timeout(&e) {
                    if stop.load(Ordering::Relaxed) {
                        break; // shutting down: abandon the partial line
                    }
                    continue;
                }
                return Err(e);
            }
        }
    }
    let n = bytes.len();
    buf.push_str(&String::from_utf8_lossy(&bytes));
    Ok(n)
}

fn read_exact_timeout(
    reader: &mut BufReader<TcpStream>,
    buf: &mut [u8],
    stop: &AtomicBool,
) -> Result<(), RequestError> {
    let mut filled = 0;
    while filled < buf.len() {
        if stop.load(Ordering::Relaxed) {
            return Err(RequestError::Fatal);
        }
        match reader.read(&mut buf[filled..]) {
            Ok(0) => return Err(RequestError::Fatal),
            Ok(n) => filled += n,
            Err(ref e) if is_timeout(e) => {} // keep waiting for the rest
            Err(_) => return Err(RequestError::Fatal),
        }
    }
    Ok(())
}

fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn write_response(
    writer: &mut TcpStream,
    status: &str,
    ctype: Option<&str>,
    body: &[u8],
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    if let Some(ct) = ctype {
        head.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    head.push_str("Connection: keep-alive\r\n\r\n");
    writer.write_all(head.as_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

/// Turn a request into an HTTP `(status, content-type, body)`.
fn respond(req: &Request, tx: &Sender<Job>) -> (&'static str, Option<&'static str>, Vec<u8>) {
    match req.method.as_str() {
        "POST" => match super::json::parse(&String::from_utf8_lossy(&req.body)) {
            Ok(msg) => match process(&msg, tx) {
                Some(resp) => (
                    "200 OK",
                    Some("application/json"),
                    resp.render().into_bytes(),
                ),
                None => ("202 Accepted", None, Vec::new()), // a notification
            },
            Err(e) => (
                "200 OK",
                Some("application/json"),
                rpc_error(&Json::Null, -32700, &format!("parse error: {e}"))
                    .render()
                    .into_bytes(),
            ),
        },
        // The streamable-HTTP server-initiated SSE stream isn't offered (no
        // server-pushed messages), so a GET is Method Not Allowed — the client
        // falls back to POST-only, which is all these tools need.
        "GET" => ("405 Method Not Allowed", None, Vec::new()),
        _ => ("405 Method Not Allowed", None, Vec::new()),
    }
}

/// Dispatch one JSON-RPC message. `None` for a notification (no `id` → no
/// response). Handles a single object; a batch array maps element-wise.
fn process(msg: &Json, tx: &Sender<Job>) -> Option<Json> {
    if let Json::Arr(items) = msg {
        let out: Vec<Json> = items.iter().filter_map(|m| process(m, tx)).collect();
        return (!out.is_empty()).then_some(Json::Arr(out));
    }
    let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
    let id = msg.get("id").cloned();
    // A message with no `id` is a notification: run nothing that needs a reply,
    // and never send a response body.
    let is_notification = id.is_none();

    let result = match method {
        "initialize" => {
            let version = msg
                .get("params")
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Json::as_str)
                .unwrap_or(PROTOCOL_VERSION)
                .to_owned();
            Ok(Json::obj([
                ("protocolVersion", Json::str(version)),
                ("capabilities", Json::obj([("tools", Json::obj([]))])),
                (
                    "serverInfo",
                    Json::obj([
                        ("name", Json::str("slopgb")),
                        ("version", Json::str(env!("CARGO_PKG_VERSION"))),
                    ]),
                ),
            ]))
        }
        "ping" => Ok(Json::obj([])),
        "tools/list" => Ok(Json::obj([("tools", tool_defs())])),
        "tools/call" => tool_call(msg, tx),
        _ if method.starts_with("notifications/") => return None,
        _ => Err((-32601, format!("method not found: {method}"))),
    };

    if is_notification {
        return None;
    }
    let id = id.unwrap_or(Json::Null);
    Some(match result {
        Ok(r) => rpc_ok(&id, r),
        Err((code, m)) => rpc_error(&id, code, &m),
    })
}

/// Execute a `tools/call`, returning an MCP tool-result value (tool-level errors
/// are `isError` results, so the agent sees them — not JSON-RPC errors).
fn tool_call(msg: &Json, tx: &Sender<Job>) -> Result<Json, (i64, String)> {
    let params = msg.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(Json::as_str)
        .unwrap_or("");
    let args = params.and_then(|p| p.get("arguments"));
    match build_call(name, args) {
        Ok(call) => Ok(match run_on_ui(call, tx) {
            Ok(ToolResult::Text(t)) => tool_content(vec![text_block(&t)], false),
            Ok(ToolResult::Image(png)) => tool_content(
                vec![Json::obj([
                    ("type", Json::str("image")),
                    ("data", Json::str(base64(&png))),
                    ("mimeType", Json::str("image/png")),
                ])],
                false,
            ),
            Err(e) => tool_content(vec![text_block(&e)], true),
        }),
        Err(e) => Ok(tool_content(vec![text_block(&e)], true)),
    }
}

/// Forward a call to the UI thread and wait (bounded) for its reply.
fn run_on_ui(call: Call, tx: &Sender<Job>) -> Result<ToolResult, String> {
    let (rtx, rrx) = mpsc::sync_channel(1);
    tx.send(Job { call, reply: rtx })
        .map_err(|_| "emulator is shutting down".to_owned())?;
    match rrx.recv_timeout(REPLY_TIMEOUT) {
        Ok(res) => res,
        Err(_) => Err("emulator did not respond (paused too long?)".to_owned()),
    }
}

/// Build a typed [`Call`] from a tool name + arguments, or a descriptive error.
fn build_call(name: &str, args: Option<&Json>) -> Result<Call, String> {
    let arg = |k: &str| -> Result<String, String> {
        args.and_then(|a| a.get(k))
            .and_then(Json::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("tool '{name}' needs a string argument '{k}'"))
    };
    match name {
        "disassemble" => Ok(Call::Disassemble {
            from: arg("from")?,
            to: arg("to")?,
        }),
        "peek" => Ok(Call::Peek {
            from: arg("from")?,
            to: arg("to")?,
        }),
        "cdl" => Ok(Call::Cdl {
            from: arg("from")?,
            to: arg("to")?,
        }),
        "cdl-ranges" => Ok(Call::CdlRanges),
        "vram" => Ok(Call::Vram { view: arg("view")? }),
        "screencap" => Ok(Call::Screencap),
        "breakpoint" => Ok(Call::Breakpoint {
            addr: arg("address")?,
        }),
        "registers" => Ok(Call::Registers),
        "expr" => Ok(Call::Expr {
            expr: arg("expression")?,
        }),
        other => Err(format!("unknown tool '{other}'")),
    }
}

fn text_block(s: &str) -> Json {
    Json::obj([("type", Json::str("text")), ("text", Json::str(s))])
}

fn tool_content(content: Vec<Json>, is_error: bool) -> Json {
    Json::obj([
        ("content", Json::Arr(content)),
        ("isError", Json::Bool(is_error)),
    ])
}

fn rpc_ok(id: &Json, result: Json) -> Json {
    Json::obj([
        ("jsonrpc", Json::str("2.0")),
        ("id", id.clone()),
        ("result", result),
    ])
}

fn rpc_error(id: &Json, code: i64, message: &str) -> Json {
    Json::obj([
        ("jsonrpc", Json::str("2.0")),
        ("id", id.clone()),
        (
            "error",
            Json::obj([
                ("code", Json::Num(code as f64)),
                ("message", Json::str(message)),
            ]),
        ),
    ])
}

/// A `{type:object, properties, required}` input schema for one tool.
fn schema(props: &[(&str, &str)]) -> Json {
    let properties = Json::Obj(
        props
            .iter()
            .map(|(k, desc)| {
                (
                    (*k).to_owned(),
                    Json::obj([
                        ("type", Json::str("string")),
                        ("description", Json::str(*desc)),
                    ]),
                )
            })
            .collect(),
    );
    let required = Json::Arr(props.iter().map(|(k, _)| Json::str(*k)).collect());
    Json::obj([
        ("type", Json::str("object")),
        ("properties", properties),
        ("required", required),
    ])
}

fn tool(name: &str, desc: &str, props: &[(&str, &str)]) -> Json {
    Json::obj([
        ("name", Json::str(name)),
        ("description", Json::str(desc)),
        ("inputSchema", schema(props)),
    ])
}

/// The MCP tool catalogue advertised by `tools/list`.
fn tool_defs() -> Json {
    let range = &[
        ("from", "start address, AAAA or BB:AAAA hex (BB = bank)"),
        ("to", "end address (inclusive), same region/bank as `from`"),
    ][..];
    Json::Arr(vec![
        tool(
            "disassemble",
            "Disassemble a range. Rows: `BB:AAAA<tab>label<tab>instruction<tab>cycles`.",
            range,
        ),
        tool("peek", "Dump memory bytes, 16 per row.", range),
        tool(
            "cdl",
            "Dump code/data-log access (r/w/x per byte, `.` if none), 16 per row.",
            range,
        ),
        tool(
            "cdl-ranges",
            "List the continuous address ranges the code/data log has recorded \
             so far (non-`.`), one `AAAA-AAAA` / `BB:AAAA-BB:AAAA` range per line.",
            &[],
        ),
        tool(
            "vram",
            "Capture a VRAM view as a PNG.",
            &[("view", "one of: bg, win, tile0, tile1, oam, palette")],
        ),
        tool(
            "screencap",
            "Capture the current Game Boy (Color) screen (160x144) as a PNG.",
            &[],
        ),
        tool(
            "breakpoint",
            "Set a PC breakpoint.",
            &[("address", "AAAA or BB:AAAA hex")],
        ),
        tool("registers", "Read the CPU + LCD register state.", &[]),
        tool(
            "expr",
            "Evaluate a bgb-style debugger expression (hex default, registers, `[addr]`).",
            &[("expression", "e.g. `bc+1`, `[ff80]`, `pc`")],
        ),
    ])
}

/// Standard base64 (for the PNG image content). Std-only, no dep.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
