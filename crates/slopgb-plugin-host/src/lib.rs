//! Runtime that loads slopgb wasm plugins and drives them against a live
//! `GameBoy`. Guest SDK is `slopgb-plugin-api`; guide is
//! `docs/ui-state/plugin-api.md`.

mod host;
mod snapshot;

pub use host::{LoadError, LoadedPlugin, PluginHost};
pub use snapshot::Snapshot;
