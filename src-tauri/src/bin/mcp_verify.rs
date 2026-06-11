// Manual harness for P2-3: start the loopback MCP server against a real DB so an
// external CLI (codex/claude) can connect. See debug_probe::run_mcp_verify_from_env.
fn main() {
    if let Err(err) =
        tauri::async_runtime::block_on(lumenfolio_desktop_lib::debug_probe::run_mcp_verify_from_env())
    {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
