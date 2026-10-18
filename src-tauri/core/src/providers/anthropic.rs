use futures_util::StreamExt;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;

use crate::error::{Error, Result};
use crate::providers::types::*;

/// Anthropic Messages API. Notable differences from the OpenAI shape: the
/// system prompt is a top-level field, tool results are `user` turns holding
/// `tool_result` blocks, and streaming deltas arrive as typed events.
pub async fn stream(
    req: &CompletionRequest,
    tx: UnboundedSender<StreamEvent>,
    client: reqwest::Client,
) -> Result<RawCompletion> {
    let mut body = json!({
        "model": req.model,
        "stream": true,
        "max_tokens": req.max_tokens.unwrap_or(8_192),
        "messages": build_messages(req),
    });
    if !req.system.is_empty() {
        body["system"] = json!(req.system);
    }
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters }))
            .collect::<Vec<_>>());
    }

    let url = join_url(&req.base_url, "/messages");
    let mut http =
        client.post(&url).json(&body).header("accept", "text/event-stream");
    if let Some(k) = req.api_key.as_deref().filter(|k| !k.is_empty()) {
        http = http.header("x-api-key", k);
    }
    // Required by the API; harmless for compatible proxies.
    http = http.header("anthropic-version", "2023-06-01");
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
    // block index -> (id, name, accumulated partial json)
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
            let Ok(v) = serde_json::from_str::<serde_json::Value>(payload.trim()) else {
                continue;
            };
            let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or_default();

            match kind {
                "message_start" => {
                    if let Some(u) = v
                        .get("message")
                        .and_then(|m| m.get("usage"))
                        .and_then(parse_usage)
                    {
                        out.usage = u;
                    }
                }
                "content_block_start" => {
                    let index =
                        v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let block = v.get("content_block").cloned().unwrap_or(serde_json::Value::Null);
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let id = block
                            .get("id")
                            .and_then(|i| i.as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| crate::util::new_id("call"));
                        let name = block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let _ = tx.send(StreamEvent::ToolCallStart {
                            index,
                            id: id.clone(),
                            name: name.clone(),
                        });
                        announced.insert(index);
                        calls.insert(index, (id, name, String::new()));
                    }
                }
                "content_block_delta" => {
                    let index =
                        v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let delta = v.get("delta").cloned().unwrap_or(serde_json::Value::Null);
                    let dtype = delta.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                    match dtype {
                        "text_delta" => {
                            if let Some(t) = delta.get("text").and_then(|t| t.as_str()) {
                                out.text.push_str(t);
                                let _ = tx.send(StreamEvent::TextDelta { text: t.to_string() });
                            }
                        }
                        "thinking_delta" => {
                            if let Some(t) = delta.get("thinking").and_then(|t| t.as_str()) {
                                reasoning.push_str(t);
                                let _ = tx.send(StreamEvent::ReasoningDelta { text: t.to_string() });
                            }
                        }
                        "input_json_delta" => {
                            if let Some(frag) =
                                delta.get("partial_json").and_then(|t| t.as_str())
                                && !frag.is_empty()
                            {
                                if let Some(e) = calls.get_mut(&index) {
                                    e.2.push_str(frag);
                                }
                                let _ = tx.send(StreamEvent::ToolCallArgs {
                                    index,
                                    delta: frag.to_string(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                "message_delta" => {
                    if let Some(u) = v.get("usage").and_then(parse_usage) {
                        // message_delta reports only the output side.
                        out.usage.input_tokens = out.usage.input_tokens.max(u.input_tokens);
                        out.usage.output_tokens = u.output_tokens;
                    }
                    if let Some(reason) =
                        v.get("delta").and_then(|d| d.get("stop_reason")).and_then(|r| r.as_str())
                    {
                        out.finish = match reason {
                            "tool_use" => FinishReason::ToolUse,
                            "max_tokens" => FinishReason::Length,
                            "refusal" => FinishReason::ContentFilter,
                            _ => FinishReason::Stop,
                        };
                    }
                }
                "error" => {
                    let msg = v
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("stream error");
                    return Err(Error::Provider(msg.to_string()));
                }
                _ => {}
            }
        }
    }

    out.tool_calls = calls
        .into_values()
        .map(|(id, name, arguments)| ProposedToolCall { id, name, arguments })
        .collect();
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

/// Anthropic wants tool results batched into a single user turn, so adjacent
/// results are merged here.
pub fn build_messages(req: &CompletionRequest) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::new();

    for m in &req.messages {
        match m {
            Incoming::User { text, images } => {
                let mut content = vec![json!({ "type": "text", "text": text })];
                for img in images {
                    content.push(json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": img.mime, "data": img.data_base64 }
                    }));
                }
                out.push(json!({ "role": "user", "content": content }));
            }
            Incoming::Assistant { text, tool_calls } => {
                let mut content = Vec::new();
                if !text.is_empty() {
                    content.push(json!({ "type": "text", "text": text }));
                }
                for c in tool_calls {
                    let input: serde_json::Value = serde_json::from_str(&c.arguments)
                        .unwrap_or_else(|_| json!({ "_raw": c.arguments }));
                    content.push(
                        json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": input }),
                    );
                }
                out.push(json!({ "role": "assistant", "content": content }));
            }
            Incoming::ToolResult { call_id, content, is_error, .. } => {
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": content,
                    "is_error": is_error
                });
                // Merge into the previous turn when it is already a tool result.
                let merge = out
                    .last()
                    .and_then(|m| m.get("role"))
                    .and_then(|r| r.as_str())
                    == Some("user")
                    && out
                        .last()
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                        .is_some_and(|a| {
                            a.first()
                                .and_then(|b| b.get("type"))
                                .and_then(|t| t.as_str())
                                == Some("tool_result")
                        });
                if merge {
                    let arr = out.last_mut().unwrap()["content"].as_array_mut().unwrap();
                    arr.push(block);
                } else {
                    out.push(json!({ "role": "user", "content": [block] }));
                }
            }
        }
    }
    out
}

fn parse_usage(v: &serde_json::Value) -> Option<Usage> {
    let g = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let u = Usage {
        input_tokens: g("input_tokens"),
        output_tokens: g("output_tokens"),
        cached_input_tokens: g("cache_read_input_tokens"),
    };
    (u.input_tokens > 0 || u.output_tokens > 0).then_some(u)
}

fn join_url(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_with(messages: Vec<Incoming>) -> CompletionRequest {
        CompletionRequest {
            dialect: crate::models::Dialect::Anthropic,
            base_url: "https://api.anthropic.com/v1".into(),
            model: "m".into(),
            api_key: Some("k".into()),
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
    fn system_is_not_a_message() {
        let msgs = build_messages(&req_with(vec![]));
        assert!(msgs.is_empty(), "system must be sent as its own field");
    }

    #[test]
    fn tool_results_merge_into_one_user_turn() {
        let msgs = build_messages(&req_with(vec![
            Incoming::ToolResult {
                call_id: "a".into(),
                name: "read".into(),
                content: "1".into(),
                is_error: false,
            },
            Incoming::ToolResult {
                call_id: "b".into(),
                name: "read".into(),
                content: "2".into(),
                is_error: false,
            },
        ]));
        assert_eq!(msgs.len(), 1, "adjacent results must not split the turn");
        assert_eq!(msgs[0]["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_user_turn_after_a_result_starts_a_new_block() {
        let msgs = build_messages(&req_with(vec![
            Incoming::ToolResult {
                call_id: "a".into(),
                name: "read".into(),
                content: "1".into(),
                is_error: false,
            },
            Incoming::User { text: "next".into(), images: vec![] },
        ]));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1]["content"][0]["type"], "text");
    }

    #[test]
    fn malformed_tool_arguments_do_not_panic() {
        let msgs = build_messages(&req_with(vec![Incoming::Assistant {
            text: String::new(),
            tool_calls: vec![ProposedToolCall {
                id: "c".into(),
                name: "read".into(),
                arguments: "{not json".into(),
            }],
        }]));
        assert_eq!(msgs[0]["content"][0]["input"]["_raw"], "{not json");
    }

    #[test]
    fn usage_reads_anthropic_field_names() {
        let u = parse_usage(&json!({
            "input_tokens": 12, "output_tokens": 5, "cache_read_input_tokens": 3
        }))
        .unwrap();
        assert_eq!((u.input_tokens, u.output_tokens, u.cached_input_tokens), (12, 5, 3));
    }
}
