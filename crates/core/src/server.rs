//! Lokaler HTTP-Server (OpenAI-kompatibel).

use crate::error::ProxyError;
use crate::proxy::ProxyState;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub struct ProxyHandle {
    pub addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    join: JoinHandle<()>,
}

impl ProxyHandle {
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.join).await;
        self.join.abort();
    }

    pub fn is_finished(&self) -> bool {
        self.join.is_finished()
    }
}

pub fn router(state: Arc<ProxyState>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/models", get(models))
        .route("/v1/chat/completions", post(chat))
        .route("/chat/completions", post(chat))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // Bilder als Base64
        .layer(middleware::from_fn_with_state(state.clone(), origin_guard))
        .with_state(state)
}

/// Startet den Server im Hintergrund. Schlägt fehl, wenn der Port belegt ist.
pub async fn start(state: Arc<ProxyState>) -> Result<ProxyHandle, String> {
    let cfg = state.config();
    let bind = format!("{}:{}", cfg.server.host, cfg.server.port);
    let listener = tokio::net::TcpListener::bind(&bind).await.map_err(|e| format!("{bind}: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let (tx, rx) = oneshot::channel::<()>();
    let app = router(state);
    let join = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = rx.await;
        });
        if let Err(e) = server.await {
            tracing::error!("Proxy beendet: {e}");
        }
    });
    tracing::info!("LLM Bridge lauscht auf http://{addr}/v1");
    Ok(ProxyHandle { addr, shutdown: Some(tx), join })
}

/// Weist Anfragen ab, die eine Webseite im Browser ausgelöst hat. Ohne diese Prüfung
/// könnte jede Seite per `fetch` (CSRF) oder DNS-Rebinding den API-Schlüssel des Nutzers verbrauchen.
async fn origin_guard(State(state): State<Arc<ProxyState>>, req: Request, next: Next) -> Response {
    if let Err(e) = check_origin(&state.config().server.host, req.headers()) {
        tracing::warn!("{e}");
        return e.into_response();
    }
    next.run(req).await
}

/// Ein gesetzter `Origin` muss lokal sein. Lauscht der Proxy nur auf Loopback, muss auch
/// der `Host`-Header lokal sein – ein fremder Name dort bedeutet DNS-Rebinding.
fn check_origin(bind_host: &str, headers: &HeaderMap) -> Result<(), ProxyError> {
    if let Some(origin) = headers.get(header::ORIGIN) {
        let local = origin
            .to_str()
            .ok()
            .and_then(|o| o.split_once("://"))
            .is_some_and(|(_, authority)| is_loopback_name(host_part(authority)));
        if !local {
            return Err(ProxyError::Forbidden(format!(
                "Anfragen von Webseiten sind nicht erlaubt (Origin: {})",
                origin.to_str().unwrap_or("?")
            )));
        }
    }
    if is_loopback_name(bind_host) {
        if let Some(host) = headers.get(header::HOST) {
            if !host.to_str().ok().is_some_and(|h| is_loopback_name(host_part(h))) {
                return Err(ProxyError::Forbidden(format!(
                    "Unerwarteter Host-Header: {}",
                    host.to_str().unwrap_or("?")
                )));
            }
        }
    }
    Ok(())
}

/// Hostname ohne Port aus `host:port` oder `[v6]:port`.
fn host_part(authority: &str) -> &str {
    match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    }
}

fn is_loopback_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn check_auth(state: &ProxyState, headers: &HeaderMap) -> Result<(), ProxyError> {
    let token = state.config().server.access_token;
    if token.is_empty() {
        return Ok(());
    }
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .or_else(|| headers.get("x-api-key").and_then(|v| v.to_str().ok()));
    if given == Some(token.as_str()) {
        Ok(())
    } else {
        Err(ProxyError::Unauthorized("Ungültiges lokales Zugriffstoken".into()))
    }
}

/// Kurzer Client-Name aus `X-Client-Name` oder dem User-Agent (`MacWhisper/12 CFNetwork/…` → `MacWhisper`).
pub fn client_name(headers: &HeaderMap) -> String {
    if let Some(n) = headers.get("x-client-name").and_then(|v| v.to_str().ok()) {
        let n = n.trim();
        if !n.is_empty() {
            return n.chars().take(60).collect();
        }
    }
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").trim();
    let first = ua.split(|c: char| c == '/' || c.is_whitespace()).next().unwrap_or("");
    if first.is_empty() {
        "unbekannt".into()
    } else {
        first.chars().take(60).collect()
    }
}

async fn root() -> impl IntoResponse {
    Json(json!({
        "name": "LLM Bridge",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": ["/v1/chat/completions", "/v1/models", "/health"]
    }))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn models(State(state): State<Arc<ProxyState>>, headers: HeaderMap) -> Response {
    if let Err(e) = check_auth(&state, &headers) {
        return e.into_response();
    }
    let cfg = state.config();
    let data: Vec<Value> = cfg
        .models
        .iter()
        .map(|m| json!({ "id": m.alias, "object": "model", "created": 0, "owned_by": m.upstream }))
        .collect();
    Json(json!({ "object": "list", "data": data })).into_response()
}

async fn chat(State(state): State<Arc<ProxyState>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    if let Err(e) = check_auth(&state, &headers) {
        return e.into_response();
    }
    // Nur JSON annehmen: `text/plain` ginge im Browser ohne CORS-Preflight durch.
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"));
    if !is_json {
        return ProxyError::UnsupportedMediaType("Content-Type muss application/json sein".into()).into_response();
    }
    let body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return ProxyError::BadRequest(format!("Ungültiges JSON: {e}")).into_response(),
    };
    match state.chat(body, client_name(&headers)).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("{e}");
            e.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::stats::Stats;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(k.clone(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn origin_and_host_rules() {
        let ok = |bind: &str, h: &[(header::HeaderName, &str)]| check_origin(bind, &headers(h)).is_ok();
        // Clients ohne Browser schicken kein Origin.
        assert!(ok("127.0.0.1", &[(header::HOST, "127.0.0.1:4000")]));
        assert!(ok("127.0.0.1", &[(header::HOST, "localhost:4000")]));
        assert!(ok("127.0.0.1", &[(header::HOST, "[::1]:4000")]));
        assert!(ok("127.0.0.1", &[]));
        // DNS-Rebinding: fremder Name im Host-Header.
        assert!(!ok("127.0.0.1", &[(header::HOST, "evil.example:4000")]));
        assert!(!ok("localhost", &[(header::HOST, "localhost.evil.example:4000")]));
        // Bei Bindung an alle Interfaces ist der Host-Header beliebig (LAN-Name, IP).
        assert!(ok("0.0.0.0", &[(header::HOST, "macbook.lan:4000")]));
        // Browser-Origin: nur lokal.
        assert!(ok("127.0.0.1", &[(header::ORIGIN, "http://localhost:3000")]));
        assert!(!ok("127.0.0.1", &[(header::ORIGIN, "https://evil.example")]));
        assert!(!ok("0.0.0.0", &[(header::ORIGIN, "https://evil.example")]));
        assert!(!ok("127.0.0.1", &[(header::ORIGIN, "null")]));
    }

    #[tokio::test]
    async fn browser_requests_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.server.port = 0;
        let state = ProxyState::new(cfg, Stats::open(&dir.path().join("s.sqlite")).unwrap());
        let handle = start(state).await.unwrap();
        let url = format!("http://{}/v1/chat/completions", handle.addr);
        let body = r#"{"model":"gibt-es-nicht","messages":[]}"#;
        let client = reqwest::Client::new();
        let status = |rb: reqwest::RequestBuilder| async move { rb.send().await.unwrap().status().as_u16() };

        let json = || client.post(&url).header(header::CONTENT_TYPE, "application/json").body(body);
        assert_eq!(status(json().header(header::ORIGIN, "https://evil.example")).await, 403);
        assert_eq!(status(json().header(header::HOST, "evil.example")).await, 403);
        assert_eq!(status(client.post(&url).header(header::CONTENT_TYPE, "text/plain").body(body)).await, 415);
        assert_eq!(status(client.post(&url).body(body)).await, 415);
        // Regulärer Client kommt bis zur Modellauflösung durch.
        assert_eq!(status(json()).await, 404);

        handle.stop().await;
    }
}
