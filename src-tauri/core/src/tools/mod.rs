use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::models::Mode;
use crate::providers::ToolSchema;

pub mod exec;
pub mod fswalk;
pub mod mutate;
pub mod todolist;

pub use exec::Bash;
pub use fswalk::{Glob, Grep, ListDir, ReadFile};
pub use mutate::{unified_diff, DeleteFile, EditFile, WriteFile};
pub use todolist::{TodoRead, TodoWrite};

/// Boxed future so the trait stays object-safe without a proc-macro dependency.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutcome>> + Send + 'a>>;

/// Everything a tool is allowed to know about the session it runs in.
pub struct ToolContext {
    /// Directories the agent may touch. Paths are resolved against the first
    /// root that contains them.
    pub roots: Vec<PathBuf>,
    pub session_id: String,
    pub agent_mode: Mode,
    pub ignore: Vec<String>,
    /// Longest output a tool should return before it truncates itself.
    pub max_output_bytes: usize,
}

impl ToolContext {
    pub fn new(roots: Vec<PathBuf>, session_id: impl Into<String>) -> Self {
        Self {
            roots,
            session_id: session_id.into(),
            agent_mode: Mode::Build,
            ignore: Vec::new(),
            max_output_bytes: 200_000,
        }
    }

    /// Resolves a possibly relative path, refusing anything outside the roots.
    pub fn resolve(&self, candidate: &str) -> Result<PathBuf> {
        let raw = PathBuf::from(candidate);
        let absolute = if raw.is_absolute() {
            raw.clone()
        } else {
            // Relative paths resolve against the first root, which is what a
            // single-folder project means in practice.
            self.roots
                .first()
                .map(|r| r.join(&raw))
                .unwrap_or_else(|| raw.clone())
        };
        let normalized = crate::paths::normalize(&absolute);
        if self.roots.is_empty() {
            return Err(Error::Denied("this session has no workspace open".into()));
        }
        let allowed = self
            .roots
            .iter()
            .any(|r| normalized.starts_with(crate::paths::normalize(r)));
        if !allowed {
            return Err(Error::Denied(format!(
                "path is outside the workspace: {candidate}"
            )));
        }
        Ok(normalized)
    }

    /// Best display form of a path: relative to its root when possible.
    pub fn display(&self, p: &std::path::Path) -> String {
        for r in &self.roots {
            if let Ok(rel) = p.strip_prefix(crate::paths::normalize(r)) {
                let s = rel.to_string_lossy().replace('\\', "/");
                return if s.is_empty() { ".".into() } else { s };
            }
        }
        crate::paths::display_path(p)
    }

    fn clip(&self, s: String) -> String {
        if s.len() <= self.max_output_bytes {
            return s;
        }
        let cut = floor_char_boundary(&s, self.max_output_bytes);
        format!(
            "{}\n\n[truncated at {} bytes; {} total]",
            &s[..cut],
            cut,
            s.len()
        )
    }
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// What a tool hands back: text for the model plus optional structured detail
/// the UI renders (a diff, a todo list, a file preview).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutcome {
    /// Fed back to the model as the tool result.
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,
    pub is_error: bool,
}

impl ToolOutcome {
    pub fn text(content: impl Into<String>) -> Self {
        Self { content: content.into(), diff: None, display: None, is_error: false }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self { content: content.into(), diff: None, display: None, is_error: true }
    }

    pub fn with_diff(mut self, diff: impl Into<String>) -> Self {
        self.diff = Some(diff.into());
        self
    }

    pub fn with_display(mut self, d: Display) -> Self {
        self.display = Some(d);
        self
    }
}

/// Rich presentation hint for the transcript.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Display {
    File { path: String, bytes: u64, preview: String },
    Diff { path: String },
    TodoList { items: Vec<crate::models::TodoItem> },
    Command { command: String, exit_code: Option<i32> },
    Matches { count: usize },
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// JSON Schema for the arguments object.
    fn parameters(&self) -> Value;
    /// One-line label for the approval prompt.
    fn summary(&self, args: &Value) -> String;

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            parameters: self.parameters(),
        }
    }

    fn mutates(&self) -> bool {
        crate::permissions::mutating_tool(self.name())
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a>;
}

/// The set of tools a session may use.
#[derive(Default)]
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    /// Full set, used when a session is in Build mode.
    pub fn full(ignore: Vec<String>) -> Self {
        let mut r = Self::default();
        r.register(Box::new(ReadFile));
        r.register(Box::new(ListDir));
        r.register(Box::new(Glob));
        r.register(Box::new(Grep { ignore }));
        r.register(Box::new(Bash::default()));
        r.register(Box::new(WriteFile));
        r.register(Box::new(EditFile));
        r.register(Box::new(DeleteFile));
        r.register(Box::new(TodoRead));
        r.register(Box::new(TodoWrite));
        r
    }

    /// Read-only subset for Plan mode: the agent can look but not touch.
    pub fn read_only(ignore: Vec<String>) -> Self {
        let mut r = Self::default();
        r.register(Box::new(ReadFile));
        r.register(Box::new(ListDir));
        r.register(Box::new(Glob));
        r.register(Box::new(Grep { ignore }));
        r
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.retain(|t| t.name() != tool.name());
        self.tools.push(tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.iter().map(|t| t.schema()).collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|t| t.name()).collect()
    }
}

/// Reads a required string argument, with a helpful message when it is absent.
pub fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::invalid(format!("missing required argument `{key}`")))
}

pub fn arg_opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

pub fn arg_u32(args: &Value, key: &str, default: u32) -> u32 {
    args.get(key).and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(default)
}

pub fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolution_rejects_paths_outside_the_workspace() {
        let ctx = ToolContext::new(vec![PathBuf::from("/work/app")], "s1");
        assert!(ctx.resolve("src/a.rs").is_ok());
        assert!(ctx.resolve("/work/app/src/a.rs").is_ok());
        assert!(ctx.resolve("../secrets.txt").is_err());
        assert!(ctx.resolve("/etc/passwd").is_err());
    }

    #[test]
    fn traversal_inside_the_root_still_resolves() {
        let ctx = ToolContext::new(vec![PathBuf::from("/work/app")], "s1");
        let p = ctx.resolve("src/../src/a.rs").unwrap();
        assert!(p.ends_with("src/a.rs"));
    }

    #[test]
    fn session_without_a_root_refuses_everything() {
        let ctx = ToolContext::new(vec![], "s1");
        assert!(ctx.resolve("a.rs").is_err());
    }

    #[test]
    fn display_is_relative_to_the_root() {
        let ctx = ToolContext::new(vec![PathBuf::from("/work/app")], "s1");
        assert_eq!(ctx.display(std::path::Path::new("/work/app/src/a.rs")), "src/a.rs");
    }

    #[test]
    fn output_is_clipped_on_a_char_boundary() {
        let mut ctx = ToolContext::new(vec![PathBuf::from("/w")], "s");
        ctx.max_output_bytes = 5;
        // "日本語" is 9 bytes; clipping at 5 must not split a character.
        let out = ctx.clip("日本語テスト".repeat(10));
        assert!(out.contains("[truncated at 3 bytes"), "got: {out}");
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn argument_helpers_validate_types() {
        let a = json!({ "path": "x", "n": 5, "b": true });
        assert_eq!(arg_str(&a, "path").unwrap(), "x");
        assert_eq!(arg_u32(&a, "n", 1), 5);
        assert_eq!(arg_u32(&a, "missing", 7), 7);
        assert!(arg_str(&a, "missing").is_err());
        assert!(arg_bool(&a, "b", false));
    }

    #[test]
    fn read_only_registry_has_no_mutating_tools() {
        let r = Registry::read_only(vec![]);
        for t in r.names() {
            assert!(!crate::permissions::mutating_tool(t), "{t} must not be exposed in plan mode");
        }
        assert!(r.names().contains(&"read"));
    }

    #[test]
    fn full_registry_exposes_the_expected_tools() {
        let r = Registry::full(vec![]);
        for expected in ["read", "list", "glob", "grep", "bash", "todo_read", "todo_write"] {
            assert!(r.names().contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn every_tool_publishes_a_valid_schema() {
        let r = Registry::full(vec![]);
        for s in r.schemas() {
            assert!(!s.name.is_empty());
            assert!(!s.description.is_empty());
            assert_eq!(
                s.parameters.get("type").and_then(|t| t.as_str()),
                Some("object"),
                "{} must declare an object schema",
                s.name
            );
        }
    }
}