use futures_util::StreamExt;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;

use crate::error::{Error, Result};
use crate::providers::types::*;

/// OpenAI-compatible `/chat/completions`. This one adapter serves OpenAI,
/// OpenRouter, Groq, Mistral, DeepSeek, xAI, Together, Fireworks, GitHub Models,
/// Ollama, LM Studio, llama.cpp and every user-added provider that speaks the
/// OpenAI dialect.
///
/// Deltas are pushed to `tx` as they arrive so the UI can render progressively;
/// the assembled result is returned once the stream ends.
pub async fn stream(
    req: &CompletionRequest,
    tx: UnboundedSender<StreamEvent>,
    client: reqwest::Client,
) -> Result<RawCompletion> {
    let mut body = json!({
        "model": req.model,
        "stream": true,
        "stream_options": { "include_usage": true },
        "messages": build_messages(req),
    });

    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }
    if let Some(m) = req.max_tokens {
        body["max_tokens"] = json!(m);
    }
    if let Some(effort) = req.reasoning_effort.as_deref() {
        body["reasoning_effort"] = json!(effort);
    }
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            }))
            .collect::<Vec<_>>());
        body["tool_choice"] = json!("auto");
    }

    let url = join_url(&req.base_url, "/chat/completions");
    let mut http =
        client.post(&url).json(&body).header("accept", "text/event-stream");
    if let Some(k) = req.api_key.as_deref().filter(|k| !k.is_empty()) {
        http = http.bearer_auth(k);
    }
    for (k, v) in &req.extra_headers {
        http = http.header(k.as_str(), v.as_str());
    }

    let res = http.send().await.map_err(|e| Error::Network(e.to_string()))?;
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
    // tool index -> (id, name, accumulated argument JSON)
    let mut calls: std::collections::BTreeMap<u32, (String, String, String)> =
        std::collections::BTreeMap::new();
    let mut announced: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();

    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|e| Error::Network(e.to_string()))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(idx) = buf.find('\n') {
            let line = buf[..idx].trim_end().to_string();
            buf.drain(..=idx);
            let Some(payload) = line.strip_prefix("data:") else { continue };
            let payload = payload.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
                // Keep-alive comments and non-JSON frames are not fatal.
                continue;
            };
            // Some gateways report failures inside an HTTP 200 stream.
            if let Some(err) = v.get("error") {
                return Err(Error::Provider(format!("{err}")));
            }
            if let Some(u) = v.get("usage").and_then(parse_usage) {
                out.usage = u;
            }
            let Some(choice) = v.get("choices").and_then(|c| c.get(0)) else { continue };

            if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
                out.finish = match reason {
                    "tool_calls" | "function_call" => FinishReason::ToolUse,
                    "length" => FinishReason::Length,
                    "content_filter" => FinishReason::ContentFilter,
                    _ => FinishReason::Stop,
                };
            }
            let Some(delta) = choice.get("delta") else { continue };

            // Reasoning traces arrive under vendor-specific field names.
            for key in ["reasoning_content", "reasoning", "thinking"] {
                if let Some(t) = delta.get(key).and_then(|v| v.as_str())
                    && !t.is_empty()
                {
                    reasoning.push_str(t);
                    let _ = tx.send(StreamEvent::ReasoningDelta { text: t.to_string() });
                }
            }

            if let Some(t) = delta.get("content").and_then(|v| v.as_str())
                && !t.is_empty()
            {
                out.text.push_str(t);
                let _ = tx.send(StreamEvent::TextDelta { text: t.to_string() });
            }

            let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) else {
                continue;
            };
            for tc in tcs {
                let index = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let entry = calls.entry(index).or_default();
                if announced.insert(index) {
                    let id = tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| crate::util::new_id("call"));
                    let name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let _ = tx.send(StreamEvent::ToolCallStart {
                        index,
                        id: id.clone(),
                        name: name.clone(),
                    });
                    entry.0 = id;
                    entry.1 = name;
                }
                if let Some(args) = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|v| v.as_str())
                    && !args.is_empty()
                {
                    entry.2.push_str(args);
                    let _ = tx.send(StreamEvent::ToolCallArgs { index, delta: args.to_string() });
                }
            }
        }
    }

    out.tool_calls = calls
        .into_values()
        .map(|(id, name, arguments)| ProposedToolCall { id, name, arguments })
        .collect();
    // Local servers often close without a finish_reason after emitting calls.
    if !out.tool_calls.is_empty() {
        out.finish = FinishReason::ToolUse;
    }
    if !reasoning.is_empty() {
        out.reasoning = Some(reasoning);
    }
    let _ = tx.send(StreamEvent::Usage { usage: out.usage });
    let _ = tx.send(StreamEvent::Done { reason: out.finish });
    Ok(out)
}

pub fn build_messages(req: &CompletionRequest) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    if !req.system.is_empty() {
        out.push(json!({ "role": "system", "content": req.system }));
    }
    for m in &req.messages {
        match m {
            Incoming::User { text, images } => {
                if images.is_empty() {
                    out.push(json!({ "role": "user", "content": text }));
                } else {
                    let mut parts = vec![json!({ "type": "text", "text": text })];
                    for img in images {
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", img.mime, img.data_base64)
                            }
                        }));
                    }
                    out.push(json!({ "role": "user", "content": parts }));
                }
            }
            Incoming::Assistant { text, tool_calls } => {
                let mut msg = json!({ "role": "assistant", "content": text });
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = json!(tool_calls
                        .iter()
                        .map(|c| json!({
                            "id": c.id,
                            "type": "function",
                            "function": { "name": c.name, "arguments": c.arguments }
                        }))
                        .collect::<Vec<_>>());
                }
                out.push(msg);
            }
            Incoming::ToolResult { call_id, content, .. } => {
                out.push(
                    json!({ "role": "tool", "tool_call_id": call_id, "content": content }),
                );
            }
        }
    }
    out
}

pub fn parse_usage(v: &serde_json::Value) -> Option<Usage> {
    let g = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let u = Usage {
        input_tokens: g("prompt_tokens"),
        output_tokens: g("completion_tokens"),
        cached_input_tokens: v
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32,
    };
    (u.input_tokens > 0 || u.output_tokens > 0).then_some(u)
}

/// `/models` is optional on many endpoints, so a failure here is reported but
/// never blocks the provider.
pub async fn list_models(
    base_url: &str,
    api_key: Option<&str>,
    client: reqwest::Client,
) -> Result<Vec<String>> {
    let url = join_url(base_url, "/models");
    let mut http = client.get(&url);
    if let Some(k) = api_key.filter(|k| !k.is_empty()) {
        http = http.bearer_auth(k);
    }
    let res = http.send().await.map_err(|e| Error::Network(e.to_string()))?;
    if !res.status().is_success() {
        return Err(Error::Provider(format!("{} listing models", res.status())));
    }
    let v: serde_json::Value = res.json().await?;
    let ids = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        Err(Error::Provider("endpoint returned no models".into()))
    } else {
        Ok(ids)
    }
}

pub fn join_url(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_with(messages: Vec<Incoming>) -> CompletionRequest {
        CompletionRequest {
            dialect: crate::models::Dialect::OpenAi,
            base_url: "http://x".into(),
            model: "m".into(),
            api_key: None,
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
    fn user_text_only_uses_plain_content() {
        let msgs = build_messages(&req_with(vec![Incoming::User {
            text: "hi".into(),
            images: vec![],
        }]));
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["content"], "hi");
    }

    #[test]
    fn images_become_multipart_content() {
        let msgs = build_messages(&req_with(vec![Incoming::User {
            text: "look".into(),
            images: vec![ImagePart { mime: "image/png".into(), data_base64: "AAA".into() }],
        }]));
        let parts = msgs[1]["content"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1]["type"], "image_url");
        assert!(parts[1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn assistant_tool_calls_are_preserved() {
        let msgs = build_messages(&req_with(vec![Incoming::Assistant {
            text: "looking".into(),
            tool_calls: vec![ProposedToolCall {
                id: "c1".into(),
                name: "read".into(),
                arguments: r#"{"path":"a"}"#.into(),
            }],
        }]));
        assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "read");
    }

    #[test]
    fn tool_results_reference_their_call_id() {
        let msgs = build_messages(&req_with(vec![Incoming::ToolResult {
            call_id: "call_7".into(),
            name: "read".into(),
            content: "ok".into(),
            is_error: false,
        }]));
        assert_eq!(msgs[1]["tool_call_id"], "call_7");
    }

    #[test]
    fn url_join_is_slash_safe() {
        assert_eq!(join_url("http://a/v1/", "/models"), "http://a/v1/models");
        assert_eq!(join_url("http://a/v1", "/models"), "http://a/v1/models");
    }

    #[test]
    fn usage_parses_prompt_completion_and_cache() {
        let u = parse_usage(&json!({
            "prompt_tokens": 10,
            "completion_tokens": 4,
            "prompt_tokens_details": { "cached_tokens": 6 }
        }))
        .unwrap();
        assert_eq!((u.input_tokens, u.output_tokens, u.cached_input_tokens), (10, 4, 6));
        assert!(parse_usage(&json!({})).is_none());
    }
}
