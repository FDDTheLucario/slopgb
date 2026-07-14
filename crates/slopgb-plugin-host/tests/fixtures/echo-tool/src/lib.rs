//! Tool-plugin round-trip fixture: echoes its args and reads PC through the
//! view, so the host test can assert both the arg path and introspection work.

use slopgb_plugin_api::{GameBoyView, Reg, ToolPlugin, ToolResult, slopgb_tool_plugin};

struct EchoTool;

impl ToolPlugin for EchoTool {
    fn new() -> Self {
        EchoTool
    }

    fn name(&self) -> &str {
        "echo"
    }

    fn call(&mut self, args: &str, gb: &GameBoyView) -> ToolResult {
        ToolResult::Text(format!("{args}|pc={:04X}", gb.reg(Reg::Pc)))
    }
}

slopgb_tool_plugin!(EchoTool);
