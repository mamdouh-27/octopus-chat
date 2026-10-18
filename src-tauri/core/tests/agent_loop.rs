//! End-to-end tests for the agent loop against a mock OpenAI-compatible
//! endpoint. These cover the parts unit tests cannot: SSE parsing, the tool
//! round-trip, permission gating and streaming delivery.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use octopus_core::agent::{
    Approvals, Harness, ModelRef, Reporter, RunConfig, RunOutcome, StopReason,
};
use octopus_core::db::Store;
use octopus_core::error::Result;
use octopus_core::models::{CustomModel, CustomProvider, Dialect, Mode, PermissionMode};
use octopus_core::permissions::Rule;
use octopus_core::providers::{Providers, Usage};
use octopus_core::settings::Settings;
use octopus_core::tools::{Registry, ToolOutcome};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A recorded SSE frame the mock server should send on the next request.
type Script = Vec<String>;

/// Minimal single-connection HTTP server that replies with a canned SSE body.
struct Mock {
    port: u16,
    calls: Arc<AtomicUsize>,
    received: Arc<Mutex<String>>,
}

impl Mock {
    async fn start(scripts: Vec<Script>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let scripts = Arc::new(Mutex::new(scripts.into_iter()));
        let calls = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(String::new()));

        let c = calls.clone();
        let r = received.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let script = scripts.lock().unwrap().next();
                let r = r.clone();
                let c = c.clone();
tokio::spawn(async move {
                    let Some(script) = script else { return };
                    // The request must be read to completion before replying:
                    // closing the socket early resets the connection and the
                    // client reports "error sending request" instead of a
                    // protocol error.
                    let buf = read_request(&mut socket).await;
                    *r.lock().unwrap() = String::from_utf8_lossy(&buf).into_owned();
                    c.fetch_add(1, Ordering::SeqCst);

                    let mut body = String::new();
                    for frame in script {
                        body.push_str(&format!("data: {frame}\n\n"));
                    }
                    body.push_str("data: [DONE]\n\n");

                    let head = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: text/event-stream\r\n\
                         Cache-Control: no-cache\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(body.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });

        Ok(Self { port, calls, received })
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    fn request_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn last_request(&self) -> String {
        self.received.lock().unwrap().clone()
    }
}

/// Collects everything the harness reports so assertions can inspect it.
#[derive(Default)]
struct Recorder {
    text: Mutex<String>,
    reasoning: Mutex<String>,
    tools: Mutex<Vec<(String, bool)>>,
    approvals: Mutex<Vec<String>>,
    errors: Mutex<Vec<String>>,
}

impl Recorder {
    fn snapshot(&self) -> (String, String, Vec<(String, bool)>, Vec<String>) {
        (
            self.text.lock().unwrap().clone(),
            self.reasoning.lock().unwrap().clone(),
            self.tools.lock().unwrap().clone(),
            self.approvals.lock().unwrap().clone(),
        )
    }
}

impl Reporter for Recorder {
    fn text(&self, _s: &str, d: &str) {
        self.text.lock().unwrap().push_str(d);
    }
    fn reasoning(&self, _s: &str, d: &str) {
        self.reasoning.lock().unwrap().push_str(d);
    }
    fn tool_start(&self, _s: &str, _c: &str, _n: &str, _a: &str) {}
    fn tool_end(&self, _s: &str, call_id: &str, o: &ToolOutcome) {
        self.tools.lock().unwrap().push((call_id.to_string(), o.is_error));
    }
    fn needs_approval(&self, _s: &str, call_id: &str, _r: octopus_core::agent::ApprovalRequest) {
        self.approvals.lock().unwrap().push(call_id.to_string());
    }
    fn status(&self, _s: &str, _t: &str) {}
    fn error(&self, _s: &str, m: &str) {
        self.errors.lock().unwrap().push(m.to_string());
    }
}

/// A throwaway store in a temp directory.
struct TempStore {
    // Held so the directory outlives the database file.
    _dir: tempdir::TempDir,
    path: std::path::PathBuf,
}

impl TempStore {
    fn new() -> Result<Self> {
        let dir = tempdir::TempDir::new("octopus-test")?;
        let path = dir.path().join("t.db");
        Store::new(octopus_core::db::open(&path)?);
        Ok(Self { _dir: dir, path })
    }

    /// The harness owns its store, so each caller opens its own connection.
    fn store(&self) -> Result<Store> {
        Ok(Store::new(octopus_core::db::open(&self.path)?))
    }
}

/// Registers the mock as a custom provider and returns its id.
fn register(db: &std::path::Path, base_url: &str) -> Result<String> {
    let store = Store::new(octopus_core::db::open(db)?);
    let p = CustomProvider {
        id: "mock".into(),
        label: "Mock".into(),
        dialect: Dialect::OpenAi,
        base_url: base_url.to_string(),
        local: true,
        api_key: None,
        headers: vec![],
        models: vec![CustomModel {
            id: "mock-1".into(),
            label: "Mock 1".into(),
            dialect: Dialect::OpenAi,
            context_window: Some(128_000),
            max_output: Some(8_192),
            supports_tools: true,
            supports_vision: false,
            reasoning: false,
            input_cost: None,
            output_cost: None,
        }],
    };
let saved = Providers::new(store, reqwest::Client::new()).save_custom_provider(p)?;
    Ok(saved.id)
}

fn config(provider: &str, roots: Vec<std::path::PathBuf>, mode: Mode) -> RunConfig {
    RunConfig {
        model: ModelRef { provider: provider.into(), model: "mock-1".into() },
        system: "You are Octopus.".into(),
        history: Vec::new(),
        roots,
        session_id: "sess-test".into(),
        agent_mode: mode,
        permission_mode: PermissionMode::FullAccess,
        rules: Vec::new(),
        temperature: None,
        max_tokens: Some(1024),
        reasoning_effort: None,
        tool_names: Vec::new(),
    }
}

/// Reads one HTTP request: the head, then exactly `Content-Length` body bytes.
async fn read_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];

    let head_end = loop {
        let Ok(n) = socket.read(&mut tmp).await else { return buf };
        if n == 0 {
            return buf;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(idx) = find(&buf, b"\r\n\r\n") {
            break idx + 4;
        }
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let len: usize = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);

    while buf.len() < head_end + len {
        let Ok(n) = socket.read(&mut tmp).await else { break };
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    buf
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn frame(delta: serde_json::Value, finish: Option<&str>) -> String {
    let mut choice = serde_json::json!({ "delta": delta, "index": 0 });
    if let Some(f) = finish {
        choice["finish_reason"] = serde_json::json!(f);
    }
    serde_json::json!({ "choices": [choice] }).to_string()
}

// ------------------------------------------------------------------- tests

#[tokio::test]
async fn streams_text_and_terminates() -> Result<()> {
    let mock = Mock::start(vec![vec![
        frame(json_delta("Hello "), None),
        frame(json_delta("world"), None),
        frame(json!({}), Some("stop")),
    ]])
    .await?;
    let ts = TempStore::new()?;
    let provider = register(&ts.path, &mock.base_url())?;

    let rec = Arc::new(Recorder::default());
    let approvals = Arc::new(Approvals::new());
    let harness = Harness::new(
        Registry::full(vec![]),
        rec.clone(),
        approvals,
        Providers::new(ts.store()?, reqwest::Client::new()),
    );

    let mut history = Vec::new();
    let mut cfg = config(&provider, vec![], Mode::Build);
    cfg.history = vec![octopus_core::providers::Incoming::User {
        text: "hi".into(),
        images: vec![],
    }];
    let out: RunOutcome = harness.run(&cfg, &mut history, &Settings::default()).await?;

    let (text, _, tools, _) = rec.snapshot();
    assert_eq!(text, "Hello world", "both deltas should reach the UI");
    assert_eq!(out.stopped, StopReason::Answer);
    assert!(tools.is_empty());
    assert_eq!(mock.request_count(), 1);
    Ok(())
}

fn json_delta(text: &str) -> serde_json::Value {
    serde_json::json!({ "content": text })
}

#[tokio::test]
async fn runs_a_tool_and_feeds_the_result_back() -> Result<()> {
    let dir = tempdir::TempDir::new("octopus-tool")?;
    let file = dir.path().join("hello.txt");
    std::fs::write(&file, "alpha\nbeta\n")?;

    // First turn asks for a tool call, second turn answers using the result.
    let mock = Mock::start(vec![
        vec![
            frame(
                json!({
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": { "name": "read", "arguments": "" }
                    }]
                }),
                None,
            ),
            // Arguments stream in fragments, as they do in practice.
            frame(
                json!({ "tool_calls": [{ "index": 0, "function": { "arguments": "{\"path\":" } }] }),
                None,
            ),
            frame(
                json!({ "tool_calls": [{ "index": 0, "function": { "arguments": "\"hello.txt\"}" } }] }),
                Some("tool_calls"),
            ),
        ],
        vec![frame(json_delta("It contains alpha and beta."), Some("stop"))],
    ])
    .await?;

    let ts = TempStore::new()?;
    let provider = register(&ts.path, &mock.base_url())?;
    let rec = Arc::new(Recorder::default());
    let harness = Harness::new(
        Registry::full(vec![]),
        rec.clone(),
        Arc::new(Approvals::new()),
        Providers::new(ts.store()?, reqwest::Client::new()),
    );

    let mut history = Vec::new();
    let mut cfg = config(&provider, vec![dir.path().to_path_buf()], Mode::Build);
    cfg.history = vec![octopus_core::providers::Incoming::User {
        text: "what is in hello.txt?".into(),
        images: vec![],
    }];

    let out = harness.run(&cfg, &mut history, &Settings::default()).await?;

    assert_eq!(mock.request_count(), 2, "the tool result should trigger a second turn");
    let (_, _, tools, _) = rec.snapshot();
    assert_eq!(tools.len(), 1);
    assert!(!tools[0].1, "the read should not be an error");

    // The second request must carry the tool result back to the model.
    let second = mock.last_request();
    assert!(second.contains("\"role\":\"tool\""), "tool result missing:\n{second}");
    assert!(second.contains("alpha"), "file content missing from context");

    // And the loop must have appended the assistant + tool turns to history.
    assert!(history.len() >= 3, "history should hold user, assistant and tool turns");
    assert_eq!(out.rounds, 2);
    Ok(())
}

#[tokio::test]
async fn asks_before_a_mutating_tool_and_runs_on_approval() -> Result<()> {
    let dir = tempdir::TempDir::new("octopus-approve")?;
    let mock = Mock::start(vec![
        vec![frame(
            json!({
                "tool_calls": [{
                    "index": 0,
                    "id": "call_1",
                    "function": {
                        "name": "write",
                        "arguments": "{\"path\":\"new.txt\",\"content\":\"written\"}"
                    }
                }]
            }),
            Some("tool_calls"),
        )],
        vec![frame(json_delta("Done."), Some("stop"))],
    ])
    .await?;

    let ts = TempStore::new()?;
    let provider = register(&ts.path, &mock.base_url())?;
    let rec = Arc::new(Recorder::default());
    let approvals = Arc::new(Approvals::new());

    // Answer the prompt from another task, as the UI would.
    let handle = approvals.handle();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        handle.allow();
    });

    let harness = Harness::new(
        Registry::full(vec![]),
        rec.clone(),
        approvals,
        Providers::new(ts.store()?, reqwest::Client::new()),
    );

    let mut history = Vec::new();
    let mut cfg = config(&provider, vec![dir.path().to_path_buf()], Mode::Build);
    cfg.permission_mode = PermissionMode::Ask;
    cfg.history = vec![octopus_core::providers::Incoming::User {
        text: "write new.txt".into(),
        images: vec![],
    }];

    harness.run(&cfg, &mut history, &Settings::default()).await?;

    let (_, _, _, asked) = rec.snapshot();
    assert_eq!(asked.len(), 1, "the user should have been asked once");
    assert!(
        dir.path().join("new.txt").exists(),
        "the file should exist after approval"
    );
    Ok(())
}

#[tokio::test]
async fn a_denied_tool_stops_the_run_and_explains_itself() -> Result<()> {
    let dir = tempdir::TempDir::new("octopus-deny")?;
    let mock = Mock::start(vec![vec![frame(
        json!({
            "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "function": { "name": "write", "arguments": "{\"path\":\"x.txt\",\"content\":\"y\"}" }
            }]
        }),
        Some("tool_calls"),
    )]])
    .await?;

    let ts = TempStore::new()?;
    let provider = register(&ts.path, &mock.base_url())?;
    let rec = Arc::new(Recorder::default());
    let approvals = Arc::new(Approvals::new());
    let handle = approvals.handle();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        handle.deny("not this time");
    });

    let harness = Harness::new(
        Registry::full(vec![]),
        rec.clone(),
        approvals,
        Providers::new(ts.store()?, reqwest::Client::new()),
    );

    let mut history = Vec::new();
    let mut cfg = config(&provider, vec![dir.path().to_path_buf()], Mode::Build);
    cfg.permission_mode = PermissionMode::Ask;
    cfg.history = vec![octopus_core::providers::Incoming::User {
        text: "write x.txt".into(),
        images: vec![],
    }];

    let out = harness.run(&cfg, &mut history, &Settings::default()).await?;

    assert_eq!(out.stopped, StopReason::Denied);
    assert!(!dir.path().join("x.txt").exists(), "nothing should be written after a denial");
    let (_, _, tools, _) = rec.snapshot();
    assert!(tools[0].1, "the denied tool should be reported as an error");
    Ok(())
}

#[tokio::test]
async fn plan_mode_refuses_to_mutate_without_asking() -> Result<()> {
    let dir = tempdir::TempDir::new("octopus-plan")?;
    let mock = Mock::start(vec![vec![frame(
        json!({
            "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "function": { "name": "write", "arguments": "{\"path\":\"p.txt\",\"content\":\"y\"}" }
            }]
        }),
        Some("tool_calls"),
    )]])
    .await?;

    let ts = TempStore::new()?;
    let provider = register(&ts.path, &mock.base_url())?;
    let rec = Arc::new(Recorder::default());
    let harness = Harness::new(
        Registry::full(vec![]),
        rec.clone(),
        Arc::new(Approvals::new()),
        Providers::new(ts.store()?, reqwest::Client::new()),
    );

    let mut history = Vec::new();
    // FullAccess in Plan mode: the read-only guarantee must still hold.
    let cfg = config(&provider, vec![dir.path().to_path_buf()], Mode::Plan);
    let out = harness.run(&cfg, &mut history, &Settings::default()).await?;

    assert!(!dir.path().join("p.txt").exists(), "plan mode must not write");
    let (_, _, _, asked) = rec.snapshot();
    assert!(asked.is_empty(), "plan mode should deny outright, not ask");
    assert_eq!(out.stopped, StopReason::Denied);
    Ok(())
}

#[tokio::test]
async fn a_deny_rule_beats_full_access() -> Result<()> {
    let dir = tempdir::TempDir::new("octopus-rule")?;
    let mock = Mock::start(vec![vec![frame(
        json!({
            "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "function": { "name": "bash", "arguments": "{\"command\":\"rm -rf /\"}" }
            }]
        }),
        Some("tool_calls"),
    )]])
    .await?;

    let ts = TempStore::new()?;
    let provider = register(&ts.path, &mock.base_url())?;
    let rec = Arc::new(Recorder::default());
    let harness = Harness::new(
        Registry::full(vec![]),
        rec.clone(),
        Arc::new(Approvals::new()),
        Providers::new(ts.store()?, reqwest::Client::new()),
    );

    let mut history = Vec::new();
    let mut cfg = config(&provider, vec![dir.path().to_path_buf()], Mode::Build);
    cfg.rules = vec![Rule {
        id: "r1".into(),
        effect: octopus_core::permissions::Effect::Deny,
        pattern: "bash".into(),
        ask_first: false,
        project_id: None,
        enabled: true,
    }];

    harness.run(&cfg, &mut history, &Settings::default()).await?;
    let (_, _, _, asked) = rec.snapshot();
    assert!(asked.is_empty(), "a deny rule should not prompt");
    Ok(())
}

#[tokio::test]
async fn paths_outside_the_workspace_are_refused() -> Result<()> {
    let dir = tempdir::TempDir::new("octopus-escape")?;
    let mock = Mock::start(vec![vec![frame(
        json!({
            "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "function": { "name": "read", "arguments": "{\"path\":\"../../../etc/passwd\"}" }
            }]
        }),
        Some("tool_calls"),
    )]])
    .await?;

    let ts = TempStore::new()?;
    let provider = register(&ts.path, &mock.base_url())?;
    let rec = Arc::new(Recorder::default());
    let harness = Harness::new(
        Registry::full(vec![]),
        rec.clone(),
        Arc::new(Approvals::new()),
        Providers::new(ts.store()?, reqwest::Client::new()),
    );

    let mut history = Vec::new();
    let cfg = config(&provider, vec![dir.path().to_path_buf()], Mode::Build);
    harness.run(&cfg, &mut history, &Settings::default()).await?;

    let (_, _, tools, _) = rec.snapshot();
    assert!(tools[0].1, "the escaping read should fail");
    Ok(())
}

#[tokio::test]
async fn usage_is_reported_back() -> Result<()> {
    let mock = Mock::start(vec![vec![
        format!(
            r#"{{"choices":[{{"delta":{{"content":"ok"}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":120,"completion_tokens":45}}}}"#
        )
    ]])
    .await?;
    let ts = TempStore::new()?;
    let provider = register(&ts.path, &mock.base_url())?;

    let rec = Arc::new(Recorder::default());
    let harness = Harness::new(
        Registry::full(vec![]),
        rec,
        Arc::new(Approvals::new()),
        Providers::new(ts.store()?, reqwest::Client::new()),
    );

    let mut history = Vec::new();
    let mut cfg = config(&provider, vec![], Mode::Build);
    cfg.history = vec![octopus_core::providers::Incoming::User {
        text: "hi".into(),
        images: vec![],
    }];
    let out = harness.run(&cfg, &mut history, &Settings::default()).await?;

    let u = out.completion.usage;
    assert_eq!((u.input_tokens, u.output_tokens, u.cached_input_tokens), (120, 45, 0));
    Ok(())
}

#[tokio::test]
async fn local_models_cost_nothing() {
    let u = Usage { input_tokens: 1_000_000, output_tokens: 1_000_000, cached_input_tokens: 0 };
    assert_eq!(u.cost_usd(None, None), 0.0);
}
