use std::path::PathBuf;
use std::sync::mpsc;

use notify::{recommended_watcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// Theme preference read from `<DSH_HOME>\settings.yaml` → `ui-theme.preference`.
/// `system` means "follow the OS", which the frontend resolves via matchMedia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    Dark,
    Light,
    System,
}

#[derive(Clone, serde::Serialize)]
pub struct ThemeEvent {
    pub preference: ThemePreference,
}

/// DSH home: %DSH_HOME% when set, otherwise `~/.dsh`.
pub fn dsh_home() -> PathBuf {
    if let Some(home) = std::env::var_os("DSH_HOME") {
        return PathBuf::from(home);
    }
    dirs::home_dir()
        .map(|h| h.join(".dsh"))
        .unwrap_or_else(|| PathBuf::from(".dsh"))
}

/// The settings file dsh actually uses is `settings.yaml`; the user-facing
/// requirement mentioned `setting.yaml`, so we read both (in that order).
pub fn settings_candidates() -> Vec<PathBuf> {
    vec![
        dsh_home().join("settings.yaml"),
        dsh_home().join("setting.yaml"),
    ]
}

/// Read the current theme preference; falls back to `system` (dsh's own
/// default) when the file is missing or the key is absent/invalid.
pub fn read_theme() -> ThemePreference {
    for path in settings_candidates() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) else {
            continue;
        };
        let Some(preference) = doc
            .get("ui-theme")
            .and_then(|t| t.get("preference"))
            .and_then(|p| p.as_str())
        else {
            continue;
        };
        return match preference {
            "dark" => ThemePreference::Dark,
            "light" => ThemePreference::Light,
            _ => ThemePreference::System,
        };
    }
    ThemePreference::System
}

/// Watch `<DSH_HOME>` (plus both candidate files) for changes and broadcast
/// `theme-changed` whenever the resolved preference changes. Watching the
/// directory covers the case where settings.yaml does not exist yet; watching
/// the files covers edits. File watches are re-armed on every event so a file
/// created after startup is picked up.
pub fn spawn_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = match recommended_watcher(move |result| {
            let _ = tx.send(result);
        }) {
            Ok(w) => w,
            Err(_) => return,
        };
        let home = dsh_home();
        let _ = watcher.watch(&home, RecursiveMode::NonRecursive);

        loop {
            match rx.recv() {
                Ok(Ok(_event)) => {
                    for path in settings_candidates() {
                        let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
                    }
                    let theme = read_theme();
                    let changed = {
                        let state = app.state::<AppState>();
                        let mut guard = state.theme.lock().unwrap();
                        let changed = *guard != theme;
                        *guard = theme;
                        changed
                    };
                    if changed {
                        let _ = app.emit("theme-changed", ThemeEvent { preference: theme });
                    }
                }
                Ok(Err(_)) | Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    /// Serialize tests that mutate the process-global DSH_HOME env var.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct TempSettings {
        dir: std::path::PathBuf,
    }

    impl TempSettings {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("deepseek-harness-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }
        fn write(&self, name: &str, content: &str) {
            let mut f = std::fs::File::create(self.dir.join(name)).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }
        fn clear(&self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Run a closure with DSH_HOME pointed at `dir`, restoring it afterwards.
    fn with_dsh_home(dir: &std::path::Path, f: impl FnOnce()) {
        let saved = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", dir);
        f();
        match saved {
            Some(v) => std::env::set_var("DSH_HOME", v),
            None => std::env::remove_var("DSH_HOME"),
        }
    }

    #[test]
    fn theme_prefers_settings_yaml_over_setting_yaml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = TempSettings::new("prefers-settings");
        with_dsh_home(&tmp.dir, || {
            tmp.write("settings.yaml", "ui-theme:\n  preference: dark\n");
            tmp.write("setting.yaml", "ui-theme:\n  preference: light\n");
            assert_eq!(read_theme(), ThemePreference::Dark);
        });
        tmp.clear();
    }

    #[test]
    fn theme_reads_setting_yaml_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = TempSettings::new("fallback-setting");
        with_dsh_home(&tmp.dir, || {
            tmp.write("setting.yaml", "ui-theme:\n  preference: light\n");
            assert_eq!(read_theme(), ThemePreference::Light);
        });
        tmp.clear();
    }

    #[test]
    fn theme_missing_file_defaults_to_system() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = TempSettings::new("missing-file");
        with_dsh_home(&tmp.dir, || {
            assert_eq!(read_theme(), ThemePreference::System);
        });
        tmp.clear();
    }

    #[test]
    fn theme_invalid_value_defaults_to_system() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = TempSettings::new("invalid-value");
        with_dsh_home(&tmp.dir, || {
            tmp.write("settings.yaml", "ui-theme:\n  preference: banana\n");
            assert_eq!(read_theme(), ThemePreference::System);
        });
        tmp.clear();
    }

    #[test]
    fn theme_missing_ui_theme_section_defaults_to_system() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = TempSettings::new("missing-section");
        with_dsh_home(&tmp.dir, || {
            tmp.write("settings.yaml", "other-section:\n  key: value\n");
            assert_eq!(read_theme(), ThemePreference::System);
        });
        tmp.clear();
    }
}
