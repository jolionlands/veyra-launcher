use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct VeyraConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub hotkeys: HotkeysConfig,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub commands: Vec<CommandEntry>,
    #[serde(default)]
    pub web_search: Vec<WebSearchEntry>,
    #[serde(default)]
    pub catalogs: Vec<CatalogProfile>,
    #[serde(default)]
    pub ai: AiConfig,
}

impl VeyraConfig {
    pub fn from_toml_str(input: impl AsRef<str>) -> Result<Self, toml::de::Error> {
        toml::from_str(input.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    #[serde(default = "default_startup")]
    pub startup: bool,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default = "default_history_limit")]
    pub history_limit: u32,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            startup: default_startup(),
            local_only: false,
            history_limit: default_history_limit(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeysConfig {
    #[serde(default = "default_toggle_hotkey")]
    pub toggle: String,
    #[serde(default = "default_settings_hotkey")]
    pub settings: String,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self {
            toggle: default_toggle_hotkey(),
            settings: default_settings_hotkey(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_true")]
    pub blur: bool,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default = "default_max_results")]
    pub max_results: u32,
    #[serde(default = "default_true")]
    pub show_preview: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            opacity: default_opacity(),
            blur: default_true(),
            font_size: default_font_size(),
            max_results: default_max_results(),
            show_preview: default_true(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommandEntry {
    pub id: String,
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WebSearchEntry {
    pub id: String,
    pub alias: String,
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogProfile {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub include_patterns: Vec<String>,
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub follow_symlinks: bool,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub enabled: bool,
}

impl Default for CatalogProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            paths: Vec::new(),
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            recursive: true,
            follow_symlinks: false,
            max_depth: None,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ai_provider")]
    pub default_provider: String,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default)]
    pub warmup_on_startup: bool,
    #[serde(default)]
    pub providers: Vec<AiProvider>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: default_ai_provider(),
            local_only: false,
            warmup_on_startup: false,
            providers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiProvider {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_ai_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub supports_streaming: bool,
    #[serde(default)]
    pub supports_tools: bool,
}

impl Default for AiProvider {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            base_url: String::new(),
            model: String::new(),
            api_key_env: None,
            local_only: false,
            enabled: true,
            timeout_ms: default_ai_timeout_ms(),
            supports_streaming: false,
            supports_tools: false,
        }
    }
}

fn default_startup() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_history_limit() -> u32 {
    5000
}

fn default_toggle_hotkey() -> String {
    "Alt+Space".to_string()
}

fn default_settings_hotkey() -> String {
    "Ctrl+,".to_string()
}

fn default_theme() -> String {
    "dark-acrylic".to_string()
}

fn default_opacity() -> f32 {
    0.92
}

fn default_font_size() -> u32 {
    15
}

fn default_max_results() -> u32 {
    10
}

fn default_ai_provider() -> String {
    "local".to_string()
}

fn default_ai_timeout_ms() -> u64 {
    60_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_general_hotkeys_and_appearance_toml() {
        let value = r#"
            [general]
            startup = true
            local_only = false
            history_limit = 5000

            [hotkeys]
            toggle = "Alt+Space"
            settings = "Ctrl+,"

            [appearance]
            theme = "dark-acrylic"
            opacity = 0.92
            blur = true
            font_size = 15
            max_results = 10
            show_preview = true
        "#;

        let config = VeyraConfig::from_toml_str(value).unwrap();

        assert!(config.general.startup);
        assert!(!config.general.local_only);
        assert_eq!(config.general.history_limit, 5000);
        assert_eq!(config.hotkeys.toggle, "Alt+Space");
        assert_eq!(config.hotkeys.settings, "Ctrl+,");
        assert_eq!(config.appearance.theme, "dark-acrylic");
        assert_eq!(config.appearance.opacity, 0.92);
        assert!(config.appearance.blur);
        assert_eq!(config.appearance.font_size, 15);
        assert_eq!(config.appearance.max_results, 10);
        assert!(config.appearance.show_preview);
    }

    #[test]
    fn parses_command_and_web_search_toml() {
        let value = r#"
            [[commands]]
            id = "settings.display"
            label = "Settings: Display"
            command = "explorer.exe"
            args = ["ms-settings:display"]
            terminal = false
            requires_confirmation = false
            keywords = ["display", "monitor", "resolution"]

            [[commands]]
            id = "wm.repair_bar"
            label = "WM: Repair Bar"
            command = "%USERPROFILE%\\scripts\\repair-bar.cmd"
            terminal = true
            requires_confirmation = false
            keywords = ["bar", "repair", "window manager"]

            [[web_search]]
            id = "github.code"
            alias = "gh"
            label = "GitHub Code"
            url = "https://github.com/search?q={query}&type=code"
        "#;

        let config = VeyraConfig::from_toml_str(value).unwrap();

        assert_eq!(config.commands.len(), 2);
        assert_eq!(config.commands[0].id, "settings.display");
        assert_eq!(config.commands[0].args, vec!["ms-settings:display"]);
        assert!(!config.commands[0].terminal);
        assert_eq!(config.web_search.len(), 1);
        assert_eq!(config.web_search[0].alias, "gh");
        assert_eq!(
            config.web_search[0].url,
            "https://github.com/search?q={query}&type=code"
        );
    }

    #[test]
    fn parses_ai_toml() {
        let value = r#"
            [ai]
            enabled = true
            default_provider = "local"
            local_only = false
            warmup_on_startup = false

            [[ai.providers]]
            id = "local"
            label = "Local OpenAI-compatible"
            base_url = "http://127.0.0.1:8080/v1"
            model = "local-model"
            api_key_env = ""
            local_only = true
            enabled = true
            timeout_ms = 60000
            supports_streaming = true
            supports_tools = true
        "#;

        let config = VeyraConfig::from_toml_str(value).unwrap();

        assert!(config.ai.enabled);
        assert_eq!(config.ai.default_provider, "local");
        assert_eq!(config.ai.providers.len(), 1);
        assert_eq!(config.ai.providers[0].id, "local");
        assert!(config.ai.providers[0].supports_tools);
    }

    #[test]
    fn apply_defaults_for_missing_sections() {
        let value = "";
        let config = VeyraConfig::from_toml_str(value).unwrap();

        assert!(config.general.startup);
        assert_eq!(config.hotkeys.toggle, "Alt+Space");
        assert_eq!(config.appearance.theme, "dark-acrylic");
        assert!(config.commands.is_empty());
        assert!(config.web_search.is_empty());
        assert!(config.catalogs.is_empty());
        assert_eq!(config.ai.default_provider, "local");
        assert_eq!(config.ai.providers.len(), 0);
    }
}
