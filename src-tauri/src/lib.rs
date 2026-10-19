pub mod config;
pub mod hosts;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;

    tauri::Builder::default()
        .setup(|app| {
            let dir = config::resolve_app_config_dir(app.handle())?;
            app.manage(hosts::HostState::open(&dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            hosts::list_hosts,
            hosts::add_manual_host,
            hosts::remove_manual_host,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
