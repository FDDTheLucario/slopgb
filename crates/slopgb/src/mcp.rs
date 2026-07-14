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
pub mod plugin_host;
pub mod png;
pub mod server;
pub mod tools;
pub mod vram;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};

use slopgb_core::GameBoy;

use crate::dbg::Debugger;
use crate::symbols::SymbolTable;
use plugin_host::{FrontendToolContext, PluginMeta, ToolPlugins};
use tools::{Call, ToolResult};

/// Default port for the MCP server when the menu's port prompt is left blank.
pub const DEFAULT_PORT: u16 = 8123;

/// Parse the port typed into the "Start MCP server" prompt. A blank or
/// unparseable entry falls back to [`DEFAULT_PORT`] (mirrors
/// [`crate::link::parse_host_port`]). Never fails.
#[must_use]
pub fn parse_port(s: &str) -> u16 {
    s.trim().parse().unwrap_or(DEFAULT_PORT)
}

/// What a queued job runs: a built-in [`Call`], or a loaded tool plugin
/// (addressed by name, with its raw MCP `arguments` object as a JSON string).
pub enum ToolInvocation {
    Builtin(Call),
    Plugin { name: String, args: String },
}

/// A tool call handed from the socket thread to the UI thread, with a one-shot
/// channel for the reply. The socket thread blocks on the reply; the UI drains
/// jobs each pump.
pub struct Job {
    pub call: ToolInvocation,
    pub reply: SyncSender<Result<ToolResult, String>>,
}

/// UI-side MCP state: owns the running [`server::Server`], the job queue it
/// feeds, and the loaded tool plugins. Inert (all methods no-ops) until
/// [`Self::start`] succeeds. Held by the `App` and pumped once per event-loop
/// wake — mirrors [`crate::link::Link`].
#[derive(Default)]
pub struct Mcp {
    server: Option<server::Server>,
    rx: Option<Receiver<Job>>,
    tools: ToolPlugins,
    /// Cloned to the socket thread at [`Self::start`] so `tools/list` can
    /// advertise plugin tools without touching the UI-thread modules.
    plugin_meta: Arc<Vec<PluginMeta>>,
}

impl Mcp {
    /// Build with a set of loaded tool plugins (from `--plugins`). Their
    /// metadata is snapshot now so `tools/list` can advertise them. Use
    /// [`Mcp::default`] for none.
    #[must_use]
    pub fn with_tool_plugins(tools: ToolPlugins) -> Self {
        let plugin_meta = Arc::new(tools.metadata());
        Self {
            tools,
            plugin_meta,
            ..Self::default()
        }
    }

    /// Bind the server on `127.0.0.1:port` and begin serving. Replaces any
    /// existing server. Errors (returned) if the port is already in use.
    pub fn start(&mut self, port: u16) -> std::io::Result<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        let server = server::Server::start(port, tx, self.plugin_meta.clone())?;
        self.server = Some(server);
        self.rx = Some(rx);
        Ok(())
    }

    /// Stop the server (tear down the socket thread) — the menu's "Stop server".
    /// Idempotent: a no-op when nothing is running.
    pub fn stop(&mut self) {
        self.server = None; // Server::drop joins the socket thread
        self.rx = None;
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

    /// A short status label for the window title (bgb shows the link state there),
    /// or `None` when no server is running: `"MCP :<port>"`.
    #[must_use]
    pub fn status_label(&self) -> Option<String> {
        self.port().map(|p| format!("MCP :{p}"))
    }

    /// Per-wake pump: execute every queued tool call against the live machine
    /// and reply. A no-op when no server is running, so it is safe to call every
    /// wake (including while paused/broken — that is exactly when an agent wants
    /// to inspect). Reaps a dead socket thread.
    pub fn pump(&mut self, gb: &GameBoy, dbg: &mut Debugger, symbols: &SymbolTable) {
        if self
            .server
            .as_ref()
            .is_some_and(server::Server::is_finished)
        {
            self.server = None;
            self.rx = None;
            return;
        }
        let Some(rx) = &self.rx else { return };
        loop {
            match rx.try_recv() {
                Ok(job) => {
                    let result = match &job.call {
                        ToolInvocation::Builtin(call) => {
                            tools::dispatch(call, gb, dbg.breakpoints_mut(), symbols)
                        }
                        ToolInvocation::Plugin { name, args } => {
                            let mut ctx = FrontendToolContext {
                                gb,
                                breakpoints: dbg.breakpoints_mut(),
                                symbols,
                            };
                            self.tools
                                .dispatch(name, args, &mut ctx)
                                .unwrap_or_else(|| Err(format!("unknown tool '{name}'")))
                        }
                    };
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
