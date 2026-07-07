//! **MCP server** — slopgb hosts a Model Context Protocol server so an LLM agent
//! can drive the debugger: disassemble, peek memory, read the CDL, capture VRAM,
//! set breakpoints, read registers, and evaluate expressions against the *live*
//! machine you're watching.
//!
//! Shape mirrors [`crate::link`]: a background thread owns the socket
//! (a `TcpListener`, std-only — no serde, no HTTP crate, honoring the frontend's
//! winit/softbuffer/cpal-only, no-Cargo-dep rule) and talks to the UI thread over
//! channels, so neither blocks the other. The transport speaks the MCP
//! *streamable-HTTP* profile (POST JSON-RPC → JSON response), so it wires into a
//! client with `claude mcp add --transport http`.
//!
//! **Golden-safe:** every tool is read-only `&self` introspection except
//! `breakpoint`, which toggles the App-owned breakpoint set (not core state) —
//! and the whole server is opt-in (`--mcp-port` / `SLOPGB_MCP_PORT`, off by
//! default), so no golden path is touched.

pub mod addr;
pub mod json;
pub mod png;
pub mod server;
pub mod tools;
pub mod vram;

use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};

use slopgb_core::GameBoy;

use crate::dbg::Debugger;
use crate::symbols::SymbolTable;
use tools::{Call, ToolResult};

/// A tool call handed from the socket thread to the UI thread, with a one-shot
/// channel for the reply. The socket thread blocks on the reply; the UI drains
/// jobs each pump.
pub struct Job {
    pub call: Call,
    pub reply: SyncSender<Result<ToolResult, String>>,
}

/// UI-side MCP state: owns the running [`server::Server`] and the job queue it
/// feeds. Inert (all methods no-ops) until [`Self::start`] succeeds. Held by the
/// `App` and pumped once per event-loop wake — mirrors [`crate::link::Link`].
#[derive(Default)]
pub struct Mcp {
    server: Option<server::Server>,
    rx: Option<Receiver<Job>>,
}

impl Mcp {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind the server on `127.0.0.1:port` and begin serving. Replaces any
    /// existing server. Errors (returned) if the port is already in use.
    pub fn start(&mut self, port: u16) -> std::io::Result<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        let server = server::Server::start(port, tx)?;
        self.server = Some(server);
        self.rx = Some(rx);
        Ok(())
    }

    /// Whether a server is running.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.server.is_some()
    }

    /// The bound port, if serving.
    #[must_use]
    pub fn port(&self) -> Option<u16> {
        self.server.as_ref().map(server::Server::port)
    }

    /// Per-wake pump: execute every queued tool call against the live machine
    /// and reply. A no-op when no server is running, so it is safe to call every
    /// wake (including while paused/broken — that is exactly when an agent wants
    /// to inspect). Reaps a dead socket thread.
    pub fn pump(&mut self, gb: &GameBoy, dbg: &mut Debugger, symbols: &SymbolTable) {
        if self.server.as_ref().is_some_and(server::Server::is_finished) {
            self.server = None;
            self.rx = None;
            return;
        }
        let Some(rx) = &self.rx else { return };
        loop {
            match rx.try_recv() {
                Ok(job) => {
                    let result = tools::dispatch(&job.call, gb, dbg.breakpoints_mut(), symbols);
                    // The socket thread may have already timed out and dropped the
                    // receiver; a failed send is fine (its request is abandoned).
                    let _ = job.reply.send(result);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.rx = None;
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
