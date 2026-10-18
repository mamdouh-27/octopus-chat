use serde::{Deserialize, Serialize};

use crate::models::Dialect;

/// A provider the app knows how to talk to. Users can add more at runtime; the
/// catalog here is only the set of shortcuts we pre-configure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSpec {
    pub id: String,
    pub label: String,
    pub dialect: Dialect,
    pub base_url: String,
    /// Local providers skip key requirements and any outbound-network note.
    pub local: bool,
    pub needs_key: bool,
    pub key_url: Option<String>,
    pub docs_url: Option<String>,
    /// Model ids to offer before the user types their own.
    pub models: Vec<ModelSpec>,
    /// Some endpoints expose a model list we can query at runtime.
    pub list_models: bool,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    pub id: String,
    pub label: String,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub reasoning: bool,
    /// USD per million tokens, used for the running cost estimate.
    pub input_cost: Option<f64>,
    pub output_cost: Option<f64>,
}

impl ModelSpec {
    fn full(id: &str, ctx: u32) -> Self {
        Self {
            id: id.into(),
            label: id.into(),
            context_window: Some(ctx),
            max_output: None,
            supports_tools: true,
            supports_vision: true,
            reasoning: false,
            input_cost: None,
            output_cost: None,
        }
    }

    fn cheap(id: &str, ctx: u32) -> Self {
        Self { supports_vision: false, reasoning: false, ..Self::full(id, ctx) }
    }

    fn thinker(id: &str, ctx: u32) -> Self {
        Self { reasoning: true, ..Self::full(id, ctx) }
    }
}

fn spec(
    id: &str,
    label: &str,
    dialect: Dialect,
    base_url: &str,
    local: bool,
    needs_key: bool,
    models: Vec<ModelSpec>,
) -> ProviderSpec {
    ProviderSpec {
        id: id.into(),
        label: label.into(),
        dialect,
        base_url: base_url.into(),
        local,
        needs_key,
        key_url: None,
        docs_url: None,
        models,
        list_models: true,
        default_model: None,
    }
}

/// Built-in catalog.
///
/// Model ids track current releases; anything not listed can still be added by
/// hand from Settings, and most endpoints can be asked what they serve.
pub fn catalog() -> Vec<ProviderSpec> {
    vec![
        // ------------------------------------------------------------- local
        spec(
            "ollama",
            "Ollama (local)",
            Dialect::OpenAi,
            "http://127.0.0.1:11434/v1",
            true,
            false,
            vec![
                ModelSpec::thinker("qwen3-coder:30b", 262_144),
                ModelSpec::full("qwen3:8b", 40_960),
                ModelSpec::full("qwen3-vl:8b", 32_768),
                ModelSpec::full("llama3.3:70b-instruct-q8_0", 128_000),
                ModelSpec::full("gpt-oss:120b", 131_072),
                ModelSpec::thinker("gpt-oss:20b", 131_072),
                ModelSpec::full("devstral:24b", 131_072),
                ModelSpec::thinker("deepseek-r1:14b", 131_072),
                ModelSpec::thinker("magistral:24b", 131_072),
            ],
        )
        .local_only(),
        // Ollama's hosted tier, kept separate from the local daemon so the
        // "nothing leaves this machine" promise stays visible in the list.
        spec(
            "ollama-cloud",
            "Ollama Cloud",
            Dialect::OpenAi,
            "https://ollama.com/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("gpt-oss:120b", 131_072),
                ModelSpec::thinker("gpt-oss:20b", 131_072),
                ModelSpec::thinker("deepseek-v3.1:671b", 163_840),
                ModelSpec::thinker("qwen3-coder:480b", 262_144),
                ModelSpec::full("qwen3-vl:235b", 262_144),
            ],
        )
        .keyed("https://ollama.com/settings/keys"),
        spec(
            "lmstudio",
            "LM Studio (local)",
            Dialect::OpenAi,
            "http://127.0.0.1:1234/v1",
            true,
            false,
            vec![],
        )
        .local_only(),
        spec(
            "llamacpp",
            "llama.cpp server (local)",
            Dialect::OpenAi,
            "http://127.0.0.1:8080/v1",
            true,
            false,
            vec![],
        )
        .local_only(),
        // ------------------------------------------------------------- cloud
        spec(
            "anthropic",
            "Anthropic",
            Dialect::Anthropic,
            "https://api.anthropic.com/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("claude-opus-4-5", 200_000),
                ModelSpec::thinker("claude-sonnet-4-5", 200_000),
                ModelSpec::thinker("claude-haiku-4-5", 200_000),
            ],
        )
        .keyed("https://console.anthropic.com/settings/keys")
        .docs("https://docs.anthropic.com/en/docs/about-claude/models"),
        spec(
            "openai",
            "OpenAI",
            Dialect::OpenAi,
            "https://api.openai.com/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("gpt-6-sol", 400_000),
                ModelSpec::thinker("gpt-6", 400_000),
                ModelSpec::thinker("gpt-6-mini", 400_000),
                ModelSpec::thinker("gpt-5.1-codex", 400_000),
                ModelSpec::thinker("gpt-5.1", 400_000),
                ModelSpec::cheap("gpt-5.1-mini", 400_000),
                ModelSpec::thinker("o4-mini", 200_000),
            ],
        )
        .keyed("https://platform.openai.com/api-keys")
        .docs("https://platform.openai.com/docs/models"),
        spec(
            "openrouter",
            "OpenRouter",
            Dialect::OpenAi,
            "https://openrouter.ai/api/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("anthropic/claude-sonnet-4.5", 200_000),
                ModelSpec::thinker("openai/gpt-6", 400_000),
                ModelSpec::thinker("google/gemini-3-pro", 1_048_576),
                ModelSpec::thinker("qwen/qwen3-coder", 262_144),
                ModelSpec::thinker("deepseek/deepseek-v3.2", 163_840),
                ModelSpec::thinker("z-ai/glm-4.7", 204_800),
            ],
        )
        .keyed("https://openrouter.ai/keys"),
        spec(
            "gemini",
            "Google Gemini",
            Dialect::Gemini,
            "https://generativelanguage.googleapis.com/v1beta",
            false,
            true,
            vec![
                ModelSpec::thinker("gemini-3-pro", 1_048_576),
                ModelSpec::full("gemini-3-flash", 1_048_576),
                ModelSpec::thinker("gemini-2.5-pro", 1_048_576),
                ModelSpec::full("gemini-2.5-flash", 1_048_576),
            ],
        )
        .keyed("https://aistudio.google.com/apikey"),
spec(
            "groq",
            "Groq",
            Dialect::OpenAi,
            "https://api.groq.com/openai/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("openai/gpt-oss-120b", 131_072),
                ModelSpec::thinker("openai/gpt-oss-20b", 131_072),
                ModelSpec::thinker("deepseek/deepseek-r1-distill-llama-70b", 131_072),
                ModelSpec::cheap("llama-3.3-70b-versatile", 131_072),
                ModelSpec::cheap("meta-llama/llama-4-scout-17b-16e-instruct", 131_072),
            ],
        )
        .keyed("https://console.groq.com/keys"),
        spec(
            "mistral",
            "Mistral",
            Dialect::OpenAi,
            "https://api.mistral.ai/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("magistral-medium-latest", 40_000),
                ModelSpec::thinker("magistral-small-latest", 40_000),
                ModelSpec::thinker("devstral-medium-latest", 128_000),
                ModelSpec::thinker("devstral-small-latest", 128_000),
                ModelSpec::cheap("pixtral-large-latest", 128_000),
                ModelSpec::cheap("mistral-large-latest", 128_000),
            ],
        )
        .keyed("https://console.mistral.ai/api-keys"),
        spec(
            "deepseek",
            "DeepSeek",
            Dialect::OpenAi,
            "https://api.deepseek.com/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("deepseek-reasoner", 128_000),
                ModelSpec::cheap("deepseek-chat", 128_000),
            ],
        )
        .keyed("https://platform.deepseek.com/api_keys"),
        spec(
            "xai",
            "xAI Grok",
            Dialect::OpenAi,
            "https://api.x.ai/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("grok-4", 256_000),
                ModelSpec::thinker("grok-4-fast-reasoning", 256_000),
                ModelSpec::thinker("grok-code-fast-1", 256_000),
                ModelSpec::cheap("grok-4-fast-non-reasoning", 2_000_000),
                ModelSpec::cheap("grok-3-mini", 131_072),
            ],
        )
        .keyed("https://console.x.ai"),
        spec(
            "together",
            "Together AI",
            Dialect::OpenAi,
            "https://api.together.xyz/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("deepseek-ai/DeepSeek-R1", 128_000),
                ModelSpec::thinker("Qwen/Qwen3-235B-A22B-thinking-2507", 131_072),
                ModelSpec::thinker("openai/gpt-oss-120b", 131_072),
                ModelSpec::cheap("meta-llama/Llama-3.3-70B-Instruct-Turbo", 131_072),
                ModelSpec::cheap("Qwen/Qwen2.5-Coder-32B-Instruct", 131_072),
            ],
        )
        .keyed("https://api.together.ai/settings/api-keys"),
        spec(
            "fireworks",
            "Fireworks AI",
            Dialect::OpenAi,
            "https://api.fireworks.ai/inference/v1",
            false,
            true,
            vec![
                ModelSpec::thinker("accounts/fireworks/models/deepseek-r1", 128_000),
                ModelSpec::thinker("accounts/fireworks/models/qwen3-235b-a22b", 131_072),
                ModelSpec::thinker("accounts/fireworks/models/gpt-oss-120b", 131_072),
                ModelSpec::cheap("accounts/fireworks/models/llama-v3p3-70b-instruct", 131_072),
            ],
        )
        .keyed("https://fireworks.ai/api-keys"),
        spec(
            "github",
            "GitHub Models",
            Dialect::OpenAi,
            "https://models.github.ai/inference",
            false,
            true,
            vec![
                ModelSpec::thinker("openai/gpt-5.1", 400_000),
                ModelSpec::thinker("openai/gpt-5-mini", 400_000),
                ModelSpec::thinker("deepseek/DeepSeek-R1-0528", 128_000),
                ModelSpec::thinker("microsoft/Phi-4-reasoning", 16_384),
                ModelSpec::cheap("meta/Llama-3.3-70B-Instruct", 131_072),
            ],
        )
        .keyed("https://github.com/settings/tokens"),
    ]
}

impl ProviderSpec {
    fn local_only(mut self) -> Self {
        self.needs_key = false;
        self.list_models = true;
        self
    }

    fn keyed(mut self, url: &str) -> Self {
        self.key_url = Some(url.into());
        self
    }

    fn docs(mut self, url: &str) -> Self {
        self.docs_url = Some(url.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique() {
        let cat = catalog();
let mut ids: Vec<&str> = cat.iter().map(|p| p.id.as_str()).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate provider id in catalog");
    }

    #[test]
    fn local_providers_never_require_a_key() {
        for p in catalog().iter().filter(|p| p.local) {
            assert!(!p.needs_key, "{} is local but requires a key", p.id);
            assert!(p.base_url.contains("127.0.0.1"), "{} must stay on loopback", p.id);
        }
    }

    #[test]
    fn every_cloud_provider_has_a_key_url() {
        for p in catalog().iter().filter(|p| !p.local && p.needs_key) {
            assert!(p.key_url.is_some(), "{} needs a key but has no key_url", p.id);
        }
    }

    #[test]
    fn ollama_has_a_local_and_a_cloud_entry() {
        let local = catalog().into_iter().find(|p| p.id == "ollama").unwrap();
        let cloud = catalog().into_iter().find(|p| p.id == "ollama-cloud").unwrap();
        assert!(local.local, "the ollama entry must stay on loopback");
        assert!(!cloud.local);
        assert!(cloud.needs_key, "the hosted tier needs an account key");
        assert!(cloud.base_url.starts_with("https://"));
    }

    #[test]
    fn model_ids_do_not_contain_retired_releases() {
        // GPT-5.5 was retired in October 2026; it must not be offered.
        for p in catalog() {
            for m in &p.models {
                assert_ne!(m.id, "gpt-5.5", "gpt-5.5 is retired");
            }
        }
    }
}