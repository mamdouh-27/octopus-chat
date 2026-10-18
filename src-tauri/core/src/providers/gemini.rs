use futures_util::StreamExt;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;

use crate::error::{Error, Result};
use crate::providers::types::*;

/// Google Gemini `streamGenerateContent`. The response is a JSON array of
/// chunks rather than SSE, and the key travels as a query parameter.
pub async fn stream(
    req: &CompletionRequest,
    tx: UnboundedSender<StreamEvent>,
    client: reqwest::Client,
) -> Result<RawCompletion> {
    let mut body = json!({
        "contents": build_contents(req),
        "generationConfig": build_generation_config(req),
    });
    if !req.system.is_empty() {
        body["systemInstruction"] =
            json!({ "parts": [{ "text": req.system }] });
    }
    if !req.tools.is_empty() {
        body["tools"] = json!([{
            "functionDeclarations": req.tools.iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "parameters": strip_unsupported(t.parameters.clone()),
            })).collect::<Vec<_>>()
        }]);
    }

    let key = req.api_key.clone().unwrap_or_default();
    let url = format!(
        "{}/models/{}:streamGenerateContent?alt=sse&key={}",
        req.base_url.trim_end_matches('/'),
        req.model,
        percent_encoding::utf8_percent_encode(&key, percent_encoding::NON_ALPHANUMERIC)
    );

    let res = client
        .post(&url)
        .json(&body)
        .header("accept", "text/event-stream")
        .send()
        .await
        .map_err(|e| Error::Network(e.to_string()))?;
    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(Error::Provider(format!(
            "{status}: {}",
            crate::util::truncate(&text, 600)
        )));
    }

    read_sse(res, tx).await
}

async fn read_sse(
    res: reqwest::Response,
    tx: UnboundedSender<StreamEvent>,
) -> Result<RawCompletion> {
    let mut chunks = res.bytes_stream();
    let mut buf = String::new();
    let mut out = RawCompletion::default();
    let mut reasoning = String::new();
    let mut calls: Vec<ProposedToolCall> = Vec::new();
    let mut pending: std::collections::BTreeMap<u32, ProposedToolCall> =
        std::collections::BTreeMap::new();
    let mut announced: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut usage = Usage::default();

    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|e| Error::Network(e.to_string()))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(idx) = buf.find('\n') {
            let line = buf[..idx].trim_end().to_string();
            buf.drain(..=idx);
            let Some(payload) = line.strip_prefix("data:") else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(payload.trim()) else {
                continue;
            };
            if let Some(e) = v.get("error") {
                return Err(Error::Provider(format!("{e}")));
            }
            if let Some(md) = v.get("usageMetadata") {
                usage = Usage {
                    input_tokens: g(md, "promptTokenCount"),
                    output_tokens: g(md, "candidatesTokenCount"),
                    cached_input_tokens: g(md, "cachedContentTokenCount"),
                };
            }
            let Some(candidate) = v
                .get("candidates")
                .and_then(|c| c.get(0))
            else {
                continue;
            };
            if let Some(reason) = candidate.get("finishReason").and_then(|r| r.as_str()) {
                out.finish = match reason {
                    "STOP" => {
                        if calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolUse }
                    }
                    "MAX_TOKENS" => FinishReason::Length,
                    "SAFETY" | "RECITATION" | "BLOCKLIST" => FinishReason::ContentFilter,
                    _ => FinishReason::Stop,
                };
            }
            let Some(parts) = candidate
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            else {
                continue;
            };

            for (i, part) in parts.iter().enumerate() {
                if let Some(t) = part.get("text").and_then(|t| t.as_str())
                    && !t.is_empty()
                {
                    // `thought: true` marks a reasoning part.
                    if part.get("thought").and_then(|b| b.as_bool()).unwrap_or(false) {
                        reasoning.push_str(t);
                        let _ = tx.send(StreamEvent::ReasoningDelta { text: t.to_string() });
                    } else {
                        out.text.push_str(t);
                        let _ = tx.send(StreamEvent::TextDelta { text: t.to_string() });
                    }
                }
                if let Some(fc) = part.get("functionCall") {
                    let index = i as u32;
                    let name =
                        fc.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
                    let args = fc.get("args").cloned().unwrap_or(json!({}));
                    let args = serde_json::to_string(&args).unwrap_or_else(|_| "{}".into());
                    if announced.insert(index) {
                        let id = crate::util::new_id("call");
                        let _ = tx.send(StreamEvent::ToolCallStart {
                            index,
                            id: id.clone(),
                            name: name.clone(),
                        });
                        calls.push(ProposedToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: String::new(),
                        });
                    }
                    let _ = tx.send(StreamEvent::ToolCallArgs {
                        index,
                        delta: args.clone(),
                    });
                    if let Some(last) = calls.last_mut() {
                        last.arguments.push_str(&args);
                    }
                    pending.insert(index, ProposedToolCall { id: String::new(), name, arguments: args });
                }
            }
        }
    }

    out.tool_calls = calls;
    if !out.tool_calls.is_empty() {
        out.finish = FinishReason::ToolUse;
    }
    out.usage = usage;
    if !reasoning.is_empty() {
        out.reasoning = Some(reasoning);
    }
    let _ = tx.send(StreamEvent::Usage { usage: out.usage });
    let _ = tx.send(StreamEvent::Done { reason: out.finish });
    Ok(out)
}

fn g(v: &serde_json::Value, key: &str) -> u32 {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0) as u32
}

/// Gemini rejects several JSON-Schema keywords; drop them rather than 400.
fn strip_unsupported(schema: serde_json::Value) -> serde_json::Value {
    const BAD: &[&str] = &[
        "additionalProperties",
        "$schema",
        "default",
        "examples",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "oneOf",
        "allOf",
        "not",
        "const",
        "patternProperties",
    ];
    match schema {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter(|(k, _)| !BAD.contains(&k.as_str()))
                .map(|(k, v)| (k, strip_unsupported(v)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(strip_unsupported).collect())
        }
        other => other,
    }
}

pub fn build_contents(req: &CompletionRequest) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for m in &req.messages {
        match m {
            Incoming::User { text, images } => {
                let mut parts = vec![json!({ "text": text })];
                for img in images {
                    parts.push(json!({
                        "inlineData": { "mimeType": img.mime, "data": img.data_base64 }
                    }));
                }
                out.push(json!({ "role": "user", "parts": parts }));
            }
            Incoming::Assistant { text, tool_calls } => {
                let mut parts = Vec::new();
                if !text.is_empty() {
                    parts.push(json!({ "text": text }));
                }
                for c in tool_calls {
                    let args: serde_json::Value = serde_json::from_str(&c.arguments)
                        .unwrap_or_else(|_| json!({ "_raw": c.arguments }));
                    parts.push(json!({ "functionCall": { "name": c.name, "args": args } }));
                }
                out.push(json!({ "role": "model", "parts": parts }));
            }
            Incoming::ToolResult { name, content, .. } => {
                out.push(json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": { "name": name, "response": { "result": content } }
                    }]
                }));
            }
        }
    }
    out
}

fn build_generation_config(req: &CompletionRequest) -> serde_json::Value {
    let mut cfg = serde_json::Map::new();
    if let Some(t) = req.temperature {
        cfg.insert("temperature".into(), json!(t));
    }
    if let Some(m) = req.max_tokens {
        cfg.insert("maxOutputTokens".into(), json!(m));
    }
    if let Some(effort) = req.reasoning_effort.as_deref() {
        // Gemini names these thinking budgets rather than a single effort knob.
        let budget = match effort {
            "low" => 2_048,
            "medium" => 8_192,
            "high" => 24_576,
            _ => 0,
        };
        cfg.insert("thinkingConfig".into(), json!({ "thinkingBudget": budget }));
    }
    serde_json::Value::Object(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_with(messages: Vec<Incoming>) -> CompletionRequest {
        CompletionRequest {
            dialect: crate::models::Dialect::Gemini,
            base_url: "https://g/v1beta".into(),
            model: "gemini-3-pro".into(),
            api_key: Some("secret key".into()),
            extra_headers: vec![],
            system: "sys".into(),
            messages,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            no_timeout: false,
        }
    }

    #[test]
    fn contents_use_user_and_model_roles() {
        let c = build_contents(&req_with(vec![
            Incoming::User { text: "hi".into(), images: vec![] },
            Incoming::Assistant { text: "yo".into(), tool_calls: vec![] },
        ]));
        assert_eq!(c[0]["role"], "user");
        assert_eq!(c[1]["role"], "model");
    }

    #[test]
    fn tool_results_use_function_response_parts() {
        let c = build_contents(&req_with(vec![Incoming::ToolResult {
            call_id: "c1".into(),
            name: "read_file".into(),
            content: "hello".into(),
            is_error: false,
        }]));
        assert_eq!(c[0]["parts"][0]["functionResponse"]["name"], "read_file");
    }

    #[test]
    fn unsupported_schema_keywords_are_stripped() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": { "a": { "type": "string", "default": "x", "oneOf": [] } }
        });
        let out = strip_unsupported(schema);
        assert!(out.get("additionalProperties").is_none());
        assert!(out["properties"]["a"].get("default").is_none());
        assert!(out["properties"]["a"].get("oneOf").is_none());
        assert_eq!(out["properties"]["a"]["type"], "string");
    }

    #[test]
    fn thinking_budget_maps_from_effort() {
        let req = CompletionRequest { reasoning_effort: Some("high".into()), ..req_with(vec![]) };
        assert_eq!(build_generation_config(&req)["thinkingConfig"]["thinkingBudget"], 24_576);
    }
}
