use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Gesamte Konfiguration – wird als TOML gespeichert.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub defaults: Defaults,
    pub ui: UiConfig,
    pub upstreams: Vec<Upstream>,
    pub models: Vec<ModelRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ServerConfig {
    /// Nur an localhost binden, es sei denn, man weiß, was man tut.
    pub host: String,
    pub port: u16,
    /// Optionales lokales Token. Leer = kein Token nötig.
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Defaults {
    /// Anthropic verlangt `max_tokens`; dieser Wert gilt, wenn der Client keinen schickt.
    pub max_tokens: u32,
    /// Timeout für nicht gestreamte Anfragen in Sekunden.
    pub request_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiConfig {
    /// "auto", "de", "en", …
    pub language: String,
    pub start_minimized: bool,
    /// Proxy beim Start der App automatisch starten.
    pub autostart_proxy: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    /// `Authorization: Bearer <key>` (LangDock)
    #[default]
    Bearer,
    /// `x-api-key: <key>` (Anthropic direkt)
    XApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Upstream {
    pub id: String,
    pub name: String,
    /// z. B. `https://api.langdock.com/anthropic/eu` – `/v1` am Ende ist optional.
    pub base_url: String,
    pub auth: AuthMode,
    pub anthropic_version: String,
    /// Optional: Name einer Umgebungsvariable mit dem Schlüssel (Headless-Betrieb).
    pub api_key_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ModelRoute {
    /// Name, den der Client verwendet (z. B. `haiku`).
    pub alias: String,
    /// `id` des Upstreams.
    pub upstream: String,
    /// Modell-ID beim Upstream (z. B. `claude-haiku-5-5-default`).
    pub model: String,
    /// Optional eigener Standardwert für `max_tokens`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 4000, access_token: String::new() }
    }
}

impl Default for Defaults {
    fn default() -> Self {
        Self { max_tokens: 4096, request_timeout_secs: 300 }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { language: "auto".into(), start_minimized: false, autostart_proxy: true }
    }
}

impl Default for Upstream {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            base_url: String::new(),
            auth: AuthMode::Bearer,
            anthropic_version: "2023-06-01".into(),
            api_key_env: String::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            defaults: Defaults::default(),
            ui: UiConfig::default(),
            upstreams: vec![Upstream {
                id: "langdock-eu".into(),
                name: "LangDock EU".into(),
                base_url: "https://api.langdock.com/anthropic/eu".into(),
                auth: AuthMode::Bearer,
                ..Upstream::default()
            }],
            models: vec![ModelRoute {
                alias: "haiku".into(),
                upstream: "langdock-eu".into(),
                model: "claude-haiku-5-5-default".into(),
                max_tokens: None,
            }],
        }
    }
}

impl Upstream {
    /// Basis ohne abschließendes `/` und ohne `/v1`.
    fn root(&self) -> String {
        let mut base = self.base_url.trim().trim_end_matches('/').to_string();
        for suffix in ["/v1/messages", "/messages", "/v1"] {
            if base.ends_with(suffix) {
                base.truncate(base.len() - suffix.len());
                break;
            }
        }
        base
    }

    pub fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.root())
    }

    pub fn models_url(&self) -> String {
        format!("{}/v1/models", self.root())
    }
}

impl Config {
    pub fn upstream(&self, id: &str) -> Option<&Upstream> {
        self.upstreams.iter().find(|u| u.id == id)
    }

    /// Liefert (Upstream, Upstream-Modell, max_tokens-Override) für einen Client-Modellnamen.
    ///
    /// Reihenfolge: Alias → Upstream-Modell-ID → `upstream-id/modell` (Durchreichen).
    pub fn resolve(&self, requested: &str) -> Option<(Upstream, String, Option<u32>)> {
        let requested = requested.trim();
        if let Some(route) = self
            .models
            .iter()
            .find(|m| m.alias.eq_ignore_ascii_case(requested))
            .or_else(|| self.models.iter().find(|m| m.model == requested))
        {
            let up = self.upstream(&route.upstream)?.clone();
            return Some((up, route.model.clone(), route.max_tokens));
        }
        if let Some((up_id, model)) = requested.split_once('/') {
            if let Some(up) = self.upstream(up_id) {
                if !model.is_empty() {
                    return Some((up.clone(), model.to_string(), None));
                }
            }
        }
        None
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.server.port == 0 {
            return Err("Port darf nicht 0 sein".into());
        }
        if self.server.host.trim().is_empty() {
            return Err("Host darf nicht leer sein".into());
        }
        let mut ids = HashSet::new();
        for u in &self.upstreams {
            if u.id.trim().is_empty() {
                return Err("Jeder Upstream braucht eine ID".into());
            }
            if !u.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(format!("Upstream-ID '{}' darf nur a–z, 0–9, - und _ enthalten", u.id));
            }
            if !ids.insert(u.id.as_str()) {
                return Err(format!("Upstream-ID '{}' ist doppelt", u.id));
            }
            check_base_url(&u.id, &u.base_url)?;
        }
        let mut aliases = HashSet::new();
        for m in &self.models {
            if m.alias.trim().is_empty() || m.model.trim().is_empty() {
                return Err("Jede Modell-Zuordnung braucht Alias und Modell-ID".into());
            }
            if !aliases.insert(m.alias.to_ascii_lowercase()) {
                return Err(format!("Alias '{}' ist doppelt", m.alias));
            }
            if self.upstream(&m.upstream).is_none() {
                return Err(format!("Alias '{}' verweist auf unbekannten Upstream '{}'", m.alias, m.upstream));
            }
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cfg: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Lädt die Konfiguration oder legt beim ersten Start die Standardkonfiguration an.
    /// Eine neu angelegte Konfiguration bekommt ein zufälliges Zugriffstoken; ein bewusst
    /// geleertes Token in einer bestehenden Datei bleibt leer.
    pub fn load_or_init(path: &Path) -> Result<Self, String> {
        if path.exists() {
            Self::load(path)
        } else {
            let mut cfg = Self::default();
            cfg.server.access_token = generate_access_token();
            cfg.save(path)?;
            Ok(cfg)
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(dir) = path.parent() {
            create_private_dir(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        // atomar schreiben; die Datei enthält das Zugriffstoken, daher nur für den Besitzer lesbar
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        make_private(&tmp)?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Der API-Schlüssel geht an diese URL – daher nur `https`, `http` allein für lokale Upstreams
/// (z. B. ein LiteLLM auf demselben Rechner).
fn check_base_url(id: &str, base_url: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(base_url.trim())
        .map_err(|e| format!("Basis-URL von '{id}' ist ungültig: {e}"))?;
    let local = match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => return Err(format!("Basis-URL von '{id}' hat keinen Host")),
    };
    match url.scheme() {
        "https" => Ok(()),
        "http" if local => Ok(()),
        "http" => Err(format!("Basis-URL von '{id}' muss https:// verwenden (http:// nur für localhost)")),
        _ => Err(format!("Basis-URL von '{id}' muss mit https:// beginnen")),
    }
}

/// Legt ein Verzeichnis an; ein neu angelegtes ist nur für den Besitzer zugänglich.
/// Bestehende Verzeichnisse (etwa per `LLM_BRIDGE_CONFIG` gewählt) bleiben unverändert.
pub(crate) fn create_private_dir(dir: &Path) -> Result<(), String> {
    if dir.as_os_str().is_empty() || dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    set_mode(dir, 0o700)
}

/// Setzt eine Datei auf „nur Besitzer lesen/schreiben“ (0600).
pub(crate) fn make_private(path: &Path) -> Result<(), String> {
    set_mode(path, 0o600)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(()) // Windows: %APPDATA% ist ohnehin benutzerspezifisch
}

/// Zufälliges lokales Zugriffstoken im Format der GUI (`lb-` + Hex).
pub fn generate_access_token() -> String {
    format!("lb-{}", uuid::Uuid::new_v4().simple())
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("ai", "it-guy", "LLM Bridge")
}

/// Pfad der Konfigurationsdatei. Überschreibbar mit `LLM_BRIDGE_CONFIG`.
pub fn default_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("LLM_BRIDGE_CONFIG") {
        return PathBuf::from(p);
    }
    project_dirs()
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("llm-bridge.toml"))
}

/// Pfad der Statistik-Datenbank. Überschreibbar mit `LLM_BRIDGE_DB`.
pub fn default_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("LLM_BRIDGE_DB") {
        return PathBuf::from(p);
    }
    project_dirs()
        .map(|d| d.data_dir().join("stats.sqlite"))
        .unwrap_or_else(|| PathBuf::from("llm-bridge-stats.sqlite"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_normalisation() {
        for base in [
            "https://api.langdock.com/anthropic/eu",
            "https://api.langdock.com/anthropic/eu/",
            "https://api.langdock.com/anthropic/eu/v1",
            "https://api.langdock.com/anthropic/eu/v1/",
            "https://api.langdock.com/anthropic/eu/v1/messages",
        ] {
            let u = Upstream { base_url: base.into(), ..Upstream::default() };
            assert_eq!(u.messages_url(), "https://api.langdock.com/anthropic/eu/v1/messages");
            assert_eq!(u.models_url(), "https://api.langdock.com/anthropic/eu/v1/models");
        }
    }

    #[test]
    fn resolve_alias_and_passthrough() {
        let cfg = Config::default();
        let (up, model, _) = cfg.resolve("HAIKU").unwrap();
        assert_eq!(up.id, "langdock-eu");
        assert_eq!(model, "claude-haiku-5-5-default");
        let (_, model, _) = cfg.resolve("langdock-eu/claude-sonnet-4-6-default").unwrap();
        assert_eq!(model, "claude-sonnet-4-6-default");
        assert!(cfg.resolve("gpt-4o").is_none());
    }

    #[test]
    fn first_start_generates_token_existing_empty_token_stays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config::load_or_init(&path).unwrap();
        assert!(cfg.server.access_token.starts_with("lb-"));
        assert_eq!(cfg.server.access_token.len(), 35);
        assert_eq!(Config::load_or_init(&path).unwrap().server.access_token, cfg.server.access_token);

        let mut open = cfg;
        open.server.access_token.clear();
        open.save(&path).unwrap();
        assert_eq!(Config::load_or_init(&path).unwrap().server.access_token, "");
    }

    #[test]
    fn base_url_requires_https_except_localhost() {
        for ok in [
            "https://api.langdock.com/anthropic/eu",
            "http://localhost:4001",
            "http://127.0.0.1:4001/v1",
            "http://[::1]:4001",
        ] {
            assert!(check_base_url("x", ok).is_ok(), "{ok}");
        }
        for bad in ["http://api.anthropic.com", "http://localhost.evil.example", "ftp://localhost", "api.anthropic.com", ""] {
            assert!(check_base_url("x", bad).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn config_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("neu");
        let path = sub.join("config.toml");
        Config::default().save(&path).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&sub), 0o700);
    }

    #[test]
    fn toml_roundtrip() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }
}
