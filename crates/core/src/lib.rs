//! LLM Bridge – lokaler OpenAI-kompatibler Proxy für Anthropic-Messages-Endpunkte.
//!
//! Clients wie MacWhisper sprechen nur das OpenAI-Format (`/v1/chat/completions`).
//! Dieser Kern nimmt solche Anfragen an, übersetzt sie in das Anthropic-Format
//! (`/v1/messages`), leitet sie an den konfigurierten Upstream (z. B. LangDock)
//! weiter und übersetzt die Antwort – inklusive Streaming – zurück.
//! Jede Anfrage wird mit ihrem Token-Verbrauch in einer lokalen SQLite-Datenbank erfasst.

pub mod activity;
pub mod config;
pub mod error;
pub mod proxy;
pub mod secrets;
pub mod server;
pub mod stats;
pub mod translate;
pub mod update;

pub use config::{default_config_path, default_db_path, AuthMode, Config, ModelRoute, Upstream};
pub use proxy::{ProxyState, TestResult};
pub use server::{start, ProxyHandle};
pub use stats::{Range, Report, Stats};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
