pub mod appearance;
pub mod auth;
pub mod config;
pub mod hosts;
pub mod session;
pub mod topology;
pub mod zoom;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;

    tauri::Builder::default()
        .setup(|app| {
            let dir = config::resolve_app_config_dir(app.handle())?;
            let hosts = hosts::HostState::open(&dir);
            let lock_enabled = hosts
                .login_lock_view()
                .map(|view| view.enabled)
                .unwrap_or(false);
            let auth = auth::open_state();
            auth.set_lock_enabled(lock_enabled);
            // Zoom before the first paint. The page applies it again on load.
            if let (Ok(view), Some(window)) =
                (hosts.ui_zoom_view(), app.get_webview_window("main"))
            {
                if view.percent != 100 {
                    let _ = zoom::apply_zoom(&window, view.percent);
                }
            }
            app.manage(hosts);
            app.manage(session::SessionState::new());
            app.manage(auth);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::auth_status,
            auth::auth_setup,
            auth::auth_login,
            auth::auth_settings,
            auth::auth_set_lock,
            hosts::list_hosts,
            hosts::add_manual_host,
            hosts::remove_manual_host,
            topology::load_topology,
            session::open_session,
            session::write_session,
            session::resize_session,
            session::close_session,
            zoom::ui_zoom_load,
            zoom::ui_zoom_set,
            appearance::appearance_load,
            appearance::appearance_set,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                if let Some(state) = app.try_state::<session::SessionState>() {
                    state.shutdown_blocking();
                }
            }
        });
}
