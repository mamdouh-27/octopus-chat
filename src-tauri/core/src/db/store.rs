use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::models::*;

pub struct Store {
    pub conn: Connection,
}

fn json_list(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

impl Store {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    // ---------------------------------------------------------------- projects

    pub fn list_projects(&self, include_archived: bool) -> Result<Vec<Project>> {
        let sql = if include_archived {
            "SELECT * FROM projects ORDER BY archived, updated_at DESC"
        } else {
            "SELECT * FROM projects WHERE archived = 0 ORDER BY updated_at DESC"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], map_project)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_project(&self, id: &str) -> Result<Project> {
        self.conn
            .query_row("SELECT * FROM projects WHERE id = ?1", params![id], map_project)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("project {id}")))
    }

    pub fn upsert_project(&self, p: &Project) -> Result<()> {
        self.conn.execute(
            "INSERT INTO projects (id,name,roots,instructions,permission_mode,accent,archived,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name, roots=excluded.roots, instructions=excluded.instructions,
               permission_mode=excluded.permission_mode, accent=excluded.accent,
               archived=excluded.archived, updated_at=excluded.updated_at",
            params![
                p.id,
                p.name,
                serde_json::to_string(&p.roots)?,
                p.instructions,
                p.permission_mode.as_str(),
                p.accent,
                p.archived as i64,
                p.created_at,
                p.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ---------------------------------------------------------------- sessions

    pub fn list_sessions(
        &self,
        project_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Session>> {
        let (sql, args): (&str, Vec<&dyn rusqlite::ToSql>) = match project_id {
            Some(_) => (
                "SELECT * FROM sessions WHERE project_id = ?1
                 ORDER BY favorite DESC, updated_at DESC LIMIT ?2",
                vec![&project_id as &dyn rusqlite::ToSql, &limit as &dyn rusqlite::ToSql],
            ),
            None => (
                "SELECT * FROM sessions
                 ORDER BY favorite DESC, updated_at DESC LIMIT ?1",
                vec![&limit as &dyn rusqlite::ToSql],
            ),
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), map_session)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_session(&self, id: &str) -> Result<Session> {
        self.conn
            .query_row("SELECT * FROM sessions WHERE id = ?1", params![id], map_session)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("session {id}")))
    }

    pub fn upsert_session(&self, s: &Session) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions
               (id,project_id,title,mode,state,provider,model,reasoning_effort,temperature,
                max_tokens,permission_mode,goal,forked_from,worktree,favorite,tokens_in,
                tokens_out,cost_usd,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
             ON CONFLICT(id) DO UPDATE SET
               project_id=excluded.project_id, title=excluded.title, mode=excluded.mode,
               state=excluded.state, provider=excluded.provider, model=excluded.model,
               reasoning_effort=excluded.reasoning_effort, temperature=excluded.temperature,
               max_tokens=excluded.max_tokens, permission_mode=excluded.permission_mode,
               goal=excluded.goal, worktree=excluded.worktree, favorite=excluded.favorite,
               tokens_in=excluded.tokens_in, tokens_out=excluded.tokens_out,
               cost_usd=excluded.cost_usd, updated_at=excluded.updated_at",
            params![
                s.id,
                s.project_id,
                s.title,
                s.mode.as_str(),
                run_state_str(s.state),
                s.provider,
                s.model,
                s.reasoning_effort,
                s.temperature,
                s.max_tokens,
                s.permission_mode.as_str(),
                s.goal,
                s.forked_from,
                s.worktree,
                s.favorite as i64,
                s.tokens_in,
                s.tokens_out,
                s.cost_usd,
                s.created_at,
                s.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn touch_session(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    }

    /// First user message, used to auto-title a fresh session once.
    pub fn session_title_from_first_message(&self, id: &str) -> Result<Option<String>> {
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT content FROM messages
                 WHERE session_id = ?1 AND role = 'user' ORDER BY created_at LIMIT 1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(text.map(|t| crate::util::title_from(&t)))
    }

    // ---------------------------------------------------------------- messages

    pub fn list_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM messages WHERE session_id = ?1 ORDER BY created_at, rowid",
        )?;
        let rows = stmt.query_map(params![session_id], map_message)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_message(&self, id: &str) -> Result<Message> {
        self.conn
            .query_row("SELECT * FROM messages WHERE id = ?1", params![id], map_message)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("message {id}")))
    }

    pub fn upsert_message(&self, m: &Message) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages
               (id,session_id,role,content,reasoning,state,model,error,tokens_in,tokens_out,
                supersedes,attachments,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET
               content=excluded.content, reasoning=excluded.reasoning, state=excluded.state,
               model=excluded.model, error=excluded.error, tokens_in=excluded.tokens_in,
               tokens_out=excluded.tokens_out, attachments=excluded.attachments",
            params![
                m.id,
                m.session_id,
                role_str(m.role),
                m.content,
                m.reasoning,
                state_str(m.state),
                m.model,
                m.error,
                m.tokens_in,
                m.tokens_out,
                m.supersedes,
                serde_json::to_string(&m.attachments)?,
                m.created_at,
            ],
        )?;
        Ok(())
    }

    /// Appends a streamed chunk without rewriting the whole message row.
    pub fn append_message_content(&self, id: &str, chunk: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET content = content || ?2 WHERE id = ?1",
            params![id, chunk],
        )?;
        Ok(())
    }

    pub fn set_message_state(&self, id: &str, state: MessageState) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET state = ?2 WHERE id = ?1",
            params![id, state_str(state)],
        )?;
        Ok(())
    }

    /// Drops every message after `message_id`, which is what fork & rewind needs
    /// before replaying from an edited prompt.
    pub fn truncate_after(&self, session_id: &str, message_id: &str) -> Result<usize> {
        let ts: i64 = self
            .conn
            .query_row(
                "SELECT created_at FROM messages WHERE id = ?1 AND session_id = ?2",
                params![message_id, session_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("message {message_id}")))?;
        let n = self.conn.execute(
            "DELETE FROM messages
             WHERE session_id = ?1
               AND (created_at > ?2 OR (created_at = ?2 AND rowid > (
                     SELECT rowid FROM messages WHERE id = ?3)))",
            params![session_id, ts, message_id],
        )?;
        Ok(n)
    }

    pub fn delete_message(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        Ok(())
    }

    // -------------------------------------------------------------- tool calls

    pub fn list_tool_calls(&self, session_id: &str) -> Result<Vec<ToolCall>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM tool_calls WHERE session_id = ?1 ORDER BY started_at, rowid",
        )?;
        let rows = stmt.query_map(params![session_id], map_tool_call)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn upsert_tool_call(&self, t: &ToolCall) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tool_calls
               (id,session_id,message_id,name,input,output,state,denied_reason,diff,started_at,ended_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET
               output=excluded.output, state=excluded.state,
               denied_reason=excluded.denied_reason, diff=excluded.diff,
               ended_at=excluded.ended_at",
            params![
                t.id,
                t.session_id,
                t.message_id,
                t.name,
                t.input,
                t.output,
                tool_state_str(t.state),
                t.denied_reason,
                t.diff,
                t.started_at,
                t.ended_at,
            ],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------- checkpoints

    pub fn list_checkpoints(&self, session_id: &str) -> Result<Vec<Checkpoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM checkpoints WHERE session_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok(Checkpoint {
                id: r.get(0)?,
                session_id: r.get(1)?,
                message_id: r.get(2)?,
                label: r.get(3)?,
                git_head: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn insert_checkpoint(&self, c: &Checkpoint) -> Result<()> {
        self.conn.execute(
            "INSERT INTO checkpoints (id,session_id,message_id,label,git_head,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![c.id, c.session_id, c.message_id, c.label, c.git_head, c.created_at],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------- todos

    pub fn replace_todos(&self, session_id: &str, items: &[TodoItem]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM todos WHERE session_id = ?1", params![session_id])?;
        for (i, t) in items.iter().enumerate() {
            tx.execute(
                "INSERT INTO todos (id,session_id,content,status,position)
                 VALUES (?1,?2,?3,?4,?5)",
                params![t.id, session_id, t.content, todo_state_str(t.status), i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_todos(&self, session_id: &str) -> Result<Vec<TodoItem>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,content,status FROM todos WHERE session_id = ?1 ORDER BY position")?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok(TodoItem {
                id: r.get(0)?,
                content: r.get(1)?,
                status: todo_state_inv(r.get::<_, String>(2)?),
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    // ---------------------------------------------------------------------- kv

    pub fn kv_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO kv (key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Rough on-disk size of the store, shown in Settings > Storage.
    pub fn size_bytes(&self) -> Result<i64> {
        let page_count: i64 = self.conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let page_size: i64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        Ok(page_count * page_size)
    }
}

// ------------------------------------------------------------------- mappers

fn role_str(r: Role) -> &'static str {
    match r {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn role_inv(s: &str) -> Role {
    match s {
        "user" => Role::User,
        "tool" => Role::Tool,
        _ => Role::Assistant,
    }
}

fn state_str(s: MessageState) -> &'static str {
    match s {
        MessageState::Pending => "pending",
        MessageState::Streaming => "streaming",
        MessageState::Done => "done",
        MessageState::Error => "error",
        MessageState::Aborted => "aborted",
    }
}

fn state_inv(s: &str) -> MessageState {
    match s {
        "pending" => MessageState::Pending,
        "streaming" => MessageState::Streaming,
        "error" => MessageState::Error,
        "aborted" => MessageState::Aborted,
        _ => MessageState::Done,
    }
}

fn tool_state_str(s: ToolState) -> &'static str {
    match s {
        ToolState::Pending => "pending",
        ToolState::Running => "running",
        ToolState::Done => "done",
        ToolState::Error => "error",
        ToolState::Denied => "denied",
    }
}

fn tool_state_inv(s: &str) -> ToolState {
    match s {
        "pending" => ToolState::Pending,
        "running" => ToolState::Running,
        "error" => ToolState::Error,
        "denied" => ToolState::Denied,
        _ => ToolState::Done,
    }
}

fn todo_state_str(s: TodoState) -> &'static str {
    match s {
        TodoState::Pending => "pending",
        TodoState::Active => "active",
        TodoState::Done => "done",
    }
}

fn todo_state_inv(s: String) -> TodoState {
    match s.as_str() {
        "active" => TodoState::Active,
        "done" => TodoState::Done,
        _ => TodoState::Pending,
    }
}

fn map_project(r: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        roots: json_list(&r.get::<_, String>(2)?),
        instructions: r.get(3)?,
        permission_mode: perm_inv(&r.get::<_, String>(4)?),
        accent: r.get(5)?,
        archived: r.get::<_, i64>(6)? != 0,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

fn map_session(r: &Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        project_id: r.get(1)?,
        title: r.get(2)?,
        mode: if r.get::<_, String>(3)? == "plan" { Mode::Plan } else { Mode::Build },
        state: run_state_inv(&r.get::<_, String>(4)?),
        provider: r.get(5)?,
        model: r.get(6)?,
        reasoning_effort: r.get(7)?,
        temperature: r.get(8)?,
        max_tokens: r.get::<_, Option<i64>>(9)?.map(|v| v as u32),
        permission_mode: perm_inv(&r.get::<_, String>(10)?),
        goal: r.get(11)?,
        forked_from: r.get(12)?,
        worktree: r.get(13)?,
        favorite: r.get::<_, i64>(14)? != 0,
        tokens_in: r.get(15)?,
        tokens_out: r.get(16)?,
        cost_usd: r.get(17)?,
        created_at: r.get(18)?,
        updated_at: r.get(19)?,
    })
}

fn map_message(r: &Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get(0)?,
        session_id: r.get(1)?,
        role: role_inv(&r.get::<_, String>(2)?),
        content: r.get(3)?,
        reasoning: r.get(4)?,
        state: state_inv(&r.get::<_, String>(5)?),
        model: r.get(6)?,
        error: r.get(7)?,
        tokens_in: r.get(8)?,
        tokens_out: r.get(9)?,
        supersedes: r.get(10)?,
        attachments: serde_json::from_str(&r.get::<_, String>(11)?).unwrap_or_default(),
        created_at: r.get(12)?,
    })
}

fn map_tool_call(r: &Row<'_>) -> rusqlite::Result<ToolCall> {
    Ok(ToolCall {
        id: r.get(0)?,
        session_id: r.get(1)?,
        message_id: r.get(2)?,
        name: r.get(3)?,
        input: r.get(4)?,
        output: r.get(5)?,
        state: tool_state_inv(&r.get::<_, String>(6)?),
        denied_reason: r.get(7)?,
        diff: r.get(8)?,
        started_at: r.get(9)?,
        ended_at: r.get(10)?,
    })
}

fn perm_inv(s: &str) -> PermissionMode {
    match s {
        "auto_review" => PermissionMode::AutoReview,
        "full_access" => PermissionMode::FullAccess,
        _ => PermissionMode::Ask,
    }
}

fn run_state_inv(s: &str) -> RunState {
    match s {
        "running" => RunState::Running,
        "waiting_approval" => RunState::WaitingApproval,
        "paused" => RunState::Paused,
        "done" => RunState::Done,
        "error" => RunState::Error,
        _ => RunState::Idle,
    }
}

fn run_state_str(s: RunState) -> &'static str {
    match s {
        RunState::Idle => "idle",
        RunState::Running => "running",
        RunState::WaitingApproval => "waiting_approval",
        RunState::Paused => "paused",
        RunState::Done => "done",
        RunState::Error => "error",
    }
}

/// Row payload for the projects/sessions overview shown on the home screen.
#[derive(Debug, Serialize)]
pub struct ProjectStats {
    pub project_id: String,
    pub sessions: i64,
    pub messages: i64,
    pub tokens: i64,
}

impl Store {
    pub fn stats_for(&self, project_id: &str) -> Result<ProjectStats> {
        let row = self.conn.query_row(
            "SELECT
               (SELECT COUNT(*) FROM sessions WHERE project_id = ?1),
               (SELECT COUNT(*) FROM messages WHERE session_id IN
                  (SELECT id FROM sessions WHERE project_id = ?1)),
               (SELECT COALESCE(SUM(tokens_in + tokens_out),0) FROM sessions WHERE project_id = ?1)",
            params![project_id],
            |r| {
                Ok(ProjectStats {
                    project_id: project_id.to_string(),
                    sessions: r.get(0)?,
                    messages: r.get(1)?,
                    tokens: r.get(2)?,
                })
            },
        )?;
        Ok(row)
    }
}
