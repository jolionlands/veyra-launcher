#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::{fs, io, sync::mpsc, thread};

#[cfg(windows)]
use std::{sync::OnceLock, time::Duration};

use eframe::egui::{
    self, Align, Color32, FontId, Frame, Key, Layout, Margin, RichText, ScrollArea, Slider, Stroke,
    TextEdit, TextStyle, Vec2, WindowLevel,
};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use toml_edit::{DocumentMut, value};
use veyra_core::config::{CommandEntry, VeyraConfig, WebSearchEntry};
use veyra_core::{
    Action, ActionKind, CatalogItem, ItemCategory, SearchResult, search, seed_catalog,
};
use veyra_platform::{
    discover_file_catalog_items, discover_platform_catalog_items, execute_action, profile_dir,
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_F23, VK_LSHIFT, VK_LWIN, VK_RSHIFT, VK_RWIN, VK_SHIFT,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
            SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
            WM_SYSKEYDOWN,
        },
    },
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Veyra")
            .with_inner_size([760.0, 520.0])
            .with_min_inner_size([520.0, 340.0])
            .with_transparent(true)
            .with_decorations(false)
            .with_window_level(WindowLevel::AlwaysOnTop),
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
    window_visible: bool,
    focus_query: bool,
    selected: usize,
    last_status: Option<String>,
    profile_dir: PathBuf,
    config: VeyraConfig,
    load_messages: Vec<String>,
    path_item_count: usize,
    start_menu_item_count: usize,
    file_catalog_item_count: usize,
    file_catalog_skipped_paths: usize,
    hotkeys: HotkeyRuntime,
}

impl VeyraApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let profile_dir = profile_dir("Veyra");
        let mut runtime = load_runtime_state(&profile_dir);
        let mut hotkeys = HotkeyRuntime::new(&cc.egui_ctx);
        runtime
            .load_messages
            .extend(hotkeys.register_toggle_hotkeys(&runtime.config));

        let app = Self {
            query: String::new(),
            catalog: runtime.catalog,
            show_settings: false,
            settings_page: SettingsPage::General,
            window_visible: true,
            focus_query: true,
            selected: 0,
            last_status: None,
            profile_dir,
            config: runtime.config,
            load_messages: runtime.load_messages,
            path_item_count: runtime.path_item_count,
            start_menu_item_count: runtime.start_menu_item_count,
            file_catalog_item_count: runtime.file_catalog_item_count,
            file_catalog_skipped_paths: runtime.file_catalog_skipped_paths,
            hotkeys,
        };
        app.apply_appearance(&cc.egui_ctx);
        app
    }

    fn results(&self) -> Vec<SearchResult> {
        search(&self.catalog, &self.query)
    }

    fn reload_profile(&mut self, ctx: &egui::Context) {
        let runtime = load_runtime_state(&self.profile_dir);
        self.config = runtime.config;
        self.catalog = runtime.catalog;
        self.load_messages = runtime.load_messages;
        self.path_item_count = runtime.path_item_count;
        self.start_menu_item_count = runtime.start_menu_item_count;
        self.file_catalog_item_count = runtime.file_catalog_item_count;
        self.file_catalog_skipped_paths = runtime.file_catalog_skipped_paths;
        self.load_messages
            .extend(self.hotkeys.register_toggle_hotkeys(&self.config));
        self.apply_appearance(ctx);
        self.selected = 0;
        self.last_status = Some(format!("Reloaded {} catalog items", self.catalog.len()));
    }
}

impl eframe::App for VeyraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_global_hotkey_events(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.process_global_hotkey_events(&ctx);

        if self.local_toggle_shortcut_pressed(&ctx) {
            self.toggle_launcher_window(&ctx);
        }

        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            if self.show_settings {
                self.show_settings = false;
                self.focus_query = true;
            } else if self.query.is_empty() {
                self.hide_launcher_window(&ctx);
            } else {
                self.query.clear();
                self.selected = 0;
            }
        }

        if ctx.input(|input| input.modifiers.ctrl && input.key_pressed(Key::Comma)) {
            self.show_settings = !self.show_settings;
            self.focus_query = !self.show_settings;
        }

        if ctx.input(|input| input.modifiers.ctrl && input.key_pressed(Key::R)) {
            self.reload_profile(&ctx);
        }

        let header_response = Frame::new()
            .fill(self.header_fill())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.heading(RichText::new("Veyra").strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Hide").clicked() {
                            self.hide_launcher_window(&ctx);
                        }
                        if ui.button("Settings").clicked() {
                            self.show_settings = !self.show_settings;
                            self.focus_query = !self.show_settings;
                        }
                    });
                });
            })
            .response;

        if header_response.dragged() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        Frame::new()
            .fill(self.surface_fill())
            .stroke(self.border_stroke())
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

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

impl VeyraApp {
    fn apply_appearance(&self, ctx: &egui::Context) {
        let mut style = (*ctx.global_style()).clone();
        let base_size = effective_font_size(&self.config);
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::proportional((base_size + 5.0).min(28.0)),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(base_size));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::proportional((base_size - 1.0).max(12.0)),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::monospace((base_size - 2.0).max(11.0)),
        );
        style.spacing.item_spacing = egui::vec2(9.0, 7.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.visuals = egui::Visuals::dark();
        style.visuals.window_corner_radius = 8.into();
        style.visuals.widgets.active.corner_radius = 6.into();
        style.visuals.widgets.hovered.corner_radius = 6.into();
        style.visuals.widgets.inactive.corner_radius = 6.into();
        style.visuals.widgets.noninteractive.corner_radius = 6.into();
        ctx.set_global_style(style);
    }

    fn process_global_hotkey_events(&mut self, ctx: &egui::Context) {
        let global_toggle_requested = self
            .hotkeys
            .events
            .try_iter()
            .any(|event| self.hotkeys.is_toggle_event(event));
        let copilot_toggle_requested = self.hotkeys.copilot_events.try_iter().next().is_some();

        if global_toggle_requested || copilot_toggle_requested {
            self.toggle_launcher_window(ctx);
        }
    }

    fn local_toggle_shortcut_pressed(&self, ctx: &egui::Context) -> bool {
        ctx.input(|input| {
            let copilot_pressed = input.modifiers.shift && input.key_pressed(Key::F23);
            let alt_space_pressed = input.modifiers.alt && input.key_pressed(Key::Space);

            (copilot_pressed && !self.hotkeys.registered_label(COPILOT_TOGGLE_HOTKEY))
                || (alt_space_pressed && !self.hotkeys.registered_label(FALLBACK_TOGGLE_HOTKEY))
        })
    }

    fn toggle_launcher_window(&mut self, ctx: &egui::Context) {
        if self.window_visible {
            self.hide_launcher_window(ctx);
        } else {
            self.show_launcher_window(ctx);
        }
    }

    fn show_launcher_window(&mut self, ctx: &egui::Context) {
        self.window_visible = true;
        self.show_settings = false;
        self.focus_query = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide_launcher_window(&mut self, ctx: &egui::Context) {
        if !self.hotkeys.has_registered_toggle() {
            self.last_status =
                Some("No global toggle registered; leaving launcher visible".to_string());
            return;
        }

        self.window_visible = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn header_fill(&self) -> Color32 {
        Color32::from_rgba_unmultiplied(18, 22, 25, alpha_for_opacity(&self.config, 235))
    }

    fn surface_fill(&self) -> Color32 {
        let max_alpha = if self.config.appearance.blur {
            226
        } else {
            242
        };
        Color32::from_rgba_unmultiplied(20, 23, 27, alpha_for_opacity(&self.config, max_alpha))
    }

    fn border_stroke(&self) -> Stroke {
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(&self.config, 34)),
        )
    }

    fn render_launcher(&mut self, ui: &mut egui::Ui) {
        let search_response = ui.add(
            TextEdit::singleline(&mut self.query)
                .hint_text("Search apps, settings, files, web, and AI tools")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Heading),
        );
        if self.focus_query {
            search_response.request_focus();
            self.focus_query = false;
        }

        ui.add_space(12.0);

        let results = self.results();
        let result_limit = effective_max_results(&self.config);
        let shown_count = results.len().min(result_limit);
        if shown_count > 0 {
            self.selected = self.selected.min(shown_count - 1);
        }

        if ui.input(|input| input.key_pressed(Key::ArrowDown)) && shown_count > 0 {
            self.selected = (self.selected + 1).min(shown_count - 1);
        }
        if ui.input(|input| input.key_pressed(Key::ArrowUp)) && shown_count > 0 {
            self.selected = self.selected.saturating_sub(1);
        }
        if ui.input(|input| input.key_pressed(Key::Enter))
            && let Some(result) = results.get(self.selected)
        {
            self.execute_result(result);
        }

        let selected_preview = results.get(self.selected).cloned();
        if self.config.appearance.show_preview && ui.available_width() >= 680.0 {
            ui.horizontal(|ui| {
                let preview_width = 250.0;
                let results_width = (ui.available_width() - preview_width - 16.0).max(320.0);
                ui.vertical(|ui| {
                    ui.set_width(results_width);
                    self.render_result_list(ui, &results, shown_count);
                });
                ui.add_space(8.0);
                if let Some(result) = selected_preview.as_ref() {
                    self.render_preview_panel(ui, result, preview_width);
                }
            });
        } else {
            self.render_result_list(ui, &results, shown_count);
        }

        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            if let Some(status) = &self.last_status {
                ui.label(RichText::new(status).color(Color32::from_rgb(180, 190, 205)));
            } else {
                ui.label(
                    RichText::new(format!(
                        "Enter open    {} toggle    {} settings    Esc hide",
                        toggle_hint(&self.config),
                        self.config.hotkeys.settings
                    ))
                    .color(Color32::from_rgb(140, 148, 160)),
                );
            }
        });
    }

    fn render_result_list(
        &mut self,
        ui: &mut egui::Ui,
        results: &[SearchResult],
        shown_count: usize,
    ) {
        if shown_count == 0 {
            let message = if self.query.trim().is_empty() {
                "Start typing to search apps, files, settings, commands, web, and AI"
            } else {
                "No matches"
            };
            Frame::new()
                .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 10))
                .corner_radius(6)
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.label(RichText::new(message).color(Color32::from_rgb(165, 174, 188)));
                });
            return;
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height((ui.available_height() - 34.0).max(140.0))
            .show(ui, |ui| {
                for (index, result) in results.iter().take(shown_count).enumerate() {
                    self.render_result(ui, index, result);
                }
            });
    }

    fn render_result(&mut self, ui: &mut egui::Ui, index: usize, result: &SearchResult) {
        let selected = index == self.selected;
        let fill = if selected {
            Color32::from_rgba_unmultiplied(86, 119, 142, alpha_for_opacity(&self.config, 170))
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(&self.config, 18))
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

    fn render_preview_panel(&self, ui: &mut egui::Ui, result: &SearchResult, width: f32) {
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                255,
                255,
                255,
                alpha_for_opacity(&self.config, 13),
            ))
            .stroke(self.border_stroke())
            .corner_radius(6)
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.label(RichText::new(category_label(&result.item)).monospace());
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&result.item.label)
                        .strong()
                        .size(preview_heading_size(&self.config)),
                );
                if let Some(subtitle) = &result.item.subtitle {
                    ui.add_space(8.0);
                    ui.label(RichText::new(subtitle).color(Color32::from_rgb(170, 179, 194)));
                }
                ui.add_space(14.0);
                setting_row(ui, "Source", result.item.source.as_str());
                setting_row(ui, "Score", result.score.to_string());
                if let Some(action) = result.item.actions.first()
                    && let Some(command) = &action.command
                {
                    setting_row(ui, "Action", command.as_str());
                }
            });
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

    fn open_profile_dir(&mut self) {
        if let Err(error) = fs::create_dir_all(&self.profile_dir) {
            self.last_status = Some(format!("Could not create profile folder: {error}"));
            return;
        }

        self.open_path(self.profile_dir.clone(), "Opened profile folder");
    }

    fn open_profile_file(&mut self, profile_file: ProfileFile) {
        let path = self.profile_dir.join(profile_file.file_name());
        if let Err(error) = ensure_profile_file(&path, profile_file.template()) {
            self.last_status = Some(format!(
                "Could not create {}: {error}",
                profile_file.file_name()
            ));
            return;
        }

        self.open_path(path, format!("Opened {}", profile_file.file_name()));
    }

    fn open_path(&mut self, path: PathBuf, success_message: impl Into<String>) {
        let action = Action::open_file(path.to_string_lossy().to_string());
        match execute_action(&action) {
            Ok(()) => {
                self.last_status = Some(success_message.into());
            }
            Err(error) => {
                self.last_status = Some(format!("Could not open {}: {}", path.display(), error));
            }
        }
    }

    fn save_config_sections(&mut self) {
        let path = self.profile_dir.join(ProfileFile::Config.file_name());
        match write_config_sections(&path, &self.config) {
            Ok(()) => {
                self.last_status = Some(format!("Saved {}", ProfileFile::Config.file_name()));
            }
            Err(error) => {
                self.last_status = Some(format!("Could not save config.toml: {error}"));
            }
        }
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

    fn render_settings_page(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.settings_page.label());
        ui.add_space(8.0);

        match self.settings_page {
            SettingsPage::General => {
                setting_row(ui, "Profile", self.profile_dir.display().to_string());
                ui.horizontal(|ui| {
                    if ui.button("Open profile folder").clicked() {
                        self.open_profile_dir();
                    }
                    if ui.button("Reload profile").clicked() {
                        self.reload_profile(ui.ctx());
                    }
                });
                ui.add_space(8.0);
                setting_row(ui, "Startup", self.config.general.startup.to_string());
                let mut local_only = self.config.general.local_only;
                if ui.checkbox(&mut local_only, "Local-only mode").changed() {
                    self.config.general.local_only = local_only;
                    self.save_config_sections();
                }
                setting_row(
                    ui,
                    "History limit",
                    self.config.general.history_limit.to_string(),
                );
                setting_row(ui, "Portable mode", "Auto-detect");
            }
            SettingsPage::Appearance => {
                let mut changed = false;
                ui.horizontal_wrapped(|ui| {
                    ui.label("Theme");
                    changed |= ui
                        .selectable_value(
                            &mut self.config.appearance.theme,
                            "dark-acrylic".to_string(),
                            "Dark acrylic",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut self.config.appearance.theme,
                            "dark-compact".to_string(),
                            "Dark compact",
                        )
                        .changed();
                });

                let mut opacity = (self.config.appearance.opacity * 100.0).round();
                if ui
                    .add(Slider::new(&mut opacity, 70.0..=100.0).text("Opacity"))
                    .changed()
                {
                    self.config.appearance.opacity = opacity / 100.0;
                    changed = true;
                }

                let mut font_size = self.config.appearance.font_size;
                if ui
                    .add(Slider::new(&mut font_size, 12..=22).text("Text size"))
                    .changed()
                {
                    self.config.appearance.font_size = font_size;
                    changed = true;
                }

                let mut max_results = self.config.appearance.max_results;
                if ui
                    .add(Slider::new(&mut max_results, 4..=24).text("Visible results"))
                    .changed()
                {
                    self.config.appearance.max_results = max_results;
                    changed = true;
                }

                changed |= ui
                    .checkbox(
                        &mut self.config.appearance.blur,
                        "Acrylic-style transparency",
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut self.config.appearance.show_preview, "Preview pane")
                    .changed();

                if changed {
                    self.apply_appearance(ui.ctx());
                    self.save_config_sections();
                }
            }
            SettingsPage::Hotkeys => {
                setting_row(ui, "Toggle launcher", self.config.hotkeys.toggle.as_str());
                setting_row(ui, "Copilot key", COPILOT_TOGGLE_HOTKEY);
                setting_row(ui, "Fallback toggle", FALLBACK_TOGGLE_HOTKEY);
                setting_row(ui, "Registered", self.hotkeys.registered_labels());
                setting_row(ui, "Settings", self.config.hotkeys.settings.as_str());
                setting_row(ui, "Alternate action", "Shift+Enter");
                setting_row(ui, "Elevated action", "Ctrl+Enter");
            }
            SettingsPage::Catalogs => {
                setting_row(ui, "Total catalog items", self.catalog.len().to_string());
                if ui.button("Refresh catalogs").clicked() {
                    self.reload_profile(ui.ctx());
                }
                ui.add_space(8.0);
                setting_row(ui, "PATH executables", self.path_item_count.to_string());
                setting_row(
                    ui,
                    "Start Menu shortcuts",
                    self.start_menu_item_count.to_string(),
                );
                setting_row(ui, "File profiles", self.config.catalogs.len().to_string());
                setting_row(
                    ui,
                    "Indexed files/folders",
                    self.file_catalog_item_count.to_string(),
                );
                setting_row(
                    ui,
                    "Skipped catalog paths",
                    self.file_catalog_skipped_paths.to_string(),
                );
            }
            SettingsPage::Commands => {
                setting_row(ui, "User commands", self.config.commands.len().to_string());
                setting_row(
                    ui,
                    "Web shortcuts",
                    self.config.web_search.len().to_string(),
                );
                setting_row(ui, "Action confirmation", "Per command");
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Open commands.toml").clicked() {
                        self.open_profile_file(ProfileFile::Commands);
                    }
                    if ui.button("Open catalogs.toml").clicked() {
                        self.open_profile_file(ProfileFile::Catalogs);
                    }
                });
            }
            SettingsPage::AiProviders => {
                setting_row(ui, "Default provider", "Not configured");
                setting_row(ui, "Local-only mode", "Available");
                setting_row(ui, "Tool calling", "Manifest based");
                ui.add_space(8.0);
                if ui.button("Open ai.toml").clicked() {
                    self.open_profile_file(ProfileFile::Ai);
                }
            }
            SettingsPage::Tools => {
                setting_row(ui, "Tool manifests", "JSON");
                setting_row(ui, "External runner", "Process");
                setting_row(ui, "Confirmation", "Safety based");
            }
            SettingsPage::Diagnostics => {
                ui.horizontal(|ui| {
                    if ui.button("Reload").clicked() {
                        self.reload_profile(ui.ctx());
                    }
                    if ui.button("Open config.toml").clicked() {
                        self.open_profile_file(ProfileFile::Config);
                    }
                    if ui.button("Quit Veyra").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(8.0);
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

struct RuntimeState {
    config: VeyraConfig,
    catalog: Vec<CatalogItem>,
    load_messages: Vec<String>,
    path_item_count: usize,
    start_menu_item_count: usize,
    file_catalog_item_count: usize,
    file_catalog_skipped_paths: usize,
}

struct HotkeyRuntime {
    manager: Option<GlobalHotKeyManager>,
    registered_hotkeys: Vec<HotKey>,
    toggle_hotkey_ids: Vec<u32>,
    registered_labels: Vec<String>,
    events: mpsc::Receiver<GlobalHotKeyEvent>,
    copilot_events: mpsc::Receiver<()>,
    copilot_hook_registered: bool,
}

impl HotkeyRuntime {
    fn new(ctx: &egui::Context) -> Self {
        let (copilot_sender, copilot_events) = mpsc::channel();
        let copilot_hook_registered = spawn_copilot_keyboard_hook(ctx, copilot_sender);
        Self {
            manager: GlobalHotKeyManager::new().ok(),
            registered_hotkeys: Vec::new(),
            toggle_hotkey_ids: Vec::new(),
            registered_labels: Vec::new(),
            events: spawn_global_hotkey_event_pump(ctx),
            copilot_events,
            copilot_hook_registered,
        }
    }

    fn register_toggle_hotkeys(&mut self, config: &VeyraConfig) -> Vec<String> {
        let mut messages = Vec::new();
        if self.copilot_hook_registered {
            messages.push("Installed Windows Copilot key low-level hook".to_string());
        }
        let Some(manager) = &self.manager else {
            messages.push("Global hotkeys unavailable on this session".to_string());
            return messages;
        };

        if !self.registered_hotkeys.is_empty()
            && let Err(error) = manager.unregister_all(&self.registered_hotkeys)
        {
            messages.push(format!("Could not unregister old global hotkeys: {error}"));
        }

        self.registered_hotkeys.clear();
        self.toggle_hotkey_ids.clear();
        self.registered_labels.clear();

        for label in toggle_hotkey_candidates(&config.hotkeys.toggle) {
            match parse_global_hotkey(&label) {
                Ok(hotkey) => match manager.register(hotkey) {
                    Ok(()) => {
                        self.toggle_hotkey_ids.push(hotkey.id());
                        self.registered_hotkeys.push(hotkey);
                        self.registered_labels.push(label.clone());
                        messages.push(format!("Registered global toggle {label}"));
                    }
                    Err(error) => {
                        messages.push(format!("Could not register global toggle {label}: {error}"));
                    }
                },
                Err(error) => {
                    messages.push(format!("Could not parse global toggle {label}: {error}"));
                }
            }
        }

        messages
    }

    fn is_toggle_event(&self, event: GlobalHotKeyEvent) -> bool {
        event.state == HotKeyState::Pressed && self.toggle_hotkey_ids.contains(&event.id)
    }

    fn registered_label(&self, label: &str) -> bool {
        self.registered_labels.iter().any(|value| value == label)
    }

    fn registered_labels(&self) -> String {
        let mut labels = self.registered_labels.clone();
        if self.copilot_hook_registered {
            labels.push(COPILOT_HOOK_LABEL.to_string());
        }

        if labels.is_empty() {
            "None".to_string()
        } else {
            labels.join(", ")
        }
    }

    fn has_registered_toggle(&self) -> bool {
        self.copilot_hook_registered || !self.toggle_hotkey_ids.is_empty()
    }
}

fn spawn_global_hotkey_event_pump(ctx: &egui::Context) -> mpsc::Receiver<GlobalHotKeyEvent> {
    let (sender, receiver) = mpsc::channel();
    let ctx = ctx.clone();
    thread::spawn(move || {
        for event in GlobalHotKeyEvent::receiver() {
            if sender.send(event).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    receiver
}

#[cfg(windows)]
struct CopilotHookSink {
    sender: mpsc::Sender<()>,
    ctx: egui::Context,
}

#[cfg(windows)]
static COPILOT_HOOK_SINK: OnceLock<CopilotHookSink> = OnceLock::new();

#[cfg(windows)]
fn spawn_copilot_keyboard_hook(ctx: &egui::Context, sender: mpsc::Sender<()>) -> bool {
    let _ = COPILOT_HOOK_SINK.set(CopilotHookSink {
        sender,
        ctx: ctx.clone(),
    });

    let (ready_sender, ready_receiver) = mpsc::channel();
    thread::spawn(move || unsafe {
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(copilot_keyboard_proc),
            std::ptr::null_mut(),
            0,
        );
        let _ = ready_sender.send(!hook.is_null());
        if hook.is_null() {
            return;
        }

        let mut message = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        UnhookWindowsHookEx(hook);
    });

    ready_receiver
        .recv_timeout(Duration::from_millis(750))
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn spawn_copilot_keyboard_hook(_ctx: &egui::Context, _sender: mpsc::Sender<()>) -> bool {
    false
}

#[cfg(windows)]
unsafe extern "system" fn copilot_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 && is_key_down_message(wparam) {
        let keyboard = unsafe { *(lparam as *const KBDLLHOOKSTRUCT) };
        if keyboard.vkCode == u32::from(VK_F23) && win_shift_down() {
            if let Some(sink) = COPILOT_HOOK_SINK.get() {
                let _ = sink.sender.send(());
                sink.ctx.request_repaint();
            }
            return 1;
        }
    }

    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(windows)]
fn is_key_down_message(wparam: WPARAM) -> bool {
    wparam == WM_KEYDOWN as WPARAM || wparam == WM_SYSKEYDOWN as WPARAM
}

#[cfg(windows)]
fn win_shift_down() -> bool {
    (key_down(VK_LWIN) || key_down(VK_RWIN)) && shift_down()
}

#[cfg(windows)]
fn shift_down() -> bool {
    key_down(VK_SHIFT) || key_down(VK_LSHIFT) || key_down(VK_RSHIFT)
}

#[cfg(windows)]
fn key_down(key: u16) -> bool {
    unsafe { GetAsyncKeyState(i32::from(key)) & i16::MIN != 0 }
}

const COPILOT_HOOK_LABEL: &str = "Copilot hook";
const COPILOT_TOGGLE_HOTKEY: &str = "Win+Shift+F23";
const FALLBACK_TOGGLE_HOTKEY: &str = "Alt+Space";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileFile {
    Config,
    Commands,
    Catalogs,
    Ai,
}

impl ProfileFile {
    fn file_name(self) -> &'static str {
        match self {
            ProfileFile::Config => "config.toml",
            ProfileFile::Commands => "commands.toml",
            ProfileFile::Catalogs => "catalogs.toml",
            ProfileFile::Ai => "ai.toml",
        }
    }

    fn template(self) -> &'static str {
        match self {
            ProfileFile::Config => DEFAULT_CONFIG_TOML,
            ProfileFile::Commands => DEFAULT_COMMANDS_TOML,
            ProfileFile::Catalogs => DEFAULT_CATALOGS_TOML,
            ProfileFile::Ai => DEFAULT_AI_TOML,
        }
    }
}

const DEFAULT_CONFIG_TOML: &str = r#"[general]
startup = true
local_only = false
history_limit = 5000

[hotkeys]
toggle = "Win+Shift+F23"
settings = "Ctrl+,"

[appearance]
theme = "dark-acrylic"
opacity = 0.92
blur = true
font_size = 15
max_results = 10
show_preview = true
"#;

const DEFAULT_COMMANDS_TOML: &str = r#"[[commands]]
id = "settings.display"
label = "Settings: Display"
command = "explorer.exe"
args = ["ms-settings:display"]
terminal = false
requires_confirmation = false
keywords = ["display", "monitor", "resolution"]

[[web_search]]
id = "github.code"
alias = "gh"
label = "GitHub Code"
url = "https://github.com/search?q={query}&type=code"
"#;

const DEFAULT_CATALOGS_TOML: &str = r#"[[catalogs]]
id = "development"
label = "Development"
paths = ["%USERPROFILE%\\Development"]
include_patterns = ["*.md", "*.toml", "*.rs"]
exclude_patterns = ["**\\node_modules\\**", "**\\.git\\**"]
recursive = true
follow_symlinks = false
max_depth = 6
enabled = true
"#;

const DEFAULT_AI_TOML: &str = r#"[ai]
enabled = false
default_provider = "local"
local_only = false
warmup_on_startup = false

[[providers]]
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

fn load_runtime_state(profile_dir: &Path) -> RuntimeState {
    let (config, mut loaded_items, mut load_messages) = load_profile(profile_dir);
    let platform_items = discover_platform_catalog_items();
    let path_item_count = platform_items
        .iter()
        .filter(|item| item.source == "path")
        .count();
    let start_menu_item_count = platform_items
        .iter()
        .filter(|item| item.source == "start_menu")
        .count();
    load_messages.push(format!(
        "Discovered {path_item_count} PATH executables and {start_menu_item_count} Start Menu shortcuts"
    ));
    loaded_items.extend(platform_items);

    let file_catalog = discover_file_catalog_items(&config.catalogs);
    let file_catalog_item_count = file_catalog.items.len();
    let file_catalog_skipped_paths = file_catalog.skipped_paths;
    load_messages.push(format!(
        "Indexed {file_catalog_item_count} file catalog items from {} enabled profiles",
        file_catalog.enabled_profiles
    ));
    if file_catalog_skipped_paths > 0 {
        load_messages.push(format!(
            "Skipped {file_catalog_skipped_paths} missing or unsupported catalog paths"
        ));
    }
    loaded_items.extend(file_catalog.items);

    let mut catalog = seed_catalog();
    catalog.extend(loaded_items);

    RuntimeState {
        config,
        catalog,
        load_messages,
        path_item_count,
        start_menu_item_count,
        file_catalog_item_count,
        file_catalog_skipped_paths,
    }
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

fn ensure_profile_file(path: &Path, template: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, template)
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

    if matches!(
        mode,
        ConfigMergeMode::Full | ConfigMergeMode::CommandsOnly | ConfigMergeMode::CatalogsOnly
    ) {
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

fn toggle_hotkey_candidates(configured: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut normalized = Vec::new();

    for candidate in [configured, COPILOT_TOGGLE_HOTKEY, FALLBACK_TOGGLE_HOTKEY] {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            continue;
        }

        let normalized_candidate = normalize_global_hotkey(candidate);
        if normalized
            .iter()
            .any(|value: &String| value.eq_ignore_ascii_case(&normalized_candidate))
        {
            continue;
        }

        normalized.push(normalized_candidate);
        candidates.push(candidate.to_string());
    }

    candidates
}

fn parse_global_hotkey(hotkey: &str) -> Result<HotKey, global_hotkey::hotkey::HotKeyParseError> {
    normalize_global_hotkey(hotkey).parse()
}

fn normalize_global_hotkey(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "win" | "windows" | "meta" | "super" => "Super".to_string(),
            "shift" => "Shift".to_string(),
            "alt" | "option" => "Alt".to_string(),
            "ctrl" | "control" => "Ctrl".to_string(),
            "escape" => "Esc".to_string(),
            value if is_function_key(value) => value.to_ascii_uppercase(),
            _ => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn is_function_key(value: &str) -> bool {
    value
        .strip_prefix('f')
        .is_some_and(|suffix| suffix.parse::<u8>().is_ok())
}

fn effective_font_size(config: &VeyraConfig) -> f32 {
    config.appearance.font_size.clamp(12, 22) as f32
}

fn preview_heading_size(config: &VeyraConfig) -> f32 {
    (effective_font_size(config) + 4.0).min(26.0)
}

fn effective_max_results(config: &VeyraConfig) -> usize {
    if config.appearance.max_results == 0 {
        return 10;
    }

    config.appearance.max_results.clamp(4, 24) as usize
}

fn alpha_for_opacity(config: &VeyraConfig, max_alpha: u8) -> u8 {
    (f32::from(max_alpha) * config.appearance.opacity.clamp(0.65, 1.0)).round() as u8
}

fn toggle_hint(config: &VeyraConfig) -> String {
    if normalize_global_hotkey(&config.hotkeys.toggle)
        .eq_ignore_ascii_case(&normalize_global_hotkey(COPILOT_TOGGLE_HOTKEY))
    {
        "Copilot / Alt+Space".to_string()
    } else {
        format!("{} / Copilot", config.hotkeys.toggle)
    }
}

fn write_config_sections(path: &Path, config: &VeyraConfig) -> io::Result<()> {
    let mut document = if path.exists() {
        let raw = fs::read_to_string(path)?;
        raw.parse::<DocumentMut>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
    } else {
        DocumentMut::new()
    };

    document["general"]["startup"] = value(config.general.startup);
    document["general"]["local_only"] = value(config.general.local_only);
    document["general"]["history_limit"] = value(i64::from(config.general.history_limit));

    document["hotkeys"]["toggle"] = value(config.hotkeys.toggle.clone());
    document["hotkeys"]["settings"] = value(config.hotkeys.settings.clone());

    document["appearance"]["theme"] = value(config.appearance.theme.clone());
    document["appearance"]["opacity"] = value(f64::from(config.appearance.opacity));
    document["appearance"]["blur"] = value(config.appearance.blur);
    document["appearance"]["font_size"] = value(i64::from(config.appearance.font_size));
    document["appearance"]["max_results"] = value(i64::from(config.appearance.max_results));
    document["appearance"]["show_preview"] = value(config.appearance.show_preview);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, document.to_string())
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

    #[test]
    fn commands_file_can_include_imported_catalog_profiles() {
        let profile = temp_profile_dir();
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("commands.toml"),
            r#"
                [[commands]]
                label = "Command: Test"
                command = "test.exe"

                [[catalogs]]
                id = "imported"
                label = "Imported Files"
                paths = ["C:/Imported"]
                recursive = true
            "#,
        )
        .unwrap();

        let (config, items, _) = load_profile(&profile);

        assert_eq!(config.commands.len(), 1);
        assert_eq!(config.catalogs.len(), 1);
        assert_eq!(config.catalogs[0].id, "imported");
        assert_eq!(items.len(), 1);

        fs::remove_dir_all(&profile).ok();
    }

    #[test]
    fn ensure_profile_file_creates_missing_file_without_overwriting() {
        let profile = temp_profile_dir();
        let path = profile.join("commands.toml");

        ensure_profile_file(&path, "first").unwrap();
        ensure_profile_file(&path, "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "first");

        fs::remove_dir_all(&profile).ok();
    }

    #[test]
    fn runtime_state_indexes_imported_file_catalogs() {
        let profile = temp_profile_dir();
        let files = profile.join("files");
        fs::create_dir_all(&files).unwrap();
        fs::write(files.join("note.md"), "note").unwrap();
        fs::write(
            profile.join("commands.toml"),
            format!(
                r#"
                [[catalogs]]
                id = "docs"
                label = "Docs"
                paths = ["{}"]
                include_patterns = ["*.md"]
                recursive = true
                enabled = true
            "#,
                files.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        let runtime = load_runtime_state(&profile);

        assert_eq!(runtime.config.catalogs.len(), 1);
        assert_eq!(runtime.file_catalog_item_count, 1);
        assert!(runtime.catalog.iter().any(|item| item.label == "note.md"));

        fs::remove_dir_all(&profile).ok();
    }

    #[test]
    fn hotkey_candidates_include_copilot_and_fallback_without_duplicates() {
        let candidates = toggle_hotkey_candidates("Super+Shift+F23");

        assert_eq!(candidates, vec!["Super+Shift+F23", FALLBACK_TOGGLE_HOTKEY]);
    }

    #[test]
    fn normalizes_windows_hotkey_aliases_for_global_parser() {
        assert_eq!(
            normalize_global_hotkey("Win + Shift + F23"),
            "Super+Shift+F23"
        );
        assert!(parse_global_hotkey("Win+Shift+F23").is_ok());
    }

    #[test]
    fn clamps_appearance_values_for_ui() {
        let mut config = VeyraConfig::default();

        config.appearance.font_size = 2;
        config.appearance.max_results = 0;
        config.appearance.opacity = 0.1;
        assert_eq!(effective_font_size(&config), 12.0);
        assert_eq!(effective_max_results(&config), 10);
        assert_eq!(alpha_for_opacity(&config, 200), 130);

        config.appearance.font_size = 80;
        config.appearance.max_results = 90;
        config.appearance.opacity = 2.0;
        assert_eq!(effective_font_size(&config), 22.0);
        assert_eq!(effective_max_results(&config), 24);
        assert_eq!(alpha_for_opacity(&config, 200), 200);
    }

    #[test]
    fn write_config_sections_preserves_unrelated_sections() {
        let profile = temp_profile_dir();
        let path = profile.join("config.toml");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            &path,
            r#"
                [appearance]
                theme = "old"

                [[commands]]
                id = "keep"
                label = "Keep"
                command = "keep.exe"
            "#,
        )
        .unwrap();

        let mut config = VeyraConfig::default();
        config.general.local_only = true;
        config.hotkeys.toggle = COPILOT_TOGGLE_HOTKEY.to_string();
        config.appearance.theme = "dark-compact".to_string();
        config.appearance.opacity = 0.86;
        config.appearance.max_results = 14;

        write_config_sections(&path, &config).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let loaded = VeyraConfig::from_toml_str(&raw).unwrap();

        assert!(loaded.general.local_only);
        assert_eq!(loaded.hotkeys.toggle, COPILOT_TOGGLE_HOTKEY);
        assert_eq!(loaded.appearance.theme, "dark-compact");
        assert_eq!(loaded.appearance.max_results, 14);
        assert!(raw.contains("[[commands]]"));
        assert!(raw.contains("keep.exe"));

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
