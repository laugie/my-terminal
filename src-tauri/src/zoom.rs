//! UI zoom. The webview's own page zoom scales the interface and the terminal
//! font together; CSS `zoom` would put xterm's mouse selection off target.
//! macOS uses WKWebView `pageZoom` (macOS 11+), Windows WebView2 `ZoomFactor`.

use tauri::Manager;

use crate::config::{is_zoom_level, ZoomView};
use crate::hosts::HostState;

const APPLY_ERROR: &str = "无法调整界面缩放";

/// Apply a level to one window.
pub fn apply_zoom<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    percent: u16,
) -> Result<(), String> {
    window
        .set_zoom(f64::from(percent) / 100.0)
        .map_err(|_| APPLY_ERROR.to_string())
}

/// Apply the saved level to the calling window and return it. Idempotent.
/// Not behind the login lock, so the login screen uses it too.
#[tauri::command]
pub async fn ui_zoom_load(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, HostState>,
) -> Result<ZoomView, String> {
    let view = state.ui_zoom_view()?;
    apply_zoom(&window, view.percent)?;
    Ok(view)
}

/// Apply first, then save. A config that cannot be written still zooms for
/// this run and reports why the level was not kept.
#[tauri::command]
pub async fn ui_zoom_set(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    percent: u16,
) -> Result<ZoomView, String> {
    if !is_zoom_level(percent) {
        return Err("缩放比例无效".to_string());
    }
    apply_zoom(&window, percent)?;
    let (saved, view) = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<HostState>();
        (state.set_ui_zoom(percent), state.ui_zoom_view())
    })
    .await
    .map_err(|_| "无法保存设置".to_string())?;
    let mut view = view?;
    view.percent = percent;
    if let Err(message) = saved {
        view.message = Some(message);
    }
    Ok(view)
}
