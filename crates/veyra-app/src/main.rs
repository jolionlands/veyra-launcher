use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Color32, Frame, Key, Layout, Margin, RichText, Stroke, TextEdit, Vec2,
};
use veyra_core::config::{CommandEntry, VeyraConfig, WebSearchEntry};
use veyra_core::{
    Action, ActionKind, CatalogItem, ItemCategory, SearchResult, search, seed_catalog,
};
use veyra_platform::{discover_path_executables, execute_action, profile_dir};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Veyra")
            .with_inner_size([760.0, 520.0])
            .with_min_inner_size([520.0, 340.0])
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "Veyra",
        options,
        Box::new(|cc| Ok(Box::new(VeyraApp::new(cc)))),
    )
}

struct VeyraApp {
    query: String,
    catalog: Vec<CatalogItem>,
    show_settings: bool,
    settings_page: SettingsPage,
    selected: usize,
    last_status: Option<String>,
    profile_dir: PathBuf,
    config: VeyraConfig,
    load_messages: Vec<String>,
    path_item_count: usize,
}

impl VeyraApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let profile_dir = profile_dir("Veyra");
        let (config, mut loaded_items, mut load_messages) = load_profile(&profile_dir);
        let path_items = discover_path_executables();
        let path_item_count = path_items.len();
        load_messages.push(format!("Discovered {path_item_count} PATH executables"));
        loaded_items.extend(path_items);
        let mut catalog = seed_catalog();
        catalog.extend(loaded_items);

        Self {
            query: String::new(),
            catalog,
            show_settings: false,
            settings_page: SettingsPage::General,
            selected: 0,
            last_status: None,
            profile_dir,
            config,
            load_messages,
            path_item_count,
        }
    }

    fn results(&self) -> Vec<SearchResult> {
        search(&self.catalog, &self.query)
    }
}

impl eframe::App for VeyraApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            if self.query.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.query.clear();
            }
        }

        if ctx.input(|input| input.modifiers.ctrl && input.key_pressed(Key::Comma)) {
            self.show_settings = !self.show_settings;
        }

        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(18, 20, 24, 230))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.heading(RichText::new("Veyra").strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Settings").clicked() {
                            self.show_settings = !self.show_settings;
                        }
                    });
                });
            });

        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(20, 22, 27, 224))
            .stroke(Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(255, 255, 255, 28),
            ))
            .inner_margin(Margin::same(18))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.show_settings {
                    self.render_settings(ui);
                } else {
                    self.render_launcher(ui);
                }
            });
    }
}

impl VeyraApp {
    fn render_launcher(&mut self, ui: &mut egui::Ui) {
        ui.add(
            TextEdit::singleline(&mut self.query)
                .hint_text("Search apps, settings, files, web, and AI tools")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Heading),
        );

        ui.add_space(12.0);

        let results = self.results();
        if !results.is_empty() {
            self.selected = self.selected.min(results.len() - 1);
        }

        if ui.input(|input| input.key_pressed(Key::ArrowDown)) && !results.is_empty() {
            self.selected = (self.selected + 1).min(results.len() - 1);
        }
        if ui.input(|input| input.key_pressed(Key::ArrowUp)) && !results.is_empty() {
            self.selected = self.selected.saturating_sub(1);
        }
        if ui.input(|input| input.key_pressed(Key::Enter))
            && let Some(result) = results.get(self.selected)
        {
            self.execute_result(result);
        }

        for (index, result) in results.iter().take(10).enumerate() {
            self.render_result(ui, index, result);
        }

        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            if let Some(status) = &self.last_status {
                ui.label(RichText::new(status).color(Color32::from_rgb(180, 190, 205)));
            } else {
                ui.label(
                    RichText::new("Enter open    Ctrl+, settings    Esc clear/close")
                        .color(Color32::from_rgb(140, 148, 160)),
                );
            }
        });
    }

    fn render_result(&mut self, ui: &mut egui::Ui, index: usize, result: &SearchResult) {
        let selected = index == self.selected;
        let fill = if selected {
            Color32::from_rgba_unmultiplied(72, 104, 136, 150)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 12)
        };

        let response = Frame::new()
            .fill(fill)
            .corner_radius(6)
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(category_label(&result.item)).monospace());
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&result.item.label).strong());
                        if let Some(subtitle) = &result.item.subtitle {
                            ui.label(
                                RichText::new(subtitle).color(Color32::from_rgb(157, 166, 180)),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(result.score.to_string())
                                .color(Color32::from_rgb(128, 137, 150)),
                        );
                    });
                });
            })
            .response;

        if response.clicked() {
            self.selected = index;
        }
        if response.double_clicked() {
            self.execute_result(result);
        }
        ui.add_space(6.0);
    }

    fn execute_result(&mut self, result: &SearchResult) {
        let Some(action) = result.item.actions.first() else {
            self.last_status = Some(format!("No action registered for {}", result.item.label));
            return;
        };

        let action = self.resolve_action(action);
        match execute_action(&action) {
            Ok(()) => {
                self.last_status = Some(format!("Opened {}", result.item.label));
            }
            Err(error) => {
                self.last_status = Some(format!("Could not open {}: {}", result.item.label, error));
            }
        }
    }

    fn resolve_action(&self, action: &Action) -> Action {
        let mut action = action.clone();
        if action.kind == ActionKind::OpenUrl
            && let Some(command) = &action.command
        {
            action.command = Some(command.replace("{query}", &encode_query(&self.query)));
        }
        action
    }

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(190.0);
                ui.heading("Settings");
                ui.separator();
                for page in SettingsPage::ALL {
                    if ui
                        .selectable_label(self.settings_page == page, page.label())
                        .clicked()
                    {
                        self.settings_page = page;
                    }
                }
            });

            ui.separator();

            ui.vertical(|ui| {
                self.render_settings_page(ui);
            });
        });
    }

    fn render_settings_page(&self, ui: &mut egui::Ui) {
        ui.heading(self.settings_page.label());
        ui.add_space(8.0);

        match self.settings_page {
            SettingsPage::General => {
                setting_row(ui, "Profile", self.profile_dir.display().to_string());
                setting_row(ui, "Startup", self.config.general.startup.to_string());
                setting_row(
                    ui,
                    "History limit",
                    self.config.general.history_limit.to_string(),
                );
                setting_row(ui, "Portable mode", "Auto-detect");
            }
            SettingsPage::Appearance => {
                setting_row(ui, "Theme", self.config.appearance.theme.as_str());
                setting_row(
                    ui,
                    "Opacity",
                    format!("{:.0}%", self.config.appearance.opacity * 100.0),
                );
                setting_row(ui, "Blur", self.config.appearance.blur.to_string());
                setting_row(
                    ui,
                    "Preview pane",
                    self.config.appearance.show_preview.to_string(),
                );
            }
            SettingsPage::Hotkeys => {
                setting_row(ui, "Toggle launcher", self.config.hotkeys.toggle.as_str());
                setting_row(ui, "Settings", self.config.hotkeys.settings.as_str());
                setting_row(ui, "Alternate action", "Shift+Enter");
                setting_row(ui, "Elevated action", "Ctrl+Enter");
            }
            SettingsPage::Catalogs => {
                setting_row(ui, "Total catalog items", self.catalog.len().to_string());
                setting_row(ui, "Start Menu", "Planned");
                setting_row(ui, "PATH executables", self.path_item_count.to_string());
                setting_row(ui, "File profiles", "Planned");
            }
            SettingsPage::Commands => {
                setting_row(ui, "User commands", self.config.commands.len().to_string());
                setting_row(
                    ui,
                    "Web shortcuts",
                    self.config.web_search.len().to_string(),
                );
                setting_row(ui, "Action confirmation", "Per command");
            }
            SettingsPage::AiProviders => {
                setting_row(ui, "Default provider", "Not configured");
                setting_row(ui, "Local-only mode", "Available");
                setting_row(ui, "Tool calling", "Manifest based");
            }
            SettingsPage::Tools => {
                setting_row(ui, "Tool manifests", "JSON");
                setting_row(ui, "External runner", "Process");
                setting_row(ui, "Confirmation", "Safety based");
            }
            SettingsPage::Diagnostics => {
                setting_row(
                    ui,
                    "Last action",
                    self.last_status.as_deref().unwrap_or("None"),
                );
                setting_row(
                    ui,
                    "Platform",
                    format!("{:?}", veyra_platform::current_platform()),
                );
                setting_row(ui, "Catalog items", self.catalog.len().to_string());
                if !self.load_messages.is_empty() {
                    ui.separator();
                    for message in &self.load_messages {
                        ui.label(RichText::new(message).color(Color32::from_rgb(160, 170, 184)));
                    }
                }
            }
        }
    }
}

fn load_profile(profile_dir: &Path) -> (VeyraConfig, Vec<CatalogItem>, Vec<String>) {
    let mut config = VeyraConfig::default();
    let mut messages = Vec::new();

    merge_config_file(
        profile_dir.join("config.toml"),
        ConfigMergeMode::Full,
        &mut config,
        &mut messages,
    );
    merge_config_file(
        profile_dir.join("commands.toml"),
        ConfigMergeMode::CommandsOnly,
        &mut config,
        &mut messages,
    );
    merge_config_file(
        profile_dir.join("catalogs.toml"),
        ConfigMergeMode::CatalogsOnly,
        &mut config,
        &mut messages,
    );
    merge_config_file(
        profile_dir.join("ai.toml"),
        ConfigMergeMode::AiOnly,
        &mut config,
        &mut messages,
    );

    let items = catalog_items_from_config(&config);
    if items.is_empty() {
        messages.push(format!(
            "No user commands loaded from {}",
            profile_dir.display()
        ));
    } else {
        messages.push(format!("Loaded {} user catalog items", items.len()));
    }

    (config, items, messages)
}

fn merge_config_file(
    path: PathBuf,
    mode: ConfigMergeMode,
    target: &mut VeyraConfig,
    messages: &mut Vec<String>,
) {
    if !path.exists() {
        messages.push(format!("Skipped missing {}", path.display()));
        return;
    }

    match fs::read_to_string(&path) {
        Ok(raw) => match VeyraConfig::from_toml_str(&raw) {
            Ok(config) => {
                merge_config(target, config, mode);
                messages.push(format!("Loaded {}", path.display()));
            }
            Err(error) => {
                messages.push(format!("Could not parse {}: {}", path.display(), error));
            }
        },
        Err(error) => {
            messages.push(format!("Could not read {}: {}", path.display(), error));
        }
    }
}

fn merge_config(target: &mut VeyraConfig, mut incoming: VeyraConfig, mode: ConfigMergeMode) {
    if mode == ConfigMergeMode::Full {
        target.general = incoming.general;
        target.hotkeys = incoming.hotkeys;
        target.appearance = incoming.appearance;
    }

    if matches!(mode, ConfigMergeMode::Full | ConfigMergeMode::CommandsOnly) {
        target.commands.extend(incoming.commands);
        target.web_search.extend(incoming.web_search);
    }

    if matches!(mode, ConfigMergeMode::Full | ConfigMergeMode::CatalogsOnly) {
        target.catalogs.extend(incoming.catalogs);
    }

    if matches!(mode, ConfigMergeMode::Full | ConfigMergeMode::AiOnly) {
        if !incoming.providers.is_empty() {
            incoming.ai.providers = incoming.providers;
        }
        target.ai = incoming.ai;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigMergeMode {
    Full,
    CommandsOnly,
    CatalogsOnly,
    AiOnly,
}

fn catalog_items_from_config(config: &VeyraConfig) -> Vec<CatalogItem> {
    config
        .commands
        .iter()
        .filter_map(command_item)
        .chain(config.web_search.iter().filter_map(web_search_item))
        .collect()
}

fn command_item(command: &CommandEntry) -> Option<CatalogItem> {
    if command.command.trim().is_empty() {
        return None;
    }

    let id = non_empty(&command.id).unwrap_or_else(|| format!("command.{}", command.command));
    let label = non_empty(&command.label).unwrap_or_else(|| command.command.clone());
    let mut action = Action::launch_with_args(command.command.clone(), command.args.clone());
    action.requires_confirmation = command.requires_confirmation;

    Some(
        CatalogItem::new(id, label, ItemCategory::Command, "profile")
            .subtitle(command.command.clone())
            .keywords(command.keywords.clone())
            .action(action),
    )
}

fn web_search_item(entry: &WebSearchEntry) -> Option<CatalogItem> {
    if entry.url.trim().is_empty() {
        return None;
    }

    let id = non_empty(&entry.id).unwrap_or_else(|| format!("web.{}", entry.alias));
    let label = non_empty(&entry.label).unwrap_or_else(|| format!("Web: {}", entry.alias));
    let keywords = [entry.alias.clone(), label.clone()];

    Some(
        CatalogItem::new(id, label, ItemCategory::Web, "profile")
            .subtitle(entry.url.clone())
            .keywords(keywords)
            .action(Action::open_url(entry.url.clone())),
    )
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    General,
    Appearance,
    Hotkeys,
    Catalogs,
    Commands,
    AiProviders,
    Tools,
    Diagnostics,
}

impl SettingsPage {
    const ALL: [SettingsPage; 8] = [
        SettingsPage::General,
        SettingsPage::Appearance,
        SettingsPage::Hotkeys,
        SettingsPage::Catalogs,
        SettingsPage::Commands,
        SettingsPage::AiProviders,
        SettingsPage::Tools,
        SettingsPage::Diagnostics,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::Hotkeys => "Hotkeys",
            SettingsPage::Catalogs => "Catalogs",
            SettingsPage::Commands => "Commands",
            SettingsPage::AiProviders => "AI Providers",
            SettingsPage::Tools => "Tools",
            SettingsPage::Diagnostics => "Diagnostics",
        }
    }
}

fn encode_query(query: &str) -> String {
    query
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            _ => {
                let hex = format!("%{byte:02X}");
                hex.chars().collect()
            }
        })
        .collect()
}

fn category_label(item: &CatalogItem) -> &'static str {
    match item.category {
        veyra_core::ItemCategory::App => "APP",
        veyra_core::ItemCategory::Command => "CMD",
        veyra_core::ItemCategory::File => "FILE",
        veyra_core::ItemCategory::Folder => "DIR",
        veyra_core::ItemCategory::Setting => "SET",
        veyra_core::ItemCategory::System => "SYS",
        veyra_core::ItemCategory::Web => "WEB",
        veyra_core::ItemCategory::Ai => "AI",
        veyra_core::ItemCategory::Tool => "TOOL",
    }
}

fn setting_row(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.allocate_ui(Vec2::new(ui.available_width(), 34.0), |ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(value.into()).color(Color32::from_rgb(160, 170, 184)));
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn loads_profile_config_and_commands() {
        let profile = temp_profile_dir();
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("config.toml"),
            r#"
                [general]
                startup = false
                history_limit = 42

                [appearance]
                theme = "test-theme"
            "#,
        )
        .unwrap();
        fs::write(
            profile.join("commands.toml"),
            r#"
                [[commands]]
                id = "command.test"
                label = "Command: Test"
                command = "test.exe"
                args = ["--ok"]
                keywords = ["test"]

                [[web_search]]
                id = "web.docs"
                alias = "docs"
                label = "Docs"
                url = "https://example.com/search?q={query}"
            "#,
        )
        .unwrap();

        let (config, items, messages) = load_profile(&profile);

        assert!(!config.general.startup);
        assert_eq!(config.general.history_limit, 42);
        assert_eq!(config.appearance.theme, "test-theme");
        assert_eq!(config.commands.len(), 1);
        assert_eq!(config.web_search.len(), 1);
        assert_eq!(items.len(), 2);
        assert!(messages.iter().any(|message| message.contains("Loaded")));

        fs::remove_dir_all(&profile).ok();
    }

    #[test]
    fn commands_file_does_not_reset_main_config_sections() {
        let profile = temp_profile_dir();
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("config.toml"),
            r#"
                [appearance]
                theme = "kept"
            "#,
        )
        .unwrap();
        fs::write(
            profile.join("commands.toml"),
            r#"
                [[commands]]
                label = "Command: Test"
                command = "test.exe"
            "#,
        )
        .unwrap();

        let (config, _, _) = load_profile(&profile);

        assert_eq!(config.appearance.theme, "kept");

        fs::remove_dir_all(&profile).ok();
    }

    #[test]
    fn loads_catalogs_and_ai_without_resetting_commands() {
        let profile = temp_profile_dir();
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("commands.toml"),
            r#"
                [[commands]]
                label = "Command: Test"
                command = "test.exe"
            "#,
        )
        .unwrap();
        fs::write(
            profile.join("catalogs.toml"),
            r#"
                [[profiles]]
                id = "dev"
                label = "Development"
                paths = ["%USERPROFILE%\\Development"]
                max_depth = 4
            "#,
        )
        .unwrap();
        fs::write(
            profile.join("ai.toml"),
            r#"
                [ai]
                enabled = true
                default_provider = "local"

                [[providers]]
                id = "local"
                label = "Local"
                base_url = "http://127.0.0.1:8080/v1"
                model = "local-model"
            "#,
        )
        .unwrap();

        let (config, _, _) = load_profile(&profile);

        assert_eq!(config.commands.len(), 1);
        assert_eq!(config.catalogs.len(), 1);
        assert!(config.ai.enabled);
        assert_eq!(config.ai.providers.len(), 1);

        fs::remove_dir_all(&profile).ok();
    }

    fn temp_profile_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("veyra-app-profile-test-{nanos}"));
        path
    }
}
