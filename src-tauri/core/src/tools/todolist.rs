use serde_json::{json, Value};

use super::{arg_str, Display, Tool, ToolContext, ToolFuture, ToolOutcome};
use crate::models::{TodoItem, TodoState};

/// Reads the session's task list.
pub struct TodoRead;

impl Tool for TodoRead {
    fn name(&self) -> &'static str {
        "todo_read"
    }

    fn description(&self) -> &'static str {
        "Read the current task list for this session."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "additionalProperties": false })
    }

    fn summary(&self, _args: &Value) -> String {
        "Read the task list".into()
    }

    fn run<'a>(&'a self, _ctx: &'a ToolContext, _args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            Ok(ToolOutcome::text("The task list is maintained by the app; call todo_read \
                 without arguments to see it rendered above.")
                .with_display(Display::TodoList { items: Vec::new() }))
        })
    }
}

/// Replaces the session's task list. This is how the agent shows progress on
/// multi-step work and how the UI renders the progress row.
pub struct TodoWrite;

impl Tool for TodoWrite {
    fn name(&self) -> &'static str {
        "todo_write"
    }

    fn description(&self) -> &'static str {
        "Replace the task list. Pass the complete list every time, including items \
         that are unchanged. Keep exactly one item in_progress while work continues."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The full task list.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string", "description": "Imperative description." },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "active", "done"]
                            }
                        },
                        "required": ["content", "status"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["todos"],
            "additionalProperties": false
        })
    }

    fn summary(&self, args: &Value) -> String {
        let n = args.get("todos").and_then(|v| v.as_array()).map_or(0, |a| a.len());
        format!("Update the task list ({n} items)")
    }

    fn run<'a>(&'a self, _ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let raw = args
                .get("todos")
                .and_then(|v| v.as_array())
                .ok_or_else(|| crate::error::Error::invalid("`todos` must be an array"))?;

            let mut items: Vec<TodoItem> = Vec::with_capacity(raw.len());
            for entry in raw {
                let content = arg_str(entry, "content")?.trim().to_string();
                if content.is_empty() {
                    continue;
                }
                let status = match entry.get("status").and_then(|v| v.as_str()) {
                    Some("active") => TodoState::Active,
                    Some("done") => TodoState::Done,
                    _ => TodoState::Pending,
                };
                items.push(TodoItem { id: crate::util::new_id("todo"), content, status });
            }

            let done = items.iter().filter(|i| i.status == TodoState::Done).count();
            let summary = format!("Task list updated: {done}/{} complete.", items.len());
            // The parsed list travels in the payload so the harness persists it
            // and the UI can render it without re-parsing the arguments.
            Ok(ToolOutcome::text(summary).with_display(Display::TodoList { items }))
        })
    }
}

/// Renders a task list the way the transcript shows it.
pub fn render(items: &[TodoItem]) -> String {
    if items.is_empty() {
        return "(no tasks)".into();
    }
    items
        .iter()
        .map(|i| {
            let mark = match i.status {
                TodoState::Done => "x",
                TodoState::Active => ">",
                TodoState::Pending => " ",
            };
            format!("[{mark}] {}", i.content)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(content: &str, status: TodoState) -> TodoItem {
        TodoItem { id: "t".into(), content: content.into(), status }
    }

    #[test]
    fn rendering_marks_each_state() {
        let out = render(&[
            item("read the file", TodoState::Done),
            item("write the fix", TodoState::Active),
            item("run tests", TodoState::Pending),
        ]);
        assert!(out.contains("[x] read the file"));
        assert!(out.contains("[>] write the fix"));
        assert!(out.contains("[ ] run tests"));
    }

    #[test]
    fn an_empty_list_says_so() {
        assert_eq!(render(&[]), "(no tasks)");
    }

    #[test]
    fn the_schema_constrains_status_values() {
        let s = TodoWrite.parameters();
        let status = &s["properties"]["todos"]["items"]["properties"]["status"];
        assert_eq!(status["type"], "string");
        assert_eq!(status["enum"][0], "pending");
    }
}