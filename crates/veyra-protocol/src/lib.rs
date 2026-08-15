use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_CATALOG: &str = "catalog";
pub const METHOD_SUGGEST: &str = "suggest";
pub const METHOD_EXECUTE: &str = "execute";
pub const METHOD_SETTINGS_SCHEMA: &str = "settings_schema";
pub const METHOD_TOOL_MANIFEST: &str = "tool_manifest";
pub const METHOD_SHUTDOWN: &str = "shutdown";

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeParams {
    pub app_name: String,
    pub app_version: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResult {
    pub plugin_id: String,
    pub plugin_label: String,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    Catalog,
    Suggest,
    Execute,
    ToolManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CatalogParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogResult {
    #[serde(default)]
    pub items: Vec<ProtocolCatalogItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestParams {
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestResult {
    #[serde(default)]
    pub items: Vec<ProtocolCatalogItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteParams {
    pub item_id: String,
    pub action_id: String,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecuteResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolCatalogItem {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub category: ProtocolItemCategory,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub actions: Vec<ProtocolAction>,
    #[serde(default)]
    pub score_boost: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolItemCategory {
    App,
    Command,
    File,
    Folder,
    Setting,
    System,
    Web,
    Ai,
    #[default]
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolAction {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: ProtocolActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub run_as_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolActionKind {
    Launch,
    ShellCommand,
    OpenUrl,
    OpenFile,
    AiPrompt,
    #[default]
    ToolCall,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_execute_params_with_stable_shape() {
        let value = serde_json::to_value(ExecuteParams {
            item_id: "item.one".to_string(),
            action_id: "default".to_string(),
            query: "hello".to_string(),
        })
        .unwrap();

        assert_eq!(value["item_id"], "item.one");
        assert_eq!(value["action_id"], "default");
        assert_eq!(value["query"], "hello");
    }

    #[test]
    fn parses_catalog_result_with_default_action_kind() {
        let raw = r#"{
            "items": [{
                "id": "tool.one",
                "label": "Tool One",
                "actions": [{
                    "id": "default",
                    "label": "Run"
                }]
            }]
        }"#;

        let result: CatalogResult = serde_json::from_str(raw).unwrap();

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].category, ProtocolItemCategory::Tool);
        assert_eq!(
            result.items[0].actions[0].kind,
            ProtocolActionKind::ToolCall
        );
    }
}
