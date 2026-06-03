use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub kind: ActionKind,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub run_as_admin: bool,
}

impl Action {
    pub fn launch(command: impl Into<String>) -> Self {
        Self {
            id: "default".to_string(),
            label: "Open".to_string(),
            kind: ActionKind::Launch,
            command: Some(command.into()),
            args: Vec::new(),
            requires_confirmation: false,
            run_as_admin: false,
        }
    }

    pub fn open_url(url: impl Into<String>) -> Self {
        Self {
            id: "open_url".to_string(),
            label: "Open URL".to_string(),
            kind: ActionKind::OpenUrl,
            command: Some(url.into()),
            args: Vec::new(),
            requires_confirmation: false,
            run_as_admin: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Launch,
    ShellCommand,
    OpenUrl,
    OpenFile,
    AiPrompt,
    ToolCall,
}
