//! Runtime that loads slopgb wasm plugins and drives them against a live
//! `GameBoy`. Guest SDK is `slopgb-plugin-api`; guide is
//! `docs/ui-state/plugin-api.md`.

mod coprocessor;
mod host;
mod snapshot;
mod tool;

pub use coprocessor::LoadedCoprocessor;
pub use host::{LoadError, LoadedPlugin, PluginHost, PluginInfo};
pub use slopgb_plugin_api::ToolResult;
pub use snapshot::Snapshot;
pub use tool::LoadedTool;
