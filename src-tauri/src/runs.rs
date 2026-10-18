use std::collections::HashMap;

use octopus_core::agent::ApprovalsHandle;

/// Everything needed to steer or stop an in-flight run.
pub struct RunHandle {
    pub approvals: ApprovalsHandle,
    pub cancel: tokio_util::sync::CancellationToken,
}

/// Live runs, keyed by session id. A second run for the same session is
/// rejected rather than queued, because the UI never asks for one.
#[derive(Default)]
pub struct Runs {
    inner: std::sync::Mutex<HashMap<String, RunHandle>>,
}

impl Runs {
    pub fn insert(&self, session_id: &str, handle: RunHandle) -> Result<(), String> {
        let mut map = self.inner.lock().map_err(|_| "run registry poisoned")?;
        if map.contains_key(session_id) {
            return Err("this session is already running".into());
        }
        map.insert(session_id.to_string(), handle);
        Ok(())
    }

    pub fn finish(&self, session_id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(session_id);
        }
    }

    pub fn is_running(&self, session_id: &str) -> bool {
        self.inner
            .lock()
            .map(|m| m.contains_key(session_id))
            .unwrap_or(false)
    }

    pub fn approvals(&self, session_id: &str) -> Option<ApprovalsHandle> {
        let map = self.inner.lock().ok()?;
        map.get(session_id).map(|h| h.approvals.clone())
    }

    pub fn cancel(&self, session_id: &str) -> bool {
        let handle = {
            let mut map = match self.inner.lock() {
                Ok(m) => m,
                Err(_) => return false,
            };
            map.remove(session_id)
        };
        match handle {
            Some(h) => {
                // Wake the harness if it is blocked waiting on an approval.
                h.approvals.deny("cancelled by the user");
                h.cancel.cancel();
                true
            }
            None => false,
        }
    }

    pub fn cancel_all(&self) {
        let keys: Vec<String> = self
            .inner
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        for k in keys {
            self.cancel(&k);
        }
    }
}

/// Bridges the harness `Reporter` to Tauri events so the webview can render
/// tokens, tool calls and approval prompts as they happen.
pub struct EventReporter {
    pub app: tauri::AppHandle,
}

impl octopus_core::agent::Reporter for EventReporter {
    fn text(&self, session_id: &str, delta: &str) {
        emit(&self.app, "chat:text", (session_id, delta));
    }

    fn reasoning(&self, session_id: &str, delta: &str) {
        emit(&self.app, "chat:reasoning", (session_id, delta));
    }

    fn tool_start(&self, session_id: &str, call_id: &str, name: &str, args: &str) {
        emit(&self.app, "chat:tool-start", (session_id, call_id, name, args));
    }

    fn tool_end(&self, session_id: &str, call_id: &str, outcome: &octopus_core::tools::ToolOutcome) {
        emit(&self.app, "chat:tool-end", (session_id, call_id, outcome));
    }

    fn needs_approval(
        &self,
        session_id: &str,
        _call_id: &str,
        request: octopus_core::agent::ApprovalRequest,
    ) {
        emit(&self.app, "chat:approval", (session_id, request));
    }

    fn status(&self, session_id: &str, text: &str) {
        emit(&self.app, "chat:status", (session_id, text));
    }

    fn error(&self, session_id: &str, message: &str) {
        emit(&self.app, "chat:error", (session_id, message));
    }
}

fn emit<T: serde::Serialize + Clone>(app: &tauri::AppHandle, event: &str, payload: T) {
    use tauri::Emitter;
    if let Err(e) = app.emit(event, payload) {
        eprintln!("[octopus] failed to emit {event}: {e}");
    }
}