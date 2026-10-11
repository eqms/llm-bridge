//! Automatische Updates: neue Version suchen, signiert laden und erst installieren,
//! wenn der Proxy einige Minuten keine Anfrage bearbeitet hat.

use crate::AppState;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// So lange darf keine Anfrage gelaufen sein, bevor die App für das Update neu startet.
const QUIET: Duration = Duration::from_secs(5 * 60);
const IDLE_POLL: Duration = Duration::from_secs(30);

pub fn spawn(app: AppHandle) {
    // Entwicklungs-Builds würden sich sonst durch das veröffentlichte Release ersetzen.
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if enabled(&app) {
                if let Err(e) = check_and_install(&app).await {
                    tracing::warn!("Update fehlgeschlagen: {e}");
                    crate::set_pending_update(&app, None).await;
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

fn enabled(app: &AppHandle) -> bool {
    app.state::<AppState>().proxy.config().ui.auto_update
}

async fn check_and_install(app: &AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(());
    };
    tracing::info!("Update auf {} gefunden, lade herunter", update.version);
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    crate::set_pending_update(app, Some(update.version.clone())).await;

    let state = app.state::<AppState>();
    while !state.proxy.activity.is_idle(QUIET) {
        tokio::time::sleep(IDLE_POLL).await;
    }
    if !enabled(app) {
        tracing::info!("Automatische Updates wurden abgeschaltet, {} wird nicht installiert", update.version);
        crate::set_pending_update(app, None).await;
        return Ok(());
    }

    tracing::info!("Installiere Update {}", update.version);
    let was_running = state.status().await.running;
    state.stop().await;
    if let Err(e) = update.install(bytes) {
        if was_running {
            let _ = state.start().await;
        }
        crate::notify(app).await;
        return Err(e.to_string());
    }
    app.restart();
}

#[cfg(test)]
mod tests {
    /// App (Tauri-Plugin) und CLI müssen denselben Schlüssel und dasselbe Manifest nutzen.
    #[test]
    fn plugin_config_matches_core() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(updater["pubkey"], llm_bridge_core::update::PUBLIC_KEY);
        assert_eq!(updater["endpoints"][0], llm_bridge_core::update::MANIFEST_URL);
        assert_eq!(updater["requireSignedVersion"], true);
        assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);
    }
}
