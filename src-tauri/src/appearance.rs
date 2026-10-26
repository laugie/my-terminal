//! Terminal theme, font, the auto-beautify switch and the topology view. The frontend applies
//! them; this only keeps the choice in `config.json`.

use tauri::Manager;

use crate::config::AppearanceView;
use crate::hosts::HostState;

#[tauri::command]
pub async fn appearance_load(state: tauri::State<'_, HostState>) -> Result<AppearanceView, String> {
    state.appearance_view()
}

/// `None` keeps the current value. A config that cannot be written keeps the
/// choice for this run and reports why it was not saved.
#[tauri::command]
pub async fn appearance_set(
    app: tauri::AppHandle,
    theme: Option<String>,
    font: Option<String>,
    auto_beautify: Option<bool>,
    topology_mode: Option<String>,
) -> Result<AppearanceView, String> {
    let (saved, view) = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<HostState>();
        let mut saved = Ok(());
        if theme.is_some() || font.is_some() {
            saved = state.set_appearance(theme.as_deref(), font.as_deref());
        }
        if let Some(enabled) = auto_beautify.filter(|_| saved.is_ok()) {
            saved = state.set_auto_beautify(enabled);
        }
        if let Some(mode) = topology_mode.filter(|_| saved.is_ok()) {
            saved = state.set_topology_mode(&mode);
        }
        (saved, state.appearance_view())
    })
    .await
    .map_err(|_| "无法保存设置".to_string())?;
    let mut view = view?;
    if let Err(message) = saved {
        view.message = Some(message);
    }
    Ok(view)
}
