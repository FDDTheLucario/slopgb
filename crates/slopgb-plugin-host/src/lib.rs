//! Runtime that loads slopgb wasm plugins and drives them against a live
//! `GameBoy`. Guest SDK is `slopgb-plugin-api`; guide is
//! `docs/ui-state/plugin-api.md`.

mod coprocessor;
mod host;
mod manifest;
mod registry;
mod snapshot;
mod tool;

pub use coprocessor::LoadedCoprocessor;
pub use host::{LoadError, LoadedPlugin, PluginHost, PluginInfo};
pub use manifest::{FlagContribution, Manifest, MenuContribution};
pub use registry::{Context, PluginRegistry, RegistryError, Unit};
pub use slopgb_plugin_api::ToolResult;
pub use snapshot::Snapshot;
pub use tool::{LoadedTool, ToolContext, ToolMeta};
