use std::{
    fs::{self, OpenOptions},
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

const MAX_DIAGNOSTIC_LOG_BYTES: u64 = 10 * 1024 * 1024;
const DIAGNOSTIC_LOG_FILE: &str = "lumenfolio-diagnostics.log";

pub(crate) fn append_translation_debug(app: &AppHandle, label: &str, payload: serde_json::Value) {
    if let Err(err) = append_diagnostic_log(app, "translation", label, payload) {
        log::warn!("Failed to write translation diagnostic log: {err}");
    }
}

pub(crate) fn append_index_debug(app: &AppHandle, label: &str, payload: serde_json::Value) {
    if let Err(err) = append_diagnostic_log(app, "index", label, payload) {
        log::warn!("Failed to write index diagnostic log: {err}");
    }
}

fn append_diagnostic_log(
    app: &AppHandle,
    scope: &str,
    label: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("Failed to resolve app data dir: {err}"))?;
    fs::create_dir_all(&app_data_dir)
        .map_err(|err| format!("Failed to create app data dir: {err}"))?;
    let log_path = app_data_dir.join(DIAGNOSTIC_LOG_FILE);
    rotate_log_if_needed(&log_path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| {
            format!(
                "Failed to open diagnostic log {}: {err}",
                log_path.display()
            )
        })?;
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let line = serde_json::json!({
        "ts_ms": timestamp_ms,
        "scope": scope,
        "label": label,
        "payload": payload,
    });
    writeln!(file, "{line}").map_err(|err| {
        format!(
            "Failed to write diagnostic log {}: {err}",
            log_path.display()
        )
    })?;
    Ok(())
}

fn rotate_log_if_needed(log_path: &std::path::Path) -> Result<(), String> {
    let Ok(metadata) = fs::metadata(log_path) else {
        return Ok(());
    };
    if metadata.len() < MAX_DIAGNOSTIC_LOG_BYTES {
        return Ok(());
    }
    let rotated_path = log_path.with_extension("log.1");
    if rotated_path.exists() {
        fs::remove_file(&rotated_path).map_err(|err| {
            format!(
                "Failed to remove rotated diagnostic log {}: {err}",
                rotated_path.display()
            )
        })?;
    }
    fs::rename(log_path, &rotated_path).map_err(|err| {
        format!(
            "Failed to rotate diagnostic log {}: {err}",
            log_path.display()
        )
    })?;
    Ok(())
}
