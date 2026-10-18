use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::models::{CustomProvider, Mode, PermissionMode};
use crate::paths;
use crate::permissions::{self, Policy, Rule};
use crate::providers::{
    self, CompletionRequest, Dialect, Incoming, Providers, RawCompletion, StreamEvent,
};
use crate::settings::Settings;
use crate::tools::{Registry, ToolContext, ToolOutcome};

/// Identity of the model a session runs on. `provider` may be a catalog entry
/// or a user-added one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

/// Everything the harness needs for one turn, assembled by the caller.
pub struct RunConfig {
    pub model: ModelRef,
    pub system: String,
    pub history: Vec<Incoming>,
    pub roots: Vec<std::path::PathBuf>,
    pub session_id: String,
    pub agent_mode: Mode,
    pub permission_mode: PermissionMode,
    pub rules: Vec<Rule>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub reasoning_effort: Option<String>,
    /// Human-readable tool names, used to build the prompt preamble.
    pub tool_names: Vec<String>,
}

/// Sink for everything the harness produces. The Tauri shell implements this by
/// forwarding to the webview as events; tests implement it by collecting.
pub trait Reporter: Send + Sync {
    /// A token of assistant text.
    fn text(&self, session_id: &str, delta: &str);
    /// A token of reasoning text.
    fn reasoning(&self, session_id: &str, delta: &str);
    /// A tool call is about to run.
    fn tool_start(&self, session_id: &str, call_id: &str, name: &str, args: &str);
    /// A tool call finished.
    fn tool_end(&self, session_id: &str, call_id: &str, outcome: &ToolOutcome);
    /// The harness needs the user to approve a call.
    fn needs_approval(&self, session_id: &str, call_id: &str, request: ApprovalRequest);
    /// Progress note, used for the status line.
    fn status(&self, session_id: &str, text: &str);
    /// A fatal or recoverable error surfaced in the transcript.
    fn error(&self, session_id: &str, message: &str);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub call_id: String,
    pub tool: String,
    pub summary: String,
    pub arguments: String,
    /// Set when the tool would change files, so the UI can show a diff preview.
    pub path: Option<String>,
}

/// Answers the approval prompts raised during a run.
#[derive(Debug, Clone)]
pub enum Approval {
    Allow,
    /// Allow this tool for every remaining call in this run.
    AllowAlways,
    Deny { reason: String },
}

/// Approvals are requested through a channel so the harness can await the user
/// without blocking the runtime.
pub struct Approvals {
    rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Approval>>,
    tx: mpsc::UnboundedSender<Approval>,
    /// Answers the user already gave for the rest of this run.
    always: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl Approvals {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            rx: tokio::sync::Mutex::new(rx),
            tx,
            always: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    pub fn handle(&self) -> ApprovalsHandle {
        ApprovalsHandle { tx: self.tx.clone() }
    }

    pub async fn resolve(&self, tool: &str) -> Approval {
        if self.always.lock().map(|s| s.contains(tool)).unwrap_or(false) {
            return Approval::Allow;
        }
        let answer = {
            let mut rx = self.rx.lock().await;
            rx.recv().await
        };
        answer.unwrap_or(Approval::Deny { reason: "run cancelled".into() })
    }

    /// Remembers a tool-level approval for the rest of this run.
    pub fn remember(&self, tool: &str) {
        if let Ok(mut s) = self.always.lock() {
            s.insert(tool.to_string());
        }
    }
}

impl Default for Approvals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ApprovalsHandle {
    tx: mpsc::UnboundedSender<Approval>,
}

impl ApprovalsHandle {
    pub fn allow(&self) {
        let _ = self.tx.send(Approval::Allow);
    }
    pub fn allow_always(&self) {
        let _ = self.tx.send(Approval::AllowAlways);
    }
    pub fn deny(&self, reason: impl Into<String>) {
        let _ = self.tx.send(Approval::Deny { reason: reason.into() });
    }
}

/// What one `run` produced.
pub struct RunOutcome {
    pub completion: RawCompletion,
    /// Number of tool rounds executed.
    pub rounds: usize,
    pub stopped: StopReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished with a normal answer.
    Answer,
    /// A tool call was refused and the run was abandoned.
    Denied,
    /// The caller aborted.
    Cancelled,
}

pub struct Harness {
    pub registry: Registry,
    pub reporter: Arc<dyn Reporter>,
    pub approvals: Arc<Approvals>,
    /// Owns the store, so no database borrow is held across an await point.
    pub providers: Providers,
}

impl Harness {
    pub fn new(
        registry: Registry,
        reporter: Arc<dyn Reporter>,
        approvals: Arc<Approvals>,
        providers: Providers,
    ) -> Self {
        Self { registry, reporter, approvals, providers }
    }

    /// Runs the agent loop until the model stops asking for tools.
    ///
    /// Takes `self` by value: the harness owns its database connection, and a
    /// borrowed `&self` held across awaits would make the whole future
    /// non-`Send`. A harness is built per run, so consuming it costs nothing.
    ///
    /// `history` is extended in place so the caller can persist the final
    /// conversation without re-deriving it.
    pub async fn run(
        self,
        cfg: &RunConfig,
        history: &mut Vec<Incoming>,
        settings: &Settings,
    ) -> Result<RunOutcome> {
        let mut rounds = 0usize;
        let max_rounds = 24;

        loop {
            rounds += 1;
            if rounds > max_rounds {
                self.reporter.error(
                    &cfg.session_id,
                    &format!("stopped after {max_rounds} tool rounds to avoid looping"),
                );
                return Ok(RunOutcome {
                    completion: RawCompletion::default(),
                    rounds,
                    stopped: StopReason::Answer,
                });
            }

            // Everything the provider needs is resolved synchronously so no
            // borrow of the database connection is live across an await.
            let resolved =
                self.providers.resolve(&cfg.model.provider, &cfg.model.model)?;
            let http = self.providers.http().clone();

            let completion = one_completion(
                &self.registry,
                &self.reporter,
                cfg,
                history,
                resolved,
                http,
                settings.local_only,
            )
            .await?;
            history.push(Incoming::Assistant {
                text: completion.text.clone(),
                tool_calls: completion.tool_calls.clone(),
            });

            if completion.tool_calls.is_empty() {
                return Ok(RunOutcome {
                    completion,
                    rounds,
                    stopped: StopReason::Answer,
                });
            }

            let mut denied = false;
            for call in &completion.tool_calls {
                let (content, is_error) = match execute(
                    &self.registry,
                    &self.reporter,
                    &self.approvals,
                    cfg,
                    call,
                )
                .await
                {
                    Ok(o) => {
                        self.reporter.tool_end(&cfg.session_id, &call.id, &o);
                        (o.content, o.is_error)
                    }
                    Err(e) => {
                        // A refusal is fed back as guidance rather than treated as
                        // a crash, so the model can choose a different route.
                        let is_denied = e.kind() == "denied";
                        let o = ToolOutcome::error(e.to_string());
                        self.reporter.tool_end(&cfg.session_id, &call.id, &o);
                        if is_denied {
                            denied = true;
                        }
                        (e.to_string(), true)
                    }
                };
                history.push(Incoming::ToolResult {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    content,
                    is_error,
                });
            }

            if denied {
                return Ok(RunOutcome { completion, rounds, stopped: StopReason::Denied });
            }
        }
    }

}
/// Issues one completion and streams it to the reporter.
///
/// A free function rather than a method so that no borrow of the harness, and
/// therefore of its database connection, is live across an await point.
async fn one_completion(
    registry: &Registry,
    reporter: &Arc<dyn Reporter>,
    cfg: &RunConfig,
    history: &[Incoming],
    resolved: providers::Resolved,
    http: reqwest::Client,
    local_only: bool,
) -> Result<RawCompletion> {
    let req = CompletionRequest {
        dialect: resolved.dialect,
        base_url: resolved.base_url,
        model: cfg.model.model.clone(),
        api_key: resolved.api_key,
        extra_headers: resolved.headers,
        system: cfg.system.clone(),
        messages: history.to_vec(),
        tools: registry.schemas(),
        temperature: cfg.temperature,
        max_tokens: cfg.max_tokens,
        reasoning_effort: cfg.reasoning_effort.clone(),
        no_timeout: local_only,
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    // Forward deltas to the UI while the adapter assembles the result.
    let forward = {
        let reporter = Arc::clone(reporter);
        let sid = cfg.session_id.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                match ev {
                    StreamEvent::TextDelta { text } => reporter.text(&sid, &text),
                    StreamEvent::ReasoningDelta { text } => reporter.reasoning(&sid, &text),
                    _ => {}
                }
            }
        })
    };

let result = match req.dialect {
        Dialect::Anthropic => providers::anthropic::stream(&req, tx.clone(), http).await,
        Dialect::Gemini => providers::gemini::stream(&req, tx.clone(), http).await,
        Dialect::OpenAi => providers::openai::stream(&req, tx.clone(), http).await,
    };
    // The adapter has returned and dropped its clone, so dropping ours closes
    // the channel. Awaiting the forwarder lets it deliver whatever is still
    // buffered: aborting here would discard the tail of the response on a
    // single-threaded runtime, where the forwarder may not have been polled yet.
    drop(tx);
    let _ = forward.await;

    let completion = result?;
    // Some local servers ignore max_tokens entirely; trim so one runaway
    // response cannot blow past the context budget.
    Ok(cap_completion(completion))
}

/// Runs one tool call behind the permission gate.
async fn execute(
    registry: &Registry,
    reporter: &Arc<dyn Reporter>,
    approvals: &Approvals,
    cfg: &RunConfig,
    call: &providers::ProposedToolCall,
) -> Result<ToolOutcome> {
    let args: serde_json::Value = serde_json::from_str(&call.arguments)
        .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));

    let Some(tool) = registry.get(&call.name) else {
        return Err(Error::invalid(format!(
            "`{}` is not a tool in this session. Available: {}",
            call.name,
            registry.names().join(", ")
        )));
    };

    reporter.tool_start(&cfg.session_id, &call.id, &call.name, &call.arguments);

    let policy = Policy {
        mode: cfg.permission_mode,
        agent_mode: cfg.agent_mode,
        rules: cfg.rules.clone(),
    };

    match policy.evaluate(&call.name, Some(&call.arguments)) {
        permissions::Decision::Deny => {
            return Err(Error::Denied(format!(
                "`{}` is not permitted in this session",
                call.name
            )));
        }
        permissions::Decision::Ask => {
            let request = ApprovalRequest {
                call_id: call.id.clone(),
                tool: call.name.clone(),
                summary: tool.summary(&args),
                arguments: call.arguments.clone(),
                path: args.get("path").and_then(|v| v.as_str()).map(str::to_string),
            };
            reporter.needs_approval(&cfg.session_id, &call.id, request);
            match approvals.resolve(&call.name).await {
                Approval::Allow => {}
                Approval::AllowAlways => approvals.remember(&call.name),
                Approval::Deny { reason } => return Err(Error::Denied(reason)),
            }
        }
        permissions::Decision::Allow => {}
    }

    let mut ctx = ToolContext::new(cfg.roots.clone(), &cfg.session_id);
    ctx.agent_mode = cfg.agent_mode;
tool.run(&ctx, args).await
}

/// Hard ceiling on a single response, independent of what the provider does.
const MAX_RESPONSE_CHARS: usize = 400_000;

fn cap_completion(mut c: RawCompletion) -> RawCompletion {
    if c.text.len() > MAX_RESPONSE_CHARS {
        c.text.truncate(MAX_RESPONSE_CHARS);
        c.text.push_str("\n\n[response truncated]");
    }
    c
}

/// Builds the system prompt: persona, workspace facts and tool policy.
pub fn system_prompt(
    workspace_summary: &str,
    instructions: &str,
    agent_mode: Mode,
    tool_names: &[String],
    local_only: bool,
) -> String {
    let mut s = String::new();
    s.push_str(
        "You are Octopus, a coding agent working inside the user's own machine. \
         You are precise, you verify your work, and you never claim to have done \
         something you did not do.\n\n",
    );

    if !workspace_summary.trim().is_empty() {
        s.push_str("## Workspace\n");
        s.push_str(workspace_summary.trim());
        s.push_str("\n\n");
    }

    if !instructions.trim().is_empty() {
        s.push_str("## Project instructions\n");
        s.push_str(instructions.trim());
        s.push_str("\n\n");
    }

    s.push_str("## How to work\n");
    s.push_str(
        "- Look before you edit. Read the files you intend to change.\n\
         - Prefer the smallest change that solves the problem.\n\
         - When a task has three or more steps, track it with the todo tools and \
           keep exactly one item in progress.\n\
         - Run the project's own tests or build after changing code, and report the \
           real result including failures.\n\
         - If you are blocked or a request is ambiguous, say so instead of guessing.\n\n",
    );

    if agent_mode == Mode::Plan {
        s.push_str(
            "## Current mode: Plan\n\
             You are in Plan mode. You may read, search and inspect, but you must not \
             modify files or run commands. Produce a concrete plan and stop. The user \
             will switch to Build mode to apply it.\n\n",
        );
    } else {
        s.push_str("## Current mode: Build\nYou may modify files and run commands.\n\n");
    }

    if local_only {
        s.push_str(
            "This session is restricted to local models and on-device endpoints. Do not \
             suggest fetching remote resources.\n\n",
        );
    }

    if !tool_names.is_empty() {
        s.push_str("## Tools\n");
        s.push_str(&tool_names.join(", "));
        s.push('\n');
    }
    s
}

/// Writes a default AGENTS.md by inspecting the project, mirroring `/init` in
/// other coding agents.
pub fn scaffold_instructions(roots: &[std::path::PathBuf]) -> String {
    let mut out = String::from("# Project instructions\n\n");
    let mut found_any = false;

    for root in roots {
        let manifest = [
            "package.json", "Cargo.toml", "pyproject.toml", "go.mod", "pom.xml",
            "build.gradle.kts", "composer.json", "Gemfile", "CMakeLists.txt",
        ]
        .iter()
        .map(|m| root.join(m))
        .find(|p| p.is_file());

        if let Some(p) = manifest {
            found_any = true;
            out.push_str(&format!(
                "- {} present; match its existing conventions and scripts.\n",
                p.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        for marker in [".editorconfig", "tsconfig.json", "rustfmt.toml", ".prettierrc"] {
            if root.join(marker).is_file() {
                found_any = true;
                out.push_str(&format!("- {marker} present; follow its formatting rules.\n"));
            }
        }
    }

    if !found_any {
        out.push_str("- No build manifest was detected; inspect the tree before assuming a stack.\n");
    }
    out.push_str("\n<!-- Octopus Chat reads this file on every request in this workspace. -->\n");
    out
}

/// Placeholder reporter used before the UI is ready; drops everything.
pub struct SilentReporter;

impl Reporter for SilentReporter {
    fn text(&self, _s: &str, _d: &str) {}
    fn reasoning(&self, _s: &str, _d: &str) {}
    fn tool_start(&self, _s: &str, _c: &str, _n: &str, _a: &str) {}
    fn tool_end(&self, _s: &str, _c: &str, _o: &ToolOutcome) {}
    fn needs_approval(&self, _s: &str, _c: &str, _r: ApprovalRequest) {}
    fn status(&self, _s: &str, _t: &str) {}
    fn error(&self, _s: &str, _m: &str) {}
}

/// Loads the workspace description injected into the system prompt.
pub fn describe_workspace(roots: &[std::path::PathBuf]) -> String {
    let mut lines = Vec::new();
    for r in roots {
        lines.push(format!("- {}", paths::display_path(r)));
    }
    lines.join("\n")
}

/// Exposed so the settings screen can validate a user-entered provider before
/// it is saved.
pub fn validate_custom(p: &CustomProvider) -> Result<()> {
    if p.base_url.trim().is_empty() {
        return Err(Error::invalid("base URL is required"));
    }
    if !(p.base_url.starts_with("http://") || p.base_url.starts_with("https://")) {
        return Err(Error::invalid("base URL must start with http:// or https://"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_mode_prompt_advertises_the_right_mode() {
        let build = system_prompt("", "", Mode::Build, &[], false);
        assert!(build.contains("Current mode: Build"));
        assert!(!build.contains("must not"));

        let plan = system_prompt("", "", Mode::Plan, &[], false);
        assert!(plan.contains("Current mode: Plan"));
        assert!(plan.contains("must not"));
    }

    #[test]
    fn prompt_includes_workspace_and_instructions() {
        let s = system_prompt("- ~/work/app", "Always run tests.", Mode::Build, &[], false);
        assert!(s.contains("~/work/app"));
        assert!(s.contains("Always run tests."));
    }

    #[test]
    fn local_only_is_called_out() {
        assert!(system_prompt("", "", Mode::Build, &[], true).contains("restricted to local models"));
    }

    #[test]
    fn tool_names_are_listed() {
        let s = system_prompt(
            "",
            "",
            Mode::Build,
            &["read".into(), "edit".into()],
            false,
        );
        assert!(s.contains("## Tools"));
        assert!(s.contains("read, edit"));
    }

    #[tokio::test]
    async fn approvals_default_to_allow() {
        let a = Approvals::new();
        let h = a.handle();
        h.allow();
        assert!(matches!(a.resolve("bash").await, Approval::Allow));
    }

    #[tokio::test]
    async fn allow_always_covers_later_calls() {
        let a = Approvals::new();
        let h = a.handle();
        h.allow_always();
        assert!(matches!(a.resolve("bash").await, Approval::AllowAlways));
        // The harness records the tool-level approval, after which no further
        // answers are sent and resolve must not block.
        a.remember("bash");
        assert!(matches!(a.resolve("bash").await, Approval::Allow));
    }

    #[tokio::test]
    async fn denial_is_reported_verbatim() {
        let a = Approvals::new();
        let h = a.handle();
        h.deny("not this time");
        match a.resolve("write").await {
            Approval::Deny { reason } => assert_eq!(reason, "not this time"),
            other => panic!("expected a denial, got {other:?}"),
        }
    }

#[test]
    fn responses_are_capped() {
        let long = "x".repeat(MAX_RESPONSE_CHARS + 10);
        let c = cap_completion(RawCompletion { text: long, ..Default::default() });
        assert!(c.text.ends_with("[response truncated]"));
        assert!(c.text.len() <= MAX_RESPONSE_CHARS + 64);
    }

    #[test]
    fn custom_providers_are_validated() {
        let mut p = CustomProvider {
            id: "x".into(),
            label: "X".into(),
            dialect: Dialect::OpenAi,
            base_url: String::new(),
            local: true,
            api_key: None,
            headers: vec![],
            models: vec![],
        };
        assert!(validate_custom(&p).is_err());
        p.base_url = "ftp://nope".into();
        assert!(validate_custom(&p).is_err());
        p.base_url = "http://127.0.0.1:1234/v1".into();
        assert!(validate_custom(&p).is_ok());
    }
}
