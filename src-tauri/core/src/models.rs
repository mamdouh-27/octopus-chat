//! Wire and storage types shared by the agent, the store, and the IPC bridge.
//!
//! Every struct here is part of the Tauri IPC contract, so each one carries
//! `#[serde(rename_all = "camelCase")]`. The TypeScript side sends camelCase
//! (`permissionMode`, `tokensIn`); without the rename, Serde expects the
//! snake_case Rust spelling and rejects the whole payload with
//! `missing field ...`. That failure is invisible from the UI, which is how
//! project creation managed to be broken for so long: the request looked fine
//! and the backend simply refused it. `tests/ipc_contract.rs` pins the shape.

use serde::{Deserialize, Serialize};

pub type Millis = i64;

pub fn now_ms() -> Millis {
    chrono::Utc::now().timestamp_millis()
}

/// Which half of the agent loop is active. `Plan` may read and search but is not
/// allowed to mutate the workspace; `Build` may do both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Plan,
    Build,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Self::Build
    }
}

/// Boundary for what the agent may do without stopping to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Every tool call is proposed to the user first.
    Ask,
    /// Reads run freely; writes are auto-reviewed by a checker and only
    /// escalated to the user when the review is unsure.
    AutoReview,
    /// No prompts. Only meaningful inside a sandbox.
    FullAccess,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::AutoReview => "auto_review",
            Self::FullAccess => "full_access",
        }
    }
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Ask
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Idle,
    Running,
    WaitingApproval,
    Paused,
    Done,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Folders the agent may touch. More than one makes it a multi-root project.
    pub roots: Vec<String>,
    pub instructions: String,
    pub permission_mode: PermissionMode,
    pub accent: Option<String>,
    pub archived: bool,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub mode: Mode,
    pub state: RunState,
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub permission_mode: PermissionMode,
    pub goal: Option<String>,
    /// Set when this session was produced by rewinding an earlier message.
    pub forked_from: Option<String>,
    /// Isolated checkout for parallel agents; empty when it shares the project.
    pub worktree: Option<String>,
    pub favorite: bool,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
    pub created_at: Millis,
    pub updated_at: Millis,
}

impl Session {
    pub fn new(provider: String, model: String) -> Self {
        let now = now_ms();
        Self {
            id: crate::util::new_id("ses"),
            project_id: None,
            title: "New chat".into(),
            mode: Mode::Build,
            state: RunState::Idle,
            provider,
            model,
            reasoning_effort: None,
            temperature: None,
            max_tokens: None,
            permission_mode: PermissionMode::Ask,
            goal: None,
            forked_from: None,
            worktree: None,
            favorite: false,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageState {
    Pending,
    Streaming,
    Done,
    Error,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: Role,
    pub content: String,
    /// Model chain-of-thought summary, shown collapsed when present.
    pub reasoning: Option<String>,
    pub state: MessageState,
    pub model: Option<String>,
    pub error: Option<String>,
    pub tokens_in: i64,
    pub tokens_out: i64,
    /// When this message replaced an earlier one through fork & rewind.
    pub supersedes: Option<String>,
    pub attachments: Vec<Attachment>,
    pub created_at: Millis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub path: String,
    pub mime: String,
    pub bytes: u64,
    /// Data URL for images so the UI can render without filesystem access.
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolState {
    Pending,
    Running,
    Done,
    Error,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub name: String,
    /// Pretty-printed JSON arguments as the model produced them.
    pub input: String,
    pub output: Option<String>,
    pub state: ToolState,
    pub denied_reason: Option<String>,
    /// Unified diff for mutating tools, rendered in the transcript.
    pub diff: Option<String>,
    pub started_at: Millis,
    pub ended_at: Option<Millis>,
}

/// Snapshot of the workspace taken before the agent ran a mutating tool, used
/// to rewind. `patch` is a git diff when the project is a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub message_id: Option<String>,
    pub label: String,
    pub git_head: Option<String>,
    pub created_at: Millis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoState {
    Pending,
    Active,
    Done,
}

/// A model the user added by hand: any OpenAI/Anthropic/Gemini-compatible
/// endpoint, or a locally served model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomModel {
    pub id: String,
    pub label: String,
    /// Wire dialect the model speaks.
    pub dialect: Dialect,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    /// Marks models that can call tools; purely chat models are hidden from
    /// agent sessions.
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub reasoning: bool,
    pub input_cost: Option<f64>,
    pub output_cost: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    OpenAi,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProvider {
    pub id: String,
    pub label: String,
    pub dialect: Dialect,
    pub base_url: String,
    /// Never leaves the machine; suppresses "test connection" network notes.
    pub local: bool,
    pub api_key: Option<String>,
    pub headers: Vec<(String, String)>,
    pub models: Vec<CustomModel>,
}
