use serde::{Deserialize, Serialize};

use crate::Action;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: String,
    pub label: String,
    pub subtitle: Option<String>,
    pub category: ItemCategory,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub source: String,
    #[serde(default)]
    pub actions: Vec<Action>,
    pub icon: IconRef,
    #[serde(default)]
    pub score_boost: i32,
}

impl CatalogItem {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        category: ItemCategory,
        source: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            category,
            keywords: Vec::new(),
            source: source.into(),
            actions: Vec::new(),
            icon: IconRef::default(),
            score_boost: 0,
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn keywords(mut self, keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    pub fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    pub fn score_boost(mut self, score_boost: i32) -> Self {
        self.score_boost = score_boost;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemCategory {
    App,
    Command,
    File,
    Folder,
    Setting,
    System,
    Web,
    Ai,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconRef {
    pub name: String,
}

impl Default for IconRef {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
        }
    }
}

pub fn seed_catalog() -> Vec<CatalogItem> {
    vec![
        CatalogItem::new(
            "settings.display",
            "Settings: Display",
            ItemCategory::Setting,
            "builtin",
        )
        .subtitle("Open display and resolution settings")
        .keywords(["display", "monitor", "resolution", "settings"])
        .action(Action::launch_with_args(
            "explorer.exe",
            ["ms-settings:display"],
        ))
        .score_boost(25),
        CatalogItem::new(
            "settings.sound",
            "Settings: Sound",
            ItemCategory::Setting,
            "builtin",
        )
        .subtitle("Open sound settings")
        .keywords(["audio", "speaker", "microphone", "settings"])
        .action(Action::launch_with_args(
            "explorer.exe",
            ["ms-settings:sound"],
        )),
        CatalogItem::new(
            "system.task_manager",
            "System: Task Manager",
            ItemCategory::System,
            "builtin",
        )
        .subtitle("Inspect running processes")
        .keywords(["taskmgr", "process", "cpu", "memory"])
        .action(Action::launch("taskmgr.exe")),
        CatalogItem::new(
            "web.github",
            "Web: GitHub Code Search",
            ItemCategory::Web,
            "builtin",
        )
        .subtitle("Search GitHub code")
        .keywords(["github", "gh", "code", "search"])
        .action(Action::open_url(
            "https://github.com/search?q={query}&type=code",
        )),
        CatalogItem::new("ai.ask", "AI: Ask", ItemCategory::Ai, "builtin")
            .subtitle("Ask the configured local or remote model")
            .keywords(["ask", "chat", "ai", "assistant"])
            .score_boost(10),
    ]
}
