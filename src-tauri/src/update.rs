use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::dsh::{stream_output, CREATE_NO_WINDOW};
use crate::env;
use crate::state::AppState;

/// Version/update snapshot shared with the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateInfo {
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub last_check_date: Option<String>,
    pub dismissed_version: Option<String>,
    pub last_error: Option<String>,
}

impl Default for UpdateInfo {
    fn default() -> Self {
        Self {
            current_version: None,
            latest_version: None,
            update_available: false,
            last_check_date: None,
            dismissed_version: None,
            last_error: None,
        }
    }
}

#[derive(Clone, serde::Serialize)]
pub struct UpgradeDone {
    pub success: bool,
}

/// Persisted gate state: when we last checked and which version the user
/// dismissed, so the toast does not nag twice for the same release.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub struct Persisted {
    pub last_check_date: Option<String>,
    pub dismissed_version: Option<String>,
}

fn state_file(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("state.json"))
}

pub fn load_persisted(app: &AppHandle) -> Persisted {
    state_file(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_persisted(app: &AppHandle, persisted: &Persisted) {
    if let Some(path) = state_file(app) {
        if let Ok(json) = serde_json::to_string_pretty(persisted) {
            let _ = std::fs::write(path, json);
        }
    }
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The installed dsh version as detected in the toolchain snapshot.
pub fn installed_version(app: &AppHandle) -> Option<String> {
    app.state::<AppState>()
        .toolchain
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|t| t.dsh_version.clone())
}

/// Query the npm registry for the latest published dsh version.
pub fn latest_version() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("https://registry.npmjs.org/@deepseek-ai/dsh/latest")
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("registry 返回 HTTP {}", resp.status()));
    }
    let value: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| "registry 响应缺少 version 字段".into())
}

/// Semver comparison; prereleases (0.1.0-rc.6) order correctly.
pub fn is_update(current: &str, latest: &str) -> Option<bool> {
    let current = semver::Version::parse(current).ok()?;
    let latest = semver::Version::parse(latest).ok()?;
    Some(latest > current)
}

/// Broadcast the current update info to the frontend.
fn publish(app: &AppHandle, info: UpdateInfo) {
    *app.state::<AppState>().update_info.lock().unwrap() = info.clone();
    let _ = app.emit("update-status", info);
}

/// Check for a new dsh version, honoring the once-per-day gate unless forced.
pub fn perform_check(app: &AppHandle, force: bool) -> Result<UpdateInfo, String> {
    let mut persisted = load_persisted(app);
    let day = today();
    let current = installed_version(app);

    let mut info = app.state::<AppState>().update_info.lock().unwrap().clone();
    info.current_version = current.clone();
    info.last_error = None;

    // Once-per-day gate: skip the network when we already checked today.
    if !force && persisted.last_check_date.as_deref() == Some(day.as_str()) {
        info.last_check_date = Some(day);
        info.dismissed_version = persisted.dismissed_version.clone();
        publish(app, info.clone());
        return Ok(info);
    }

    let latest = match latest_version() {
        Ok(v) => v,
        Err(e) => {
            info.dismissed_version = persisted.dismissed_version.clone();
            publish(app, info.clone());
            if force {
                info.last_error = Some(e.clone());
                publish(app, info.clone());
                return Err(e);
            }
            return Ok(info); // silent failure on the background timer
        }
    };

    let update_available = match &current {
        Some(c) => is_update(c, &latest).unwrap_or(false),
        None => false,
    };

    persisted.last_check_date = Some(day.clone());
    save_persisted(app, &persisted);

    info.latest_version = Some(latest);
    info.update_available = update_available;
    info.last_check_date = Some(day);
    info.dismissed_version = persisted.dismissed_version.clone();
    publish(app, info.clone());
    Ok(info)
}

/// Background thread: re-check every 6 hours.
pub fn spawn_periodic_checker(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(6 * 3600));
        let _ = perform_check(&app, false);
    });
}

/// Run `npm install -g @deepseek-ai/dsh`, streaming npm output as
/// `proc-log` events, then refresh versions and broadcast the result.
pub fn run_upgrade(app: &AppHandle) -> Result<UpdateInfo, String> {
    let state = app.state::<AppState>();
    let toolchain = state.toolchain.lock().unwrap().clone().ok_or("工具链尚未初始化")?;
    let npm_cmd = toolchain.npm_cmd.clone().ok_or("未检测到 npm,请先安装 Node.js")?;

    // Same PATH-based invocation as dsh::spawn_web: `cmd /C npm ...` avoids
    // cmd.exe's quote mangling of full .cmd paths.
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "npm", "install", "-g", "@deepseek-ai/dsh"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(bin_dir) = std::path::Path::new(&npm_cmd).parent() {
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", format!("{};{}", bin_dir.to_string_lossy(), path));
        }
    }
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| format!("启动 npm 失败: {e}"))?;
    stream_output(app.clone(), child.stdout.take(), "npm");
    stream_output(app.clone(), child.stderr.take(), "npm");

    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        let _ = app.emit("upgrade-done", UpgradeDone { success: false });
        return Err(format!(
            "npm install 失败 (exit code {})",
            status.code().unwrap_or(-1)
        ));
    }

    // Re-detect the toolchain so the freshly installed version shows up.
    *state.toolchain.lock().unwrap() = Some(env::detect());

    // Reset the daily gate + dismissal so the UI re-evaluates immediately.
    let mut persisted = load_persisted(app);
    persisted.last_check_date = None;
    persisted.dismissed_version = None;
    save_persisted(app, &persisted);

    let _ = app.emit("upgrade-done", UpgradeDone { success: true });
    perform_check(app, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_prerelease_ordering() {
        assert_eq!(is_update("0.1.0-rc.5", "0.1.0-rc.6"), Some(true));
        assert_eq!(is_update("0.1.0-rc.6", "0.1.0-rc.6"), Some(false));
        assert_eq!(is_update("0.1.0-rc.10", "0.1.0-rc.9"), Some(false));
        assert_eq!(is_update("0.1.0", "0.2.0"), Some(true));
        assert_eq!(is_update("1.0.0", "1.0.1"), Some(true));
        assert_eq!(is_update("2.0.0", "1.9.9"), Some(false));
    }

    #[test]
    fn semver_invalid_inputs() {
        assert_eq!(is_update("not-a-version", "0.2.0"), None);
        assert_eq!(is_update("0.1.0", "latest"), None);
    }
}
