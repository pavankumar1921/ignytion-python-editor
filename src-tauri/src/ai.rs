//! AI chat provider adapter (OpenAI-compatible `/chat/completions`).
//!
//! No Tauri dependency, so every security rule below is unit-tested:
//!   * code only leaves the machine when the caller passes explicit consent;
//!   * a remote provider is never contacted without an API key;
//!   * keys and code are never sent over plain HTTP to a remote host;
//!   * the API key is redacted from every error message;
//!   * every failure maps to a typed, user-visible error.
//! The API key is supplied by the native layer (OS credential store) and is
//! never part of the request that comes from the web frontend.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const MAX_CONTEXT_CHARS: usize = 60_000;
const MAX_HISTORY: usize = 20;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

const SYSTEM_PROMPT: &str = "You are a careful Python coding assistant inside a small desktop \
editor. Explain code clearly and concisely. When you suggest a change, show the corrected code \
in a fenced code block and explain what changed. You cannot edit the user's files; never claim \
that you have.";

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub base_url: String,
    pub model: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CodeContext {
    /// "selection" | "file"
    pub kind: String,
    /// e.g. "hello.py, lines 3-5"
    pub label: String,
    pub text: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub config: AiConfig,
    pub history: Vec<ChatMessage>,
    pub prompt: String,
    pub context: Option<CodeContext>,
    /// Must be true whenever `context` is present. Set only by an explicit UI action.
    pub consent_to_send_code: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply {
    pub content: String,
    pub model: String,
    pub sent_code: bool,
    pub sent_chars: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum AiError {
    MissingKey(String),
    ConsentRequired(String),
    InvalidRequest(String),
    Keystore(String),
    Network(String),
    Timeout(String),
    Unauthorized(String),
    RateLimited(String),
    Provider(String),
    BadResponse(String),
}

/// Host part of a base URL, lowercased ("https://api.openai.com/v1" -> "api.openai.com").
pub fn host_of(base_url: &str) -> Option<String> {
    let rest = base_url.trim().split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?; // drop any user:pass@
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next()?.to_string() // [::1]:8080
    } else {
        authority.split(':').next()?.to_string()
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

pub fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn redact(msg: &str, key: Option<&str>) -> String {
    match key {
        Some(k) if k.len() >= 4 => msg.replace(k, "[redacted]"),
        _ => msg.to_string(),
    }
}

fn build_messages(req: &ChatRequest) -> Vec<ChatMessage> {
    let mut msgs = vec![ChatMessage {
        role: "system".into(),
        content: SYSTEM_PROMPT.into(),
    }];
    let skip = req.history.len().saturating_sub(MAX_HISTORY);
    for m in req.history.iter().skip(skip) {
        if m.role == "user" || m.role == "assistant" {
            msgs.push(m.clone());
        }
    }
    let content = match &req.context {
        Some(c) => format!(
            "{}\n\n--- {} ({}) ---\n```python\n{}\n```",
            req.prompt.trim(),
            c.kind,
            c.label,
            c.text
        ),
        None => req.prompt.trim().to_string(),
    };
    msgs.push(ChatMessage { role: "user".into(), content });
    msgs
}

fn provider_message(body: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let from_json = parsed
        .as_ref()
        .and_then(|v| v.get("error"))
        .and_then(|e| e.get("message").or(Some(e)))
        .and_then(|m| m.as_str())
        .map(str::to_string);
    let text = from_json.unwrap_or_else(|| body.trim().to_string());
    text.chars().take(300).collect()
}

pub async fn chat(req: &ChatRequest, api_key: Option<&str>) -> Result<ChatReply, AiError> {
    chat_with_timeout(req, api_key, REQUEST_TIMEOUT).await
}

pub async fn chat_with_timeout(
    req: &ChatRequest,
    api_key: Option<&str>,
    timeout: Duration,
) -> Result<ChatReply, AiError> {
    // ---- validate before anything leaves the machine
    if req.prompt.trim().is_empty() {
        return Err(AiError::InvalidRequest("Type a question first.".into()));
    }
    if req.config.model.trim().is_empty() {
        return Err(AiError::InvalidRequest("No model name is configured.".into()));
    }
    let base = req.config.base_url.trim().trim_end_matches('/');
    let host = host_of(base).ok_or_else(|| {
        AiError::InvalidRequest("The provider URL must look like https://api.openai.com/v1".into())
    })?;
    let local = is_local_host(&host);
    if base.starts_with("http://") && !local {
        return Err(AiError::InvalidRequest(
            "Refusing to send your key or code over plain HTTP to a remote host. Use https://."
                .into(),
        ));
    }
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Err(AiError::InvalidRequest("The provider URL must start with https://".into()));
    }

    if let Some(ctx) = &req.context {
        if !req.consent_to_send_code {
            return Err(AiError::ConsentRequired(
                "Code was attached but you have not agreed to send it. Nothing was sent.".into(),
            ));
        }
        if ctx.text.chars().count() > MAX_CONTEXT_CHARS {
            return Err(AiError::InvalidRequest(format!(
                "The attached code is larger than {MAX_CONTEXT_CHARS} characters. Select less code."
            )));
        }
    }

    let key = api_key.map(str::trim).filter(|k| !k.is_empty());
    if !local && key.is_none() {
        return Err(AiError::MissingKey(format!(
            "No API key is saved for {host}. Add one in the chat settings. Nothing was sent."
        )));
    }

    // ---- send
    let messages = build_messages(req);
    let sent_chars: usize = req.context.as_ref().map(|c| c.text.chars().count()).unwrap_or(0);
    let body = serde_json::json!({ "model": req.config.model, "messages": messages });

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(CONNECT_TIMEOUT.min(timeout))
        .build()
        .map_err(|e| AiError::Network(redact(&e.to_string(), key)))?;

    let mut builder = client.post(format!("{base}/chat/completions")).json(&body);
    if let Some(k) = key {
        builder = builder.bearer_auth(k);
    }

    let resp = builder.send().await.map_err(|e| {
        let text = redact(&e.to_string(), key);
        if e.is_timeout() {
            AiError::Timeout(format!("{host} did not answer in time."))
        } else if e.is_connect() {
            AiError::Network(format!("Could not connect to {host}. Check your network or the provider URL. ({text})"))
        } else {
            AiError::Network(format!("Request to {host} failed: {text}"))
        }
    })?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| AiError::Network(redact(&format!("Failed reading the response: {e}"), key)))?;

    if !status.is_success() {
        let detail = redact(&provider_message(&text), key);
        return Err(match status.as_u16() {
            401 | 403 => AiError::Unauthorized(format!(
                "{host} rejected the API key (HTTP {}). {detail}",
                status.as_u16()
            )),
            429 => AiError::RateLimited(format!("{host} is rate limiting you (HTTP 429). {detail}")),
            code => AiError::Provider(format!("{host} returned HTTP {code}. {detail}")),
        });
    }

    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| AiError::BadResponse(format!("{host} returned something that is not JSON.")))?;
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .ok_or_else(|| AiError::BadResponse(format!("{host} replied without a message.")))?;

    Ok(ChatReply {
        content: content.to_string(),
        model: value
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&req.config.model)
            .to_string(),
        sent_code: req.context.is_some(),
        sent_chars,
    })
}
