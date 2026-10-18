pub mod hosts;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(hosts::HostState::default())
        .invoke_handler(tauri::generate_handler![
            hosts::list_hosts,
            hosts::add_manual_host,
            hosts::remove_manual_host,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
