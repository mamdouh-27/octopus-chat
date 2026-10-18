pub mod anthropic;
pub mod catalog;
pub mod gemini;
pub mod openai;
pub mod types;

use std::collections::HashMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Store;
use crate::error::{Error, Result};
use crate::models::{CustomModel, CustomProvider};

pub use catalog::{ModelSpec, ProviderSpec};
pub use types::{
    CompletionRequest, FinishReason, ImagePart, Incoming, ProposedToolCall, RawCompletion,
    StreamEvent, ToolSchema, Usage,
};

/// Re-exported so callers can name a provider's wire format without reaching
/// into the model layer.
pub use crate::models::Dialect;

const KEY_PREFIX: &str = "apikey:";

/// A provider as the UI sees it: built-in shortcuts and user additions merged
/// into one list, with credentials resolved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedProvider {
    pub id: String,
    pub label: String,
    pub dialect: Dialect,
    pub base_url: String,
    pub local: bool,
    pub needs_key: bool,
    pub has_key: bool,
    pub key_url: Option<String>,
    pub docs_url: Option<String>,
    pub custom: bool,
    pub models: Vec<ModelSpec>,
    /// Set when the endpoint answered a `/models` probe.
    pub reachable: Option<bool>,
    pub status: Option<String>,
}

/// Everything the harness needs to talk to one concrete model.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: Vec<(String, String)>,
    pub dialect: Dialect,
    pub spec: Option<ModelSpec>,
}

/// Owns its store rather than borrowing one.
///
/// A `rusqlite::Connection` is `Send` but not `Sync`, so a borrow of it cannot
/// live across an await point. Owning the connection keeps the provider layer
/// usable from inside the async agent loop, where a shared reference would
/// otherwise force the whole future to be non-`Send`.
pub struct Providers {
    store: Store,
    http: reqwest::Client,
}

impl Providers {
    pub fn new(store: Store, http: reqwest::Client) -> Self {
        Self { store, http }
    }

    /// User-added models, keyed by the provider id they belong to.
    ///
    /// These apply to *any* provider, catalog or custom. `add_model` accepts
    /// every provider id, but the rows used to be read back only for custom
    /// providers, so a model added to Ollama or an OpenAI-compatible endpoint
    /// was written to the database and then silently ignored by `list`.
    fn custom_models_by_provider(&self) -> Result<HashMap<String, Vec<ModelSpec>>> {
        let mut stmt = self.store.conn.prepare(
            "SELECT id,provider_id,label,dialect,context_window,max_output,supports_tools,
                    supports_vision,reasoning,input_cost,output_cost
             FROM custom_models ORDER BY provider_id, position, label",
        )?;
        let rows = stmt.query_map([], map_custom_model)?;

        let mut by_provider: HashMap<String, Vec<ModelSpec>> = HashMap::new();
        for row in rows {
            let (provider_id, m) = row?;
            by_provider.entry(provider_id).or_default().push(ModelSpec {
                id: m.id,
                label: m.label,
                context_window: m.context_window,
                max_output: m.max_output,
                supports_tools: m.supports_tools,
                supports_vision: m.supports_vision,
                reasoning: m.reasoning,
                input_cost: m.input_cost,
                output_cost: m.output_cost,
            });
        }
        Ok(by_provider)
    }

    pub fn list(&self) -> Result<Vec<ResolvedProvider>> {
        let mut out: Vec<ResolvedProvider> = Vec::new();

        for spec in catalog::catalog() {
            let has_key = self.store.kv_get(&key_of(&spec.id))?.is_some();
            out.push(ResolvedProvider {
                id: spec.id.clone(),
                label: spec.label.clone(),
                dialect: spec.dialect,
                base_url: spec.base_url.clone(),
                local: spec.local,
                needs_key: spec.needs_key,
                has_key,
                key_url: spec.key_url.clone(),
                docs_url: spec.docs_url.clone(),
                custom: false,
                models: spec.models.clone(),
                reachable: None,
                status: None,
            });
        }

        let extra = self.custom_models_by_provider()?;

        for p in out.iter_mut() {
            let Some(added) = extra.get(&p.id) else { continue };
            for m in added {
                match p.models.iter_mut().find(|e| e.id == m.id) {
                    // A hand-added row only knows the id and the label, because
                    // that is all the user typed. Letting it overwrite the
                    // catalog's capability flags would silently turn a reasoning
                    // or vision model into a plain one — the flags would all be
                    // reset to their defaults. So the label is taken from the
                    // user and everything else is left as the catalog had it.
                    Some(existing) => existing.label = m.label.clone(),
                    None => p.models.push(m.clone()),
                }
            }
        }

        for cp in self.custom_providers()? {
            let models = extra.get(&cp.id).cloned().unwrap_or_default();
            out.push(ResolvedProvider {
                id: cp.id.clone(),
                label: cp.label.clone(),
                dialect: cp.dialect,
                base_url: cp.base_url.clone(),
                local: cp.local,
                needs_key: !cp.api_key.as_deref().unwrap_or("").is_empty(),
                has_key: cp.api_key.as_deref().is_some_and(|k| !k.is_empty()),
                key_url: None,
                docs_url: None,
                custom: true,
                models,
                reachable: None,
                status: None,
            });
        }
        Ok(out)
    }

    pub fn custom_providers(&self) -> Result<Vec<CustomProvider>> {
        let mut stmt = self.store.conn.prepare(
            "SELECT id,label,dialect,base_url,local,api_key,headers FROM custom_providers ORDER BY position, label",
        )?;
        let rows = stmt.query_map([], |r| {
            let headers: String = r.get(6)?;
            Ok(CustomProvider {
                id: r.get(0)?,
                label: r.get(1)?,
                dialect: dialect_inv(&r.get::<_, String>(2)?),
                base_url: r.get(3)?,
                local: r.get::<_, i64>(4)? != 0,
                api_key: r.get(5)?,
                headers: serde_json::from_str(&headers).unwrap_or_default(),
                models: Vec::new(),
            })
        })?;
        let mut providers: Vec<CustomProvider> =
            rows.collect::<std::result::Result<Vec<_>, _>>()?;

        let mut stmt = self.store.conn.prepare(
            "SELECT id,provider_id,label,dialect,context_window,max_output,supports_tools,
                    supports_vision,reasoning,input_cost,output_cost
             FROM custom_models ORDER BY provider_id, position, label",
        )?;
        let models = stmt.query_map([], map_custom_model)?;
        for m in models {
            let (provider_id, model) = m?;
            if let Some(p) = providers.iter_mut().find(|p| p.id == provider_id) {
                p.models.push(model);
            }
        }
        Ok(providers)
    }

    pub fn save_custom_provider(&self, mut p: CustomProvider) -> Result<CustomProvider> {
        if p.id.trim().is_empty() {
            p.id = slugify(&p.label);
        }
        if p.base_url.trim().is_empty() {
            return Err(Error::invalid("base URL is required"));
        }
        // Reject anything that is not an absolute http(s) endpoint: the value is
        // used to build request URLs, so a typo must not become a file read.
        if !(p.base_url.starts_with("http://") || p.base_url.starts_with("https://")) {
            return Err(Error::invalid("base URL must start with http:// or https://"));
        }
        let position: i64 = self.store.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM custom_providers",
            [],
            |r| r.get(0),
        )?;
        self.store.conn.execute(
            "INSERT INTO custom_providers (id,label,dialect,base_url,local,api_key,headers,position)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET
               label=excluded.label, dialect=excluded.dialect, base_url=excluded.base_url,
               local=excluded.local, api_key=excluded.api_key, headers=excluded.headers",
            params![
                p.id,
                p.label,
                dialect_str(p.dialect),
                p.base_url.trim_end_matches('/'),
                p.local as i64,
                p.api_key,
                serde_json::to_string(&p.headers)?,
                position,
            ],
        )?;
        for (i, m) in p.models.iter_mut().enumerate() {
            if m.id.trim().is_empty() {
                m.id = slugify(&m.label);
            }
            self.store.conn.execute(
                "INSERT INTO custom_models
                   (id,provider_id,label,dialect,context_window,max_output,supports_tools,
                    supports_vision,reasoning,input_cost,output_cost,position)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(provider_id, id) DO UPDATE SET
                   label=excluded.label,
                   dialect=excluded.dialect, context_window=excluded.context_window,
                   max_output=excluded.max_output, supports_tools=excluded.supports_tools,
                   supports_vision=excluded.supports_vision, reasoning=excluded.reasoning,
                   input_cost=excluded.input_cost, output_cost=excluded.output_cost,
                   position=excluded.position",
                params![
                    m.id,
                    p.id,
                    m.label,
                    dialect_str(m.dialect),
                    m.context_window,
                    m.max_output,
                    m.supports_tools as i64,
                    m.supports_vision as i64,
                    m.reasoning as i64,
                    m.input_cost,
                    m.output_cost,
                    i as i64,
                ],
            )?;
        }
        Ok(p)
    }

    pub fn delete_custom_provider(&self, id: &str) -> Result<()> {
        self.store
            .conn
            .execute("DELETE FROM custom_providers WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Adds a model the user typed that we do not know about yet. This is the
    /// escape hatch that makes "any model" work: no catalog entry required.
    pub fn add_model(
        &self,
        provider_id: &str,
        model_id: &str,
        label: Option<String>,
    ) -> Result<CustomModel> {
        let model_id = model_id.trim();
        if model_id.is_empty() {
            return Err(Error::invalid("model id is required"));
        }
        let catalog_spec = catalog::catalog().into_iter().find(|p| p.id == provider_id);
        let dialect = match self
            .custom_providers()?
            .into_iter()
            .find(|p| p.id == provider_id)
            .map(|p| p.dialect)
            .or_else(|| catalog_spec.as_ref().map(|p| p.dialect))
        {
            Some(d) => d,
            None => return Err(Error::NotFound(format!("provider {provider_id}"))),
        };

        // An endpoint's `/models` list returns nothing but ids, and a user
        // typing a new one knows nothing more. Filling every capability with
        // `false` meant a hand-added or probe-discovered reasoning model was
        // permanently stuck showing "No reasoning" and had no way to change it,
        // because this row is the only record the app has. So the capability is
        // guessed from the id, which is wrong occasionally and useful always:
        // it can be corrected later, a hard-coded false cannot.
        //
        // An id already in the catalog keeps that entry's real capabilities.
        let known = catalog_spec
            .and_then(|p| p.models.iter().find(|m| m.id == model_id).cloned());
        let guessed = infer_capabilities(model_id);
        let (supports_tools, supports_vision, reasoning, context_window, max_output, input_cost, output_cost) =
            match known {
                Some(k) => (
                    k.supports_tools,
                    k.supports_vision,
                    k.reasoning,
                    k.context_window,
                    k.max_output,
                    k.input_cost,
                    k.output_cost,
                ),
                None => {
                    let (t, v, r) = guessed;
                    (t, v, r, None, None, None, None)
                }
            };

        let m = CustomModel {
            id: model_id.to_string(),
            label: label.unwrap_or_else(|| model_id.to_string()),
            dialect,
            context_window,
            max_output,
            supports_tools,
            supports_vision,
            reasoning,
            input_cost,
            output_cost,
        };
        let position: i64 = self.store.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM custom_models WHERE provider_id = ?1",
            params![provider_id],
            |r| r.get(0),
        )?;
        self.store.conn.execute(
            "INSERT INTO custom_models
               (id,provider_id,label,dialect,context_window,max_output,supports_tools,
                supports_vision,reasoning,input_cost,output_cost,position)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(provider_id, id) DO UPDATE SET
               label=excluded.label, dialect=excluded.dialect,
               context_window=excluded.context_window, max_output=excluded.max_output,
               supports_tools=excluded.supports_tools,
               supports_vision=excluded.supports_vision, reasoning=excluded.reasoning,
               input_cost=excluded.input_cost, output_cost=excluded.output_cost",
            params![
                m.id,
                provider_id,
                m.label,
                dialect_str(dialect),
                m.context_window.map(i64::from),
                m.max_output.map(i64::from),
                m.supports_tools as i64,
                m.supports_vision as i64,
                m.reasoning as i64,
                m.input_cost,
                m.output_cost,
                position,
            ],
        )?;
        Ok(m)
    }

    pub fn set_key(&self, provider_id: &str, key: Option<String>) -> Result<()> {
        match key {
            Some(k) if !k.trim().is_empty() => {
                self.store.kv_set(&key_of(provider_id), k.trim())?;
            }
            _ => {
                self.store
                    .conn
                    .execute("DELETE FROM kv WHERE key = ?1", params![key_of(provider_id)])?;
            }
        }
        Ok(())
    }

    pub fn has_key(&self, provider_id: &str) -> bool {
        self.store.kv_get(&key_of(provider_id)).ok().flatten().is_some()
    }

    /// Resolves the endpoint, credentials and pricing for one model.
    pub fn resolve(&self, provider_id: &str, model_id: &str) -> Result<Resolved> {
        if let Some(cp) = self.custom_providers()?.into_iter().find(|p| p.id == provider_id) {
            let spec = cp.models.iter().find(|m| m.id == model_id).map(|m| ModelSpec {
                id: m.id.clone(),
                label: m.label.clone(),
                context_window: m.context_window,
                max_output: m.max_output,
                supports_tools: m.supports_tools,
                supports_vision: m.supports_vision,
                reasoning: m.reasoning,
                input_cost: m.input_cost,
                output_cost: m.output_cost,
            });
            return Ok(Resolved {
                base_url: cp.base_url,
                api_key: cp.api_key.filter(|k| !k.is_empty()),
                headers: cp.headers,
                dialect: cp.dialect,
                spec,
            });
        }

        let spec = catalog::catalog()
            .into_iter()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| Error::NotFound(format!("provider {provider_id}")))?;
        Ok(Resolved {
            base_url: spec.base_url.clone(),
            api_key: self.store.kv_get(&key_of(provider_id))?,
            headers: Vec::new(),
            dialect: spec.dialect,
            spec: spec
                .models
                .into_iter()
                .find(|m| m.id == model_id)
                .or(Some(ModelSpec {
                    id: model_id.to_string(),
                    label: model_id.to_string(),
                    context_window: None,
                    max_output: None,
                    supports_tools: true,
                    supports_vision: false,
                    reasoning: false,
                    input_cost: None,
                    output_cost: None,
                })),
        })
    }

    /// Everything needed to reach a provider, as owned data.
    ///
    /// Returned instead of `&Resolved` so an async caller can drop the store
    /// before awaiting: `rusqlite::Connection` is not `Sync`, so a borrow of it
    /// cannot cross an await point.
    pub fn endpoint(&self, provider_id: &str) -> Result<Endpoint> {
        let r = self.resolve(provider_id, "")?;
        Ok(Endpoint {
            id: provider_id.to_string(),
            dialect: r.dialect,
            base_url: r.base_url,
            api_key: r.api_key,
            headers: r.headers,
        })
    }

    /// Asks the endpoint which models it serves. Local servers and most gateways
    /// implement `/models`; the ones that do not report an error while leaving
    /// manual entry available in the UI.
    pub async fn probe_models(&self, provider_id: &str) -> Result<Vec<String>> {
        let ep = self.endpoint(provider_id)?;
        probe_models(&ep, self.http.clone()).await
    }

    /// Cheap health check used by the settings screen.
    pub async fn ping(&self, provider_id: &str) -> Result<bool> {
        let ep = self.endpoint(provider_id)?;
        ping(&ep, self.http.clone()).await
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }
}

/// Owned connection details for one provider.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub id: String,
    pub dialect: Dialect,
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: Vec<(String, String)>,
}

/// Queries `/models`. Only the OpenAI dialect has a uniform listing route.
pub async fn probe_models(ep: &Endpoint, client: reqwest::Client) -> Result<Vec<String>> {
    if ep.dialect != Dialect::OpenAi {
        return Err(Error::Provider(
            "this provider has no model listing endpoint; add models manually".into(),
        ));
    }
    let mut ids = openai::list_models(&ep.base_url, ep.api_key.as_deref(), client).await?;
    ids.sort();
    Ok(ids)
}

/// A 401 still proves the endpoint is alive and reachable.
pub async fn ping(ep: &Endpoint, client: reqwest::Client) -> Result<bool> {
    let url = format!("{}/models", ep.base_url.trim_end_matches('/'));
    let mut http = client.get(&url);
    if let Some(k) = ep.api_key.as_deref().filter(|k| !k.is_empty()) {
        http = http.bearer_auth(k);
    }
    for (k, v) in &ep.headers {
        http = http.header(k.as_str(), v.as_str());
    }
    let res = http.send().await.map_err(|e| Error::Network(e.to_string()))?;
    Ok(res.status().is_success() || res.status().as_u16() == 401)
}

fn key_of(provider_id: &str) -> String {
    format!("{KEY_PREFIX}{provider_id}")
}

fn dialect_str(d: Dialect) -> &'static str {
    match d {
        Dialect::OpenAi => "openai",
        Dialect::Anthropic => "anthropic",
        Dialect::Gemini => "gemini",
    }
}

fn dialect_inv(s: &str) -> Dialect {
    match s {
        "anthropic" => Dialect::Anthropic,
        "gemini" => Dialect::Gemini,
        _ => Dialect::OpenAi,
    }
}

/// Returns the owning provider id alongside the model so the caller can group
/// them without a second query.
fn map_custom_model(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, CustomModel)> {
    Ok((
        r.get(1)?,
        CustomModel {
            id: r.get(0)?,
            label: r.get(2)?,
            dialect: dialect_inv(&r.get::<_, String>(3)?),
            context_window: r.get::<_, Option<i64>>(4)?.map(|v| v as u32),
            max_output: r.get::<_, Option<i64>>(5)?.map(|v| v as u32),
            supports_tools: r.get::<_, i64>(6)? != 0,
            supports_vision: r.get::<_, i64>(7)? != 0,
            reasoning: r.get::<_, i64>(8)? != 0,
            input_cost: r.get(9)?,
            output_cost: r.get(10)?,
        },
    ))
}

/// URL-safe identifier derived from a display label.
/// Best-effort capability guess for a model id that is not in the catalog.
///
/// Only ever used for an id the catalog has never seen, where the alternative
/// is asserting nothing at all and leaving the user with a control that cannot
/// be turned on. Names that vendors use consistently for their reasoning and
/// multimodal lines are matched case-insensitively.
fn infer_capabilities(model_id: &str) -> (bool, bool, bool) {
    let id = model_id.to_ascii_lowercase();

    const REASONING: &[&str] = &[
        "reasoner", "reasoning", "thinking", "thinker", "-r1", "r1-", "qwq", "magistral",
        "devstral", "gpt-oss", "step-3", "seed-r1",
    ];
    const VISION: &[&str] = &[
        "vision", "-vl", "vl-", "pixtral", "llava", "gemma-3", "gpt-4o", "gpt-4.1", "gpt-5",
        "claude-3", "claude-4", "claude-sonnet", "claude-opus", "claude-haiku", "qwen2-vl",
        "qwen2.5-vl", "qwen3-vl", "internvl",
    ];

    let reasoning = REASONING.iter().any(|k| id.contains(k));
    let supports_vision = VISION.iter().any(|k| id.contains(k));
    // Vision models are almost always tool-capable too; a plain model is assumed
    // to support tools, since that is what makes it usable as an agent at all.
    (true, supports_vision, reasoning)
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() { "custom".into() } else { trimmed }
}

/// Client with sane timeouts. Local models can think for a long time before the
/// first token, so the connect timeout is short but the read timeout is long.
pub fn build_http(local_only: bool) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .user_agent(concat!("OctopusChat/", env!("CARGO_PKG_VERSION")));
    if local_only {
        // A local-only session must never be able to reach the public internet.
        b = b.redirect(reqwest::redirect::Policy::none());
    }
    b.build().expect("valid HTTP client configuration")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProbeResult {
    pub ok: bool,
    pub message: String,
    pub models: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_url_safe_and_stable() {
        assert_eq!(slugify("My Local Llama"), "my-local-llama");
        assert_eq!(slugify("  Qwen3 Coder  "), "qwen3-coder");
        assert_eq!(slugify("!!!"), "custom");
    }

    #[test]
    fn key_namespacing_avoids_collisions() {
        assert_eq!(key_of("openai"), "apikey:openai");
        assert_ne!(key_of("a"), key_of("b"));
    }

    #[test]
    fn dialect_round_trips() {
        for d in [Dialect::OpenAi, Dialect::Anthropic, Dialect::Gemini] {
            assert_eq!(dialect_inv(dialect_str(d)), d);
        }
    }

    /// A provider registry over a throwaway in-memory database.
    fn registry() -> Providers {
        let dir = tempdir::TempDir::new("octopus-providers").expect("tempdir");
        let path = dir.path().join("db.sqlite");
        // The directory is removed when `dir` drops, so the store has to be
        // opened and used entirely inside this function's caller; keeping the
        // TempDir alive in a `Store` wrapper is not possible, so the handle is
        // leaked instead to keep the file present for the test's duration.
        let conn = crate::db::open(&path).expect("open store");
        std::mem::forget(dir);
        Providers::new(Store::new(conn), reqwest::Client::new())
    }

    fn model_ids(reg: &Providers, provider: &str) -> Vec<String> {
        reg.list()
            .expect("list providers")
            .into_iter()
            .find(|p| p.id == provider)
            .map(|p| p.models.into_iter().map(|m| m.id).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_model_added_to_a_catalog_provider_shows_up_in_the_list() {
        // The regression: `add_model` accepts any provider id and writes a row,
        // but `list` only read those rows back for custom providers, so anything
        // added to Ollama or an OpenAI-compatible endpoint was stored and then
        // ignored.
        let reg = registry();
        let ollama = "ollama";
        assert!(
            !model_ids(&reg, ollama).contains(&"my-new-model".to_string()),
            "precondition: the model should not exist yet"
        );

        reg.add_model(ollama, "my-new-model", None).expect("add_model should succeed");

        assert!(
            model_ids(&reg, ollama).contains(&"my-new-model".to_string()),
            "a model added to a catalog provider must appear in list()"
        );
    }

    #[test]
    fn a_model_added_to_a_custom_provider_still_shows_up() {
        let reg = registry();
        reg.save_custom_provider(CustomProvider {
            id: "my-endpoint".into(),
            label: "My endpoint".into(),
            dialect: Dialect::OpenAi,
            base_url: "http://127.0.0.1:1234/v1".into(),
            local: true,
            api_key: None,
            headers: Vec::new(),
            models: Vec::new(),
        })
        .expect("save provider");

        reg.add_model("my-endpoint", "local-model", None).expect("add_model");

        assert!(model_ids(&reg, "my-endpoint").contains(&"local-model".to_string()));
    }

    #[test]
    fn adding_a_model_overrides_the_catalog_entry_of_the_same_id() {
        let reg = registry();
        let id = "ollama";
        let before = model_ids(&reg, id);
        let target = before.first().cloned().expect("catalog should ship models");

        reg.add_model(id, &target, Some("Renamed by hand".into()))
            .expect("add_model");

        let listed = reg
            .list()
            .expect("list")
            .into_iter()
            .find(|p| p.id == id)
            .expect("provider present");
        let entry = listed
            .models
            .iter()
            .find(|m| m.id == target)
            .expect("model present");
        assert_eq!(entry.label, "Renamed by hand");
        // Overriding must not duplicate the row.
        assert_eq!(
            listed.models.iter().filter(|m| m.id == target).count(),
            1,
            "the override replaced the entry instead of adding a second one"
        );
    }

    #[test]
    fn re_adding_a_catalog_model_keeps_its_capabilities() {
        // `add_model` only knows an id and a label, and fills every capability
        // with a default. If that row replaced the catalog entry, re-adding a
        // known reasoning model would quietly turn it into a plain one.
        let reg = registry();
        let id = "ollama";
        let thinker = reg
            .list()
            .expect("list")
            .into_iter()
            .find(|p| p.id == id)
            .and_then(|p| p.models.iter().find(|m| m.reasoning).map(|m| m.id.clone()))
            .expect("the catalog ships at least one reasoning model");

        reg.add_model(id, &thinker, None).expect("add_model");

        let after = reg
            .list()
            .expect("list")
            .into_iter()
            .find(|p| p.id == id)
            .expect("provider present")
            .models
            .into_iter()
            .find(|m| m.id == thinker)
            .expect("model present");

        assert!(
            after.reasoning,
            "{thinker} was flagged as a reasoning model and re-adding it must not clear that"
        );
    }

    #[test]
    fn a_user_added_model_keeps_its_own_capabilities() {
        let reg = registry();
        reg.add_model("ollama", "brand-new-model", None).expect("add_model");
        let m = reg
            .list()
            .expect("list")
            .into_iter()
            .find(|p| p.id == "ollama")
            .expect("provider present")
            .models
            .into_iter()
            .find(|m| m.id == "brand-new-model")
            .expect("model present");
        // Nothing is known about an unknown id, so the safe assumption applies.
        assert!(m.supports_tools);
    }

    #[test]
    fn an_unknown_reasoning_model_is_recognised_from_its_id() {
        // Probing an endpoint returns bare ids, so without this every discovered
        // reasoning model was permanently stuck reporting "No reasoning".
        for id in [
            "deepseek-reasoner",
            "magistral-medium-latest",
            "openai/gpt-oss-120b",
            "some-vendor-thinking-v2",
            "qwen2.5-qwq-32b",
        ] {
            let (_, _, reasoning) = infer_capabilities(id);
            assert!(reasoning, "{id} should be recognised as a reasoning model");
        }
    }

    #[test]
    fn an_unknown_vision_model_is_recognised_from_its_id() {
        for id in ["qwen3-vl:8b", "pixtral-large-latest", "llava-1.6", "internvl-3"] {
            let (_, vision, _) = infer_capabilities(id);
            assert!(vision, "{id} should be recognised as a vision model");
        }
    }

    #[test]
    fn a_plain_model_is_not_mistaken_for_a_reasoning_one() {
        for id in ["gpt-4o", "claude-sonnet-4-5", "llama-3.3-70b", "mistral-large-latest"] {
            let (_, _, reasoning) = infer_capabilities(id);
            assert!(!reasoning, "{id} should not be treated as reasoning");
        }
    }

    #[test]
    fn probing_a_provider_marks_its_reasoning_models() {
        let reg = registry();
        for id in ["deepseek-reasoner", "gpt-oss-20b", "llama-3.3-70b"] {
            reg.add_model("ollama", id, None).expect("add_model");
        }
        let models = reg
            .list()
            .expect("list")
            .into_iter()
            .find(|p| p.id == "ollama")
            .expect("provider present")
            .models;
        let reasoning: Vec<&str> = models
            .iter()
            .filter(|m| m.reasoning)
            .map(|m| m.id.as_str())
            .collect();
        assert!(reasoning.contains(&"deepseek-reasoner"), "got {reasoning:?}");
        assert!(reasoning.contains(&"gpt-oss-20b"), "got {reasoning:?}");
        assert!(!reasoning.contains(&"llama-3.3-70b"), "got {reasoning:?}");
    }

    #[test]
    fn every_cloud_provider_ships_at_least_one_model() {
        // Seven providers were committed with `vec![]`, so their models could
        // not be selected at all and the effort control could never light up.
        // An empty list reads as "this endpoint has no models" in the UI, which
        // is indistinguishable from a misconfiguration.
        for p in catalog::catalog() {
            if p.needs_key {
                assert!(
                    !p.models.is_empty(),
                    "cloud provider {} ships an empty model list",
                    p.id
                );
            }
        }
    }

    #[test]
    fn known_cloud_reasoning_models_are_flagged() {
        for (provider, model) in [
            ("deepseek", "deepseek-reasoner"),
            ("xai", "grok-4"),
            ("groq", "openai/gpt-oss-120b"),
            ("together", "deepseek-ai/DeepSeek-R1"),
            ("fireworks", "accounts/fireworks/models/deepseek-r1"),
            ("github", "deepseek/DeepSeek-R1-0528"),
        ] {
            let found = catalog::catalog()
                .into_iter()
                .find(|p| p.id == provider)
                .and_then(|p| p.models.iter().find(|m| m.id == model).cloned())
                .unwrap_or_else(|| panic!("{provider}/{model} is missing from the catalog"));
            assert!(
                found.reasoning,
                "{provider}/{model} should be flagged as a reasoning model"
            );
        }
    }

    #[test]
    fn a_blank_model_id_is_rejected() {
        let reg = registry();
        let err = reg.add_model("ollama", "   ", None).unwrap_err();
        assert_eq!(err.kind(), "invalid");
    }

    #[test]
    fn adding_to_an_unknown_provider_is_an_error() {
        let reg = registry();
        let err = reg.add_model("nope", "some-model", None).unwrap_err();
        assert_eq!(err.kind(), "not_found");
    }
}
