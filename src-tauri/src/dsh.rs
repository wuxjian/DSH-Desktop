use std::io::{BufRead, BufReader, Read};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// CreateProcess flag: no console window for spawned CLI processes.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Default port of the DeepSeek Harness web server; overridable via DSH_DESKTOP_PORT.
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

/// A spawned DeepSeek Harness web child process that this app owns.
pub struct DshProcess {
    pub child: Child,
    pub pid: u32,
}

/// Result of attempting to stop the DeepSeek Harness server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// Killed the child process this instance spawned.
    OwnedKilled(u32),
    /// Killed a local Windows node process that was serving DeepSeek Harness
    /// on our port (e.g. left running by a previous app session or started by
    /// hand).
    AdoptedKilled(u32),
    /// Nothing to stop that we may touch: the port is held by a foreign
    /// process (such as a WSL relay).
    Nothing,
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

/// Parse `netstat -ano -p tcp` output and return the PID listening on `port`.
///
/// Lines look like (header row excluded):
/// `TCP    127.0.0.1:3080    0.0.0.0:0    LISTENING    31532`
/// Tokens: [proto, local, foreign, state, pid].
pub fn parse_listening_pid(output: &str, port: u16) -> Option<u32> {
    let port_suffix = format!(":{port}");
    for line in output.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 5
            && fields[3].eq_ignore_ascii_case("LISTENING")
            && fields[1].ends_with(&port_suffix)
        {
            if let Ok(pid) = fields[4].parse::<u32>() {
                return Some(pid);
            }
        }
    }
    None
}

/// The PID whose process listens on `port` for any local address.
/// (0.0.0.0 / 127.0.0.1 / [::] / [::1] — WSL 转发时 Windows 侧监听者则是 wslrelay 等,由调用方区分。)
pub fn listening_pid(port: u16) -> Option<u32> {
    let out = Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_listening_pid(&String::from_utf8_lossy(&out.stdout), port)
}

/// Parse the first CSV row of `tasklist /FO CSV /NH` and return the image name.
/// Rows look like `"node.exe","31532","Console","1","31,784 K"` (note the
/// comma inside the memory column, so no naive splitting).
pub fn parse_tasklist_image(output: &str) -> Option<String> {
    for line in output.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let name = line.strip_prefix('"')?.split('"').next()?;
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Image name of the process with `pid`, or None when it no longer exists.
pub fn pid_image_name(pid: u32) -> Option<String> {
    let out = Command::new("tasklist")
        .args([
            "/FI",
            &format!("PID eq {pid}"),
            "/FO",
            "CSV",
            "/NH",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_tasklist_image(&String::from_utf8_lossy(&out.stdout))
}

/// True when `name` is the Windows node.exe (case-insensitive). Only such a
/// local listener counts as a DeepSeek Harness server the desktop may
/// kill/restart;
/// WSL relays (wslrelay.exe / vmmem / wslservice…) and everything else do not.
pub fn image_is_node(name: Option<&str>) -> bool {
    name.map_or(false, |n| n.eq_ignore_ascii_case("node.exe"))
}

/// Human-readable image name of whoever currently listens on the web port,
/// or None when nothing is listening / we failed to look it up.
pub fn listener_process_name(port: u16) -> Option<String> {
    pid_image_name(listening_pid(port)?)
}

/// Stop the DeepSeek Harness server if we are allowed to: the child this
/// instance spawned, or a local Windows node.exe that is serving dsh on our
/// port.
/// A genuinely foreign holder (WSL relay etc.) is never touched.
pub fn stop_server(app: &AppHandle) -> StopOutcome {
    let state = app.state::<AppState>();

    let owned = state.child.lock().unwrap().take();
    if let Some(p) = owned {
        kill_tree(p.pid);
        *state.failed_reason.lock().unwrap() = None;
        return StopOutcome::OwnedKilled(p.pid);
    }

    let Some(pid) = listening_pid(web_port()) else {
        return StopOutcome::Nothing;
    };
    if !image_is_node(pid_image_name(pid).as_deref()) {
        return StopOutcome::Nothing;
    }
    kill_tree(pid);
    *state.failed_reason.lock().unwrap() = None;
    StopOutcome::AdoptedKilled(pid)
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

/// Spawn the DeepSeek Harness web server (unless a dsh server is already
/// answering) and start a
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
                Ok(None) => return Err("DeepSeek Harness 已在启动中".into()),
                Err(_) => return Err("无法确认 DeepSeek Harness 进程状态".into()),
            }
        }
    }
    *state.failed_reason.lock().unwrap() = None;

    // `cmd /C dsh web` resolves dsh via PATH, which is far more robust than
    // quoting a full .cmd path (cmd.exe mangles those quotes). Prepend the
    // resolved bin directory to PATH so this also works when dsh was found
    // through the npm-bin fallback instead of PATH.
    // `--no-open`:界面由桌面端内嵌展示,不让 dsh web 自己打开系统浏览器。
    let mut args: Vec<String> = vec!["/C".into(), "dsh".into(), "web".into(), "--no-open".into()];
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
    let mut child = cmd.spawn().map_err(|e| format!("启动 DeepSeek Harness 失败: {e}"))?;
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
                        "DeepSeek Harness 进程已退出 (exit {:?}),且端口 {} 被其他程序占用",
                        code,
                        web_port()
                    ));
                }
                Err(e) => {
                    *state.failed_reason.lock().unwrap() =
                        Some(format!("DeepSeek Harness 进程已退出 (exit {code:?}),连接失败: {e}"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_listening_ipv4_loopback() {
        let out = "\r\nActive Connections\r\n\r\n  Proto  Local Address          Foreign Address        State           PID\r\n  TCP    127.0.0.1:3080         0.0.0.0:0              LISTENING       31532\r\n";
        assert_eq!(parse_listening_pid(out, 3080), Some(31532));
    }

    #[test]
    fn parse_listening_all_interfaces() {
        let out = "  TCP    0.0.0.0:3080          0.0.0.0:0              LISTENING       777";
        assert_eq!(parse_listening_pid(out, 3080), Some(777));
    }

    #[test]
    fn parse_listening_ipv6_forms() {
        let out = "TCP    [::]:3099        [::]:0        LISTENING    42";
        assert_eq!(parse_listening_pid(out, 3099), Some(42));
        let out2 = "TCP    [::1]:3080       [::]:0        LISTENING    43";
        assert_eq!(parse_listening_pid(out2, 3080), Some(43));
    }

    #[test]
    fn parse_listening_ignores_other_states_and_ports() {
        let out = "TCP    127.0.0.1:3080     127.0.0.1:49958  ESTABLISHED  1\n\
                  TCP    127.0.0.1:3081     0.0.0.0:0  LISTENING  2\n\
                  TCP    192.168.1.5:53080  0.0.0.0:0          LISTENING       99";
        assert_eq!(parse_listening_pid(out, 3080), None);
    }

    #[test]
    fn parse_listening_garbage_lines() {
        let out = "\u{feff}Active Connections\n\n  Proto  Local Address\nfoo bar\n";
        assert_eq!(parse_listening_pid(out, 3080), None);
    }

    #[test]
    fn parse_tasklist_csv_name_with_comma_in_memory() {
        let out = "\"node.exe\",\"31532\",\"Console\",\"1\",\"31,784 K\"";
        assert_eq!(parse_tasklist_image(out).as_deref(), Some("node.exe"));
    }

    #[test]
    fn parse_tasklist_no_matching_tasks() {
        let out = "INFO: No tasks are running which match the specified criteria.";
        assert_eq!(parse_tasklist_image(out), None);
    }

    #[test]
    fn parse_tasklist_wsl_relay() {
        let out = "\"wslrelay.exe\",\"8080\",\"Services\",\"0\",\"5,120 K\"";
        assert_eq!(parse_tasklist_image(out).as_deref(), Some("wslrelay.exe"));
    }

    #[test]
    fn parse_tasklist_empty_output() {
        assert_eq!(parse_tasklist_image(""), None);
        assert_eq!(parse_tasklist_image("\r\n"), None);
    }

    #[test]
    fn node_image_is_manageable_others_are_not() {
        assert!(image_is_node(Some("node.exe")));
        assert!(image_is_node(Some("NODE.EXE")));
        assert!(!image_is_node(Some("nodejs.exe")));
        assert!(!image_is_node(Some("wslrelay.exe")));
        assert!(!image_is_node(Some("vmmem")));
        assert!(!image_is_node(Some("wslservice.exe")));
        assert!(!image_is_node(Some("svchost.exe")));
        assert!(!image_is_node(None));
    }

    /// Live-system check (ignored by default): the classifier must recognize
    /// the real DeepSeek Harness listener on this machine as a local node.exe
    /// process.
    /// Run explicitly with: `cargo test -- --ignored`
    #[test]
    #[ignore]
    fn live_listener_on_3080_is_local_node() {
        let pid = listening_pid(3080).expect("3080 should be listening here");
        let name = pid_image_name(pid).expect("listener process should exist");
        assert!(
            image_is_node(Some(&name)),
            "listener is {name} (pid {pid}) — expected node.exe"
        );
    }
}
