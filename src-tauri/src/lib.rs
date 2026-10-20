pub mod config;
pub mod hosts;
pub mod session;
pub mod topology;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;

    tauri::Builder::default()
        .setup(|app| {
            let dir = config::resolve_app_config_dir(app.handle())?;
            app.manage(hosts::HostState::open(&dir));
            app.manage(session::SessionState::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            hosts::list_hosts,
            hosts::add_manual_host,
            hosts::remove_manual_host,
            topology::load_topology,
            session::open_session,
            session::write_session,
            session::resize_session,
            session::close_session,
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
