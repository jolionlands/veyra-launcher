use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use veyra_ai::{ToolManifest, ToolRunnerKind, load_tool_manifests_from_directory};
use veyra_core::config::{PluginEntry, PluginKind};
use veyra_core::{Action, ActionKind, CatalogItem, ItemCategory};
use veyra_protocol::{
    CatalogParams, CatalogResult, ExecuteParams, ExecuteResult, InitializeParams, JsonRpcRequest,
    JsonRpcResponse, METHOD_CATALOG, METHOD_EXECUTE, METHOD_INITIALIZE, METHOD_SHUTDOWN,
    METHOD_SUGGEST, PROTOCOL_VERSION, ProtocolAction, ProtocolActionKind, ProtocolCatalogItem,
    ProtocolItemCategory, RequestId, SuggestParams, SuggestResult,
};

const JSON_RPC_ACTION_PREFIX: &str = "veyra-json-rpc-plugin:";
const DEFAULT_ACTION_ID: &str = "default";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginCatalogLoad {
    pub items: Vec<CatalogItem>,
    pub diagnostics: Vec<String>,
    pub json_rpc_item_count: usize,
    pub manifest_item_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcActionRef {
    pub plugin_id: String,
    pub item_id: String,
    pub action_id: String,
}

pub fn process_plugin_item(entry: &PluginEntry) -> Option<CatalogItem> {
    if !entry.enabled || entry.kind != PluginKind::Process || entry.command.trim().is_empty() {
        return None;
    }

    let id = plugin_id(entry);
    let label = non_empty(&entry.label).unwrap_or_else(|| entry.command.clone());
    let subtitle = non_empty(&entry.description).unwrap_or_else(|| entry.command.clone());
    let mut action = Action::launch_with_args(entry.command.clone(), entry.args.clone());
    action.requires_confirmation = entry.requires_confirmation;

    Some(
        CatalogItem::new(id, label, ItemCategory::Tool, "plugin")
            .subtitle(subtitle)
            .keywords(entry.keywords.clone())
            .action(action),
    )
}

pub fn load_plugin_extensions(profile_dir: &Path, plugins: &[PluginEntry]) -> PluginCatalogLoad {
    let mut load = PluginCatalogLoad::default();

    for plugin in plugins {
        if !plugin.enabled || plugin.kind != PluginKind::JsonRpcStdio {
            continue;
        }

        match load_json_rpc_catalog(plugin) {
            Ok(items) => {
                load.json_rpc_item_count += items.len();
                load.diagnostics.push(format!(
                    "Loaded {} JSON-RPC catalog items from {}",
                    items.len(),
                    plugin_label(plugin)
                ));
                load.items.extend(items);
            }
            Err(error) => {
                load.error_count += 1;
                load.diagnostics.push(format!(
                    "Could not load JSON-RPC plugin {}: {error}",
                    plugin_label(plugin)
                ));
            }
        }
    }

    for directory in manifest_directories(profile_dir) {
        if !directory.is_dir() {
            continue;
        }

        match load_tool_manifests_from_directory(&directory) {
            Ok(manifests) => {
                for manifest in manifests {
                    match manifest_item(manifest) {
                        Some(item) => {
                            load.manifest_item_count += 1;
                            load.items.push(item);
                        }
                        None => {
                            load.error_count += 1;
                            load.diagnostics.push(format!(
                                "Skipped unsupported tool manifest in {}",
                                directory.display()
                            ));
                        }
                    }
                }
            }
            Err(error) => {
                load.error_count += 1;
                load.diagnostics.push(format!(
                    "Could not load tool manifests from {}: {error}",
                    directory.display()
                ));
            }
        }
    }

    if load.manifest_item_count > 0 {
        load.diagnostics.push(format!(
            "Loaded {} tool manifest items",
            load.manifest_item_count
        ));
    }

    load
}

pub fn load_plugin_suggestions(plugins: &[PluginEntry], query: &str) -> PluginCatalogLoad {
    let mut load = PluginCatalogLoad::default();
    if query.trim().is_empty() {
        return load;
    }

    for plugin in plugins {
        if !plugin.enabled || plugin.kind != PluginKind::JsonRpcStdio {
            continue;
        }

        match load_json_rpc_suggestions(plugin, query) {
            Ok(items) => {
                load.json_rpc_item_count += items.len();
                load.items.extend(items);
            }
            Err(error) => {
                load.error_count += 1;
                load.diagnostics.push(format!(
                    "Could not load JSON-RPC suggestions from {}: {error}",
                    plugin_label(plugin)
                ));
            }
        }
    }

    load
}

pub fn json_rpc_action_command(
    plugin_id: impl Into<String>,
    item_id: impl Into<String>,
    action_id: impl Into<String>,
) -> String {
    let reference = JsonRpcActionRef {
        plugin_id: plugin_id.into(),
        item_id: item_id.into(),
        action_id: action_id.into(),
    };
    format!(
        "{JSON_RPC_ACTION_PREFIX}{}",
        serde_json::to_string(&reference).expect("json-rpc action reference serializes")
    )
}

pub fn parse_json_rpc_action_command(command: &str) -> Option<JsonRpcActionRef> {
    let raw = command.strip_prefix(JSON_RPC_ACTION_PREFIX)?;
    serde_json::from_str(raw).ok()
}

pub fn execute_json_rpc_action(
    plugin: &PluginEntry,
    action: &JsonRpcActionRef,
    query: &str,
) -> Result<ExecuteResult, PluginError> {
    if plugin.kind != PluginKind::JsonRpcStdio {
        return Err(PluginError::InvalidPluginKind);
    }

    let timeout = plugin_timeout(plugin);
    let mut session = JsonRpcSession::spawn(plugin, timeout)?;
    session.request::<_, Value>(
        METHOD_INITIALIZE,
        InitializeParams {
            app_name: "Veyra".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
        },
    )?;
    let result = session.request(
        METHOD_EXECUTE,
        ExecuteParams {
            item_id: action.item_id.clone(),
            action_id: action.action_id.clone(),
            query: query.to_string(),
        },
    )?;
    session.shutdown();
    Ok(result)
}

fn load_json_rpc_catalog(plugin: &PluginEntry) -> Result<Vec<CatalogItem>, PluginError> {
    let timeout = plugin_timeout(plugin);
    let mut session = JsonRpcSession::spawn(plugin, timeout)?;
    session.request::<_, Value>(
        METHOD_INITIALIZE,
        InitializeParams {
            app_name: "Veyra".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
        },
    )?;
    let result: CatalogResult = session.request(METHOD_CATALOG, CatalogParams::default())?;
    session.shutdown();

    let plugin_id = plugin_id(plugin);
    Ok(result
        .items
        .into_iter()
        .filter_map(|item| protocol_item(plugin, &plugin_id, item))
        .collect())
}

fn load_json_rpc_suggestions(
    plugin: &PluginEntry,
    query: &str,
) -> Result<Vec<CatalogItem>, PluginError> {
    let timeout = plugin_timeout(plugin);
    let mut session = JsonRpcSession::spawn(plugin, timeout)?;
    session.request::<_, Value>(
        METHOD_INITIALIZE,
        InitializeParams {
            app_name: "Veyra".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
        },
    )?;
    let result: SuggestResult = session.request(
        METHOD_SUGGEST,
        SuggestParams {
            query: query.to_string(),
        },
    )?;
    session.shutdown();

    let plugin_id = plugin_id(plugin);
    Ok(result
        .items
        .into_iter()
        .filter_map(|item| protocol_item(plugin, &plugin_id, item))
        .collect())
}

fn protocol_item(
    plugin: &PluginEntry,
    plugin_id: &str,
    item: ProtocolCatalogItem,
) -> Option<CatalogItem> {
    if item.id.trim().is_empty() || item.label.trim().is_empty() {
        return None;
    }

    let item_id = item.id;
    let mut catalog_item = CatalogItem::new(
        format!("{plugin_id}:{item_id}"),
        item.label,
        protocol_category(item.category),
        "plugin",
    )
    .keywords(item.keywords)
    .score_boost(item.score_boost);

    if let Some(subtitle) = item.subtitle {
        catalog_item = catalog_item.subtitle(subtitle);
    }

    let actions = if item.actions.is_empty() {
        vec![ProtocolAction {
            id: DEFAULT_ACTION_ID.to_string(),
            label: "Run".to_string(),
            kind: ProtocolActionKind::ToolCall,
            command: None,
            args: Vec::new(),
            requires_confirmation: plugin.requires_confirmation,
            run_as_admin: false,
        }]
    } else {
        item.actions
    };

    for action in actions {
        catalog_item = catalog_item.action(protocol_action(plugin_id, &item_id, action));
    }

    Some(catalog_item)
}

fn protocol_action(plugin_id: &str, item_id: &str, action: ProtocolAction) -> Action {
    let action_id = non_empty(&action.id).unwrap_or_else(|| DEFAULT_ACTION_ID.to_string());
    let label = non_empty(&action.label).unwrap_or_else(|| "Run".to_string());
    let mut output = Action {
        id: action_id.clone(),
        label,
        kind: protocol_action_kind(action.kind),
        command: action.command,
        args: action.args,
        requires_confirmation: action.requires_confirmation,
        run_as_admin: action.run_as_admin,
    };

    if output.kind == ActionKind::ToolCall {
        output.command = Some(json_rpc_action_command(plugin_id, item_id, action_id));
    }

    output
}

fn protocol_action_kind(kind: ProtocolActionKind) -> ActionKind {
    match kind {
        ProtocolActionKind::Launch => ActionKind::Launch,
        ProtocolActionKind::ShellCommand => ActionKind::ShellCommand,
        ProtocolActionKind::OpenUrl => ActionKind::OpenUrl,
        ProtocolActionKind::OpenFile => ActionKind::OpenFile,
        ProtocolActionKind::AiPrompt => ActionKind::AiPrompt,
        ProtocolActionKind::ToolCall => ActionKind::ToolCall,
    }
}

fn protocol_category(category: ProtocolItemCategory) -> ItemCategory {
    match category {
        ProtocolItemCategory::App => ItemCategory::App,
        ProtocolItemCategory::Command => ItemCategory::Command,
        ProtocolItemCategory::File => ItemCategory::File,
        ProtocolItemCategory::Folder => ItemCategory::Folder,
        ProtocolItemCategory::Setting => ItemCategory::Setting,
        ProtocolItemCategory::System => ItemCategory::System,
        ProtocolItemCategory::Web => ItemCategory::Web,
        ProtocolItemCategory::Ai => ItemCategory::Ai,
        ProtocolItemCategory::Tool => ItemCategory::Tool,
    }
}

fn manifest_item(manifest: ToolManifest) -> Option<CatalogItem> {
    if !manifest_platform_allowed(&manifest) {
        return None;
    }

    let mut action = match manifest.runner.kind {
        ToolRunnerKind::Process => Action::launch_with_args(
            manifest.runner.command.clone(),
            manifest.runner.args.clone(),
        ),
        ToolRunnerKind::Builtin => Action {
            id: "default".to_string(),
            label: "Run".to_string(),
            kind: ActionKind::ToolCall,
            command: Some(manifest.name.clone()),
            args: Vec::new(),
            requires_confirmation: manifest.requires_confirmation(),
            run_as_admin: manifest.safety.requires_admin,
        },
    };
    action.requires_confirmation = manifest.requires_confirmation();
    action.run_as_admin = manifest.safety.requires_admin;

    Some(
        CatalogItem::new(
            format!("tool_manifest:{}", manifest.name),
            manifest.name,
            ItemCategory::Tool,
            "tool_manifest",
        )
        .subtitle(manifest.description)
        .keywords(manifest.keywords)
        .action(action),
    )
}

fn manifest_platform_allowed(manifest: &ToolManifest) -> bool {
    if manifest.platforms.is_empty() {
        return true;
    }

    let current = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    };

    manifest
        .platforms
        .iter()
        .any(|platform| platform.eq_ignore_ascii_case(current))
}

fn manifest_directories(profile_dir: &Path) -> Vec<PathBuf> {
    vec![profile_dir.join("tools"), profile_dir.join("plugins")]
}

struct JsonRpcSession {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: mpsc::Receiver<String>,
    timeout: Duration,
    next_id: u64,
}

impl JsonRpcSession {
    fn spawn(plugin: &PluginEntry, timeout: Duration) -> Result<Self, PluginError> {
        let command = expand_env_vars(&plugin.command);
        let args = plugin
            .args
            .iter()
            .map(|arg| expand_env_vars(arg))
            .collect::<Vec<_>>();
        let mut child = Command::new(&command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| PluginError::Spawn {
                command: command.clone(),
                source,
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or(PluginError::MissingPipe("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(PluginError::MissingPipe("stdout"))?;
        let (sender, lines) = mpsc::channel();

        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if !line.trim().is_empty() && sender.send(line).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            lines,
            timeout,
            next_id: 1,
        })
    }

    fn request<P, R>(&mut self, method: &str, params: P) -> Result<R, PluginError>
    where
        P: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let id = self.next_id;
        self.next_id += 1;
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::Number(id),
            method: method.to_string(),
            params: serde_json::to_value(params)?,
        };

        let raw = serde_json::to_string(&request)?;
        writeln!(self.stdin, "{raw}")?;
        self.stdin.flush()?;

        loop {
            let line = self
                .lines
                .recv_timeout(self.timeout)
                .map_err(|_| PluginError::Timeout {
                    method: method.to_string(),
                    timeout_ms: self.timeout.as_millis(),
                })?;
            let response: JsonRpcResponse = serde_json::from_str(&line)?;
            if response.id != RequestId::Number(id) {
                continue;
            }
            if let Some(error) = response.error {
                return Err(PluginError::Remote {
                    code: error.code,
                    message: error.message,
                });
            }
            let result = response.result.unwrap_or(Value::Null);
            return serde_json::from_value(result).map_err(PluginError::Json);
        }
    }

    fn shutdown(&mut self) {
        let _ = self.request::<_, Value>(METHOD_SHUTDOWN, Value::Null);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for JsonRpcSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin kind is not json_rpc_stdio")]
    InvalidPluginKind,
    #[error("failed to spawn plugin command `{command}`")]
    Spawn {
        command: String,
        source: std::io::Error,
    },
    #[error("plugin process did not expose {0}")]
    MissingPipe(&'static str),
    #[error("plugin request `{method}` timed out after {timeout_ms} ms")]
    Timeout { method: String, timeout_ms: u128 },
    #[error("plugin returned JSON-RPC error {code}: {message}")]
    Remote { code: i64, message: String },
    #[error("plugin I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("plugin JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

fn plugin_id(plugin: &PluginEntry) -> String {
    non_empty(&plugin.id).unwrap_or_else(|| format!("plugin.{}", plugin.command))
}

fn plugin_label(plugin: &PluginEntry) -> String {
    non_empty(&plugin.label)
        .or_else(|| non_empty(&plugin.id))
        .unwrap_or_else(|| plugin.command.clone())
}

fn plugin_timeout(plugin: &PluginEntry) -> Duration {
    Duration::from_millis(plugin.timeout_ms.max(250))
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn expand_env_vars(value: &str) -> String {
    let mut expanded = expand_percent_env(value);
    expanded = expand_dollar_env(&expanded);

    if let Some(rest) = expanded.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    {
        return home.join(rest).to_string_lossy().to_string();
    }

    expanded
}

fn expand_percent_env(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;

    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push('%');
            output.push_str(after_start);
            return output;
        };

        let name = &after_start[..end];
        if let Some(replacement) = std::env::var_os(name) {
            output.push_str(&replacement.to_string_lossy());
        } else {
            output.push('%');
            output.push_str(name);
            output.push('%');
        }
        rest = &after_start[end + 1..];
    }

    output.push_str(rest);
    output
}

fn expand_dollar_env(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            output.push(ch);
            continue;
        }

        if chars.peek() == Some(&'{') {
            chars.next();
            let mut name = String::new();
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
                name.push(next);
            }
            push_env_or_literal(&mut output, &name, true);
            continue;
        }

        let mut name = String::new();
        while let Some(next) = chars.peek().copied() {
            if next == '_' || next.is_ascii_alphanumeric() {
                name.push(next);
                chars.next();
            } else {
                break;
            }
        }

        if name.is_empty() {
            output.push('$');
        } else {
            push_env_or_literal(&mut output, &name, false);
        }
    }

    output
}

fn push_env_or_literal(output: &mut String, name: &str, braced: bool) {
    if let Some(replacement) = std::env::var_os(name) {
        output.push_str(&replacement.to_string_lossy());
    } else if braced {
        output.push_str("${");
        output.push_str(name);
        output.push('}');
    } else {
        output.push('$');
        output.push_str(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn process_plugins_become_catalog_items_by_default() {
        let plugin = PluginEntry {
            id: "plugin.test".to_string(),
            label: "Plugin: Test".to_string(),
            description: "Run a test plugin".to_string(),
            command: "test.exe".to_string(),
            args: vec!["--ok".to_string()],
            keywords: vec!["test".to_string()],
            ..Default::default()
        };

        let item = process_plugin_item(&plugin).unwrap();

        assert_eq!(item.category, ItemCategory::Tool);
        assert_eq!(item.source, "plugin");
        assert_eq!(item.actions[0].command.as_deref(), Some("test.exe"));
        assert_eq!(item.actions[0].args, vec!["--ok"]);
    }

    #[test]
    fn disabled_or_stdio_plugins_do_not_become_process_items() {
        let mut plugin = PluginEntry {
            command: "test.exe".to_string(),
            enabled: false,
            ..Default::default()
        };
        assert!(process_plugin_item(&plugin).is_none());

        plugin.enabled = true;
        plugin.kind = PluginKind::JsonRpcStdio;
        assert!(process_plugin_item(&plugin).is_none());
    }

    #[test]
    fn encodes_and_decodes_json_rpc_action_refs() {
        let command = json_rpc_action_command("plugin.one", "item.one", "default");
        let parsed = parse_json_rpc_action_command(&command).unwrap();

        assert_eq!(parsed.plugin_id, "plugin.one");
        assert_eq!(parsed.item_id, "item.one");
        assert_eq!(parsed.action_id, "default");
    }

    #[test]
    fn blank_queries_do_not_spawn_plugin_suggestions() {
        let plugin = PluginEntry {
            id: "plugin.test".to_string(),
            kind: PluginKind::JsonRpcStdio,
            command: "missing-command".to_string(),
            ..Default::default()
        };

        let load = load_plugin_suggestions(&[plugin], "   ");

        assert!(load.items.is_empty());
        assert_eq!(load.error_count, 0);
    }

    #[test]
    fn loads_manifest_tools_from_profile_directory() {
        let profile = temp_profile_dir();
        let tools = profile.join("tools");
        fs::create_dir_all(&tools).unwrap();
        fs::write(
            tools.join("repair.json"),
            r#"{
                "name": "repair",
                "description": "Run repair",
                "keywords": ["repair"],
                "runner": {
                    "kind": "process",
                    "command": "repair.cmd",
                    "args": ["--safe"]
                },
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                },
                "safety": {
                    "level": "execute",
                    "requires_confirmation": false,
                    "requires_admin": false
                },
                "timeout_ms": 30000
            }"#,
        )
        .unwrap();

        let load = load_plugin_extensions(&profile, &[]);

        assert_eq!(load.manifest_item_count, 1);
        assert_eq!(load.items[0].id, "tool_manifest:repair");
        assert!(load.items[0].actions[0].requires_confirmation);

        fs::remove_dir_all(profile).ok();
    }

    fn temp_profile_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("veyra-plugin-profile-test-{nanos}"));
        path
    }
}
