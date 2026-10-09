//! Übersetzung OpenAI Chat Completions ⇄ Anthropic Messages.

use crate::error::ProxyError;
use serde_json::{json, Map, Value};

/// Ergebnis der Request-Übersetzung.
#[derive(Debug, Clone)]
pub struct Translated {
    pub body: Value,
    pub stream: bool,
    /// `stream_options.include_usage` – Client möchte am Ende einen Usage-Chunk.
    pub include_usage: bool,
}

/// Token-Verbrauch im Anthropic-Sinn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl Usage {
    pub fn from_anthropic(u: &Value) -> Self {
        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
        Self {
            input_tokens: n("input_tokens"),
            output_tokens: n("output_tokens"),
            cache_read_tokens: n("cache_read_input_tokens"),
            cache_write_tokens: n("cache_creation_input_tokens"),
        }
    }

    /// Übernimmt jeweils den größeren Wert (Anthropic liefert kumulative Zähler).
    pub fn merge_max(&mut self, other: Usage) {
        self.input_tokens = self.input_tokens.max(other.input_tokens);
        self.output_tokens = self.output_tokens.max(other.output_tokens);
        self.cache_read_tokens = self.cache_read_tokens.max(other.cache_read_tokens);
        self.cache_write_tokens = self.cache_write_tokens.max(other.cache_write_tokens);
    }

    /// OpenAI zählt Cache-Tokens zu den Prompt-Tokens.
    pub fn to_openai(self) -> Value {
        let prompt = self.input_tokens + self.cache_read_tokens + self.cache_write_tokens;
        json!({
            "prompt_tokens": prompt,
            "completion_tokens": self.output_tokens,
            "total_tokens": prompt + self.output_tokens,
            "prompt_tokens_details": { "cached_tokens": self.cache_read_tokens }
        })
    }
}

fn text_block(s: &str) -> Option<Value> {
    if s.is_empty() {
        None
    } else {
        Some(json!({ "type": "text", "text": s }))
    }
}

fn image_block(url: &str) -> Value {
    // data:image/png;base64,AAAA…
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((meta, data)) = rest.split_once(',') {
            let media_type = meta.split(';').next().unwrap_or("image/png");
            return json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media_type, "data": data }
            });
        }
    }
    json!({ "type": "image", "source": { "type": "url", "url": url } })
}

/// Text aus OpenAI-Content (String oder Teile-Array) zusammenziehen.
fn flatten_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => {
            parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn content_blocks(content: &Value) -> Vec<Value> {
    match content {
        Value::String(s) => text_block(s).into_iter().collect(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                Some("text") | Some("input_text") => p.get("text").and_then(Value::as_str).and_then(text_block),
                Some("image_url") => {
                    let url = p.pointer("/image_url/url").or_else(|| p.get("image_url")).and_then(Value::as_str)?;
                    Some(image_block(url))
                }
                _ => None,
            })
            .collect(),
        Value::Null => vec![],
        other => text_block(&other.to_string()).into_iter().collect(),
    }
}

struct MessageBuilder {
    messages: Vec<Value>,
}

impl MessageBuilder {
    fn push(&mut self, role: &str, mut blocks: Vec<Value>) {
        if blocks.is_empty() {
            return;
        }
        if let Some(last) = self.messages.last_mut() {
            if last["role"] == role {
                if let Some(arr) = last["content"].as_array_mut() {
                    arr.append(&mut blocks);
                    return;
                }
            }
        }
        self.messages.push(json!({ "role": role, "content": blocks }));
    }
}

/// OpenAI-Request → Anthropic-Request.
pub fn openai_to_anthropic(
    req: &Value,
    upstream_model: &str,
    default_max_tokens: u32,
) -> Result<Translated, ProxyError> {
    let msgs = req
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| ProxyError::BadRequest("Feld 'messages' fehlt oder ist kein Array".into()))?;

    let mut system_parts: Vec<String> = Vec::new();
    let mut b = MessageBuilder { messages: Vec::new() };

    for m in msgs {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = m.get("content").unwrap_or(&Value::Null);
        match role {
            "system" | "developer" => {
                let t = flatten_text(content);
                if !t.trim().is_empty() {
                    system_parts.push(t);
                }
            }
            "assistant" => {
                let mut blocks = content_blocks(content);
                if let Some(calls) = m.get("tool_calls").and_then(Value::as_array) {
                    for c in calls {
                        let args = c.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}");
                        let input: Value = serde_json::from_str(args).unwrap_or_else(|_| json!({}));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": c.get("id").and_then(Value::as_str).unwrap_or("call_0"),
                            "name": c.pointer("/function/name").and_then(Value::as_str).unwrap_or("tool"),
                            "input": if input.is_object() { input } else { json!({ "value": input }) },
                        }));
                    }
                }
                b.push("assistant", blocks);
            }
            "tool" => {
                let id = m.get("tool_call_id").and_then(Value::as_str).unwrap_or("call_0");
                b.push(
                    "user",
                    vec![json!({ "type": "tool_result", "tool_use_id": id, "content": flatten_text(content) })],
                );
            }
            "function" => {
                let name = m.get("name").and_then(Value::as_str).unwrap_or("function");
                b.push("user", text_block(&format!("[{name}] {}", flatten_text(content))).into_iter().collect());
            }
            _ => b.push("user", content_blocks(content)),
        }
    }

    if b.messages.is_empty() {
        return Err(ProxyError::BadRequest("Keine Nachrichten mit Inhalt übergeben".into()));
    }
    // Anthropic: erste Nachricht muss vom Nutzer kommen.
    if b.messages[0]["role"] != "user" {
        b.messages.insert(0, json!({ "role": "user", "content": [{ "type": "text", "text": "…" }] }));
    }

    // JSON-Modus nachbilden
    if req.pointer("/response_format/type").and_then(Value::as_str) == Some("json_object") {
        system_parts.push("Antworte ausschließlich mit gültigem JSON. / Respond with valid JSON only.".into());
    }

    let max_tokens = req
        .get("max_completion_tokens")
        .or_else(|| req.get("max_tokens"))
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .unwrap_or(default_max_tokens as u64);

    let mut out = Map::new();
    out.insert("model".into(), json!(upstream_model));
    out.insert("max_tokens".into(), json!(max_tokens));
    out.insert("messages".into(), Value::Array(b.messages));
    if !system_parts.is_empty() {
        out.insert("system".into(), json!(system_parts.join("\n\n")));
    }
    if let Some(t) = req.get("temperature").and_then(Value::as_f64) {
        out.insert("temperature".into(), json!(t.clamp(0.0, 1.0)));
    } else if let Some(p) = req.get("top_p").and_then(Value::as_f64) {
        // Anthropic erlaubt bei neueren Modellen nur eins von beiden.
        out.insert("top_p".into(), json!(p.clamp(0.0, 1.0)));
    }
    let stops: Vec<String> = match req.get("stop") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => vec![],
    };
    let stops: Vec<String> = stops.into_iter().filter(|s| !s.trim().is_empty()).collect();
    if !stops.is_empty() {
        out.insert("stop_sequences".into(), json!(stops));
    }

    // Tools
    if let Some(tools) = req.get("tools").and_then(Value::as_array) {
        let converted: Vec<Value> = tools
            .iter()
            .filter_map(|t| {
                let f = t.get("function")?;
                Some(json!({
                    "name": f.get("name")?.as_str()?,
                    "description": f.get("description").and_then(Value::as_str).unwrap_or(""),
                    "input_schema": f.get("parameters").cloned().unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                }))
            })
            .collect();
        if !converted.is_empty() {
            out.insert("tools".into(), Value::Array(converted));
            match req.get("tool_choice") {
                Some(Value::String(s)) if s == "required" => {
                    out.insert("tool_choice".into(), json!({ "type": "any" }));
                }
                Some(Value::String(s)) if s == "none" => {
                    out.insert("tool_choice".into(), json!({ "type": "none" }));
                }
                Some(Value::Object(o)) => {
                    if let Some(name) = o.get("function").and_then(|f| f.get("name")).and_then(Value::as_str) {
                        out.insert("tool_choice".into(), json!({ "type": "tool", "name": name }));
                    }
                }
                _ => {}
            }
        }
    }

    let stream = req.get("stream").and_then(Value::as_bool).unwrap_or(false);
    if stream {
        out.insert("stream".into(), json!(true));
    }
    let include_usage = req.pointer("/stream_options/include_usage").and_then(Value::as_bool).unwrap_or(false);

    Ok(Translated { body: Value::Object(out), stream, include_usage })
}

pub fn map_stop_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        Some("refusal") => "content_filter",
        _ => "stop",
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn completion_id(anthropic_id: Option<&str>) -> String {
    match anthropic_id {
        Some(id) if !id.is_empty() => format!("chatcmpl-{id}"),
        _ => format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
    }
}

/// Anthropic-Antwort → OpenAI ChatCompletion. Liefert zusätzlich die Usage für die Statistik.
pub fn anthropic_to_openai(resp: &Value, client_model: &str) -> (Value, Usage) {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = resp.get("content").and_then(Value::as_array) {
        for b in blocks {
            match b.get("type").and_then(Value::as_str) {
                Some("text") => text.push_str(b.get("text").and_then(Value::as_str).unwrap_or("")),
                Some("tool_use") => tool_calls.push(json!({
                    "id": b.get("id").cloned().unwrap_or(json!("call_0")),
                    "type": "function",
                    "function": {
                        "name": b.get("name").cloned().unwrap_or(json!("tool")),
                        "arguments": b.get("input").map(|i| i.to_string()).unwrap_or_else(|| "{}".into()),
                    }
                })),
                _ => {}
            }
        }
    }
    let usage = resp.get("usage").map(Usage::from_anthropic).unwrap_or_default();
    let mut message = json!({ "role": "assistant", "content": text });
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let out = json!({
        "id": completion_id(resp.get("id").and_then(Value::as_str)),
        "object": "chat.completion",
        "created": now_secs(),
        "model": client_model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": map_stop_reason(resp.get("stop_reason").and_then(Value::as_str)),
            "logprobs": null
        }],
        "usage": usage.to_openai(),
    });
    (out, usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_and_messages() {
        let req = json!({
            "model": "haiku",
            "messages": [
                {"role": "system", "content": "Du bist ein Diktat-Korrektor."},
                {"role": "user", "content": "hallo welt"},
            ],
            "temperature": 1.4,
            "stop": "END",
        });
        let t = openai_to_anthropic(&req, "claude-haiku-5-5-default", 4096).unwrap();
        assert_eq!(t.body["system"], "Du bist ein Diktat-Korrektor.");
        assert_eq!(t.body["max_tokens"], 4096);
        assert_eq!(t.body["temperature"], 1.0);
        assert_eq!(t.body["stop_sequences"], json!(["END"]));
        assert_eq!(t.body["messages"][0]["content"][0]["text"], "hallo welt");
        assert!(!t.stream);
    }

    #[test]
    fn merges_roles_and_tools() {
        let req = json!({
            "messages": [
                {"role": "user", "content": "a"},
                {"role": "user", "content": [{"type":"text","text":"b"},{"type":"image_url","image_url":{"url":"data:image/jpeg;base64,QUJD"}}]},
                {"role": "assistant", "content": null, "tool_calls": [{"id":"c1","type":"function","function":{"name":"get","arguments":"{\"x\":1}"}}]},
                {"role": "tool", "tool_call_id": "c1", "content": "42"},
            ],
            "max_tokens": 50,
            "stream": true,
            "stream_options": {"include_usage": true},
            "tools": [{"type":"function","function":{"name":"get","parameters":{"type":"object"}}}],
            "tool_choice": "required"
        });
        let t = openai_to_anthropic(&req, "m", 4096).unwrap();
        let msgs = t.body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["content"].as_array().unwrap().len(), 3);
        assert_eq!(msgs[0]["content"][2]["source"]["media_type"], "image/jpeg");
        assert_eq!(msgs[1]["content"][0]["input"]["x"], 1);
        assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
        assert_eq!(t.body["tool_choice"]["type"], "any");
        assert_eq!(t.body["max_tokens"], 50);
        assert!(t.stream && t.include_usage);
    }

    #[test]
    fn response_mapping() {
        let resp = json!({
            "id": "msg_1", "type": "message", "role": "assistant",
            "content": [{"type":"text","text":"Hallo"}],
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 10, "output_tokens": 3, "cache_read_input_tokens": 5}
        });
        let (out, usage) = anthropic_to_openai(&resp, "haiku");
        assert_eq!(out["choices"][0]["message"]["content"], "Hallo");
        assert_eq!(out["choices"][0]["finish_reason"], "length");
        assert_eq!(out["usage"]["prompt_tokens"], 15);
        assert_eq!(out["usage"]["total_tokens"], 18);
        assert_eq!(usage.cache_read_tokens, 5);
        assert_eq!(out["model"], "haiku");
    }

    #[test]
    fn rejects_empty() {
        let req = json!({"messages": [{"role":"system","content":"x"}]});
        assert!(openai_to_anthropic(&req, "m", 1).is_err());
    }
}
