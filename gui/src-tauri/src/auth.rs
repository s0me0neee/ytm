// tauri::command's macro expansion emits an unreachable!() on async fns, which the workspace lints deny by default.
#![allow(clippy::unreachable)]

use tauri::{AppHandle, State};
use ytm_core::Browser;

use crate::state::AppState;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn auth_status(state: State<'_, AppState>) -> bool {
    state.session.is_set_up()
}

/// Every browser whose cookie store can be read, by display label (e.g. `"Chrome"`).
#[tauri::command]
pub fn list_browsers() -> Vec<String> {
    Browser::ALL.iter().map(|b| b.label().to_string()).collect()
}

/// Reads cookies from `browser`'s own profile (by display label), writes
/// `browser.json`, then bootstraps the library the same way `setup()` does
/// for an already-signed-in session.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub async fn sign_in(app: AppHandle, state: State<'_, AppState>, browser: String) -> Result<(), String> {
    let parsed = Browser::parse(&browser).ok_or_else(|| format!("unknown browser: {browser}"))?;
    state
        .session
        .setup_with_browser(parsed)
        .map_err(|e| e.to_string())?;

    // bootstrap() is sync (see its own doc comment for why) and must run on a
    // spawn_blocking thread, not inline here on this command's async worker.
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::library::bootstrap(&app, &state))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_browser_parses_back() {
        for label in list_browsers() {
            assert!(
                Browser::parse(&label).is_some(),
                "{label} didn't round-trip through Browser::parse"
            );
        }
    }
}
