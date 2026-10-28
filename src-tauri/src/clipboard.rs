//! System clipboard text for the terminal.
//!
//! The page cannot rely on `navigator.clipboard`: reading shows WebKit's
//! "Paste" callout on macOS and asks for permission in WebView2, so copy and
//! the Windows right-click paste go through the app instead. Keyboard and menu
//! pastes still arrive as the webview's own `paste` event.

use tauri::Manager;

/// Larger text is refused both ways.
pub const MAX_CLIPBOARD_BYTES: usize = 4 * 1024 * 1024;

const TOO_LARGE: &str = "剪贴板内容过大";

/// Refuse text over [`MAX_CLIPBOARD_BYTES`].
pub fn check_size(text: &str) -> Result<(), String> {
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err(TOO_LARGE.to_string());
    }
    Ok(())
}

fn require_access(app: &tauri::AppHandle) -> Result<(), String> {
    let auth = app.state::<crate::auth::AuthState>();
    crate::auth::require_access(&auth, auth.lock_enabled()).map_err(|err| err.to_string())
}

/// Clipboard text, or an empty string when it holds no text.
#[tauri::command]
pub async fn clipboard_read_text(app: tauri::AppHandle) -> Result<String, String> {
    require_access(&app)?;
    let text = tauri::async_runtime::spawn_blocking(|| {
        let mut clipboard = arboard::Clipboard::new().map_err(|_| "无法读取剪贴板".to_string())?;
        match clipboard.get_text() {
            Ok(text) => Ok(text),
            Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
            Err(_) => Err("无法读取剪贴板".to_string()),
        }
    })
    .await
    .map_err(|_| "无法读取剪贴板".to_string())??;
    check_size(&text)?;
    Ok(text)
}

#[tauri::command]
pub async fn clipboard_write_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
    require_access(&app)?;
    check_size(&text)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut clipboard = arboard::Clipboard::new().map_err(|_| "无法写入剪贴板".to_string())?;
        clipboard
            .set_text(text)
            .map_err(|_| "无法写入剪贴板".to_string())
    })
    .await
    .map_err(|_| "无法写入剪贴板".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_limit_is_inclusive() {
        assert_eq!(check_size(""), Ok(()));
        assert_eq!(check_size(&"a".repeat(MAX_CLIPBOARD_BYTES)), Ok(()));
        assert_eq!(
            check_size(&"a".repeat(MAX_CLIPBOARD_BYTES + 1)),
            Err(TOO_LARGE.to_string())
        );
    }
}
