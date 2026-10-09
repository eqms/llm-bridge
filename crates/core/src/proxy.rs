//! Weiterleitung an den Anthropic-kompatiblen Upstream inkl. SSE-Übersetzung.

use crate::config::{AuthMode, Config, Upstream};
use crate::error::{upstream_message, ProxyError};
use crate::secrets;
use crate::stats::{now_ms, RequestRecord, Stats};
use crate::translate::{self, Usage};
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub struct ProxyState {
    config: RwLock<Config>,
    client: reqwest::Client,
    pub stats: Stats,
    keys: Mutex<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub status: u16,
    pub latency_ms: u64,
    pub message: String,
}

impl ProxyState {
    pub fn new(config: Config, stats: Stats) -> Arc<Self> {
        // reqwest ist ohne eigenen Krypto-Provider gebaut; schon installiert ist kein Fehler.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(180))
            .user_agent(concat!("llm-bridge/", env!("CARGO_PKG_VERSION")))
            // Keine Weiterleitungen: reqwest würde `x-api-key` an das Ziel mitschicken.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("HTTP-Client");
        Arc::new(Self { config: RwLock::new(config), client, stats, keys: Mutex::new(HashMap::new()) })
    }

    pub fn config(&self) -> Config {
        self.config.read().map(|c| c.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    pub fn set_config(&self, cfg: Config) {
        match self.config.write() {
            Ok(mut c) => *c = cfg,
            Err(p) => *p.into_inner() = cfg,
        }
    }

    /// Nach Änderung eines Schlüssels aufrufen.
    pub fn invalidate_key(&self, upstream_id: &str) {
        if let Ok(mut k) = self.keys.lock() {
            k.remove(upstream_id);
        }
    }

    /// Schlüssel aus Cache, sonst aus Umgebung/Schlüsselbund (blockierend → eigener Thread).
    async fn api_key(&self, up: &Upstream) -> Result<String, ProxyError> {
        if let Some(k) = self.keys.lock().ok().and_then(|k| k.get(&up.id).cloned()) {
            return Ok(k);
        }
        let up2 = up.clone();
        let key = tokio::task::spawn_blocking(move || secrets::get_api_key(&up2))
            .await
            .map_err(|e| ProxyError::Internal(e.to_string()))?
            .map_err(ProxyError::Internal)?
            .ok_or_else(|| {
                ProxyError::MissingApiKey(format!(
                    "Für Upstream '{}' ist kein API-Schlüssel hinterlegt (GUI, `llm-bridge set-key {}` oder {})",
                    up.id,
                    up.id,
                    secrets::env_var_name(&up.id)
                ))
            })?;
        if let Ok(mut k) = self.keys.lock() {
            k.insert(up.id.clone(), key.clone());
        }
        Ok(key)
    }

    fn request(&self, method: reqwest::Method, url: String, up: &Upstream, key: &str) -> reqwest::RequestBuilder {
        let rb = self
            .client
            .request(method, url)
            .header("anthropic-version", up.anthropic_version.as_str())
            .header(header::ACCEPT, "application/json, text/event-stream");
        match up.auth {
            AuthMode::Bearer => rb.bearer_auth(key),
            AuthMode::XApiKey => match reqwest::header::HeaderValue::from_str(key) {
                Ok(mut v) => {
                    v.set_sensitive(true); // nie in Debug-/Trace-Logs
                    rb.header("x-api-key", v)
                }
                // Ungültige Zeichen: reqwest meldet den Fehler beim Senden.
                Err(_) => rb.header("x-api-key", key),
            },
        }
    }

    /// Haupteinstieg für `/v1/chat/completions`.
    pub async fn chat(self: &Arc<Self>, body: Value, client_name: String) -> Result<Response, ProxyError> {
        let cfg = self.config();
        let requested = body.get("model").and_then(Value::as_str).unwrap_or("").to_string();
        let (up, model, max_override) = cfg.resolve(&requested).ok_or_else(|| {
            let known: Vec<_> = cfg.models.iter().map(|m| m.alias.as_str()).collect();
            ProxyError::ModelNotFound(format!(
                "Modell '{requested}' ist nicht zugeordnet. Verfügbar: {}",
                if known.is_empty() { "–".into() } else { known.join(", ") }
            ))
        })?;
        let translated =
            translate::openai_to_anthropic(&body, &model, max_override.unwrap_or(cfg.defaults.max_tokens))?;
        let key = self.api_key(&up).await?;

        let mut recorder = Recorder::new(
            self.stats.clone(),
            RequestRecord {
                ts: now_ms(),
                client: client_name,
                alias: if requested.is_empty() { model.clone() } else { requested.clone() },
                upstream: up.id.clone(),
                model: model.clone(),
                stream: translated.stream,
                ..Default::default()
            },
        );

        let mut rb = self.request(reqwest::Method::POST, up.messages_url(), &up, &key).json(&translated.body);
        if !translated.stream {
            rb = rb.timeout(Duration::from_secs(cfg.defaults.request_timeout_secs.max(10)));
        }
        let resp = match rb.send().await {
            Ok(r) => r,
            Err(e) => {
                let err = ProxyError::Connect(e.to_string());
                recorder.finish(502, Some(err.to_string()));
                return Err(err);
            }
        };

        let status = resp.status();
        if let Some(message) = redirect_message(&resp) {
            let err = ProxyError::Upstream { status: 502, message };
            recorder.finish(502, Some(err.to_string()));
            return Err(err);
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let mut message = upstream_message(&text);
            if status == StatusCode::NOT_FOUND {
                message = format!("{message} – Modell-ID '{model}' oder Basis-URL prüfen ({})", up.messages_url());
            }
            if status.as_u16() == 401 || status.as_u16() == 403 {
                self.invalidate_key(&up.id);
            }
            let err = ProxyError::Upstream { status: status.as_u16(), message };
            recorder.finish(status.as_u16(), Some(err.to_string()));
            return Err(err);
        }

        let is_sse = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if translated.stream {
            let client_model = recorder.rec.alias.clone();
            let stream = if is_sse {
                sse_stream(resp, client_model, translated.include_usage, recorder).boxed()
            } else {
                // Upstream hat trotz stream=true JSON geliefert → als Stream nachbilden
                let v: Value =
                    resp.json().await.map_err(|e| ProxyError::Upstream { status: 502, message: e.to_string() })?;
                json_as_stream(v, client_model, translated.include_usage, recorder).boxed()
            };
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .header("x-accel-buffering", "no")
                .body(Body::from_stream(stream))
                .unwrap());
        }

        let v: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                let err = ProxyError::Upstream { status: 502, message: format!("Ungültige Antwort: {e}") };
                recorder.finish(502, Some(err.to_string()));
                return Err(err);
            }
        };
        let (out, usage) = translate::anthropic_to_openai(&v, &recorder.rec.alias.clone());
        recorder.usage = usage;
        recorder.finish(200, None);
        Ok((StatusCode::OK, axum::Json(out)).into_response())
    }

    /// Minimaler Test-Request (1 Token) über eine Modell-Zuordnung.
    pub async fn test_route(self: &Arc<Self>, model: &str) -> TestResult {
        let start = Instant::now();
        let body = json!({
            "model": model,
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 1
        });
        let res = self.chat(body, "LLM Bridge (Test)".into()).await;
        let latency_ms = start.elapsed().as_millis() as u64;
        match res {
            Ok(r) => TestResult { ok: true, status: r.status().as_u16(), latency_ms, message: "OK".into() },
            Err(e) => TestResult { ok: false, status: e.status().as_u16(), latency_ms, message: e.to_string() },
        }
    }

    /// Fragt `GET /v1/models` beim Upstream ab.
    pub async fn upstream_models(self: &Arc<Self>, upstream_id: &str) -> Result<Vec<String>, String> {
        let cfg = self.config();
        let up = cfg.upstream(upstream_id).ok_or_else(|| format!("Unbekannter Upstream '{upstream_id}'"))?;
        let key = self.api_key(up).await.map_err(|e| e.to_string())?;
        let resp = self
            .request(reqwest::Method::GET, up.models_url(), up, &key)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(message) = redirect_message(&resp) {
            return Err(message);
        }
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("{} – {}", status.as_u16(), upstream_message(&text)));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let list = v.get("data").and_then(Value::as_array).or_else(|| v.as_array()).cloned().unwrap_or_default();
        let mut ids: Vec<String> = list
            .iter()
            .filter_map(|m| m.get("id").or_else(|| m.get("name")).and_then(Value::as_str).map(String::from))
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

/// Schreibt genau einen Statistik-Eintrag – auch wenn der Client die Verbindung abbricht.
struct Recorder {
    stats: Stats,
    rec: RequestRecord,
    usage: Usage,
    start: Instant,
    done: bool,
}

impl Recorder {
    fn new(stats: Stats, rec: RequestRecord) -> Self {
        Self { stats, rec, usage: Usage::default(), start: Instant::now(), done: false }
    }

    fn finish(&mut self, status: u16, error: Option<String>) {
        if self.done {
            return;
        }
        self.done = true;
        let mut rec = std::mem::take(&mut self.rec);
        rec.status = status;
        rec.error = error;
        rec.duration_ms = self.start.elapsed().as_millis() as u64;
        rec.input_tokens = self.usage.input_tokens;
        rec.output_tokens = self.usage.output_tokens;
        rec.cache_read_tokens = self.usage.cache_read_tokens;
        rec.cache_write_tokens = self.usage.cache_write_tokens;
        self.stats.record(rec);
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if !self.done {
            self.finish(499, Some("Verbindung vom Client abgebrochen".into()));
        }
    }
}

fn sse(v: &Value) -> Result<Bytes, Infallible> {
    Ok(Bytes::from(format!("data: {v}\n\n")))
}

const DONE: &[u8] = b"data: [DONE]\n\n";

/// Hält den Zustand während der Übersetzung eines Anthropic-Streams.
struct StreamState {
    id: String,
    created: u64,
    model: String,
    /// Anthropic-Block-Index → OpenAI-Tool-Call-Index
    tool_index: HashMap<u64, usize>,
    next_tool: usize,
    stop_reason: Option<String>,
}

impl StreamState {
    fn new(model: String) -> Self {
        Self {
            id: translate::completion_id(None),
            created: translate::now_secs(),
            model,
            tool_index: HashMap::new(),
            next_tool: 0,
            stop_reason: None,
        }
    }

    fn chunk(&self, delta: Value, finish: Option<&str>) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }]
        })
    }

    fn usage_chunk(&self, usage: Usage) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [],
            "usage": usage.to_openai()
        })
    }

    /// Verarbeitet ein Anthropic-Event und liefert die zugehörigen OpenAI-Chunks.
    fn handle(&mut self, data: &Value, usage: &mut Usage, error: &mut Option<String>) -> Vec<Value> {
        let ty = data.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "message_start" => {
                if let Some(id) = data.pointer("/message/id").and_then(Value::as_str) {
                    self.id = translate::completion_id(Some(id));
                }
                if let Some(u) = data.pointer("/message/usage") {
                    usage.merge_max(Usage::from_anthropic(u));
                }
                vec![self.chunk(json!({ "role": "assistant", "content": "" }), None)]
            }
            "content_block_start" => {
                let idx = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                let block = data.get("content_block").cloned().unwrap_or(Value::Null);
                match block.get("type").and_then(Value::as_str) {
                    Some("tool_use") => {
                        let ti = self.next_tool;
                        self.next_tool += 1;
                        self.tool_index.insert(idx, ti);
                        vec![self.chunk(
                            json!({ "tool_calls": [{
                                "index": ti,
                                "id": block.get("id").cloned().unwrap_or(json!("call_0")),
                                "type": "function",
                                "function": { "name": block.get("name").cloned().unwrap_or(json!("tool")), "arguments": "" }
                            }]}),
                            None,
                        )]
                    }
                    Some("text") => match block.get("text").and_then(Value::as_str) {
                        Some(t) if !t.is_empty() => vec![self.chunk(json!({ "content": t }), None)],
                        _ => vec![],
                    },
                    _ => vec![],
                }
            }
            "content_block_delta" => {
                let idx = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                let delta = data.get("delta").cloned().unwrap_or(Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let t = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        if t.is_empty() {
                            vec![]
                        } else {
                            vec![self.chunk(json!({ "content": t }), None)]
                        }
                    }
                    Some("input_json_delta") => {
                        let ti = self.tool_index.get(&idx).copied().unwrap_or(0);
                        let part = delta.get("partial_json").and_then(Value::as_str).unwrap_or("");
                        vec![self
                            .chunk(json!({ "tool_calls": [{ "index": ti, "function": { "arguments": part } }] }), None)]
                    }
                    _ => vec![], // thinking_delta, signature_delta …
                }
            }
            "message_delta" => {
                if let Some(r) = data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(r.to_string());
                }
                if let Some(u) = data.get("usage") {
                    usage.merge_max(Usage::from_anthropic(u));
                }
                vec![]
            }
            "message_stop" => {
                vec![self.chunk(json!({}), Some(translate::map_stop_reason(self.stop_reason.as_deref())))]
            }
            "error" => {
                let msg = data
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Unbekannter Stream-Fehler")
                    .to_string();
                *error = Some(msg);
                vec![]
            }
            _ => vec![], // ping
        }
    }
}

fn error_chunk(msg: &str) -> Value {
    json!({ "error": { "message": msg, "type": "upstream_error" } })
}

fn sse_stream(
    resp: reqwest::Response,
    client_model: String,
    include_usage: bool,
    mut recorder: Recorder,
) -> impl futures_util::Stream<Item = Result<Bytes, Infallible>> + Send {
    async_stream::stream! {
        let mut events = resp.bytes_stream().eventsource();
        let mut st = StreamState::new(client_model);
        let mut error: Option<String> = None;
        let mut finished = false;
        while let Some(ev) = events.next().await {
            let ev = match ev {
                Ok(ev) => ev,
                Err(e) => { error = Some(format!("Stream unterbrochen: {e}")); break; }
            };
            let data: Value = match serde_json::from_str(&ev.data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let ty = data.get("type").and_then(Value::as_str).unwrap_or("").to_string();
            let mut usage = recorder.usage;
            let chunks = st.handle(&data, &mut usage, &mut error);
            recorder.usage = usage;
            for c in &chunks {
                yield sse(c);
            }
            if ty == "message_stop" { finished = true; break; }
            if ty == "error" { break; }
        }
        if !finished && error.is_none() {
            error = Some("Stream vorzeitig beendet".into());
        }
        if let Some(e) = &error {
            yield sse(&error_chunk(e));
        }
        if include_usage {
            yield sse(&st.usage_chunk(recorder.usage));
        }
        yield Ok(Bytes::from_static(DONE));
        let status = if error.is_some() { 502 } else { 200 };
        recorder.finish(status, error);
    }
}

fn json_as_stream(
    v: Value,
    client_model: String,
    include_usage: bool,
    mut recorder: Recorder,
) -> impl futures_util::Stream<Item = Result<Bytes, Infallible>> + Send {
    async_stream::stream! {
        let (out, usage) = translate::anthropic_to_openai(&v, &client_model);
        recorder.usage = usage;
        let st = StreamState::new(client_model);
        let msg = &out["choices"][0]["message"];
        yield sse(&st.chunk(json!({ "role": "assistant", "content": msg["content"] }), None));
        if let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) {
            let calls: Vec<Value> = calls.iter().enumerate().map(|(i, c)| {
                let mut c = c.clone();
                c["index"] = json!(i);
                c
            }).collect();
            yield sse(&st.chunk(json!({ "tool_calls": calls }), None));
        }
        let finish = out["choices"][0]["finish_reason"].as_str().unwrap_or("stop").to_string();
        yield sse(&st.chunk(json!({}), Some(&finish)));
        if include_usage {
            yield sse(&st.usage_chunk(usage));
        }
        yield Ok(Bytes::from_static(DONE));
        recorder.finish(200, None);
    }
}

/// Weiterleitungen werden nicht verfolgt; der Nutzer soll stattdessen die Basis-URL korrigieren.
fn redirect_message(resp: &reqwest::Response) -> Option<String> {
    if !resp.status().is_redirection() {
        return None;
    }
    let target = resp.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("?");
    Some(format!("Upstream leitet weiter ({} → {target}) – Basis-URL anpassen", resp.status().as_u16()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_are_reported_not_followed() {
        let resp = |status: u16| {
            let r = axum::http::Response::builder().status(status).header(header::LOCATION, "https://evil.example/");
            reqwest::Response::from(r.body("").unwrap())
        };
        let msg = redirect_message(&resp(307)).unwrap();
        assert!(msg.contains("307") && msg.contains("https://evil.example/"), "{msg}");
        assert!(redirect_message(&resp(200)).is_none());
        assert!(redirect_message(&resp(404)).is_none());
    }

    #[test]
    fn stream_state_text_and_tools() {
        let mut st = StreamState::new("haiku".into());
        let mut u = Usage::default();
        let mut err = None;
        let events = [
            json!({"type":"message_start","message":{"id":"msg_9","usage":{"input_tokens":12,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hal"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu1","name":"f"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"a\":"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}),
            json!({"type":"message_stop"}),
        ];
        let chunks: Vec<Value> = events.iter().flat_map(|e| st.handle(e, &mut u, &mut err)).collect();
        assert_eq!(chunks[0]["id"], "chatcmpl-msg_9");
        assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(chunks[1]["choices"][0]["delta"]["content"], "Hal");
        assert_eq!(chunks[3]["choices"][0]["delta"]["tool_calls"][0]["id"], "tu1");
        assert_eq!(chunks[4]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"], "{\"a\":");
        assert_eq!(chunks.last().unwrap()["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(u.input_tokens, 12);
        assert_eq!(u.output_tokens, 9);
        assert!(err.is_none());
    }
}
