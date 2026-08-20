use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::{dsh, env, state::AppState, theme, update::UpdateInfo};

/// Everything the frontend needs to render its shell in one call.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub toolchain: env::ToolchainStatus,
    pub web_status: dsh::WebStatus,
    pub failed_reason: Option<String>,
    pub web_port: u16,
    pub theme: theme::ThemePreference,
    pub update: UpdateInfo,
    /// Whether the running DeepSeek Harness web can be restarted/stopped from
    /// here: either this instance spawned it, or the listener on the web port
    /// is a local Windows node.exe process (probe already confirmed it serves
    /// dsh).
    pub managed: bool,
    /// When `managed` is false: the image name of whoever holds the web port
    /// (e.g. "wslrelay.exe" for a DeepSeek Harness web running inside WSL).
    pub external_process: Option<String>,
}

#[tauri::command]
pub async fn get_status(app: AppHandle) -> Result<StatusPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let toolchain = {
            let mut guard = state.toolchain.lock().unwrap();
            match guard.as_ref() {
                Some(t) => t.clone(),
                None => {
                    let detected = env::detect();
                    *guard = Some(detected.clone());
                    detected
                }
            }
        };
        let (web_status, failed_reason) = dsh::compute_status(&app);
        let theme = *state.theme.lock().unwrap();
        let update = state.update_info.lock().unwrap().clone();
        let owned = state.child.lock().unwrap().is_some();
        let listener_name = dsh::listener_process_name(dsh::web_port());
        let managed = owned || dsh::image_is_node(listener_name.as_deref());
        let external_process = if managed { None } else { listener_name };
        Ok(StatusPayload {
            toolchain,
            web_status,
            failed_reason,
            web_port: dsh::web_port(),
            theme,
            update,
            managed,
            external_process,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn start_dsh(app: AppHandle) -> Result<dsh::WebStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dsh::spawn_web(&app)?;
        let (status, _) = dsh::compute_status(&app);
        Ok(status)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn stop_dsh(app: AppHandle) -> Result<dsh::WebStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dsh::stop_server(&app);
        let (status, _) = dsh::compute_status(&app);
        dsh::emit_status(&app, status);
        Ok(status)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn restart_dsh(app: AppHandle) -> Result<dsh::WebStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Stop whatever DeepSeek Harness server we may manage: the child this
        // run spawned, or a local node.exe serving our port (previous session
        // / manual start).
        // A genuinely foreign holder (e.g. a WSL relay for a server inside
        // WSL) is never touched — just report the current status instead of
        // spawning a Windows-side instance that would conflict with it.
        if matches!(dsh::stop_server(&app), dsh::StopOutcome::Nothing) {
            let (status, _) = dsh::compute_status(&app);
            return Ok(status);
        }
        // Wait until the port is actually free before spawning again.
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if !matches!(dsh::probe(), Ok(true)) {
                break;
            }
        }
        // Refuse to spawn on top of a server that would not release the port.
        if matches!(dsh::probe(), Ok(true)) {
            return Err("DeepSeek Harness 未能释放端口,重启中止".to_string());
        }
        dsh::spawn_web(&app)?;
        let (status, _) = dsh::compute_status(&app);
        dsh::emit_status(&app, status);
        Ok(status)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn check_update(app: AppHandle, force: bool) -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || crate::update::perform_check(&app, force))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn upgrade_dsh(app: AppHandle) -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || crate::update::run_upgrade(&app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn dismiss_update(app: AppHandle, version: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut persisted = crate::update::load_persisted(&app);
        persisted.dismissed_version = Some(version.clone());
        crate::update::save_persisted(&app, &persisted);
        let mut info = app.state::<AppState>().update_info.lock().unwrap().clone();
        info.dismissed_version = Some(version);
        let _ = app.emit("update-status", info);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn open_in_browser(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(dsh::web_url(), None::<String>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_nodejs(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url("https://nodejs.org/zh-cn/", None::<String>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_theme() -> theme::ThemePreference {
    theme::read_theme()
}
