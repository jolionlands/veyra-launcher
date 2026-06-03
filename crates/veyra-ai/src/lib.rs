use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

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
    pub fn parse(json: &str) -> Result<Self, ToolManifestError> {
        parse_tool_manifest(json)
    }

    pub fn validate(&self) -> Result<(), ToolManifestError> {
        if self.name.trim().is_empty() {
            return Err(ToolManifestError::Validation {
                message: "tool name must be present".to_string(),
            });
        }

        if self.description.trim().is_empty() {
            return Err(ToolManifestError::Validation {
                message: "tool description must be present".to_string(),
            });
        }

        if self.runner.command.trim().is_empty() {
            return Err(ToolManifestError::Validation {
                message: "runner command must be present".to_string(),
            });
        }

        if !self.parameters.is_object() {
            return Err(ToolManifestError::Validation {
                message: "parameters must be a JSON object".to_string(),
            });
        }

        if self.timeout_ms == 0 {
            return Err(ToolManifestError::InvalidTimeout {
                value: self.timeout_ms,
            });
        }

        if !self.safety.level.is_supported() {
            return Err(ToolManifestError::UnsupportedSafety {
                level: self.safety.level.to_string(),
            });
        }

        Ok(())
    }

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
    #[serde(other)]
    Unsupported,
}

impl SafetyLevel {
    fn is_supported(&self) -> bool {
        !matches!(self, Self::Unsupported)
    }
}

impl std::fmt::Display for SafetyLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SafetyLevel::Read => f.write_str("read"),
            SafetyLevel::Write => f.write_str("write"),
            SafetyLevel::Execute => f.write_str("execute"),
            SafetyLevel::Admin => f.write_str("admin"),
            SafetyLevel::Network => f.write_str("network"),
            SafetyLevel::Unsupported => f.write_str("unsupported"),
        }
    }
}

#[derive(Debug, Error)]
pub enum ToolManifestError {
    #[error("invalid manifest JSON: {source}")]
    Parse {
        #[from]
        source: serde_json::Error,
    },
    #[error("manifest validation failed: {message}")]
    Validation { message: String },
    #[error("unsupported safety behavior: {level}")]
    UnsupportedSafety { level: String },
    #[error("invalid timeout_ms: {value} (must be greater than zero)")]
    InvalidTimeout { value: u64 },
    #[error("could not read manifest directory '{path}': {source}")]
    DirectoryRead { path: String, source: io::Error },
    #[error("could not read manifest file '{path}': {source}")]
    FileRead { path: String, source: io::Error },
    #[error("manifest file '{path}' contains an invalid manifest: {source}")]
    FileInvalid {
        path: String,
        source: Box<ToolManifestError>,
    },
}

pub fn parse_tool_manifest(json: &str) -> Result<ToolManifest, ToolManifestError> {
    let manifest: ToolManifest = serde_json::from_str(json)?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn load_tool_manifests_from_directory<P: AsRef<Path>>(
    directory: P,
) -> Result<Vec<ToolManifest>, ToolManifestError> {
    let directory = directory.as_ref();
    let entries = fs::read_dir(directory).map_err(|err| ToolManifestError::DirectoryRead {
        path: directory.display().to_string(),
        source: err,
    })?;

    let mut manifest_paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();

    manifest_paths.sort();

    manifest_paths
        .iter()
        .map(|path| parse_tool_manifest_file(path))
        .collect()
}

fn parse_tool_manifest_file(path: &Path) -> Result<ToolManifest, ToolManifestError> {
    let raw = fs::read_to_string(path).map_err(|err| ToolManifestError::FileRead {
        path: path.display().to_string(),
        source: err,
    })?;

    parse_tool_manifest(&raw).map_err(|err| ToolManifestError::FileInvalid {
        path: path.display().to_string(),
        source: Box::new(err),
    })
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("veyra-ai-manifest-test-{nanos}"));
        path
    }

    fn sample_manifest_json() -> &'static str {
        r#"
            {
              "name": "repair",
              "description": "Run a repair command",
              "keywords": ["repair"],
              "platforms": ["windows"],
              "runner": {
                "kind": "process",
                "command": "repair.cmd",
                "args": []
              },
              "parameters": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
              },
              "safety": {
                "level": "read",
                "requires_confirmation": false,
                "requires_admin": false
              },
              "timeout_ms": 30000
            }
        "#
    }

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

    #[test]
    fn parse_tool_manifest_validates_timeout() {
        let manifest_json = r#"{
            "name": "repair",
            "description": "Run a repair command",
            "runner": {
                "kind": "process",
                "command": "repair.cmd",
                "args": []
            },
            "parameters": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "safety": {
                "level": "read",
                "requires_confirmation": false,
                "requires_admin": false
            },
            "timeout_ms": 0
        }"#;

        let result = parse_tool_manifest(manifest_json);
        assert!(matches!(
            result,
            Err(ToolManifestError::InvalidTimeout { value: 0 })
        ));
    }

    #[test]
    fn parse_tool_manifest_rejects_unsupported_safety() {
        let manifest_json = r#"{
            "name": "repair",
            "description": "Run a repair command",
            "runner": {
                "kind": "process",
                "command": "repair.cmd",
                "args": []
            },
            "parameters": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "safety": {
                "level": "danger",
                "requires_confirmation": false,
                "requires_admin": false
            },
            "timeout_ms": 30000
        }"#;

        let result = parse_tool_manifest(manifest_json);
        assert!(
            matches!(result, Err(ToolManifestError::UnsupportedSafety { level }) if level == "unsupported")
        );
    }

    #[test]
    fn load_tool_manifests_from_directory_ignores_non_json() {
        let base = temp_dir();
        fs::create_dir_all(&base).unwrap();
        let manifest_path = base.join("tool.json");
        let other_path = base.join("readme.txt");
        fs::write(&manifest_path, sample_manifest_json()).unwrap();
        fs::write(&other_path, "ignore me").unwrap();

        let manifests = load_tool_manifests_from_directory(&base).unwrap();

        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].name, "repair");
        assert_eq!(manifests[0].runner.kind, ToolRunnerKind::Process);

        fs::remove_dir_all(&base).ok();
    }
}
