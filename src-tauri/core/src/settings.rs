use serde::{Deserialize, Serialize};

use crate::db::Store;
use crate::error::Result;
use crate::models::PermissionMode;

const KEY: &str = "app_settings";

/// User preferences. Persisted as a single JSON blob in `kv` so adding a field
/// does not require a migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    /// Forces light or dark rendering; `None` follows the OS.
    pub appearance: Option<String>,
    pub font_family: String,
    pub font_size: u8,
    pub ui_scale: f32,
    pub code_font_family: String,
    pub line_height: f32,
    pub reduce_motion: bool,
    pub high_contrast: bool,
    /// Editor-style keybindings, e.g. `mod+k` -> command palette.
    pub keybinds: std::collections::BTreeMap<String, String>,
    pub default_provider: String,
    pub default_model: String,
    pub default_permission_mode: PermissionMode,
    pub send_on_enter: bool,
    pub auto_title: bool,
    pub stream_markdown: bool,
    pub show_reasoning: bool,
    pub show_token_cost: bool,
    pub prevent_sleep_while_running: bool,
    pub notify_on_complete: bool,
    pub notify_on_input_needed: bool,
    pub local_only: bool,
    pub telemetry: bool,
    /// Directories excluded from indexing, search and git tracking.
    pub ignore_globs: Vec<String>,
    pub max_context_messages: u32,
    pub compact_threshold_percent: u8,
}

impl Default for Settings {
    fn default() -> Self {
        let mut keybinds = std::collections::BTreeMap::new();
        for (cmd, keys) in [
            ("commandPalette", "mod+k"),
            ("newChat", "mod+n"),
            ("toggleSidebar", "mod+b"),
            ("toggleMode", "mod+shift+m"),
            ("search", "mod+f"),
            ("settings", "mod+,"),
            ("send", "mod+enter"),
            ("stop", "escape"),
            ("newLine", "shift+enter"),
            ("toggleTerminal", "mod+`"),
            ("toggleArtifacts", "mod+shift+a"),
        ] {
            keybinds.insert(cmd.to_string(), keys.to_string());
        }

        Self {
            theme: "octopus-dark".into(),
            appearance: None,
            font_family: "Inter".into(),
            font_size: 14,
            ui_scale: 1.0,
            code_font_family: "JetBrains Mono".into(),
            line_height: 1.6,
            reduce_motion: false,
            high_contrast: false,
            keybinds,
            default_provider: "ollama".into(),
            default_model: String::new(),
            default_permission_mode: PermissionMode::Ask,
            send_on_enter: true,
            auto_title: true,
            stream_markdown: true,
            show_reasoning: true,
            show_token_cost: true,
            prevent_sleep_while_running: true,
            notify_on_complete: true,
            notify_on_input_needed: true,
            local_only: false,
            telemetry: false,
            ignore_globs: vec![
                "node_modules".into(),
                "target".into(),
                "dist".into(),
                "build".into(),
                ".git".into(),
                ".venv".into(),
                "__pycache__".into(),
                ".next".into(),
                ".cache".into(),
            ],
            max_context_messages: 200,
            compact_threshold_percent: 85,
        }
    }
}

impl Settings {
    pub fn load(store: &Store) -> Self {
        store
            .kv_get(KEY)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.kv_set(KEY, &serde_json::to_string(self)?)
    }

    /// Clamps user input so a bad value can never produce an unusable UI.
    pub fn normalized(mut self) -> Self {
        self.font_size = self.font_size.clamp(11, 20);
        self.ui_scale = self.ui_scale.clamp(0.8, 1.6);
        self.line_height = self.line_height.clamp(1.2, 2.0);
        self.compact_threshold_percent = self.compact_threshold_percent.clamp(50, 95);
        self.max_context_messages = self.max_context_messages.clamp(20, 2000);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_json() {
        let s = Settings::default();
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.theme, s.theme);
        assert_eq!(back.keybinds.len(), s.keybinds.len());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let partial: Settings = serde_json::from_str(r#"{"theme":"nord"}"#).unwrap();
        assert_eq!(partial.theme, "nord");
        assert_eq!(partial.font_size, Settings::default().font_size);
        assert!(!partial.keybinds.is_empty());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let s = Settings { font_size: 99, ui_scale: 9.0, ..Default::default() }.normalized();
        assert_eq!(s.font_size, 20);
        assert_eq!(s.ui_scale, 1.6);
    }
}
