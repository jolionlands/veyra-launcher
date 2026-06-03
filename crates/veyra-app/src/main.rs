use eframe::egui::{
    self, Align, Color32, Frame, Key, Layout, Margin, RichText, Stroke, TextEdit, Vec2,
};
use veyra_core::{CatalogItem, SearchResult, search, seed_catalog};
use veyra_platform::profile_dir;

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
    selected: usize,
}

impl VeyraApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        Self {
            query: String::new(),
            catalog: seed_catalog(),
            show_settings: false,
            selected: 0,
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

        for (index, result) in results.iter().take(10).enumerate() {
            self.render_result(ui, index, result);
        }

        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.label(
                RichText::new("Ctrl+, settings    Esc clear/close")
                    .color(Color32::from_rgb(140, 148, 160)),
            );
        });
    }

    fn render_result(&mut self, ui: &mut egui::Ui, index: usize, result: &SearchResult) {
        let selected = index == self.selected;
        let fill = if selected {
            Color32::from_rgba_unmultiplied(72, 104, 136, 150)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 12)
        };

        Frame::new()
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
            });
        ui.add_space(6.0);
    }

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(190.0);
                ui.heading("Settings");
                ui.separator();
                for item in [
                    "General",
                    "Appearance",
                    "Hotkeys",
                    "Catalogs",
                    "Commands",
                    "AI Providers",
                    "Tools",
                    "Diagnostics",
                ] {
                    let _ = ui.selectable_label(item == "General", item);
                }
            });

            ui.separator();

            ui.vertical(|ui| {
                ui.heading("General");
                ui.add_space(8.0);
                setting_row(ui, "Profile", profile_dir("Veyra").display().to_string());
                setting_row(ui, "Startup", "Planned");
                setting_row(ui, "Global hotkey", "Alt+Space");
                setting_row(ui, "Local-only AI mode", "Available in provider settings");
                setting_row(ui, "External plugins", "JSON-RPC over stdio");
            });
        });
    }
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
