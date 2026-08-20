mod commands;
mod dsh;
mod env;
mod state;
mod theme;
mod update;

use state::AppState;
use tauri::Manager;

/// Plugin that injects window-control buttons into the DeepSeek Harness web iframe.
fn inject_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::<tauri::Wry>::new("dsh-inject")
        .js_init_script_on_all_frames(include_str!("inject.js"))
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(inject_plugin())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch focuses the existing window instead of opening
            // a new one (and spawning a second server).
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::start_dsh,
            commands::stop_dsh,
            commands::restart_dsh,
            commands::check_update,
            commands::upgrade_dsh,
            commands::dismiss_update,
            commands::open_in_browser,
            commands::open_nodejs,
            commands::get_theme
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            {
                let state = app.state::<AppState>();
                *state.toolchain.lock().unwrap() = Some(env::detect());
                *state.theme.lock().unwrap() = theme::read_theme();
            }
            theme::spawn_watcher(handle.clone());
            dsh::spawn_status_poller(handle.clone());
            update::spawn_periodic_checker(handle.clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {
            // 关闭窗口/退出应用时不再终止 DeepSeek Harness 服务:让它留在后台继续运行,
            // 下次打开桌面端时 spawn_web 会通过端口探测自动复用已运行的服务器。
            // 需要停止时可在界面中点击「停止」。
        });
}
