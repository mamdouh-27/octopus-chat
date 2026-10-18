use serde::{Deserialize, Serialize};

use crate::models::Dialect;

/// One block of conversation history, normalized across dialects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Incoming {
    User {
        text: String,
        /// `mime` + base64 payload, already stripped of any data-url prefix.
        images: Vec<ImagePart>,
    },
    Assistant {
        text: String,
        tool_calls: Vec<ProposedToolCall>,
    },
    /// Result of a tool the agent ran, fed back so the model can react.
    ToolResult {
        call_id: String,
        name: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImagePart {
    pub mime: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON argument string as streamed; parsed lazily so a truncated
    /// stream still surfaces something readable in the UI.
    pub arguments: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cached_input_tokens: u32,
}

impl Usage {
    /// Best-effort cost in USD. Local models report zero.
    pub fn cost_usd(&self, input_per_mtok: Option<f64>, output_per_mtok: Option<f64>) -> f64 {
        let i = input_per_mtok.unwrap_or(0.0) * self.input_tokens as f64 / 1_000_000.0;
        let o = output_per_mtok.unwrap_or(0.0) * self.output_tokens as f64 / 1_000_000.0;
        i + o
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolUse,
    Length,
    ContentFilter,
    Error,
}

/// Incremental output from a streaming completion.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum StreamEvent {
    TextDelta { text: String },
    ReasoningDelta { text: String },
    /// Streamed tool call fragments, keyed by the model's tool-call index.
    ToolCallStart { index: u32, id: String, name: String },
    ToolCallArgs { index: u32, delta: String },
    Usage { usage: Usage },
    Done { reason: FinishReason },
}

/// Fully assembled result of one completion, after every delta was also
/// forwarded to the UI through the event channel.
#[derive(Debug, Clone, Default)]
pub struct RawCompletion {
    pub text: String,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ProposedToolCall>,
    pub usage: Usage,
    pub finish: FinishReason,
}

impl Default for FinishReason {
    fn default() -> Self {
        Self::Stop
    }
}

/// A tool the model is allowed to call this turn.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments object.
    pub parameters: serde_json::Value,
}

/// Everything needed to issue one completion request.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub dialect: Dialect,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub extra_headers: Vec<(String, String)>,
    pub system: String,
    pub messages: Vec<Incoming>,
    pub tools: Vec<ToolSchema>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub reasoning_effort: Option<String>,
    /// Local endpoints are allowed to run long without a keepalive.
    pub no_timeout: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_is_zero_for_local_models() {
        let u = Usage { input_tokens: 1_000_000, output_tokens: 1_000_000, ..Default::default() };
        assert_eq!(u.cost_usd(None, None), 0.0);
    }

    #[test]
    fn cost_sums_both_directions() {
        let u = Usage { input_tokens: 1_000_000, output_tokens: 500_000, ..Default::default() };
        let c = u.cost_usd(Some(3.0), Some(15.0));
        assert!((c - 10.5).abs() < 1e-9, "got {c}");
    }
}
