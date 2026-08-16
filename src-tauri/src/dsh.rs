use std::io::{BufRead, BufReader, Read};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// CreateProcess flag: no console window for spawned CLI processes.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Default port of `dsh web`; overridable via DSH_DESKTOP_PORT.
pub const DEFAULT_WEB_PORT: u16 = 3080;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WebStatus {
    NotInstalled,
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Clone, serde::Serialize)]
pub struct LogEvent {
    pub source: String,
    pub line: String,
}

#[derive(Clone, serde::Serialize)]
pub struct WebStatusEvent {
    pub status: WebStatus,
}

/// A spawned `dsh web` child process that this app owns.
pub struct DshProcess {
    pub child: Child,
    pub pid: u32,
}

pub fn web_port() -> u16 {
    std::env::var("DSH_DESKTOP_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_WEB_PORT)
}

pub fn web_url() -> String {
    format!("http://127.0.0.1:{}/", web_port())
}

/// Probe the local web server.
///
/// * `Ok(true)`  — a dsh-looking service answered 200 (recognized by the
///   "DeepSeek Harness" markers in the served HTML).
/// * `Ok(false)` — something else answered on the port (wrong service).
/// * `Err(_)`    — nothing is listening.
pub fn probe() -> Result<bool, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client.get(web_url()).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Ok(false);
    }
    // Read the first chunk (up to 8 KiB) and look for dsh markers.
    let mut buf = [0u8; 8192];
    let n = resp.read(&mut buf).unwrap_or(0);
    let body = String::from_utf8_lossy(&buf[..n]).to_lowercase();
    Ok(body.contains("deepseek") || body.contains("dsh") || body.contains("harness"))
}

/// Compute the current web status from process state plus a live probe.
pub fn compute_status(app: &AppHandle) -> (WebStatus, Option<String>) {
    let state = app.state::<AppState>();
    let dsh_installed = state
        .toolchain
        .lock()
        .unwrap()
        .as_ref()
        .map(|t| t.dsh_found)
        .unwrap_or(false);
    if !dsh_installed {
        return (WebStatus::NotInstalled, None);
    }
    match probe() {
        Ok(true) => return (WebStatus::Running, None),
        Ok(false) => {
            // A service answered but it is not dsh; only report the conflict
            // once no child of ours is left that could still be booting.
            if state.child.lock().unwrap().is_none() {
                return (
                    WebStatus::Failed,
                    Some(format!(
                        "端口 {} 被其他程序占用,且响应不是 dsh 服务",
                        web_port()
                    )),
                );
            }
        }
        Err(_) => { /* nothing listening yet */ }
    }
    if state.child.lock().unwrap().is_some() {
        return (WebStatus::Starting, None);
    }
    let reason = state.failed_reason.lock().unwrap().clone();
    if reason.is_some() {
        return (WebStatus::Failed, reason);
    }
    (WebStatus::Stopped, None)
}

pub fn emit_status(app: &AppHandle, status: WebStatus) {
    *app.state::<AppState>().web_status.lock().unwrap() = Some(status);
    let _ = app.emit("web-status-changed", WebStatusEvent { status });
}

pub fn stream_output(app: AppHandle, out: Option<impl Read + Send + 'static>, source: &'static str) {
    if let Some(out) = out {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = app.emit(
                    "proc-log",
                    LogEvent {
                        source: source.to_string(),
                        line,
                    },
                );
            }
        });
    }
}

/// Spawn `dsh web` (unless a dsh server is already answering) and start a
/// readiness watcher that flips the status to Running as soon as the server
/// responds.
pub fn spawn_web(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let toolchain = state.toolchain.lock().unwrap().clone().ok_or("工具链尚未初始化")?;
    let dsh_cmd = toolchain
        .dsh_cmd
        .clone()
        .ok_or("未检测到 dsh,请先执行: npm install -g @deepseek-ai/dsh")?;

    if matches!(probe(), Ok(true)) {
        emit_status(app, WebStatus::Running);
        return Ok(());
    }

    {
        let mut guard = state.child.lock().unwrap();
        if let Some(existing) = guard.as_mut() {
            match existing.child.try_wait() {
                Ok(Some(_)) => *guard = None, // dead child: reap and respawn
                Ok(None) => return Err("dsh web 已在启动中".into()),
                Err(_) => return Err("无法确认 dsh web 进程状态".into()),
            }
        }
    }
    *state.failed_reason.lock().unwrap() = None;

    // `cmd /C dsh web` resolves dsh via PATH, which is far more robust than
    // quoting a full .cmd path (cmd.exe mangles those quotes). Prepend the
    // resolved bin directory to PATH so this also works when dsh was found
    // through the npm-bin fallback instead of PATH.
    let mut args: Vec<String> = vec!["/C".into(), "dsh".into(), "web".into()];
    if web_port() != DEFAULT_WEB_PORT {
        // dsh web --port <N> is part of the web app's own flag family.
        args.push("--port".into());
        args.push(web_port().to_string());
    }
    let mut cmd = Command::new("cmd");
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(bin_dir) = Path::new(&dsh_cmd).parent() {
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", format!("{};{}", bin_dir.to_string_lossy(), path));
        }
    }
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| format!("启动 dsh web 失败: {e}"))?;
    let pid = child.id();
    stream_output(app.clone(), child.stdout.take(), "dsh");
    stream_output(app.clone(), child.stderr.take(), "dsh");

    *state.child.lock().unwrap() = Some(DshProcess { child, pid });
    emit_status(app, WebStatus::Starting);
    spawn_readiness_watcher(app.clone());
    Ok(())
}

fn spawn_readiness_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(300);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(500));
            if app.state::<AppState>().child.lock().unwrap().is_none() {
                return; // our child exited; the poller classifies the outcome
            }
            if matches!(probe(), Ok(true)) {
                emit_status(&app, WebStatus::Running);
                return;
            }
        }
    });
}

/// Stop the dsh web process we spawned. Returns true when we owned one.
/// A server started outside this app is never touched.
pub fn stop_owned(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let process = state.child.lock().unwrap().take();
    if let Some(p) = process {
        kill_tree(p.pid);
        *state.failed_reason.lock().unwrap() = None;
        true
    } else {
        false
    }
}

/// Kill a process and its whole tree (Windows).
pub fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

/// Background thread: every 5s, reap an exited child and broadcast the web
/// status when it changed.
pub fn spawn_status_poller(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));

        let state = app.state::<AppState>();
        let mut exited_code: Option<Option<i32>> = None;
        {
            let mut guard = state.child.lock().unwrap();
            if let Some(p) = guard.as_mut() {
                if let Ok(Some(status)) = p.child.try_wait() {
                    exited_code = Some(status.code());
                    *guard = None;
                }
            }
        }

        if let Some(code) = exited_code {
            match probe() {
                // Another dsh server is already answering — fine, we attach.
                Ok(true) => {}
                Ok(false) => {
                    *state.failed_reason.lock().unwrap() = Some(format!(
                        "dsh web 进程已退出 (exit {:?}),且端口 {} 被其他程序占用",
                        code,
                        web_port()
                    ));
                }
                Err(e) => {
                    *state.failed_reason.lock().unwrap() =
                        Some(format!("dsh web 进程已退出 (exit {code:?}),连接失败: {e}"));
                }
            }
        }

        let (status, _) = compute_status(&app);
        let changed = {
            let mut guard = state.web_status.lock().unwrap();
            let changed = *guard != Some(status);
            *guard = Some(status);
            changed
        };
        if changed {
            let _ = app.emit("web-status-changed", WebStatusEvent { status });
        }
    });
}
