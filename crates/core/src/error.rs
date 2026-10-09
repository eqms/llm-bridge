use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Fehler, die der Proxy an den Client zurückgibt – immer im OpenAI-Fehlerformat,
/// damit Clients wie MacWhisper die Meldung sauber anzeigen.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    /// Anfrage von fremder Herkunft (Browser-Origin oder Host-Header) – Schutz vor CSRF und DNS-Rebinding.
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    UnsupportedMediaType(String),
    #[error("{0}")]
    ModelNotFound(String),
    #[error("{0}")]
    MissingApiKey(String),
    #[error("Upstream-Fehler ({status}): {message}")]
    Upstream { status: u16, message: String },
    #[error("Upstream nicht erreichbar: {0}")]
    Connect(String),
    #[error("{0}")]
    Internal(String),
}

impl ProxyError {
    pub fn status(&self) -> StatusCode {
        match self {
            ProxyError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ProxyError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ProxyError::Forbidden(_) => StatusCode::FORBIDDEN,
            ProxyError::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ProxyError::ModelNotFound(_) => StatusCode::NOT_FOUND,
            ProxyError::MissingApiKey(_) => StatusCode::UNAUTHORIZED,
            ProxyError::Upstream { status, .. } => StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY),
            ProxyError::Connect(_) => StatusCode::BAD_GATEWAY,
            ProxyError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            ProxyError::BadRequest(_) | ProxyError::UnsupportedMediaType(_) => "invalid_request_error",
            ProxyError::Unauthorized(_) | ProxyError::MissingApiKey(_) => "authentication_error",
            ProxyError::Forbidden(_) => "permission_error",
            ProxyError::ModelNotFound(_) => "model_not_found",
            ProxyError::Upstream { .. } => "upstream_error",
            ProxyError::Connect(_) => "upstream_unreachable",
            ProxyError::Internal(_) => "internal_error",
        }
    }

    pub fn to_openai_json(&self) -> serde_json::Value {
        json!({
            "error": {
                "message": self.to_string(),
                "type": self.kind(),
                "code": self.status().as_u16(),
            }
        })
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        (self.status(), Json(self.to_openai_json())).into_response()
    }
}

/// Extrahiert eine lesbare Fehlermeldung aus einer Upstream-Antwort.
/// Versteht Anthropic (`{"error":{"message":..}}`), LangDock (`{"message":..}`) und Rohtext.
pub fn upstream_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(m) = v.pointer("/error/message").and_then(|m| m.as_str()) {
            return m.to_string();
        }
        if let Some(m) = v.get("message").and_then(|m| m.as_str()) {
            return m.to_string();
        }
        if let Some(m) = v.get("error").and_then(|m| m.as_str()) {
            return m.to_string();
        }
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        "leere Antwort".into()
    } else {
        trimmed.chars().take(500).collect()
    }
}
