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
    pub owned: bool,
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
        Ok(StatusPayload {
            toolchain,
            web_status,
            failed_reason,
            web_port: dsh::web_port(),
            theme,
            update,
            owned,
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
        dsh::stop_owned(&app);
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
        dsh::stop_owned(&app);
        // Wait until the port is actually free before spawning again.
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if dsh::probe().is_err() {
                break;
            }
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
