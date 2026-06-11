// Manual harnesses for Mode B (local-agent MCP). With LUMEN_VERIFY_Q set, runs the
// full agentic dispatch (generate_answer_agentic) end-to-end and prints the answer +
// captured citations. Without it, just starts the MCP server and keeps it alive so an
// external CLI can connect. See debug_probe::run_{agentic_probe,mcp_verify}_from_env.
fn main() {
    let fut = async {
        if std::env::var("LUMEN_VERIFY_Q").is_ok() {
            lumenfolio_desktop_lib::debug_probe::run_agentic_probe_from_env().await
        } else {
            lumenfolio_desktop_lib::debug_probe::run_mcp_verify_from_env().await
        }
    };
    if let Err(err) = tauri::async_runtime::block_on(fut) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
