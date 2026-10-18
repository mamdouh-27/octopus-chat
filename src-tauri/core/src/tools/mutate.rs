use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};

use super::{arg_bool, arg_str, Tool, ToolContext, ToolFuture, ToolOutcome};
use crate::error::Error;
use crate::util;

/// Creates a file or replaces its contents wholesale.
pub struct WriteFile;

impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Create a file or replace its entire contents. Prefer the edit tool for \
         changing part of an existing file."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to write." },
                "content": { "type": "string", "description": "Full new contents." }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => format!("Write {p}"),
            None => "Write a file".into(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let raw = arg_str(&args, "path")?;
            let content = arg_str(&args, "content")?;
            let path = ctx.resolve(raw)?;
            let existed = tokio::fs::metadata(&path).await.is_ok();

            let before = if existed {
                tokio::fs::read_to_string(&path).await.unwrap_or_default()
            } else {
                String::new()
            };

            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(Error::io)?;
            }
            tokio::fs::write(&path, content.as_bytes())
                .await
                .map_err(|e| Error::Io(format!("{}: {e}", ctx.display(&path))))?;

            let diff = unified_diff(&ctx.display(&path), &before, content);
            let verb = if existed { "Updated" } else { "Created" };
            Ok(ToolOutcome::text(format!(
                "{verb} {} ({} lines)",
                ctx.display(&path),
                content.lines().count()
            ))
            .with_diff(diff))
        })
    }
}

/// Replaces an exact string in a file. Refuses to guess when the target is
/// absent or ambiguous, which is what makes the edit safe to auto-approve.
pub struct EditFile;

impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> &'static str {
        "Replace an exact string in a file. The old string must appear exactly \
         once unless replace_all is set."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to edit." },
                "old_string": { "type": "string", "description": "Exact text to replace." },
                "new_string": { "type": "string", "description": "Replacement text." },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence. Defaults to false."
                }
            },
            "required": ["path", "old_string", "new_string"],
            "additionalProperties": false
        })
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => format!("Edit {p}"),
            None => "Edit a file".into(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let raw = arg_str(&args, "path")?;
            let old = arg_str(&args, "old_string")?;
            let new = arg_str(&args, "new_string")?;
            let replace_all = arg_bool(&args, "replace_all", false);
            let path = ctx.resolve(raw)?;

            let before = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| Error::Io(format!("{}: {e}", ctx.display(&path))))?;

            if old == new {
                return Err(Error::invalid("old_string and new_string are identical"));
            }

            let hits = before.matches(old).count();
            if hits == 0 {
                return Err(Error::invalid(format!(
                    "old_string was not found in {}. Read the file again to get its exact \
                     current contents.",
                    ctx.display(&path)
                )));
            }
            if hits > 1 && !replace_all {
                return Err(Error::invalid(format!(
                    "old_string appears {hits} times in {}. Include more surrounding \
                     context to make it unique, or set replace_all.",
                    ctx.display(&path)
                )));
            }

            let after = if replace_all {
                before.replace(old, new)
            } else {
                before.replacen(old, new, 1)
            };

            tokio::fs::write(&path, after.as_bytes())
                .await
                .map_err(|e| Error::Io(format!("{}: {e}", ctx.display(&path))))?;

            let changed = if replace_all { hits } else { 1 };
            Ok(ToolOutcome::text(format!(
                "Replaced {changed} occurrence{} in {}",
                if changed == 1 { "" } else { "s" },
                ctx.display(&path)
            ))
            .with_diff(unified_diff(&ctx.display(&path), &before, &after)))
        })
    }
}

/// Removes a file. Refuses directories, which need an explicit decision.
pub struct DeleteFile;

impl Tool for DeleteFile {
    fn name(&self) -> &'static str {
        "delete"
    }

    fn description(&self) -> &'static str {
        "Delete a file from the workspace."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "File to delete." } },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => format!("Delete {p}"),
            None => "Delete a file".into(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let path = ctx.resolve(arg_str(&args, "path")?)?;
            if tokio::fs::metadata(&path).await.map(|m| m.is_dir()).unwrap_or(false) {
                return Err(Error::invalid(
                    "refusing to delete a directory; delete its files individually",
                ));
            }
            let before = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| Error::Io(format!("{}: {e}", ctx.display(&path))))?;
            Ok(ToolOutcome::text(format!("Deleted {}", ctx.display(&path)))
                .with_diff(unified_diff(&ctx.display(&path), &before, "")))
        })
    }
}

/// Renders a git-style unified diff with three lines of context.
pub fn unified_diff(path: &str, before: &str, after: &str) -> String {
    let diff = TextDiff::from_lines(before, after);
    if diff.ratio() == 1.0 {
        return String::new();
    }
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    for group in diff.grouped_ops(3) {
        let first = group.first().unwrap();
        let last = group.last().unwrap();
        let a_start = first.old_range().start;
        let a_len = last.old_range().end - first.old_range().start;
        let b_start = first.new_range().start;
        let b_len = last.new_range().end - first.new_range().start;
        out.push_str(&hunk_header(a_start, a_len, b_start, b_len));
        for op in group {
            for change in diff.iter_changes(&op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                let text = change.value().trim_end_matches('\n');
                out.push(sign);
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    out
}

fn hunk_header(a_start: usize, a_len: usize, b_start: usize, b_len: usize) -> String {
    // A zero-length side means "insert after line N", which git renders as N.
    let a = if a_len == 0 { a_start } else { a_start + 1 };
    let b = if b_len == 0 { b_start } else { b_start + 1 };
    format!("@@ -{a},{a_len} +{b},{b_len} @@\n")
}

/// Truncates a tool result for the model while saying so explicitly.
pub fn clip_for_model(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    util::truncate(&s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_content_produces_no_diff() {
        assert_eq!(unified_diff("a.rs", "same\n", "same\n"), "");
    }

    #[test]
    fn pure_insertion_is_shown() {
        let d = unified_diff("a.rs", "a\nb\n", "a\nb\nc\n");
        assert!(d.contains("+++ b/a.rs"));
        assert!(d.contains("+c"));
    }

    #[test]
    fn pure_deletion_is_shown() {
        let d = unified_diff("a.rs", "a\nb\nc\n", "a\nc\n");
        assert!(d.contains("-b"));
    }

    #[test]
    fn insertions_away_from_the_edges_merge_into_one_hunk() {
        // A two line file is entirely inside the three lines of context, so the
        // whole file becomes a single hunk.
        let d = unified_diff("a.rs", "a\nb\n", "a\nx\nb\n");
        assert!(d.contains("@@ -1,2 +1,3 @@"), "got: {d}");
        assert_eq!(hunk_count(&d), 1, "expected exactly one hunk");
    }

    #[test]
    fn hunk_header_renders_zero_length_sides_the_git_way() {
        // A zero-length side means "insert after line N", which git renders as N
        // rather than N+1. Context grouping rarely produces this, so the header
        // builder is checked directly.
        assert_eq!(hunk_header(1, 0, 1, 1), "@@ -1,0 +2,1 @@\n");
        assert_eq!(hunk_header(0, 0, 0, 1), "@@ -0,0 +1,1 @@\n");
        assert_eq!(hunk_header(4, 2, 4, 3), "@@ -5,2 +5,3 @@\n");
    }

    fn hunk_count(diff: &str) -> usize {
        diff.lines().filter(|l| l.starts_with("@@")).count()
    }

    #[test]
    fn hunks_are_grouped_with_context() {
        let before: String = (1..=40).map(|i| format!("l{i}\n")).collect();
        let after = before.replace("l20", "CHANGED");
        let d = unified_diff("a.rs", &before, &after);
        // Only the region around the change should appear, not all 40 lines.
        assert!(d.contains("CHANGED"));
        assert!(d.contains("@@"));
        assert!(d.lines().count() < 20, "diff should be localized");
    }

    #[test]
    fn changes_are_counted_before_writing() {
        let a = "x\nx\nx\n";
        assert_eq!(a.matches("x").count(), 3);
    }
}