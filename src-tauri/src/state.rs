use crate::dsh::{DshProcess, WebStatus};
use crate::env::ToolchainStatus;
use crate::theme::ThemePreference;
use crate::update::UpdateInfo;
use std::sync::Mutex;

/// Shared application state. Every field is a `Mutex` because background
/// threads (status poller, theme watcher, periodic updater) and command
/// handlers touch the same state concurrently.
pub struct AppState {
    /// The DeepSeek Harness web child process we spawned, if any. Only a process
    /// recorded
    /// here counts as "owned" and gets killed on exit / stop / restart.
    pub child: Mutex<Option<DshProcess>>,
    /// Why the last spawn attempt failed, when the server is down.
    pub failed_reason: Mutex<Option<String>>,
    /// Detected node/npm/dsh toolchain, primed at startup.
    pub toolchain: Mutex<Option<ToolchainStatus>>,
    /// Latest theme preference read from settings.yaml.
    pub theme: Mutex<ThemePreference>,
    /// Latest update/version information.
    pub update_info: Mutex<UpdateInfo>,
    /// Last web status we broadcast, used to only emit on change.
    pub web_status: Mutex<Option<WebStatus>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            failed_reason: Mutex::new(None),
            toolchain: Mutex::new(None),
            theme: Mutex::new(ThemePreference::System),
            update_info: Mutex::new(UpdateInfo::default()),
            web_status: Mutex::new(None),
        }
    }
}
