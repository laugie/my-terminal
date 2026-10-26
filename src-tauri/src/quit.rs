//! Ask before closing the window or quitting while sessions are open.
//!
//! The close button and Alt+F4 arrive as `CloseRequested` and are held with
//! `prevent_close`. On macOS, tao does not implement
//! `applicationShouldTerminate`, so the stock Quit item (`terminate:`) cannot
//! be held; the app menu uses its own Quit item with Cmd+Q instead. Dock
//! "Quit" and log out still go through the system and are not asked.
//!
//! The page shows the in-app dialog (`window.confirm` does nothing in the
//! macOS webview, see #31) and reports back. A page that never reports the
//! dialog cannot trap the app: the next close after [`PROMPT_GRACE`] goes
//! through.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};

use crate::session::SessionState;

/// How long an unanswered request holds the next one back.
pub const PROMPT_GRACE: Duration = Duration::from_secs(3);

/// Event sent to the page when a close needs confirming.
pub const CLOSE_REQUESTED_EVENT: &str = "close-requested";

/// Menu id of the app's own Quit item (macOS).
pub const QUIT_MENU_ID: &str = "my-terminal-quit";

/// The last request sent to the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prompt {
    pub since: Instant,
    /// The page reported the dialog.
    pub shown: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Allow,
    Ask,
}

/// Close at once with no open sessions or after a confirm. Otherwise ask,
/// unless the previous request was never shown and is older than the grace.
pub fn decide(
    confirmed: bool,
    sessions: usize,
    previous: Option<Prompt>,
    now: Instant,
) -> CloseDecision {
    if confirmed || sessions == 0 {
        return CloseDecision::Allow;
    }
    match previous {
        Some(prompt)
            if !prompt.shown && now.saturating_duration_since(prompt.since) >= PROMPT_GRACE =>
        {
            CloseDecision::Allow
        }
        _ => CloseDecision::Ask,
    }
}

#[derive(Default)]
struct Inner {
    confirmed: bool,
    prompt: Option<Prompt>,
}

#[derive(Default)]
pub struct QuitState {
    inner: Mutex<Inner>,
}

impl QuitState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CloseRequested {
    sessions: usize,
}

/// True when the window may close (or the app quit) now. Otherwise the page
/// has been asked and the caller must hold the close.
pub fn request_close<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let sessions = app
        .try_state::<SessionState>()
        .map(|state| state.open_count())
        .unwrap_or(0);
    let Some(state) = app.try_state::<QuitState>() else {
        return true;
    };
    let now = Instant::now();
    let mut guard = state.lock();
    match decide(guard.confirmed, sessions, guard.prompt, now) {
        CloseDecision::Allow => {
            guard.confirmed = true;
            true
        }
        CloseDecision::Ask => {
            guard.prompt = Some(Prompt {
                since: now,
                shown: false,
            });
            drop(guard);
            // Without a page to ask, do not keep the window open.
            app.emit(CLOSE_REQUESTED_EVENT, CloseRequested { sessions })
                .is_err()
        }
    }
}

/// The page has shown the dialog for the last request.
#[tauri::command]
pub fn close_prompt_shown(state: tauri::State<'_, QuitState>) {
    if let Some(prompt) = state.lock().prompt.as_mut() {
        prompt.shown = true;
    }
}

/// The user confirmed: quit. Sessions are ended by the exit handler.
#[tauri::command]
pub fn quit_confirmed(app: tauri::AppHandle, state: tauri::State<'_, QuitState>) {
    state.lock().confirmed = true;
    app.exit(0);
}

/// The default macOS menu with the stock Quit item replaced by one the app
/// can hold. Everything else matches `Menu::default`.
#[cfg(target_os = "macos")]
pub fn app_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{
        AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID,
        WINDOW_SUBMENU_ID,
    };

    let pkg_info = app.package_info();
    let config = app.config();
    let about = AboutMetadata {
        name: Some(pkg_info.name.clone()),
        version: Some(pkg_info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config.bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };
    let quit = MenuItem::with_id(
        app,
        QUIT_MENU_ID,
        format!("Quit {}", pkg_info.name),
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    let window_menu = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    let help_menu = Submenu::with_id_and_items(app, HELP_SUBMENU_ID, "Help", true, &[])?;
    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                pkg_info.name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app, None, Some(about))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &quit,
                ],
            )?,
            &Submenu::with_items(
                app,
                "File",
                true,
                &[&PredefinedMenuItem::close_window(app, None)?],
            )?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "View",
                true,
                &[&PredefinedMenuItem::fullscreen(app, None)?],
            )?,
            &window_menu,
            &help_menu,
        ],
    )
}
