#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

fn main() {
    // Re-invoked with the `mcp-memory` subcommand, this same binary acts as an
    // out-of-process MCP server exposing one task's memory to opencode (wired in
    // via OPENCODE_CONFIG_CONTENT in the runner). Branch here, before any Tauri
    // setup, so the MCP path never spins up a GUI/event loop.
    if std::env::args().nth(1).as_deref() == Some(opencode_runner_lib::mcp_memory::SUBCOMMAND) {
        opencode_runner_lib::mcp_memory::run_stdio_server();
        return;
    }
    opencode_runner_lib::run();
}
