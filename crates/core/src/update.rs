//! Update-Quelle (GitHub-Releases) und Signaturprüfung für die CLI.
//!
//! Die App nutzt das Tauri-Update-Plugin mit demselben Schlüssel und Manifest;
//! die CLI lädt ihre eigene Programmdatei und prüft sie hier.

use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use std::time::Duration;

/// Manifest des zuletzt veröffentlichten Releases (Entwürfe sind dort nicht sichtbar).
pub const MANIFEST_URL: &str = "https://github.com/eqms/llm-bridge/releases/latest/download/latest.json";
const DOWNLOAD_BASE: &str = "https://github.com/eqms/llm-bridge/releases/download";

/// Öffentlicher minisign-Schlüssel, Base64 wie in `tauri.conf.json` → `plugins.updater.pubkey`.
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEU4RTFFNDg0RTRBMjc5NkIKUldScmVhTGtoT1RoNkVYemJCUTVud1dhQ1ViMWp3bGVTVE4rYUJlTk8vRFYwSmxpVU12QTkyU3AK";

/// Zielplattform dieses Builds, z. B. `aarch64-apple-darwin`.
pub const TARGET: &str = env!("LLM_BRIDGE_TARGET");

pub fn current_version() -> Version {
    Version::parse(crate::VERSION).expect("CARGO_PKG_VERSION ist SemVer")
}

/// Name der CLI-Programmdatei im Release, z. B. `llm-bridge-aarch64-apple-darwin`.
pub fn cli_asset() -> String {
    format!("llm-bridge-{TARGET}{}", std::env::consts::EXE_SUFFIX)
}

fn base64_text(s: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| format!("Signatur ist kein Base64: {e}"))?;
    String::from_utf8(bytes).map_err(|_| "Signatur ist kein UTF-8".into())
}

/// Prüft `data` gegen die Base64-codierte minisign-Signatur und verlangt, dass die
/// Signatur für genau `version` erstellt wurde. Sonst ließe sich ein manipuliertes
/// Manifest mit einer älteren, korrekt signierten Datei kombinieren.
pub fn verify(data: &[u8], signature_b64: &str, version: &Version) -> Result<(), String> {
    let key = PublicKey::decode(&base64_text(PUBLIC_KEY)?).map_err(|e| format!("Öffentlicher Schlüssel: {e}"))?;
    let sig = Signature::decode(&base64_text(signature_b64)?).map_err(|e| format!("Signatur unlesbar: {e}"))?;
    key.verify(data, &sig, false).map_err(|e| format!("Signatur ungültig: {e}"))?;
    let signed = sig
        .trusted_comment()
        .split('\t')
        .find_map(|f| f.strip_prefix("version:"))
        .ok_or("Signatur nennt keine Version")?;
    match Version::parse(signed.trim_start_matches('v')) {
        Ok(v) if &v == version => Ok(()),
        _ => Err(format!("Signatur gilt für Version {signed}, erwartet {version}")),
    }
}

fn client() -> Result<reqwest::Client, String> {
    // reqwest ist ohne eigenen Krypto-Provider gebaut; schon installiert ist kein Fehler.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Weiterleitungen sind hier nötig (GitHub leitet Downloads um) und unkritisch: es geht kein Schlüssel mit.
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .user_agent(concat!("llm-bridge/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

async fn get(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes, String> {
    let resp = client.get(url).send().await.map_err(|e| format!("{url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status().as_u16()));
    }
    resp.bytes().await.map_err(|e| format!("{url}: {e}"))
}

/// Version des zuletzt veröffentlichten Releases.
pub async fn latest_version() -> Result<Version, String> {
    let body = get(&client()?, MANIFEST_URL).await.map_err(|e| {
        if e.ends_with("HTTP 404") {
            "Noch kein veröffentlichtes Release mit Update-Manifest gefunden".to_string()
        } else {
            e
        }
    })?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("Update-Manifest unlesbar: {e}"))?;
    let v = manifest["version"].as_str().ok_or("Update-Manifest ohne Version")?;
    Version::parse(v.trim_start_matches('v')).map_err(|e| format!("Ungültige Version '{v}': {e}"))
}

/// Lädt die CLI-Programmdatei für `version` und gibt sie nur zurück, wenn die Signatur stimmt.
pub async fn download_cli(version: &Version) -> Result<Vec<u8>, String> {
    let client = client()?;
    let url = format!("{DOWNLOAD_BASE}/v{version}/{}", cli_asset());
    let sig = get(&client, &format!("{url}.sig")).await?;
    let data = get(&client, &url).await?;
    verify(&data, &String::from_utf8_lossy(&sig), version)?;
    Ok(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mit `cargo tauri signer sign --app-version 0.0.1` und dem Release-Schlüssel signiert.
    const FIXTURE: &[u8] = b"llm-bridge update fixture\n";
    const FIXTURE_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVScmVhTGtoT1RoNkJUaTJVQkJLOGJyaWF2Umlxc2xSRGlNTEFYVlp2dWlQYVBEVmg1VnNYY1lEVUJuU0tvVm50QTBDREZjSTZDWUhxai9JcWJocTlJaFBRQ0JwaGd3RWcwPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNzMxNjAzCWZpbGU6Zml4dHVyZS5iaW4JdmVyc2lvbjowLjAuMQpwTzJyRWxyeFB0d2pTTXdYNk1pb2JWNUdYbENXYlhPVmtuOGtHK3l4NG01OXNWdjhPSVk3Y2Y5QXpoall5eUlkZ3VjNkxWYUkzNEFPK05QVGM2YUVCZz09Cg==";

    #[test]
    fn accepts_matching_signature_and_version() {
        verify(FIXTURE, FIXTURE_SIG, &Version::new(0, 0, 1)).unwrap();
    }

    #[test]
    fn rejects_tampered_data_and_wrong_version() {
        assert!(verify(b"manipuliert\n", FIXTURE_SIG, &Version::new(0, 0, 1)).is_err());
        let err = verify(FIXTURE, FIXTURE_SIG, &Version::new(9, 9, 9)).unwrap_err();
        assert!(err.contains("0.0.1"), "{err}");
        assert!(verify(FIXTURE, "kein base64!", &Version::new(0, 0, 1)).is_err());
    }

    #[test]
    fn asset_name_matches_release_layout() {
        assert!(cli_asset().starts_with("llm-bridge-"));
        assert!(cli_asset().contains(TARGET));
    }
}
