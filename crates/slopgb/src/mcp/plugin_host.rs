//! Tool-plugin hosting for the MCP server: loads tier-2 `LoadedTool` wasm
//! modules and dispatches their tools against the live machine.
//!
//! [`FrontendToolContext`] is the bridge the borrow-based tool host calls back
//! into: the scalar reads come off the [`GameBoy`], and the formatted/rendered
//! results delegate to the same [`crate::mcp::tools`] code the built-in tools
//! use, so a ported plugin's output is byte-identical. The one mutation
//! (`set_breakpoint`) pokes the App-owned breakpoint set, exactly like the
//! built-in `breakpoint` tool — golden-safe (empty by default).

use std::fs;
use std::path::Path;

use slopgb_core::{GameBoy, SCREEN_H, SCREEN_W};
use slopgb_plugin_host::{LoadedTool, ToolContext, ToolResult as PluginToolResult};

use crate::dbg::Breakpoints;
use crate::mcp::addr::Addr;
use crate::mcp::json::{self, Json};
use crate::mcp::{tools, vram};
use crate::symbols::SymbolTable;

/// The live machine + debugger surface a tool plugin reads through. Borrowed for
/// the duration of one tool call.
pub struct FrontendToolContext<'a> {
    pub gb: &'a GameBoy,
    pub breakpoints: &'a mut Breakpoints,
    pub symbols: &'a SymbolTable,
}

impl ToolContext for FrontendToolContext<'_> {
    fn gb(&self) -> &GameBoy {
        self.gb
    }
    fn set_breakpoint(&mut self, addr: u16) {
        self.breakpoints.set(addr);
    }
    fn registers(&self) -> String {
        tools::registers(self.gb)
    }
    fn cdl_ranges(&self) -> String {
        tools::cdl_ranges(self.gb)
    }
    fn disassemble(&self, bank: u16, from: u16, to: u16) -> String {
        tools::disassemble(
            self.gb,
            self.symbols,
            Addr { bank, addr: from },
            Addr { bank, addr: to },
        )
    }
    fn vram_png(&self, view: &str, scale: u32) -> Vec<u8> {
        match vram::capture(self.gb, view) {
            Ok(bmp) => tools::encode_scaled(&bmp.px, bmp.w, bmp.h, scale),
            Err(_) => Vec::new(),
        }
    }
    fn screencap_png(&self, scale: u32) -> Vec<u8> {
        tools::encode_scaled(self.gb.frame(), SCREEN_W, SCREEN_H, scale)
    }
    fn eval_expr(&self, expr: &str) -> String {
        tools::expr_eval(self.gb, expr)
    }
}

/// One tool a loaded plugin advertises, for MCP `tools/list`. The socket thread
/// gets a clone of these (the modules themselves stay on the UI thread).
#[derive(Clone, Debug)]
pub struct PluginMeta {
    pub name: String,
    pub description: String,
    /// The parsed input schema (a `{"type":"object", …}` value), or a minimal
    /// object schema when the plugin's schema string doesn't parse.
    pub schema: Json,
}

/// The loaded tool-plugin modules. Empty unless `--plugins` pointed at a
/// directory with tool modules; a non-tool (tier-1) module there is skipped.
#[derive(Default)]
pub struct ToolPlugins {
    modules: Vec<LoadedTool>,
}

impl ToolPlugins {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load tool plugins from the `--plugins` / `SLOPGB_PLUGINS_DIR` directory in
    /// `opts` — the same directory tier-1 plugins use.
    #[must_use]
    pub fn from_options(opts: &crate::cli::Options) -> Self {
        let dir = opts
            .plugins_dir
            .clone()
            .or_else(|| std::env::var_os("SLOPGB_PLUGINS_DIR").map(std::path::PathBuf::from));
        Self::load(dir.as_deref())
    }

    /// Load every tool module in `dir` (`None` → empty). A `*.wasm` that isn't a
    /// tool module (e.g. a tier-1 plugin) fails [`LoadedTool::load`] and is
    /// skipped, so it can coexist with tier-1 plugins in the same directory.
    #[must_use]
    pub fn load(dir: Option<&Path>) -> Self {
        let mut out = Self::new();
        let Some(dir) = dir else { return out };
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("slopgb: cannot read plugins dir '{}': {e}", dir.display());
                return out;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "wasm") {
                continue;
            }
            match fs::read(&path).ok().and_then(|b| LoadedTool::load(&b).ok()) {
                Some(m) => out.modules.push(m),
                None => { /* not a tool module (or unreadable): tier-1 owns it */ }
            }
        }
        out
    }

    /// Every tool across all modules, for `tools/list`. On a name collision the
    /// first-loaded wins (deterministic; a client sees each name once).
    #[must_use]
    pub fn metadata(&self) -> Vec<PluginMeta> {
        let mut out: Vec<PluginMeta> = Vec::new();
        for m in &self.modules {
            for t in m.tools() {
                if out.iter().any(|p| p.name == t.name) {
                    continue;
                }
                let schema = json::parse(&t.schema).unwrap_or_else(|_| empty_schema());
                out.push(PluginMeta {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    schema,
                });
            }
        }
        out
    }

    /// Dispatch a call to the tool named `name`, or `None` if no loaded plugin
    /// exposes it. Errors (a trap, a bad index) come back as the `Err` string.
    #[must_use]
    pub fn dispatch(
        &self,
        name: &str,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Option<Result<tools::ToolResult, String>> {
        for m in &self.modules {
            if let Some(idx) = m.index_of(name) {
                return Some(
                    m.call(idx, args, ctx)
                        .map(convert)
                        .map_err(|e| e.to_string()),
                );
            }
        }
        None
    }
}

/// A minimal object input schema, for a plugin whose schema string didn't parse.
fn empty_schema() -> Json {
    Json::obj([
        ("type", Json::str("object")),
        ("properties", Json::obj([])),
        ("required", Json::Arr(Vec::new())),
    ])
}

fn convert(r: PluginToolResult) -> tools::ToolResult {
    match r {
        PluginToolResult::Text(s) => tools::ToolResult::Text(s),
        PluginToolResult::Image(b) => tools::ToolResult::Image(b),
    }
}

#[cfg(test)]
#[path = "plugin_host_tests.rs"]
mod tests;
