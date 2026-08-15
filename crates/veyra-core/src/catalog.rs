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
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut items = vec![
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
        CatalogItem::new("web.google", "Web: Google", ItemCategory::Web, "builtin")
            .subtitle("Search Google")
            .keywords(["google", "search", "web"])
            .action(Action::open_url("https://www.google.com/search?q={query}")),
        CatalogItem::new(
            "web.duckduckgo",
            "Web: DuckDuckGo",
            ItemCategory::Web,
            "builtin",
        )
        .subtitle("Search DuckDuckGo")
        .keywords(["duckduckgo", "ddg", "search", "web"])
        .action(Action::open_url("https://duckduckgo.com/?q={query}")),
        CatalogItem::new("web.bing", "Web: Bing", ItemCategory::Web, "builtin")
            .subtitle("Search Bing")
            .keywords(["bing", "search", "web"])
            .action(Action::open_url("https://www.bing.com/search?q={query}")),
        CatalogItem::new("ai.ask", "AI: Ask", ItemCategory::Ai, "builtin")
            .subtitle("Ask the configured local or remote model")
            .keywords(["ask", "chat", "ai", "assistant"])
            .action(Action::ai_prompt())
            .score_boost(10),
    ];

    #[cfg(windows)]
    items.extend(windows_seed_catalog());

    items
}

#[cfg(windows)]
fn windows_seed_catalog() -> Vec<CatalogItem> {
    vec![
        CatalogItem::new("system.notepad", "Notepad", ItemCategory::App, "builtin")
            .subtitle("Simple text editor")
            .keywords(["notepad", "text", "editor", "txt"])
            .action(Action::launch("notepad.exe")),
        CatalogItem::new(
            "system.calculator",
            "Calculator",
            ItemCategory::App,
            "builtin",
        )
        .subtitle("Calculator")
        .keywords(["calculator", "calc", "math"])
        .action(Action::launch("calc.exe")),
        CatalogItem::new("system.paint", "Paint", ItemCategory::App, "builtin")
            .subtitle("Paint")
            .keywords(["paint", "mspaint", "drawing"])
            .action(Action::launch("mspaint.exe")),
        CatalogItem::new(
            "system.snipping_tool",
            "Snipping Tool",
            ItemCategory::App,
            "builtin",
        )
        .subtitle("Capture screenshots")
        .keywords(["snip", "screenshot", "capture", "snipping"])
        .action(Action::launch("snippingtool.exe")),
        CatalogItem::new(
            "system.file_explorer",
            "File Explorer",
            ItemCategory::System,
            "builtin",
        )
        .subtitle("Open File Explorer")
        .keywords(["explorer", "files", "folder", "directories"])
        .action(Action::launch("explorer.exe")),
        CatalogItem::new(
            "system.command_prompt",
            "Command Prompt",
            ItemCategory::Command,
            "builtin",
        )
        .subtitle("Windows command prompt")
        .keywords(["cmd", "command", "terminal", "console"])
        .action(Action::launch("cmd.exe")),
        CatalogItem::new(
            "system.powershell",
            "PowerShell",
            ItemCategory::Command,
            "builtin",
        )
        .subtitle("Windows PowerShell")
        .keywords(["powershell", "ps", "shell"])
        .action(Action::launch("powershell.exe")),
        CatalogItem::new(
            "system.terminal",
            "Windows Terminal",
            ItemCategory::Command,
            "builtin",
        )
        .subtitle("Modern tabbed terminal")
        .keywords(["terminal", "wt", "tabbed"])
        .action(Action::launch("wt.exe")),
        CatalogItem::new(
            "system.control_panel",
            "Control Panel",
            ItemCategory::Setting,
            "builtin",
        )
        .subtitle("Classic system settings")
        .keywords(["control", "panel", "settings"])
        .action(Action::launch("control.exe")),
        CatalogItem::new(
            "settings.network",
            "Settings: Network",
            ItemCategory::Setting,
            "builtin",
        )
        .subtitle("Open network and internet settings")
        .keywords(["network", "wifi", "ethernet", "internet", "settings"])
        .action(Action::launch_with_args(
            "explorer.exe",
            ["ms-settings:network"],
        )),
        CatalogItem::new(
            "settings.bluetooth",
            "Settings: Bluetooth",
            ItemCategory::Setting,
            "builtin",
        )
        .subtitle("Open Bluetooth settings")
        .keywords(["bluetooth", "devices", "settings"])
        .action(Action::launch_with_args(
            "explorer.exe",
            ["ms-settings:bluetooth"],
        )),
        CatalogItem::new(
            "settings.apps",
            "Settings: Apps",
            ItemCategory::Setting,
            "builtin",
        )
        .subtitle("Open apps and features")
        .keywords(["apps", "uninstall", "programs", "settings"])
        .action(Action::launch_with_args(
            "explorer.exe",
            ["ms-settings:apps-features"],
        )),
        CatalogItem::new(
            "settings.personalization",
            "Settings: Personalization",
            ItemCategory::Setting,
            "builtin",
        )
        .subtitle("Open personalization settings")
        .keywords(["personalization", "theme", "wallpaper", "settings"])
        .action(Action::launch_with_args(
            "explorer.exe",
            ["ms-settings:personalization"],
        )),
        CatalogItem::new(
            "settings.system",
            "Settings: System",
            ItemCategory::Setting,
            "builtin",
        )
        .subtitle("Open Windows system settings")
        .keywords(["system", "about", "settings"])
        .action(Action::launch_with_args("explorer.exe", ["ms-settings:"])),
        CatalogItem::new(
            "folder.documents",
            "Documents",
            ItemCategory::Folder,
            "builtin",
        )
        .subtitle("Open Documents folder")
        .keywords(["documents", "docs", "folder"])
        .action(Action::open_file("%USERPROFILE%\\Documents")),
        CatalogItem::new(
            "folder.downloads",
            "Downloads",
            ItemCategory::Folder,
            "builtin",
        )
        .subtitle("Open Downloads folder")
        .keywords(["downloads", "folder"])
        .action(Action::open_file("%USERPROFILE%\\Downloads")),
        CatalogItem::new("folder.desktop", "Desktop", ItemCategory::Folder, "builtin")
            .subtitle("Open Desktop folder")
            .keywords(["desktop", "folder"])
            .action(Action::open_file("%USERPROFILE%\\Desktop")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActionKind;

    #[test]
    fn ai_seed_item_has_prompt_action() {
        let catalog = seed_catalog();
        let item = catalog
            .iter()
            .find(|item| item.id == "ai.ask")
            .expect("ai seed item");

        assert_eq!(item.actions.len(), 1);
        assert_eq!(item.actions[0].kind, ActionKind::AiPrompt);
    }
}
