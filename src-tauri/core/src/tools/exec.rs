use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::{arg_str, arg_u32, Display, Tool, ToolContext, ToolFuture, ToolOutcome};
use crate::error::{Error, Result};
use crate::util;

/// Commands that are refused regardless of permission mode, because they are
/// destructive in a way no user prompt should wave through by reflex.
const ALWAYS_DENIED: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "mkfs",
    "format ",
    "dd if=",
    ":(){",
    "shutdown",
    "del /f /s /q c:",
    "remove-item -recurse -force c:",
];

#[derive(Debug, Clone, Default)]
pub struct Bash {
    /// Extra working directory, used when a session runs in a worktree.
    pub cwd: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashRequest {
    pub command: String,
}

impl Bash {
    fn screen(&self, command: &str) -> Result<()> {
        let lower = command.to_ascii_lowercase();
        for bad in ALWAYS_DENIED {
            if lower.contains(&bad.to_ascii_lowercase()) {
                return Err(Error::Denied(format!(
                    "`{bad}` is on the always-refused list and cannot be approved"
                )));
            }
        }
        Ok(())
    }
}

impl Tool for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Run a shell command in the workspace. Returns stdout, stderr and the exit \
         code. Long-running commands are stopped after the timeout. Chain commands \
         with && rather than newlines where possible."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to run." },
                "cwd": { "type": "string", "description": "Working directory, relative to the workspace." },
                "timeout_secs": { "type": "integer", "description": "Defaults to 120, capped at 600." }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("command").and_then(|v| v.as_str()) {
            Some(c) => util::truncate(c.trim(), 80),
            None => "Run a command".into(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let command = arg_str(&args, "command")?.trim().to_string();
            if command.is_empty() {
                return Err(Error::invalid("command is empty"));
            }
            self.screen(&command)?;

            let cwd = match args.get("cwd").and_then(|v| v.as_str()) {
                Some(c) => ctx.resolve(c)?,
                None => self
                    .cwd
                    .clone()
                    .filter(|p| ctx.roots.iter().any(|r| p.starts_with(r)))
                    .or_else(|| ctx.roots.first().cloned())
                    .ok_or_else(|| Error::invalid("no working directory for this session"))?,
            };
            let timeout = arg_u32(&args, "timeout_secs", 120).clamp(1, 600);

            let mut cmd = if cfg!(windows) {
                let mut c = Command::new("cmd");
                c.arg("/C");
                c
            } else {
                let mut c = Command::new("sh");
                c.arg("-c");
                c
            };
            cmd.arg(&command)
                .current_dir(&cwd)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .env("NO_COLOR", "1")
                .env("CI", "1")
                .env("GIT_TERMINAL_PROMPT", "0");

            let mut child = cmd
                .spawn()
                .map_err(|e| Error::Io(format!("could not start a shell: {e}")))?;

            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let (out, err) = tokio::join!(drain(stdout), drain(stderr));

            let status = match tokio::time::timeout(Duration::from_secs(timeout as u64), child.wait())
                .await
            {
                Ok(s) => s.map_err(Error::io)?,
                Err(_) => {
                    let _ = child.kill().await;
                    return Ok(ToolOutcome::text(format!(
                        "Command timed out after {timeout}s and was stopped.\n\
                         Partial output:\n{out}\n{err}"
                    ))
                    .with_display(Display::Command { command, exit_code: None }));
                }
            };

            let code = status.code();
            let mut content = String::new();
            if !out.trim().is_empty() {
                content.push_str(out.trim_end());
                content.push('\n');
            }
            if !err.trim().is_empty() {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str("[stderr]\n");
                content.push_str(err.trim_end());
                content.push('\n');
            }
            if content.trim().is_empty() {
                content.push_str("(no output)");
            }
            content.push_str(&format!("\n[exit code: {}]", code.unwrap_or(-1)));

            let mut outcome = ToolOutcome::text(ctx.clip(content))
                .with_display(Display::Command { command, exit_code: code });
            // A non-zero exit is information, not a tool crash: the model needs
            // to see the output and decide. Only a spawn failure is an error.
            outcome.is_error = false;
            Ok(outcome)
        })
    }
}

/// Reads a pipe to a string, capping it so a runaway command cannot exhaust
/// memory.
async fn drain<R>(reader: Option<R>) -> String
where
    R: tokio::io::AsyncRead + Unpin,
{
    const CAP: usize = 512 * 1024;
    let Some(reader) = reader else { return String::new() };
    let mut lines = BufReader::new(reader).lines();
    let mut out = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        if out.len() >= CAP {
            out.push_str("\n[output truncated]\n");
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catastrophic_commands_are_refused() {
        let b = Bash::default();
        for bad in ["rm -rf /", "MKFS.ext4 /dev/sda", "shutdown now", "dd if=/dev/zero of=/dev/sda"] {
            assert!(b.screen(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn ordinary_commands_pass_the_screen() {
        let b = Bash::default();
        for ok in ["npm test", "cargo build", "rm -rf node_modules", "git status"] {
            assert!(b.screen(ok).is_ok(), "{ok} should be allowed");
        }
    }

    #[test]
    fn the_screen_is_case_insensitive() {
        assert!(Bash::default().screen("SHUTDOWN /s").is_err());
    }

    #[tokio::test]
    async fn drain_handles_a_closed_pipe() {
        assert_eq!(drain(Some(tokio::io::empty())).await, "");
    }

    #[tokio::test]
    async fn drain_collects_lines() {
        let bytes: &[u8] = b"one\ntwo\n";
        let out = drain(Some(std::io::Cursor::new(bytes))).await;
        assert!(out.contains("one") && out.contains("two"));
    }
}