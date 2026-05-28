fn main() {
    if let Err(err) =
        tauri::async_runtime::block_on(lumenfolio_desktop_lib::debug_probe::run_from_env())
    {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
