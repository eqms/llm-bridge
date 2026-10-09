//! API-Schlüssel liegen im System-Schlüsselbund (macOS Keychain, Windows
//! Anmeldeinformationsverwaltung, Linux Secret Service) – nie in der Config.
//! Für Server ohne Schlüsselbund gibt es Umgebungsvariablen als Fallback.

use crate::config::Upstream;

const SERVICE: &str = "ai.it-guy.llmbridge";

/// Standard-Umgebungsvariable für einen Upstream, z. B. `LLM_BRIDGE_KEY_LANGDOCK_EU`.
pub fn env_var_name(upstream_id: &str) -> String {
    let id: String =
        upstream_id.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect();
    format!("LLM_BRIDGE_KEY_{id}")
}

/// Woher ein Schlüssel stammt – nur für die Anzeige.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeySource {
    Env,
    Keyring,
    None,
}

fn from_env(up: &Upstream) -> Option<String> {
    let candidates = [up.api_key_env.trim().to_string(), env_var_name(&up.id)];
    candidates.iter().filter(|n| !n.is_empty()).find_map(|n| std::env::var(n).ok().filter(|v| !v.trim().is_empty()))
}

fn entry(upstream_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, upstream_id).map_err(|e| e.to_string())
}

/// Liefert den Schlüssel (Umgebung vor Schlüsselbund).
pub fn get_api_key(up: &Upstream) -> Result<Option<String>, String> {
    if let Some(k) = from_env(up) {
        return Ok(Some(k));
    }
    match entry(&up.id)?.get_password() {
        Ok(k) if !k.trim().is_empty() => Ok(Some(k)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Schlüsselbund: {e}")),
    }
}

pub fn key_source(up: &Upstream) -> KeySource {
    if from_env(up).is_some() {
        return KeySource::Env;
    }
    match entry(&up.id).and_then(|e| e.get_password().map_err(|e| e.to_string())) {
        Ok(k) if !k.trim().is_empty() => KeySource::Keyring,
        _ => KeySource::None,
    }
}

pub fn set_api_key(upstream_id: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return delete_api_key(upstream_id);
    }
    entry(upstream_id)?.set_password(key).map_err(|e| format!("Schlüsselbund: {e}"))
}

pub fn delete_api_key(upstream_id: &str) -> Result<(), String> {
    match entry(upstream_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Schlüsselbund: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Schreibt in den echten Schlüsselbund – nur manuell: `cargo test -p llm-bridge-core -- --ignored`
    #[test]
    #[ignore]
    fn keyring_roundtrip() {
        let id = "llm-bridge-selftest";
        let up = Upstream { id: id.into(), ..Upstream::default() };
        set_api_key(id, "test-wert").unwrap();
        assert_eq!(get_api_key(&up).unwrap().as_deref(), Some("test-wert"));
        assert_eq!(key_source(&up), KeySource::Keyring);
        delete_api_key(id).unwrap();
        assert_eq!(get_api_key(&up).unwrap(), None);
        delete_api_key(id).unwrap(); // zweimal löschen ist kein Fehler
    }

    #[test]
    fn env_names() {
        assert_eq!(env_var_name("langdock-eu"), "LLM_BRIDGE_KEY_LANGDOCK_EU");
    }
}
