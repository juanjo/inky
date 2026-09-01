// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `Inky --mcp`: serve MCP over stdio for agents and exit — no window, no Tauri.
    if std::env::args().skip(1).any(|a| a == "--mcp") {
        std::process::exit(inky_lib::mcp::serve_stdio_blocking());
    }
    inky_lib::run()
}
