//! Tray-Icon mit lokalisiertem Menü.

use crate::{resolve_language, show_main_window, AppState, Status};
use llm_bridge_core::stats::Range;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main";

struct Texts {
    running: &'static str,
    stopped: &'static str,
    open: &'static str,
    start: &'static str,
    stop: &'static str,
    quit: &'static str,
    tokens_24h: &'static str,
    update_pending: &'static str,
}

fn texts(lang: &str) -> Texts {
    match lang {
        "de" => Texts {
            running: "● Läuft",
            stopped: "○ Gestoppt",
            open: "LLM Bridge öffnen",
            start: "Proxy starten",
            stop: "Proxy stoppen",
            quit: "Beenden",
            tokens_24h: "Tokens (24 h)",
            update_pending: "Update {v} – wird installiert, sobald untätig",
        },
        _ => Texts {
            running: "● Running",
            stopped: "○ Stopped",
            open: "Open LLM Bridge",
            start: "Start proxy",
            stop: "Stop proxy",
            quit: "Quit",
            tokens_24h: "Tokens (24 h)",
            update_pending: "Update {v} – installs when idle",
        },
    }
}

fn build_menu(app: &AppHandle, status: &Status) -> tauri::Result<Menu<tauri::Wry>> {
    let t = texts(&status.language);
    let state_line = match &status.address {
        Some(a) if status.running => format!("{} – {a}", t.running),
        _ => t.stopped.to_string(),
    };
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "status", state_line, false, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open", t.open, true, None::<&str>)?,
            &MenuItem::with_id(app, "toggle", if status.running { t.stop } else { t.start }, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?,
        ],
    )?;
    if let Some(v) = &status.pending_update {
        let text = t.update_pending.replace("{v}", v);
        menu.insert(&MenuItem::with_id(app, "update", text, false, None::<&str>)?, 1)?;
    }
    Ok(menu)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    let initial = Status {
        running: false,
        address: None,
        base_url: String::new(),
        last_error: None,
        version: llm_bridge_core::VERSION,
        config_path: String::new(),
        db_path: String::new(),
        language: resolve_language(&app.state::<AppState>().proxy.config().ui.language),
        pending_update: None,
    };
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("LLM Bridge")
        .menu(&build_menu(app, &initial)?)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "toggle" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let running = state.status().await.running;
                    if running {
                        state.stop().await;
                    } else if let Err(e) = state.start().await {
                        tracing::error!("{e}");
                    }
                    crate::notify(&app).await;
                });
            }
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    app.state::<AppState>().stop().await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if !cfg!(target_os = "macos") {
                    show_main_window(tray.app_handle());
                }
            }
        })
        .build(app)?;
    update_tooltip(app);
    Ok(())
}

pub fn refresh(app: &AppHandle, status: &Status) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_menu(app, status) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    update_tooltip(app);
}

fn fmt_n(n: u64, sep: char) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

pub fn update_tooltip(app: &AppHandle) {
    let state = app.state::<AppState>();
    let lang = resolve_language(&state.proxy.config().ui.language);
    let t = texts(&lang);
    let sep = if lang == "de" { '.' } else { ',' };
    let total = state.proxy.stats.summary(Range::Day).map(|s| s.total_tokens).unwrap_or(0);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(format!("LLM Bridge – {}: {}", t.tokens_24h, fmt_n(total, sep))));
    }
}
