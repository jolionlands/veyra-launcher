use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiProviderConfig {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub supports_streaming: bool,
    #[serde(default)]
    pub supports_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    pub runner: ToolRunner,
    #[serde(default = "empty_object_schema")]
    pub parameters: Value,
    pub safety: ToolSafety,
    #[serde(default = "default_tool_timeout_ms")]
    pub timeout_ms: u64,
}

impl ToolManifest {
    pub fn requires_confirmation(&self) -> bool {
        self.safety.requires_confirmation
            || matches!(
                self.safety.level,
                SafetyLevel::Write
                    | SafetyLevel::Execute
                    | SafetyLevel::Admin
                    | SafetyLevel::Network
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRunner {
    pub kind: ToolRunnerKind,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRunnerKind {
    Process,
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSafety {
    pub level: SafetyLevel,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub requires_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    Read,
    Write,
    Execute,
    Admin,
    Network,
}

fn enabled_by_default() -> bool {
    true
}

fn default_timeout_ms() -> u64 {
    60_000
}

fn default_tool_timeout_ms() -> u64 {
    30_000
}

fn empty_object_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_tools_require_confirmation() {
        let manifest = ToolManifest {
            name: "repair".to_string(),
            description: "Run a repair command".to_string(),
            keywords: Vec::new(),
            platforms: vec!["windows".to_string()],
            runner: ToolRunner {
                kind: ToolRunnerKind::Process,
                command: "repair.cmd".to_string(),
                args: Vec::new(),
            },
            parameters: empty_object_schema(),
            safety: ToolSafety {
                level: SafetyLevel::Execute,
                requires_confirmation: false,
                requires_admin: false,
            },
            timeout_ms: default_tool_timeout_ms(),
        };

        assert!(manifest.requires_confirmation());
    }
}
