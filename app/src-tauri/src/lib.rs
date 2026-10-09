//! Tauri-Hülle um den Proxy-Kern: Tray-Icon, Fenster, Befehle für die Oberfläche.

mod tray;

use llm_bridge_core::secrets::{self, KeySource};
use llm_bridge_core::stats::{Range, Report};
use llm_bridge_core::{config, Config, ProxyHandle, ProxyState, Stats, TestResult};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::Mutex;

pub struct AppState {
    proxy: Arc<ProxyState>,
    handle: Mutex<Option<ProxyHandle>>,
    last_error: std::sync::Mutex<Option<String>>,
    config_path: PathBuf,
    db_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    running: bool,
    address: Option<String>,
    base_url: String,
    last_error: Option<String>,
    version: &'static str,
    config_path: String,
    db_path: String,
    language: String,
}

/// Sprache für Backend-Texte (Tray). `auto` → Systemsprache.
pub fn resolve_language(setting: &str) -> String {
    let lang = if setting.is_empty() || setting == "auto" {
        sys_locale::get_locale().unwrap_or_else(|| "en".into())
    } else {
        setting.to_string()
    };
    lang.split(['-', '_']).next().unwrap_or("en").to_ascii_lowercase()
}

impl AppState {
    async fn status(&self) -> Status {
        let cfg = self.proxy.config();
        let handle = self.handle.lock().await;
        let running = handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false);
        let address = handle.as_ref().filter(|_| running).map(|h| h.addr.to_string());
        let host = if cfg.server.host == "0.0.0.0" { "127.0.0.1".to_string() } else { cfg.server.host.clone() };
        Status {
            running,
            address,
            base_url: format!("http://{host}:{}/v1", cfg.server.port),
            last_error: self.last_error.lock().ok().and_then(|e| e.clone()),
            version: llm_bridge_core::VERSION,
            config_path: self.config_path.display().to_string(),
            db_path: self.db_path.display().to_string(),
            language: resolve_language(&cfg.ui.language),
        }
    }

    fn set_error(&self, e: Option<String>) {
        if let Ok(mut le) = self.last_error.lock() {
            *le = e;
        }
    }

    async fn start(&self) -> Result<(), String> {
        let mut handle = self.handle.lock().await;
        if handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false) {
            return Ok(());
        }
        match llm_bridge_core::start(self.proxy.clone()).await {
            Ok(h) => {
                *handle = Some(h);
                self.set_error(None);
                Ok(())
            }
            Err(e) => {
                self.set_error(Some(e.clone()));
                Err(e)
            }
        }
    }

    async fn stop(&self) {
        if let Some(h) = self.handle.lock().await.take() {
            h.stop().await;
        }
    }
}

type Cmd<T> = Result<T, String>;

async fn notify(app: &AppHandle) {
    let state = app.state::<AppState>();
    let status = state.status().await;
    tray::refresh(app, &status);
    let _ = app.emit("status-changed", &status);
}

#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Cmd<Status> {
    Ok(state.status().await)
}

#[tauri::command]
async fn start_proxy(app: AppHandle, state: State<'_, AppState>) -> Cmd<Status> {
    let res = state.start().await;
    notify(&app).await;
    res?;
    Ok(state.status().await)
}

#[tauri::command]
async fn stop_proxy(app: AppHandle, state: State<'_, AppState>) -> Cmd<Status> {
    state.stop().await;
    notify(&app).await;
    Ok(state.status().await)
}

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Config {
    state.proxy.config()
}

#[tauri::command]
async fn save_config(app: AppHandle, state: State<'_, AppState>, config: Config) -> Cmd<Status> {
    config.validate()?;
    config.save(&state.config_path)?;
    let old = state.proxy.config();
    state.proxy.set_config(config.clone());
    let was_running = state.handle.lock().await.is_some();
    if was_running && (old.server.host != config.server.host || old.server.port != config.server.port) {
        state.stop().await;
        let _ = state.start().await;
    }
    notify(&app).await;
    Ok(state.status().await)
}

#[tauri::command]
async fn key_status(state: State<'_, AppState>) -> Cmd<HashMap<String, KeySource>> {
    let ups = state.proxy.config().upstreams;
    tokio::task::spawn_blocking(move || ups.iter().map(|u| (u.id.clone(), secrets::key_source(u))).collect())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_api_key(state: State<'_, AppState>, upstream_id: String, key: String) -> Cmd<()> {
    let id = upstream_id.clone();
    tokio::task::spawn_blocking(move || secrets::set_api_key(&id, &key)).await.map_err(|e| e.to_string())??;
    state.proxy.invalidate_key(&upstream_id);
    Ok(())
}

#[tauri::command]
async fn delete_api_key(state: State<'_, AppState>, upstream_id: String) -> Cmd<()> {
    let id = upstream_id.clone();
    tokio::task::spawn_blocking(move || secrets::delete_api_key(&id)).await.map_err(|e| e.to_string())??;
    state.proxy.invalidate_key(&upstream_id);
    Ok(())
}

#[tauri::command]
async fn test_model(state: State<'_, AppState>, model: String) -> Cmd<TestResult> {
    Ok(state.proxy.test_route(&model).await)
}

#[tauri::command]
async fn upstream_models(state: State<'_, AppState>, upstream_id: String) -> Cmd<Vec<String>> {
    state.proxy.upstream_models(&upstream_id).await
}

#[tauri::command]
async fn get_report(state: State<'_, AppState>, range: String) -> Cmd<Report> {
    let range = Range::parse(&range).unwrap_or(Range::Week);
    let stats = state.proxy.stats.clone();
    tokio::task::spawn_blocking(move || stats.report(range, 100)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reset_stats(app: AppHandle, state: State<'_, AppState>) -> Cmd<()> {
    let stats = state.proxy.stats.clone();
    tokio::task::spawn_blocking(move || stats.reset()).await.map_err(|e| e.to_string())??;
    let _ = app.emit("stats-reset", ());
    notify(&app).await;
    Ok(())
}

#[tauri::command]
async fn export_csv(state: State<'_, AppState>, range: String) -> Cmd<String> {
    let range = Range::parse(&range).unwrap_or(Range::All);
    let dir = directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(|p| p.to_path_buf()).or_else(|| Some(d.home_dir().to_path_buf())))
        .unwrap_or_else(std::env::temp_dir);
    let stamp = llm_bridge_core::stats::now_ms() / 1000;
    let path = dir.join(format!("llm-bridge-stats-{stamp}.csv"));
    let stats = state.proxy.stats.clone();
    let p2 = path.clone();
    tokio::task::spawn_blocking(move || stats.export_csv(range, &p2)).await.map_err(|e| e.to_string())??;
    Ok(path.display().to_string())
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Cmd<bool> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Cmd<bool> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    al.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
async fn quit_app(app: AppHandle, state: State<'_, AppState>) -> Cmd<()> {
    state.stop().await;
    app.exit(0);
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hyper=warn,reqwest=warn,tao=warn,wry=warn".into()),
        )
        .with_target(false)
        .init();

    let config_path = config::default_config_path();
    let db_path = config::default_db_path();
    let (cfg, cfg_error) = match Config::load_or_init(&config_path) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(format!("Konfiguration ungültig, Standardwerte aktiv: {e}"))),
    };
    let stats = Stats::open(&db_path).or_else(|e| {
        tracing::error!("Statistik-DB nicht nutzbar ({e}), verwende Arbeitsspeicher");
        Stats::in_memory()
    });
    let stats = match stats {
        Ok(s) => s,
        Err(e) => panic!("Statistik konnte nicht initialisiert werden: {e}"),
    };
    let start_minimized_flag = std::env::args().any(|a| a == "--minimized");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .manage(AppState {
            proxy: ProxyState::new(cfg.clone(), stats),
            handle: Mutex::new(None),
            last_error: std::sync::Mutex::new(cfg_error),
            config_path,
            db_path,
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_proxy,
            stop_proxy,
            get_config,
            save_config,
            key_status,
            set_api_key,
            delete_api_key,
            test_model,
            upstream_models,
            get_report,
            reset_stats,
            export_csv,
            get_autostart,
            set_autostart,
            quit_app,
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::create(app.handle())?;

            // Neue Statistik-Einträge live an die Oberfläche und den Tray-Tooltip weiterreichen
            let handle = app.handle().clone();
            let mut rx = app.state::<AppState>().proxy.stats.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(rec) => {
                            let _ = handle.emit("request-recorded", &rec);
                            tray::update_tooltip(&handle);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            });

            let handle = app.handle().clone();
            let autostart_proxy = cfg.ui.autostart_proxy;
            tauri::async_runtime::spawn(async move {
                if autostart_proxy {
                    let state = handle.state::<AppState>();
                    if let Err(e) = state.start().await {
                        tracing::error!("Proxy konnte nicht starten: {e}");
                    }
                }
                notify(&handle).await;
            });

            if !(cfg.ui.start_minimized || start_minimized_flag) {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Schließen versteckt nur das Fenster – der Proxy läuft im Tray weiter.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("Tauri-App konnte nicht gestartet werden")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                show_main_window(_app);
            }
        });
}
