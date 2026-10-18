use std::path::PathBuf;
use std::sync::Arc;

use octopus_core::agent::{self, Approvals, Harness, ModelRef, Reporter, RunConfig};
use octopus_core::db::Store;
use octopus_core::error::{Error, Result};
use octopus_core::models::*;
use octopus_core::paths;
use octopus_core::permissions::Rule;
use octopus_core::providers::{Incoming, ModelSpec, Providers, ResolvedProvider, Usage};
use octopus_core::settings::Settings;
use octopus_core::tools::Registry;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::runs::{EventReporter, Runs};
use crate::state::{AppState, open_store};

/// Round-trips the IPC bridge.
#[tauri::command]
pub fn ping() -> Result<String> {
    Ok(format!("octopus-chat {}", env!("CARGO_PKG_VERSION")))
}

// ------------------------------------------------------------ folder picking

/// Opens the native folder picker and returns the chosen directories.
///
/// This deliberately calls `rfd` from Rust rather than going through
/// `tauri-plugin-dialog`. The plugin route depends on a webview capability
/// (`dialog:allow-open`); when that grant is missing or stale the JS call
/// rejects and the user is left with a button that silently does nothing.
/// Running the dialog on the Rust side has no capability dependency at all.
///
/// `rfd` blocks, so it runs on a blocking thread rather than the async runtime.
#[tauri::command]
pub async fn pick_project_dirs() -> Result<Option<Vec<String>>> {
    let picked = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Choose project folders")
            .set_directory(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .pick_folders()
            .map(|dirs| dirs.into_iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>())
    })
    .await
    .map_err(|e| Error::msg(format!("folder picker failed: {e}")))?;

// Same normalisation the typed path goes through, so a project created by
    // browsing and one created by typing end up with identical roots.
    Ok(picked.map(|dirs| dirs.iter().map(|d| paths::strip_extended_prefix(d)).collect()))
}

/// Validates one candidate project root.
///
/// Returns the canonical path on success. Exists as a command so the frontend can
/// confirm a typed path before enabling the create button, instead of failing
/// later with a less obvious error.
#[tauri::command]
pub fn validate_project_dir(path: String) -> Result<String> {
    if path.trim().is_empty() {
        return Err(Error::invalid("Enter a folder path."));
    }
    paths::canonical_folder(&PathBuf::from(&path))
}
// ------------------------------------------------------------------ settings

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Result<Settings> {
    state.with(|s| Ok(Settings::load(s)))
}

#[tauri::command]
pub fn settings_save(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    let s = settings.normalized();
    state.with(|st| {
        s.save(st)?;
        Ok(s)
    })
}

// ------------------------------------------------------------------ projects

#[tauri::command]
pub fn projects_list(state: State<'_, AppState>, include_archived: bool) -> Result<Vec<Project>> {
    state.with(|s| s.list_projects(include_archived))
}

#[tauri::command]
pub fn project_save(state: State<'_, AppState>, project: Project) -> Result<Project> {
    state.with(|s| {
        let mut p = project;
        p.updated_at = now_ms();
        if p.id.trim().is_empty() {
            p.id = octopus_core::util::new_id("prj");
            p.created_at = now_ms();
        }
        if p.name.trim().is_empty() {
            p.name = p
                .roots
                .first()
                .and_then(|r| PathBuf::from(r).file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "Untitled project".into());
        }
        s.upsert_project(&p)?;
        Ok(p)
    })
}

#[tauri::command]
pub fn project_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    state.with(|s| s.delete_project(&id))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub roots: Vec<String>,
    pub git_branch: Option<String>,
    pub is_repo: bool,
    pub entry_points: Vec<String>,
}

/// Inspects a folder so the UI can show what the agent is about to work on.
#[tauri::command]
pub async fn workspace_inspect(roots: Vec<String>) -> Result<WorkspaceInfo> {
    let mut resolved = Vec::new();
    for r in &roots {
        let p = PathBuf::from(r);
        if !p.is_dir() {
            return Err(Error::invalid(format!("not a folder: {r}")));
        }
        resolved.push(paths::normalize(&p));
    }
    let first = resolved.first().cloned().unwrap_or_default();
    let is_repo = first.join(".git").exists();
    let branch = if is_repo {
        std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&first)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    let mut entry_points = Vec::new();
    for name in [
        "README.md", "AGENTS.md", "package.json", "Cargo.toml", "pyproject.toml", "go.mod",
    ] {
        if first.join(name).is_file() {
            entry_points.push(name.to_string());
        }
    }

    Ok(WorkspaceInfo {
        roots: resolved.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        git_branch: branch,
        is_repo,
        entry_points,
    })
}

/// Writes a starter AGENTS.md after inspecting the project.
#[tauri::command]
pub fn instructions_scaffold(roots: Vec<String>) -> Result<String> {
    let paths: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    Ok(agent::scaffold_instructions(&paths))
}

// ------------------------------------------------------------------ sessions

#[tauri::command]
pub fn sessions_list(
    state: State<'_, AppState>,
    project_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<Session>> {
    state.with(|s| s.list_sessions(project_id.as_deref(), limit.unwrap_or(200)))
}

#[tauri::command]
pub fn session_get(state: State<'_, AppState>, id: String) -> Result<Session> {
    state.with(|s| s.get_session(&id))
}

#[tauri::command]
pub fn session_create(
    state: State<'_, AppState>,
    provider: Option<String>,
    model: Option<String>,
    project_id: Option<String>,
) -> Result<Session> {
    state.with(|s| {
        let settings = Settings::load(s);
        let mut session = Session::new(
            provider.unwrap_or(settings.default_provider),
            model.unwrap_or(settings.default_model),
        );
        session.project_id = project_id;
        session.permission_mode = settings.default_permission_mode;
        s.upsert_session(&session)?;
        Ok(session)
    })
}

#[tauri::command]
pub fn session_update(state: State<'_, AppState>, session: Session) -> Result<Session> {
    state.with(|s| {
        let mut session = session;
        session.updated_at = now_ms();
        s.upsert_session(&session)?;
        Ok(session)
    })
}

#[tauri::command]
pub fn session_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    state.with(|s| s.delete_session(&id))
}

#[tauri::command]
pub fn messages_list(state: State<'_, AppState>, session_id: String) -> Result<Transcript> {
    state.with(|s| {
        Ok(Transcript {
            messages: s.list_messages(&session_id)?,
            tool_calls: s.list_tool_calls(&session_id)?,
            todos: s.list_todos(&session_id)?,
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub todos: Vec<TodoItem>,
}

/// Fork & rewind: restores the workspace to the state it had at `message_id`,
/// drops every later message, and returns the trimmed transcript so the edited
/// prompt can be replayed.
///
/// The restore runs before the messages are cut, so a failed restore leaves the
/// conversation untouched rather than half-rewound.
#[tauri::command]
pub fn session_rewind(
    state: State<'_, AppState>,
    session_id: String,
    message_id: String,
) -> Result<Transcript> {
    state.with(|s| {
        let session = s.get_session(&session_id)?;
        let roots = project_roots(s, session.project_id.as_deref());
        let message = s.get_message(&message_id)?;

        if let Some(root) = roots.first() {
            let dir = PathBuf::from(root);
            if octopus_core::git::is_repo(&dir) {
                let checkpoints = s.list_checkpoints(&session_id)?;
                // Rewind to the newest checkpoint taken at or before this point.
                if let Some(cp) = checkpoints
                    .iter()
                    .filter(|c| c.created_at <= message.created_at)
                    .max_by_key(|c| c.created_at)
                {
                    octopus_core::git::restore_checkpoint(&dir, cp)?;
                } else {
                    octopus_core::git::run_checked(&dir, &["reset", "--hard", "HEAD"])?;
                }
            }
        }

        s.truncate_after(&session_id, &message_id)?;
        s.touch_session(&session_id)?;
        Ok(Transcript {
            messages: s.list_messages(&session_id)?,
            tool_calls: s.list_tool_calls(&session_id)?,
            todos: s.list_todos(&session_id)?,
        })
    })
}

/// Resolves a session's project roots, falling back to the session's worktree.
pub fn project_roots(store: &Store, project_id: Option<&str>) -> Vec<String> {
    project_id
        .and_then(|id| store.get_project(id).ok())
        .map(|p| p.roots)
        .unwrap_or_default()
}

// ----------------------------------------------------------------- providers

#[tauri::command]
pub fn providers_list(state: State<'_, AppState>) -> Result<Vec<ResolvedProvider>> {
    Ok(Providers::new(open_store()?, state.http()).list()?)
}

#[tauri::command]
pub fn provider_set_key(state: State<'_, AppState>, provider: String, key: Option<String>) -> Result<()> {
    Providers::new(open_store()?, state.http()).set_key(&provider, key)
}

#[tauri::command]
pub fn provider_save_custom(state: State<'_, AppState>, provider: CustomProvider) -> Result<CustomProvider> {
    Providers::new(open_store()?, state.http()).save_custom_provider(provider)
}

#[tauri::command]
pub fn provider_delete_custom(state: State<'_, AppState>, id: String) -> Result<()> {
    Providers::new(open_store()?, state.http()).delete_custom_provider(&id)
}

#[tauri::command]
pub fn provider_models(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Vec<ModelSpec>> {
    let all = Providers::new(open_store()?, state.http()).list()?;
    all.into_iter()
        .find(|p| p.id == provider)
        .map(|p| p.models)
        .ok_or_else(|| Error::NotFound(provider))
}

#[tauri::command]
pub async fn provider_probe_models(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Vec<String>> {
    // Resolved to owned data under the lock, then awaited without the store:
    // a `rusqlite::Connection` borrow cannot cross an await point.
    let http = state.http();
    let endpoint = Providers::new(open_store()?, http.clone()).endpoint(&provider)?;
    octopus_core::providers::probe_models(&endpoint, http).await
}

#[tauri::command]
pub async fn provider_ping(state: State<'_, AppState>, provider: String) -> Result<bool> {
    let http = state.http();
    let endpoint = Providers::new(open_store()?, http.clone()).endpoint(&provider)?;
    octopus_core::providers::ping(&endpoint, http).await
}

/// Registers a model the catalog does not know about. This is what makes any
/// model reachable, including ones released after this build.
#[tauri::command]
pub fn model_add(
    state: State<'_, AppState>,
    provider: String,
    model: String,
    label: Option<String>,
) -> Result<CustomModel> {
    Providers::new(open_store()?, state.http()).add_model(&provider, &model, label)
}

// --------------------------------------------------------------------- rules

#[tauri::command]
pub fn rules_list(state: State<'_, AppState>, project_id: Option<String>) -> Result<Vec<Rule>> {
    state.with(|s| Ok(load_rules(s, &rules_key(project_id.as_deref()))))
}

#[tauri::command]
pub fn rules_save(
    state: State<'_, AppState>,
    project_id: Option<String>,
    rules: Vec<Rule>,
) -> Result<Vec<Rule>> {
    state.with(|s| {
        s.kv_set(&rules_key(project_id.as_deref()), &serde_json::to_string(&rules)?)?;
        Ok(rules)
    })
}

/// Reads a rule set from storage, treating unreadable data as "no rules" so a
/// corrupt blob can never block the app from starting.
fn load_rules(store: &Store, key: &str) -> Vec<Rule> {
    store
        .kv_get(key)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<Vec<Rule>>(&raw).ok())
        .unwrap_or_default()
}

fn rules_key(project_id: Option<&str>) -> String {
    match project_id {
        Some(id) => format!("rules:{id}"),
        None => "rules:global".into(),
    }
}

// ---------------------------------------------------------------- conversation

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    pub session_id: String,
    pub text: String,
    /// Absolute paths of images to attach.
    pub images: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAck {
    pub message: Message,
}

/// Appends the user's message and starts an agent run in the background. The
/// answer arrives as `chat:*` events, so this returns as soon as the turn is
/// under way.
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    runs: State<'_, Arc<Runs>>,
    request: SendRequest,
) -> Result<SendAck> {
    if request.text.trim().is_empty() && request.images.is_empty() {
        return Err(Error::invalid("message is empty"));
    }

    let (session, attachments, roots, instructions) = state.with(|s| {
        let session = s.get_session(&request.session_id)?;
        let project = match &session.project_id {
            Some(id) => s.get_project(id).ok(),
            None => None,
        };
        let (roots, instructions) = match &project {
            Some(p) => (
                p.roots.iter().map(PathBuf::from).collect::<Vec<_>>(),
                merge_instructions(&p.instructions, &p.roots),
            ),
            None => (Vec::new(), String::new()),
        };
        let attachments = build_attachments(&request.images)?;
        Ok((session, attachments, roots, instructions))
    })?;

    let user_message = Message {
        id: octopus_core::util::new_id("msg"),
        session_id: request.session_id.clone(),
        role: Role::User,
        content: request.text.clone(),
        reasoning: None,
        state: MessageState::Done,
        model: None,
        error: None,
        tokens_in: 0,
        tokens_out: 0,
        supersedes: None,
        attachments,
        created_at: now_ms(),
    };

    state.with(|s| {
        s.upsert_message(&user_message)?;
        s.touch_session(&session.id)?;
        // First user message names the thread.
        if user_message.content.trim().len() > 12 {
            if let Ok(Some(title)) = s.session_title_from_first_message(&session.id)
                && title != "New chat"
            {
                let mut updated = s.get_session(&session.id)?;
                if updated.title == "New chat" {
                    updated.title = title;
                    s.upsert_session(&updated)?;
                }
            }
        }
        Ok(())
    })?;

    if runs.is_running(&session.id) {
        // The webview queues follow-ups rather than starting a second run, so
        // reaching here means two clients raced. Say so instead of dropping
        // the message on the floor.
        return Err(Error::msg("a run is already in progress for this chat"));
    }

    let approvals = Arc::new(Approvals::new());
    let cancel = tokio_util::sync::CancellationToken::new();
    runs.insert(
        &session.id,
        crate::runs::RunHandle {
            approvals: approvals.handle(),
            cancel: cancel.clone(),
        },
    )
    .map_err(Error::msg)?;

    let reporter: Arc<dyn Reporter> = Arc::new(EventReporter { app: app.clone() });

    let cfg = RunConfig {
        model: ModelRef { provider: session.provider.clone(), model: session.model.clone() },
        system: agent::system_prompt(
            &agent::describe_workspace(&roots),
            &instructions,
            session.mode,
            &[],
            false,
        ),
        history: Vec::new(),
        roots,
        session_id: session.id.clone(),
        agent_mode: session.mode,
        permission_mode: session.permission_mode,
        rules: state.with(|s| {
            let key = rules_key(session.project_id.as_deref());
            Ok(load_rules(s, &key))
        })?,
        temperature: session.temperature,
        max_tokens: session.max_tokens,
        reasoning_effort: session.reasoning_effort.clone(),
        tool_names: Vec::new(),
    };

    let registry = match session.mode {
        Mode::Plan => Registry::read_only(Vec::new()),
        Mode::Build => Registry::full(Vec::new()),
    };

    let runs_registry = runs.inner().clone();
    let session_id = session.id.clone();
    let http = state.http();

    tauri::async_runtime::spawn(async move {
        let result = execute_turn(
            &app, &session_id, cfg, registry, reporter, approvals, cancel, http,
        )
        .await;
        runs_registry.finish(&session_id);
        if let Err(e) = result {
            let _ = app.emit("chat:error", (&session_id, e.to_string()));
        }
    });

    Ok(SendAck { message: user_message })
}

#[allow(clippy::too_many_arguments)]
async fn execute_turn(
    app: &AppHandle,
    session_id: &str,
    mut cfg: RunConfig,
    registry: Registry,
    reporter: Arc<dyn Reporter>,
    approvals: Arc<Approvals>,
    cancel: tokio_util::sync::CancellationToken,
    http: reqwest::Client,
) -> Result<()> {
    let mut history = load_history(app, session_id)?;

    cfg.tool_names = registry.names().iter().map(|s| s.to_string()).collect();

    let assistant_id = octopus_core::util::new_id("msg");
    let mut assistant = Message {
        id: assistant_id.clone(),
        session_id: session_id.to_string(),
        role: Role::Assistant,
        content: String::new(),
        reasoning: None,
        state: MessageState::Streaming,
        model: Some(cfg.model.model.clone()),
        error: None,
        tokens_in: 0,
        tokens_out: 0,
        supersedes: None,
        attachments: Vec::new(),
        created_at: now_ms(),
    };
    persist(app, &assistant)?;

    // The spawned task outlives this command, so it gets its own connection
    // rather than borrowing the window's.
    let store = open_store()?;
    let harness = Harness::new(registry, reporter, approvals, Providers::new(store, http.clone()));
    let settings = {
        let state = app.state::<AppState>();
        state.with(|s| Ok(Settings::load(s)))?
    };

    // Racing the cancel token means abort stops the run promptly even while a
    // provider request is in flight, instead of waiting for it to finish.
    let finished = tokio::select! {
        biased;
        _ = cancel.cancelled() => None,
        result = harness.run(&cfg, &mut history, &settings) => Some(result?),
    };
    let Some(outcome) = finished else {
        // Keep whatever streamed so far, then mark the turn as cut short.
        assistant.content = streamed_so_far(&history);
        assistant.state = MessageState::Aborted;
        persist(app, &assistant)?;
        let _ = app.emit("chat:aborted", (session_id, assistant));
        return Ok(());
    };

    assistant.content = outcome.completion.text.clone();
    assistant.reasoning = outcome.completion.reasoning.clone();
    assistant.state = MessageState::Done;
    assistant.tokens_in = outcome.completion.usage.input_tokens as i64;
    assistant.tokens_out = outcome.completion.usage.output_tokens as i64;
    persist(app, &assistant)?;

    record_usage(app, session_id, &outcome.completion.usage)?;
    let _ = app.emit("chat:done", (session_id, &assistant));
    Ok(())
}

fn load_history(app: &AppHandle, session_id: &str) -> Result<Vec<Incoming>> {
    let state = app.state::<AppState>();
    state.with(|s| {
        let messages = s.list_messages(session_id)?;
        let mut out = Vec::new();
        for m in messages {
            match m.role {
                Role::User => {
                    let images = m
                        .attachments
                        .iter()
                        .filter_map(|a| {
                            let data = a.preview.as_ref()?;
                            let b64 = data.split_once("base64,").map(|(_, b)| b.to_string())?;
                            Some(octopus_core::providers::ImagePart { mime: a.mime.clone(), data_base64: b64 })
                        })
                        .collect();
                    out.push(Incoming::User { text: m.content, images });
                }
                Role::Assistant => {
                    // Assistant turns are rebuilt from the tool-call log so the
                    // model sees the same tool results it produced last time.
                    out.push(Incoming::Assistant {
                        text: m.content,
                        tool_calls: s
                            .list_tool_calls(session_id)?
                            .into_iter()
                            .filter(|t| t.message_id == m.id)
                            .map(|t| octopus_core::providers::ProposedToolCall {
                                id: t.id,
                                name: t.name,
                                arguments: t.input,
                            })
                            .collect(),
                    });
                    for t in s.list_tool_calls(session_id)?.into_iter().filter(|t| t.message_id == m.id) {
                        if let Some(o) = t.output {
                            out.push(Incoming::ToolResult {
                                call_id: t.id,
                                name: t.name,
                                content: o,
                                is_error: t.state == ToolState::Error,
                            });
                        }
                    }
                }
                Role::Tool => {}
            }
        }
        Ok(out)
    })
}

/// Best-effort recovery of the assistant text produced before an abort. The
/// live text arrived over events, so the history copy is the fallback.
fn streamed_so_far(history: &[Incoming]) -> String {
    history
        .iter()
        .rev()
        .find_map(|m| match m {
            Incoming::Assistant { text, .. } if !text.trim().is_empty() => {
                Some(text.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn persist(app: &AppHandle, m: &Message) -> Result<()> {
    let state = app.state::<AppState>();
    state.with(|s| {
        s.upsert_message(m)?;
        s.touch_session(&m.session_id)
    })
}

fn record_usage(app: &AppHandle, session_id: &str, usage: &Usage) -> Result<()> {
    let state = app.state::<AppState>();
    state.with(|s| {
        let mut session = s.get_session(session_id)?;
        session.tokens_in += usage.input_tokens as i64;
        session.tokens_out += usage.output_tokens as i64;
        session.updated_at = now_ms();
        s.upsert_session(&session)
    })
}

// ------------------------------------------------------------------ approvals

#[tauri::command]
pub fn approve(
    runs: State<'_, Arc<Runs>>,
    session_id: String,
    call_id: String,
    allow: bool,
    always: bool,
    reason: Option<String>,
) -> Result<()> {
    let approvals = runs
        .approvals(&session_id)
        .ok_or_else(|| Error::msg("this session is not waiting for approval"))?;
    match (allow, always) {
        (_, true) => approvals.allow_always(),
        (true, false) => approvals.allow(),
        (false, false) => approvals.deny(reason.unwrap_or_else(|| "the user declined".into())),
    }
    let _ = call_id;
    Ok(())
}

#[tauri::command]
pub fn abort_run(runs: State<'_, Arc<Runs>>, session_id: String) -> Result<()> {
    if !runs.cancel(&session_id) {
        return Err(Error::msg("nothing is running in this session"));
    }
    Ok(())
}

#[tauri::command]
pub fn session_is_running(runs: State<'_, Arc<Runs>>, session_id: String) -> Result<bool> {
    Ok(runs.is_running(&session_id))
}

// --------------------------------------------------------------------- todos

#[tauri::command]
pub fn todos_save(state: State<'_, AppState>, session_id: String, todos: Vec<TodoItem>) -> Result<()> {
    state.with(|s| s.replace_todos(&session_id, &todos))
}

// ----------------------------------------------------------------- checkpoints

/// Snapshots the working tree so fork & rewind has something to restore.
#[tauri::command]
pub fn checkpoint_create(
    state: State<'_, AppState>,
    session_id: String,
    message_id: Option<String>,
    label: Option<String>,
) -> Result<Checkpoint> {
    state.with(|s| {
        let session = s.get_session(&session_id)?;
        let roots = project_roots(s, session.project_id.as_deref());
        let Some(root) = roots.first() else {
            return Err(Error::msg("this chat has no project folder"));
        };
        let dir = PathBuf::from(root);
        if !octopus_core::git::is_repo(&dir) {
            return Err(Error::msg("checkpoints need a git repository"));
        }
        octopus_core::git::create_checkpoint(
            s,
            &session_id,
            message_id.as_deref(),
            &dir,
            label.as_deref().unwrap_or("checkpoint"),
        )
    })
}

#[tauri::command]
pub fn checkpoint_list(state: State<'_, AppState>, session_id: String) -> Result<Vec<Checkpoint>> {
    state.with(|s| s.list_checkpoints(&session_id))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitState {
    pub is_repo: bool,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub changed: Vec<octopus_core::git::StatusEntry>,
    pub diff: String,
}

/// Working tree state for the status bar and the rewind warning.
#[tauri::command]
pub fn git_state(state: State<'_, AppState>, session_id: String) -> Result<GitState> {
    state.with(|s| {
        let session = s.get_session(&session_id)?;
        let roots = project_roots(s, session.project_id.as_deref());
        let Some(root) = roots.first() else {
            return Ok(GitState {
                is_repo: false,
                head: None,
                branch: None,
                changed: Vec::new(),
                diff: String::new(),
            });
        };
        let dir = PathBuf::from(root);
        if !octopus_core::git::is_repo(&dir) {
            return Ok(GitState {
                is_repo: false,
                head: None,
                branch: None,
                changed: Vec::new(),
                diff: String::new(),
            });
        }
        Ok(GitState {
            is_repo: true,
            head: octopus_core::git::head(&dir)?,
            branch: octopus_core::git::branch(&dir)?,
            changed: octopus_core::git::status(&dir)?,
            diff: octopus_core::git::diff(&dir)?,
        })
    })
}

// -------------------------------------------------------------------- helpers

fn merge_instructions(project_instructions: &str, roots: &[String]) -> String {
    let mut out = String::new();
    if !project_instructions.trim().is_empty() {
        out.push_str(project_instructions.trim());
        out.push_str("\n\n");
    }
    // A file named AGENTS.md in the workspace is treated as project rules.
    for r in roots {
        let p = PathBuf::from(r).join("AGENTS.md");
        if let Ok(text) = std::fs::read_to_string(&p)
            && !text.trim().is_empty()
        {
            out.push_str("### AGENTS.md\n");
            out.push_str(text.trim());
            out.push_str("\n\n");
            break;
        }
    }
    out
}

/// Builds attachments from the references the composer collected.
///
/// Each entry is either an absolute path on disk or a `data:<mime>;base64,...`
/// url. Pasting a screenshot produces the latter, because a pasted image has no
/// path to read back, so both forms have to be accepted.
fn build_attachments(images: &[String]) -> Result<Vec<Attachment>> {
    const MAX_BYTES: usize = 20 * 1024 * 1024;

    let mut out = Vec::new();
    for item in images {
        if let Some(payload) = octopus_core::util::data_url_payload(item) {
            let mime = item
                .split_once("data:")
                .and_then(|(head, _)| head.split(';').next().map(|m| m.to_string()))
                .unwrap_or_else(|| "image/png".into());
            let bytes = base64_len(payload);
            if bytes > MAX_BYTES {
                return Err(Error::invalid("that image is larger than the 20 MB limit"));
            }
            out.push(Attachment {
                id: octopus_core::util::new_id("att"),
                path: format!("pasted:{mime}"),
                mime,
                bytes: bytes as u64,
                preview: Some(item.clone()),
            });
            continue;
        }

        let p = PathBuf::from(item);
        let mime = match p.extension().and_then(|e| e.to_str()) {
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            Some("bmp") => "image/bmp",
            _ => continue,
        };
        let bytes = std::fs::read(&p).map_err(|e| Error::io(format!("{}: {e}", p.display())))?;
        if bytes.len() > MAX_BYTES {
            return Err(Error::invalid(format!(
                "{} is larger than the 20 MB attachment limit",
                p.display()
            )));
        }
        out.push(Attachment {
            id: octopus_core::util::new_id("att"),
            path: item.clone(),
            mime: mime.to_string(),
            bytes: bytes.len() as u64,
            preview: Some(format!("data:{mime};base64,{}", base64_encode(&bytes))),
        });
    }
    Ok(out)
}

/// Length of the decoded payload behind a base64 data url.
fn base64_len(payload: &str) -> usize {
    let clean = payload.trim_end_matches('=');
    clean.len() * 3 / 4
}

fn base64_encode(bytes: &[u8]) -> String {
    octopus_core::util::base64_encode(bytes)
}

#[tauri::command]
pub fn storage_info(state: State<'_, AppState>) -> Result<StorageInfo> {
    let (db_bytes, path) = state.with(|s| Ok((s.size_bytes()?, paths::db_path()?)))?;
    Ok(StorageInfo { database_bytes: db_bytes, database_path: path.to_string_lossy().into_owned() })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub database_bytes: i64,
    pub database_path: String,
}

#[tauri::command]
pub fn app_paths() -> Result<octopus_core::paths::AppPaths> {
    octopus_core::paths::AppPaths::resolve()
}
// --------------------------------------------------------------- right panel

/// Roots of the chat's project, which every panel command is confined to.
fn panel_roots(store: &Store, session_id: &str) -> Result<Vec<PathBuf>> {
    let session = store.get_session(session_id)?;
    let roots: Vec<PathBuf> = project_roots(store, session.project_id.as_deref())
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if roots.is_empty() {
        return Err(Error::invalid("open a project folder first"));
    }
    Ok(roots)
}

#[tauri::command]
pub fn panel_tree(
    state: State<'_, AppState>,
    session_id: String,
    path: Option<String>,
    depth: Option<u32>,
) -> Result<octopus_core::fsview::TreeNode> {
    let ignore = state.with(|s| Ok(Settings::load(s).ignore_globs))?;
    state.with(|s| {
        let roots = panel_roots(s, &session_id)?;
        octopus_core::fsview::tree(&roots, path.as_deref().unwrap_or(""), &ignore, depth.unwrap_or(3) as usize)
    })
}

#[tauri::command]
pub fn panel_read(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
) -> Result<octopus_core::fsview::FileContent> {
    state.with(|s| {
        let roots = panel_roots(s, &session_id)?;
        octopus_core::fsview::read(&roots, &path, 2 * 1024 * 1024)
    })
}

#[tauri::command]
pub fn panel_write(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    text: String,
) -> Result<octopus_core::fsview::FileContent> {
    state.with(|s| {
        let roots = panel_roots(s, &session_id)?;
        octopus_core::fsview::write(&roots, &path, &text)
    })
}

#[tauri::command]
pub fn panel_changed(state: State<'_, AppState>, session_id: String) -> Result<Vec<String>> {
    let ignore = state.with(|s| Ok(Settings::load(s).ignore_globs))?;
    state.with(|s| {
        let roots = panel_roots(s, &session_id)?;
        octopus_core::fsview::changed_files(&roots, &ignore)
    })
}

/// Reads a file as base64 for the composer's image thumbnails.
///
/// Deliberately not confined to the project roots: the user picked this file
/// explicitly through a native dialog or by dropping it on the window, which is
/// the same authority as the attachment itself.
#[tauri::command]
pub fn panel_read_base64(path: String) -> Result<String> {
    let p = PathBuf::from(&path);
    let meta = std::fs::metadata(&p).map_err(|e| Error::io(format!("{path}: {e}")))?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err(Error::invalid("that image is larger than the 20 MB limit"));
    }
    let bytes = std::fs::read(&p).map_err(|e| Error::io(format!("{path}: {e}")))?;
    Ok(base64_encode(&bytes))
}

// ------------------------------------------------------------ window controls

/// Native window buttons. The window is undecorated so these replace the frame.
#[tauri::command]
pub fn window_minimize(window: tauri::Window) -> Result<()> {
    window.minimize().map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
pub fn window_toggle_maximize(window: tauri::Window) -> Result<()> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize()
    } else {
        window.maximize()
    }
    .map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
pub fn window_close(window: tauri::Window) -> Result<()> {
    window.close().map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
pub fn set_local_only(state: State<'_, AppState>, local_only: bool) -> Result<()> {
    state.with(|s| {
        let mut settings = Settings::load(s);
        settings.local_only = local_only;
        settings.save(s)
    })
}
