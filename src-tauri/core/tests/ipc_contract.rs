//! Guards the shape of every struct that crosses the Tauri IPC boundary.
//!
//! The frontend is TypeScript and sends camelCase. If a Rust struct in
//! `octopus_core::models` loses its `#[serde(rename_all = "camelCase")]`, Serde
//! rejects the entire payload with `missing field ...` and the UI only sees a
//! generic failure. Project creation was broken this way for a long time, so
//! each payload here is written exactly as `src/lib/ipc.ts` sends it.

use octopus_core::models::*;

fn sample_project() -> serde_json::Value {
    serde_json::json!({
        "id": "",
        "name": "demo",
        "roots": ["D:/tmp/demo"],
        "instructions": "",
        "permissionMode": "ask",
        "accent": null,
        "archived": false,
        "createdAt": 1,
        "updatedAt": 2
    })
}

#[test]
fn project_accepts_camel_case() {
    let p: Project = serde_json::from_value(sample_project()).expect("project should deserialise");
    assert_eq!(p.name, "demo");
    assert_eq!(p.permission_mode, PermissionMode::Ask);
    assert_eq!(p.created_at, 1);
}

#[test]
fn project_serialises_back_to_camel_case() {
    let p: Project = serde_json::from_value(sample_project()).unwrap();
    let out = serde_json::to_value(&p).unwrap();
    assert!(out.get("permissionMode").is_some(), "got {out}");
    assert!(out.get("permission_mode").is_none(), "got {out}");
}

#[test]
fn session_accepts_camel_case() {
    let raw = serde_json::json!({
        "id": "ses_1",
        "projectId": "prj_1",
        "title": "chat",
        "mode": "build",
        "state": "idle",
        "provider": "ollama",
        "model": "qwen3",
        "reasoningEffort": null,
        "temperature": null,
        "maxTokens": null,
        "permissionMode": "auto_review",
        "goal": null,
        "forkedFrom": null,
        "worktree": null,
        "favorite": false,
        "tokensIn": 10,
        "tokensOut": 20,
        "costUsd": 0.5,
        "createdAt": 1,
        "updatedAt": 2
    });
    let s: Session = serde_json::from_value(raw).expect("session should deserialise");
    assert_eq!(s.project_id.as_deref(), Some("prj_1"));
    assert_eq!(s.tokens_in, 10);
    assert_eq!(s.cost_usd, 0.5);
    assert_eq!(s.permission_mode, PermissionMode::AutoReview);
}

#[test]
fn message_with_attachments_accepts_camel_case() {
    let raw = serde_json::json!({
        "id": "msg_1",
        "sessionId": "ses_1",
        "role": "user",
        "content": "hi",
        "reasoning": null,
        "state": "done",
        "model": null,
        "error": null,
        "tokensIn": 1,
        "tokensOut": 2,
        "supersedes": null,
        "attachments": [
            { "id": "a1", "path": "C:/x.png", "mime": "image/png", "bytes": 9, "preview": null }
        ],
        "createdAt": 5
    });
    let m: Message = serde_json::from_value(raw).expect("message should deserialise");
    assert_eq!(m.session_id, "ses_1");
    assert_eq!(m.attachments.len(), 1);
    assert_eq!(m.attachments[0].mime, "image/png");
}

#[test]
fn tool_call_and_checkpoint_accept_camel_case() {
    let tc: ToolCall = serde_json::from_value(serde_json::json!({
        "id": "tc_1",
        "sessionId": "ses_1",
        "messageId": "msg_1",
        "name": "read",
        "input": "{}",
        "output": null,
        "state": "done",
        "deniedReason": null,
        "diff": null,
        "startedAt": 1,
        "endedAt": 2
    }))
    .expect("tool call should deserialise");
    assert_eq!(tc.message_id, "msg_1");
    assert_eq!(tc.started_at, 1);

    let cp: Checkpoint = serde_json::from_value(serde_json::json!({
        "id": "cp_1",
        "sessionId": "ses_1",
        "messageId": null,
        "label": "before",
        "gitHead": null,
        "createdAt": 3
    }))
    .expect("checkpoint should deserialise");
    assert_eq!(cp.label, "before");
}

#[test]
fn custom_provider_and_model_accept_camel_case() {
    let p: CustomProvider = serde_json::from_value(serde_json::json!({
        "id": "custom",
        "label": "My endpoint",
        "dialect": "openai",
        "baseUrl": "http://localhost:1234/v1",
        "local": true,
        "apiKey": null,
        "headers": [["X-A", "1"]],
        "models": [{
            "id": "my-model",
            "label": "My model",
            "dialect": "openai",
            "contextWindow": 32768,
            "maxOutput": 4096,
            "supportsTools": true,
            "supportsVision": false,
            "reasoning": false,
            "inputCost": null,
            "outputCost": null
        }]
    }))
    .expect("provider should deserialise");
    assert_eq!(p.base_url, "http://localhost:1234/v1");
    assert_eq!(p.models[0].context_window, Some(32768));
    assert!(p.models[0].supports_tools);
}

/// The attachment JSON is the one place these structs are written to SQLite as a
/// blob rather than as columns, so it gets its own guard on the stored shape.
#[test]
fn attachment_json_round_trips_through_the_database_encoding() {
    let stored = serde_json::to_string(&vec![Attachment {
        id: "a1".into(),
        path: "C:/x.png".into(),
        mime: "image/png".into(),
        bytes: 9,
        preview: None,
    }])
    .unwrap();
    let back: Vec<Attachment> = serde_json::from_str(&stored).unwrap();
    assert_eq!(back[0].path, "C:/x.png");
}