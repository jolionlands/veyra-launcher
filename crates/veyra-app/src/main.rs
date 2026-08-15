#![cfg_attr(windows, windows_subsystem = "windows")]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::{
    fs, io,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use eframe::egui::{
    self, Align, Color32, FontId, Frame, Key, Layout, Margin, RichText, ScrollArea, Slider, Stroke,
    TextEdit, TextStyle, Vec2, WindowLevel,
};
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, value};
use veyra_core::config::{AiProvider, AiProviderKind, CommandEntry, VeyraConfig, WebSearchEntry};
use veyra_core::{
    Action, ActionKind, CatalogItem, ItemCategory, SearchIndex, SearchResult, seed_catalog,
};
use veyra_platform::{
    discover_file_catalog_items, discover_platform_catalog_items, execute_action,
    is_platform_cache_fresh, load_fresh_cached_platform_catalog_items, profile_dir,
    save_cached_platform_catalog_items,
};
use veyra_plugin::{
    execute_json_rpc_action, load_plugin_extensions, load_plugin_suggestions,
    parse_json_rpc_action_command, process_plugin_item,
};

mod ai_logging;
mod ai_prompt;
mod ai_tools;
mod ai_transport;
mod history;
mod hotkeys;
mod windowing;
use ai_logging::{
    ai_chat_log_path, ai_chat_snapshot_dir, append_ai_chat_log, ensure_ai_chat_log_file,
    save_ai_chat_snapshot,
};
use ai_prompt::{
    AiContextItem, AiPromptPlan, format_ai_model_prompt, prompt_needs_conversation_context,
};
use ai_tools::{
    AiToolCall, ai_answer_display_text, ai_tool_call, ai_tool_call_param, normalize_ai_tool_name,
    parse_ai_function_calls,
};
use ai_transport::{
    call_ai_provider, format_process_ai_prompt, prewarm_ai_provider, shutdown_warm_ai_processes,
};
#[cfg(test)]
use ai_transport::{
    chat_completions_url, clean_process_ai_answer, is_local_http_endpoint,
    parse_chat_completion_answer, response_error_excerpt,
};
use history::{LaunchHistory, history_path, load_launch_history, save_launch_history};
use hotkeys::{COPILOT_TOGGLE_HOTKEY, FALLBACK_TOGGLE_HOTKEY, HotkeyRuntime};
#[cfg(any(not(windows), test))]
use windowing::window_position;
use windowing::{
    WindowLayoutMode, apply_native_backdrop, apply_native_backdrop_for_config,
    configure_process_dpi_awareness, effective_layout_scale, layout_size_matches, min_window_size,
    native_capture_target_monitor, native_show_launcher_window, window_size_for_monitor,
};
#[cfg(windows)]
use windowing::{native_center_window, native_monitor_logical_size};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::SYSTEMTIME,
    System::{
        DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        },
        Memory::{GlobalLock, GlobalUnlock},
        SystemInformation::GetLocalTime,
        Time::{
            DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation, TIME_ZONE_ID_INVALID,
        },
    },
};

fn main() -> eframe::Result<()> {
    configure_process_dpi_awareness();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Veyra")
            .with_inner_size([680.0, 76.0])
            .with_min_inner_size([360.0, 64.0])
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop),
        ..Default::default()
    };

    eframe::run_native(
        "Veyra",
        options,
        Box::new(|cc| Ok(Box::new(VeyraApp::new(cc)))),
    )
}

const PLUGIN_SUGGEST_DEBOUNCE: Duration = Duration::from_millis(160);
const AI_COMPOSE_INPUT_HEIGHT: f32 = 34.0;
const AI_COMPOSE_BUTTON_HEIGHT: f32 = 30.0;
const AI_SEND_BUTTON_WIDTH: f32 = 58.0;
const AI_CLIP_BUTTON_WIDTH: f32 = 54.0;
const AI_THINKING_STATUS_WIDTH: f32 = 88.0;
const AI_THINKING_STATUS_COMPACT_WIDTH: f32 = 26.0;
const AI_TOOL_SUGGESTION_ROW_HEIGHT: f32 = 50.0;
const AI_TOOL_ROW_TRAILING_WIDTH: f32 = 68.0;
const AI_TOOL_RUN_BUTTON_WIDTH: f32 = 48.0;

struct VeyraApp {
    query: String,
    catalog: Vec<CatalogItem>,
    search_index: SearchIndex,
    show_settings: bool,
    settings_page: SettingsPage,
    window_visible: bool,
    focus_query: bool,
    selected: usize,
    last_status: Option<String>,
    profile_dir: PathBuf,
    config: VeyraConfig,
    launch_history: LaunchHistory,
    load_messages: Vec<String>,
    path_item_count: usize,
    start_menu_item_count: usize,
    file_catalog_item_count: usize,
    file_catalog_skipped_paths: usize,
    plugin_process_item_count: usize,
    plugin_json_rpc_item_count: usize,
    tool_manifest_item_count: usize,
    plugin_error_count: usize,
    runtime_load_ms: u128,
    runtime_refreshing: bool,
    runtime_sender: mpsc::Sender<RuntimeUpdate>,
    runtime_events: mpsc::Receiver<RuntimeUpdate>,
    plugin_suggestion_items: Vec<CatalogItem>,
    plugin_suggestion_query: String,
    plugin_suggestion_generation: u64,
    plugin_suggestion_refreshing: bool,
    plugin_suggestion_pending_query: String,
    plugin_suggestion_due_at: Option<Instant>,
    plugin_suggestion_sender: mpsc::Sender<PluginSuggestionBatch>,
    plugin_suggestion_events: mpsc::Receiver<PluginSuggestionBatch>,
    ai_response: Option<AiResponse>,
    ai_panel_expanded: bool,
    show_ai_conversation: bool,
    ai_conversation_input: String,
    ai_focus_conversation_input: bool,
    ai_conversation_messages: Vec<AiConversationMessage>,
    ai_session_id: u64,
    ai_session_provider_id: Option<String>,
    ai_turn_index: u32,
    ai_request_generation: u64,
    ai_request_running: bool,
    ai_response_sender: mpsc::Sender<AiResponse>,
    ai_response_events: mpsc::Receiver<AiResponse>,
    ai_warmup_generation: u64,
    ai_warmup_running: bool,
    ai_warmup_sender: mpsc::Sender<AiWarmupEvent>,
    ai_warmup_events: mpsc::Receiver<AiWarmupEvent>,
    hotkeys: HotkeyRuntime,
    layout_mode: Option<WindowLayoutMode>,
    layout_size: Option<Vec2>,
    native_center_settle_frames: u8,
}

impl VeyraApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        apply_native_backdrop(cc);
        let profile_dir = profile_dir("Veyra");
        const PLATFORM_CACHE_TTL_SECONDS: u64 = veyra_platform::PLATFORM_CACHE_DEFAULT_TTL_SECONDS;
        let platform_cache_fresh =
            is_platform_cache_fresh(&profile_dir, PLATFORM_CACHE_TTL_SECONDS);
        let mut runtime = load_runtime_state_with_cache(&profile_dir, false);
        let launch_history = load_launch_history(&profile_dir);
        let mut hotkeys = HotkeyRuntime::new(&cc.egui_ctx);
        runtime
            .load_messages
            .extend(hotkeys.register_toggle_hotkeys(&runtime.config));
        let search_index = SearchIndex::new(&runtime.catalog);
        let (runtime_sender, runtime_events) = mpsc::channel();
        let (plugin_suggestion_sender, plugin_suggestion_events) = mpsc::channel();
        let (ai_response_sender, ai_response_events) = mpsc::channel();
        let (ai_warmup_sender, ai_warmup_events) = mpsc::channel();

        let mut app = Self {
            query: String::new(),
            catalog: runtime.catalog,
            search_index,
            show_settings: false,
            settings_page: SettingsPage::General,
            window_visible: true,
            focus_query: true,
            selected: 0,
            last_status: None,
            profile_dir,
            config: runtime.config,
            launch_history,
            load_messages: runtime.load_messages,
            path_item_count: runtime.path_item_count,
            start_menu_item_count: runtime.start_menu_item_count,
            file_catalog_item_count: runtime.file_catalog_item_count,
            file_catalog_skipped_paths: runtime.file_catalog_skipped_paths,
            plugin_process_item_count: runtime.plugin_process_item_count,
            plugin_json_rpc_item_count: runtime.plugin_json_rpc_item_count,
            tool_manifest_item_count: runtime.tool_manifest_item_count,
            plugin_error_count: runtime.plugin_error_count,
            runtime_load_ms: runtime.runtime_load_ms,
            runtime_refreshing: false,
            runtime_sender,
            runtime_events,
            plugin_suggestion_items: Vec::new(),
            plugin_suggestion_query: String::new(),
            plugin_suggestion_generation: 0,
            plugin_suggestion_refreshing: false,
            plugin_suggestion_pending_query: String::new(),
            plugin_suggestion_due_at: None,
            plugin_suggestion_sender,
            plugin_suggestion_events,
            ai_response: None,
            ai_panel_expanded: false,
            show_ai_conversation: false,
            ai_conversation_input: String::new(),
            ai_focus_conversation_input: false,
            ai_conversation_messages: Vec::new(),
            ai_session_id: unix_timestamp_millis(),
            ai_session_provider_id: None,
            ai_turn_index: 0,
            ai_request_generation: 0,
            ai_request_running: false,
            ai_response_sender,
            ai_response_events,
            ai_warmup_generation: 0,
            ai_warmup_running: false,
            ai_warmup_sender,
            ai_warmup_events,
            hotkeys,
            layout_mode: None,
            layout_size: None,
            native_center_settle_frames: 0,
        };
        app.apply_appearance(&cc.egui_ctx);
        if platform_cache_fresh {
            app.last_status = Some(format!(
                "Loaded {} catalog items from cache in {} ms",
                app.catalog.len(),
                app.runtime_load_ms
            ));
        } else {
            app.last_status = Some("Scanning platform catalogs...".to_string());
            app.start_runtime_refresh(&cc.egui_ctx, true, false);
        }
        app
    }

    fn results(&self) -> Vec<SearchResult> {
        let mut results = Vec::new();
        if self.query.trim().is_empty() {
            return results;
        }
        if let Some(result) = calculator_search_result(&self.query) {
            results.push(result);
        }
        if let Some(result) = unit_converter_search_result(&self.query) {
            results.push(result);
        }
        if let Some(result) = snippet_search_result(&self.config, &self.query) {
            results.push(result);
        }
        if let Some(result) = komorebi_search_result(&self.query) {
            results.push(result);
        }
        if let Some(result) = aurora_search_result(&self.query) {
            results.push(result);
        }
        if let Some(result) = self.elevated_search_result(&self.query) {
            results.push(result);
        }
        if let Some(provider) = enabled_ai_provider_for_query(&self.config, &self.query)
            && let Some(result) = ai_prompt_search_result(&self.query, provider)
        {
            results.push(result);
        }
        if let Some(result) = web_search_alias_result(&self.config, &self.query) {
            results.push(result);
        }
        results.extend(self.plugin_suggestion_results());
        let mut catalog_results = self.search_index.search(&self.catalog, &self.query);
        if enabled_ai_provider_for_query(&self.config, &self.query).is_none() {
            catalog_results.retain(|result| {
                result.item.category != ItemCategory::Ai || result.item.source != "builtin"
            });
        }
        self.apply_history_boosts(&mut catalog_results);
        results.extend(catalog_results);
        if let Some(result) = web_search_result(&self.query) {
            results.push(result);
        }
        dedupe_results(results)
    }

    fn elevated_search_result(&self, query: &str) -> Option<SearchResult> {
        let trimmed = query.trim();
        let lowered = trimmed.to_ascii_lowercase();
        let target = if let Some(_rest) = lowered.strip_prefix("admin ") {
            &trimmed["admin ".len()..]
        } else if let Some(_rest) = lowered.strip_prefix("sudo ") {
            &trimmed["sudo ".len()..]
        } else if let Some(_rest) = lowered.strip_prefix("elevate ") {
            &trimmed["elevate ".len()..]
        } else {
            return None;
        };

        let target = target.trim();
        if target.is_empty() {
            return None;
        }

        let mut matches = self.search_index.search(&self.catalog, target);
        if matches.is_empty() {
            return None;
        }

        let mut result = matches.remove(0);
        if result.item.actions.is_empty() {
            return None;
        }

        result.item.actions[0].run_as_admin = true;
        result.item.id = format!("elevated:{}", result.item.id);
        result.item.label = format!("Run {} as administrator", result.item.label);
        result.item.subtitle = Some(
            result
                .item
                .subtitle
                .clone()
                .unwrap_or_else(|| "UAC prompt will appear".to_string()),
        );
        result.score = 3500;
        Some(result)
    }

    fn apply_history_boosts(&self, results: &mut [SearchResult]) {
        for result in results.iter_mut() {
            result.score += self.launch_history.boost_for(&result.item, &self.query);
        }

        results.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.item.label.cmp(&b.item.label))
        });
    }

    fn plugin_suggestion_results(&self) -> Vec<SearchResult> {
        if self.query.trim().is_empty() || self.plugin_suggestion_query != self.query {
            return Vec::new();
        }

        self.plugin_suggestion_items
            .iter()
            .cloned()
            .map(|item| SearchResult {
                score: 925 + item.score_boost,
                item,
            })
            .collect()
    }

    fn reload_profile(&mut self, ctx: &egui::Context) {
        self.runtime_refreshing = true;
        self.last_status = Some("Reloading profile and catalogs".to_string());
        self.start_runtime_refresh(ctx, true, true);
    }

    fn start_runtime_refresh(
        &mut self,
        ctx: &egui::Context,
        force_refresh: bool,
        clear_search_on_apply: bool,
    ) {
        self.runtime_refreshing = true;
        let profile_dir = self.profile_dir.clone();
        let sender = self.runtime_sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let runtime = load_runtime_state_with_cache(&profile_dir, force_refresh);
            let _ = sender.send(RuntimeUpdate {
                runtime,
                clear_search_on_apply,
            });
            ctx.request_repaint();
        });
    }

    fn poll_runtime_events(&mut self, ctx: &egui::Context) {
        while let Ok(update) = self.runtime_events.try_recv() {
            self.apply_runtime_state(ctx, update);
        }
        self.poll_ai_response_events(ctx);
        self.poll_ai_warmup_events(ctx);
        self.poll_plugin_suggestion_events(ctx);
        self.maybe_start_due_plugin_suggestion_refresh(ctx);
    }

    fn poll_ai_response_events(&mut self, ctx: &egui::Context) {
        while let Ok(mut response) = self.ai_response_events.try_recv() {
            if response.generation != self.ai_request_generation {
                continue;
            }

            self.ai_request_running = false;
            self.attach_ai_tool_suggestions(&mut response);
            let base_status = match &response.result {
                AiResponseResult::Pending => "AI request started".to_string(),
                AiResponseResult::Answer(_) if !response.tool_suggestions.is_empty() => {
                    "AI suggested an action".to_string()
                }
                AiResponseResult::Answer(_) => "AI answer ready".to_string(),
                AiResponseResult::Error(error) => format!("AI failed: {error}"),
            };
            let eval = evaluate_ai_response(&response);
            response.eval = Some(eval.clone());
            self.last_status = Some(ai_status_with_eval(&base_status, &eval));
            self.record_ai_conversation_response(&response);
            if let Err(error) = append_ai_chat_log(
                &self.profile_dir,
                &response,
                &self.ai_conversation_messages,
                &eval,
            ) {
                self.last_status = Some(format!("{base_status}; chat log failed: {error}"));
            }
            self.ai_panel_expanded = true;
            self.show_ai_conversation = false;
            self.ai_focus_conversation_input = true;
            self.ai_response = Some(response);
            self.invalidate_window_layout();
            ctx.request_repaint();
        }
    }

    fn poll_ai_warmup_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.ai_warmup_events.try_recv() {
            if event.generation != self.ai_warmup_generation {
                continue;
            }

            self.ai_warmup_running = false;
            self.last_status = Some(match event.result {
                Ok(()) => format!("{} is warmed", event.provider_label),
                Err(error) => format!("AI warmup failed for {}: {error}", event.provider_label),
            });
            ctx.request_repaint();
        }
    }

    fn poll_plugin_suggestion_events(&mut self, ctx: &egui::Context) {
        while let Ok(batch) = self.plugin_suggestion_events.try_recv() {
            if batch.generation != self.plugin_suggestion_generation || batch.query != self.query {
                continue;
            }

            self.plugin_suggestion_items = batch.items;
            self.plugin_suggestion_query = batch.query;
            self.plugin_suggestion_refreshing = false;
            if batch.error_count > 0 {
                self.last_status = Some(format!(
                    "{} plugin suggestion error(s); see Diagnostics",
                    batch.error_count
                ));
            }
            if !batch.diagnostics.is_empty() {
                self.load_messages.extend(batch.diagnostics);
            }
            ctx.request_repaint();
        }
    }

    fn apply_runtime_state(&mut self, ctx: &egui::Context, update: RuntimeUpdate) {
        let runtime = update.runtime;
        self.config = runtime.config;
        self.catalog = runtime.catalog;
        self.search_index = SearchIndex::new(&self.catalog);
        self.load_messages = runtime.load_messages;
        self.path_item_count = runtime.path_item_count;
        self.start_menu_item_count = runtime.start_menu_item_count;
        self.file_catalog_item_count = runtime.file_catalog_item_count;
        self.file_catalog_skipped_paths = runtime.file_catalog_skipped_paths;
        self.plugin_process_item_count = runtime.plugin_process_item_count;
        self.plugin_json_rpc_item_count = runtime.plugin_json_rpc_item_count;
        self.tool_manifest_item_count = runtime.tool_manifest_item_count;
        self.plugin_error_count = runtime.plugin_error_count;
        self.runtime_load_ms = runtime.runtime_load_ms;
        self.runtime_refreshing = false;
        self.load_messages
            .extend(self.hotkeys.register_toggle_hotkeys(&self.config));
        self.apply_appearance(ctx);
        if update.clear_search_on_apply || self.query.trim().is_empty() {
            self.clear_search_session();
        } else {
            self.selected = 0;
            self.invalidate_window_layout();
            self.schedule_plugin_suggestion_refresh(ctx);
        }
        self.last_status = Some(format!(
            "Reloaded {} catalog items in {} ms",
            self.catalog.len(),
            self.runtime_load_ms
        ));
        self.maybe_start_ai_warmup(ctx);
    }
}

impl eframe::App for VeyraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_runtime_events(ctx);
        self.process_global_hotkey_events(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_runtime_events(&ctx);
        self.process_global_hotkey_events(&ctx);

        if self.local_toggle_shortcut_pressed(&ctx) {
            self.toggle_launcher_window(&ctx);
        }

        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            self.show_settings = false;
            self.clear_search_session();
            self.hide_launcher_window(&ctx);
        }

        if ctx.input(|input| input.modifiers.ctrl && input.key_pressed(Key::Comma)) {
            self.show_settings = !self.show_settings;
            self.focus_query = !self.show_settings;
        }

        if ctx.input(|input| input.modifiers.ctrl && input.key_pressed(Key::R)) {
            self.reload_profile(&ctx);
        }

        self.sync_window_layout(&ctx);

        let surface_response = Frame::new()
            .fill(self.surface_fill())
            .stroke(self.border_stroke())
            .corner_radius(8)
            .inner_margin(Margin::same(if self.show_settings { 14 } else { 8 }))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.show_settings {
                    self.render_settings(ui);
                } else {
                    self.render_launcher(ui);
                }
            })
            .response;

        if surface_response.dragged() && !ctx.egui_wants_keyboard_input() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

impl Drop for VeyraApp {
    fn drop(&mut self) {
        shutdown_warm_ai_processes();
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
        style.spacing.button_padding = egui::vec2(11.0, 7.0);
        style.spacing.interact_size = egui::vec2(44.0, 34.0);
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::TRANSPARENT;
        style.visuals.window_fill = Color32::from_rgb(18, 20, 22);
        style.visuals.extreme_bg_color = Color32::from_rgb(14, 16, 18);
        style.visuals.faint_bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 10);
        style.visuals.selection.bg_fill = Color32::from_rgb(40, 96, 92);
        style.visuals.selection.stroke =
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(132, 232, 216, 120));
        style.visuals.window_corner_radius = 8.into();
        style.visuals.widgets.active.corner_radius = 6.into();
        style.visuals.widgets.hovered.corner_radius = 6.into();
        style.visuals.widgets.inactive.corner_radius = 6.into();
        style.visuals.widgets.noninteractive.corner_radius = 6.into();
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 16);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 28);
        style.visuals.widgets.active.bg_fill = Color32::from_rgba_unmultiplied(40, 96, 92, 190);
        ctx.set_global_style(style);
        apply_native_backdrop_for_config(&self.config);
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
        self.clear_search_session();
        self.window_visible = true;
        self.show_settings = false;
        self.focus_query = true;
        self.invalidate_window_layout();
        native_capture_target_monitor();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.sync_window_layout(ctx);
        native_show_launcher_window();
        ctx.request_repaint();
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

    fn clear_search_session(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.focus_query = true;
        self.invalidate_window_layout();
        self.clear_ai_response();
        self.clear_plugin_suggestions();
    }

    fn finish_search_session(&mut self, ctx: &egui::Context) {
        self.clear_search_session();
        self.hide_launcher_window(ctx);
        ctx.request_repaint();
    }

    fn record_successful_launch_for_query(&mut self, result: &SearchResult, query: &str) {
        self.launch_history.record(&result.item, query);
        if let Err(error) = save_launch_history(&self.profile_dir, &self.launch_history) {
            self.load_messages
                .push(format!("Could not save launch history: {error}"));
        }
    }

    fn clear_launch_history(&mut self) {
        self.launch_history = LaunchHistory::default();
        let path = history_path(&self.profile_dir);
        match fs::remove_file(&path) {
            Ok(()) => {
                self.last_status = Some("Cleared launch history".to_string());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.last_status = Some("Launch history is already empty".to_string());
            }
            Err(error) => {
                self.last_status = Some(format!("Could not clear launch history: {error}"));
            }
        }
    }

    fn clear_plugin_suggestions(&mut self) {
        self.plugin_suggestion_items.clear();
        self.plugin_suggestion_query.clear();
        self.plugin_suggestion_pending_query.clear();
        self.plugin_suggestion_due_at = None;
        self.plugin_suggestion_generation = self.plugin_suggestion_generation.wrapping_add(1);
        self.plugin_suggestion_refreshing = false;
    }

    fn clear_ai_response(&mut self) {
        if self.ai_response.is_some() || self.ai_request_running {
            self.ai_request_generation = self.ai_request_generation.wrapping_add(1);
        }
        self.ai_response = None;
        self.ai_panel_expanded = false;
        self.show_ai_conversation = false;
        self.ai_conversation_input.clear();
        self.reset_ai_session();
        self.ai_focus_conversation_input = false;
        self.ai_request_running = false;
    }

    fn reset_ai_session(&mut self) {
        self.ai_conversation_messages.clear();
        self.ai_session_provider_id = None;
        self.ai_turn_index = 0;
        self.ai_session_id = unix_timestamp_millis().max(self.ai_session_id.saturating_add(1));
    }

    fn next_ai_turn_index(&mut self) -> u32 {
        self.ai_turn_index = self.ai_turn_index.saturating_add(1);
        self.ai_turn_index
    }

    fn maybe_start_ai_warmup(&mut self, ctx: &egui::Context) {
        if self.ai_warmup_running {
            return;
        }
        let Some(provider) = ai_warmup_provider(&self.config).cloned() else {
            return;
        };

        let provider_label = ai_provider_label(&provider);
        let generation = self.ai_warmup_generation.wrapping_add(1);
        self.ai_warmup_generation = generation;
        self.ai_warmup_running = true;
        self.last_status = Some(format!("Warming {provider_label}"));

        let sender = self.ai_warmup_sender.clone();
        let repaint_ctx = ctx.clone();
        thread::spawn(move || {
            let result = prewarm_ai_provider(provider);
            let _ = sender.send(AiWarmupEvent {
                generation,
                provider_label,
                result,
            });
            repaint_ctx.request_repaint();
        });
        ctx.request_repaint();
    }

    fn schedule_plugin_suggestion_refresh(&mut self, ctx: &egui::Context) {
        let query = self.query.clone();
        if query.trim().is_empty() {
            self.clear_plugin_suggestions();
            return;
        }

        if self.plugin_suggestion_query == query && !self.plugin_suggestion_refreshing {
            return;
        }

        self.plugin_suggestion_pending_query = query;
        self.plugin_suggestion_due_at = Some(Instant::now() + PLUGIN_SUGGEST_DEBOUNCE);
        ctx.request_repaint_after(PLUGIN_SUGGEST_DEBOUNCE);
    }

    fn maybe_start_due_plugin_suggestion_refresh(&mut self, ctx: &egui::Context) {
        let Some(due_at) = self.plugin_suggestion_due_at else {
            return;
        };

        let now = Instant::now();
        if now < due_at {
            ctx.request_repaint_after(due_at.saturating_duration_since(now));
            return;
        }

        let query = std::mem::take(&mut self.plugin_suggestion_pending_query);
        self.plugin_suggestion_due_at = None;
        if query != self.query || query.trim().is_empty() {
            return;
        }

        self.start_plugin_suggestion_refresh(ctx, query);
    }

    fn start_plugin_suggestion_refresh(&mut self, ctx: &egui::Context, query: String) {
        self.plugin_suggestion_generation = self.plugin_suggestion_generation.wrapping_add(1);
        let generation = self.plugin_suggestion_generation;
        self.plugin_suggestion_query = query.clone();
        self.plugin_suggestion_items.clear();
        self.plugin_suggestion_refreshing = true;

        let plugins = self.config.plugins.clone();
        let sender = self.plugin_suggestion_sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let load = load_plugin_suggestions(&plugins, &query);
            let _ = sender.send(PluginSuggestionBatch {
                generation,
                query,
                items: load.items,
                diagnostics: load.diagnostics,
                error_count: load.error_count,
            });
            ctx.request_repaint();
        });
    }

    fn sync_window_layout(&mut self, ctx: &egui::Context) {
        if !self.window_visible {
            return;
        }

        let mode = match (self.show_settings, self.query.trim().is_empty()) {
            (true, _) => WindowLayoutMode::Settings,
            (false, _) if self.ai_response.is_some() => WindowLayoutMode::LauncherAi,
            (false, true) => WindowLayoutMode::LauncherCompact,
            (false, false) => WindowLayoutMode::LauncherResults,
        };
        let result_count = match mode {
            WindowLayoutMode::LauncherResults => self.results().len(),
            _ => 0,
        };
        let layout_scale = effective_layout_scale(ctx.pixels_per_point());
        #[cfg(windows)]
        let Some(monitor_size) = native_monitor_logical_size(layout_scale) else {
            ctx.request_repaint();
            return;
        };
        #[cfg(not(windows))]
        let monitor_size = ctx
            .input(|input| input.viewport().monitor_size)
            .unwrap_or(Vec2::new(1440.0, 900.0));
        let size = window_size_for_monitor(mode, monitor_size, layout_scale, result_count);

        let layout_matches = self.layout_mode == Some(mode)
            && self
                .layout_size
                .is_some_and(|current| layout_size_matches(current, size));

        if layout_matches && self.native_center_settle_frames == 0 {
            return;
        }

        if !layout_matches {
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min_window_size(mode)));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            #[cfg(not(windows))]
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(window_position(
                mode,
                monitor_size,
                size,
                layout_scale,
            )));
            self.layout_mode = Some(mode);
            self.layout_size = Some(size);
            #[cfg(windows)]
            {
                self.native_center_settle_frames = 8;
            }
            #[cfg(not(windows))]
            {
                self.native_center_settle_frames = 0;
            }
        }

        #[cfg(windows)]
        if self.native_center_settle_frames > 0 {
            native_center_window(mode);
            self.native_center_settle_frames -= 1;
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn invalidate_window_layout(&mut self) {
        self.layout_mode = None;
        self.layout_size = None;
        self.native_center_settle_frames = 0;
    }

    fn surface_fill(&self) -> Color32 {
        let max_alpha = if self.config.appearance.blur {
            176
        } else {
            242
        };
        Color32::from_rgba_unmultiplied(18, 20, 23, alpha_for_opacity(&self.config, max_alpha))
    }

    fn border_stroke(&self) -> Stroke {
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(&self.config, 72)),
        )
    }

    fn render_launcher(&mut self, ui: &mut egui::Ui) {
        if self.try_execute_selected_from_keyboard(ui) {
            return;
        }

        if self.ai_response.is_some() {
            self.render_ai_capture_panel(ui);
            return;
        }

        if self.query.trim().is_empty() {
            ui.add_space(((ui.available_height() - SEARCH_ROW_HEIGHT) / 2.0).max(0.0));
            self.render_search_box(ui);
            return;
        }
        self.render_search_box(ui);
        self.render_launcher_status(ui);

        ui.add_space(10.0);

        let results = self.results();
        let result_limit = effective_max_results(&self.config);
        let shown_count = results.len().min(result_limit);
        if shown_count > 0 {
            self.selected = self.selected.min(shown_count - 1);
        }

        if ui.input(|input| input.key_pressed(Key::ArrowDown)) && shown_count > 0 {
            self.selected = step_selection(SelectionDirection::Down, self.selected, shown_count);
        }
        if ui.input(|input| input.key_pressed(Key::ArrowUp)) && shown_count > 0 {
            self.selected = step_selection(SelectionDirection::Up, self.selected, shown_count);
        }

        self.render_result_list(ui, &results, shown_count);
    }

    fn render_launcher_status(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.launcher_status_text() else {
            return;
        };

        ui.add_space(5.0);
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                255,
                255,
                255,
                alpha_for_opacity(&self.config, 8),
            ))
            .corner_radius(6)
            .inner_margin(Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.ai_request_running || self.ai_warmup_running {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(status)
                            .size(12.0)
                            .color(Color32::from_rgb(164, 176, 184)),
                    );
                });
            });
    }

    fn launcher_status_text(&self) -> Option<String> {
        if self.ai_request_running {
            return self
                .ai_response
                .as_ref()
                .map(|response| format!("Asking {}...", response.provider_label))
                .or_else(|| Some("Asking AI...".to_string()));
        }
        if self.ai_warmup_running {
            return Some("Warming AI provider...".to_string());
        }
        if self.query.trim().is_empty() {
            return None;
        }

        self.last_status.as_ref().and_then(|status| {
            let trimmed = status.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
    }

    fn render_ai_capture_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(response) = self.ai_response.clone() else {
            return false;
        };

        let copy_text = copyable_ai_response_text(&response);
        let has_actions = !response.tool_suggestions.is_empty();
        let panel_min_height = if has_actions { 300.0 } else { 320.0 };
        let panel_height = ui.available_height().max(panel_min_height);
        Frame::new()
            .fill(content_fill(&self.config))
            .stroke(subtle_stroke(&self.config))
            .corner_radius(8)
            .inner_margin(Margin::same(if has_actions { 8 } else { 12 }))
            .show(ui, |ui| {
                ui.set_height((panel_height - 2.0).max(panel_min_height));

                ui.horizontal(|ui| {
                    Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(142, 210, 132, 190))
                        .corner_radius(4)
                        .inner_margin(Margin::symmetric(5, if has_actions { 11 } else { 15 }))
                        .show(ui, |ui| {
                            ui.allocate_space(Vec2::new(2.0, 1.0));
                        });

                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("Veyra AI")
                                .size(16.0)
                                .strong()
                                .color(Color32::from_rgb(236, 241, 244)),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(ai_response_subtitle(&response))
                                    .size(12.0)
                                    .color(Color32::from_rgb(146, 157, 166)),
                            )
                            .wrap(),
                        );
                    });
                });

                ui.add_space(if has_actions { 4.0 } else { 8.0 });
                if has_actions {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "{} - {}",
                                ai_context_status_label(&response.request),
                                ai_tool_status_label(&response.request)
                            ))
                            .size(11.0)
                            .color(Color32::from_rgb(146, 157, 166)),
                        )
                        .wrap(),
                    );
                } else {
                    render_ai_request_meta(ui, &response.request, true);
                    if let Some(eval) = &response.eval {
                        ui.add_space(4.0);
                        render_ai_eval_meta(ui, eval);
                    }
                }

                if !has_actions {
                    ui.add_space(10.0);
                    self.render_ai_toolbar_controls(ui, copy_text.clone());
                    ui.add_space(8.0);
                    let compose_reserved = ai_compose_reserved_height(false);
                    let thread_height =
                        (ui.available_height() - compose_reserved).clamp(72.0, 260.0);
                    if thread_height > 40.0 {
                        self.render_ai_conversation_messages(
                            ui,
                            "ai-capture-thread",
                            thread_height,
                        );
                        ui.add_space(6.0);
                    }
                    self.render_ai_compose_row(ui, true, false);
                } else {
                    ui.add_space(6.0);
                    let compose_reserved = ai_compose_reserved_height(true);
                    let suggestions_height =
                        (ui.available_height() - compose_reserved).clamp(80.0, 132.0);
                    self.render_ai_tool_suggestions(
                        ui,
                        &response.tool_suggestions,
                        suggestions_height,
                    );
                    ui.add_space(6.0);
                    self.render_ai_compose_row(ui, true, true);
                }
            });

        self.ai_response.is_some()
    }

    fn render_ai_toolbar_controls(&mut self, ui: &mut egui::Ui, copy_text: Option<String>) {
        ui.horizontal_wrapped(|ui| {
            if let Some(text) = copy_text.clone()
                && ai_toolbar_button(ui, "Copy", "Copy the visible AI answer").clicked()
            {
                ui.ctx().copy_text(text);
                self.last_status = Some("Copied AI answer".to_string());
            }
            if ai_toolbar_button(ui, "Save", "Save this chat as Markdown").clicked() {
                self.save_current_ai_chat_snapshot();
            }
            if ai_toolbar_button(ui, "Log", "Open the append-only AI chat log").clicked() {
                self.open_ai_chat_log();
            }
            if ai_toolbar_button(ui, "Clear", "Clear the captured AI conversation").clicked() {
                self.clear_ai_response();
            }
        });
    }

    fn render_ai_conversation_messages(
        &self,
        ui: &mut egui::Ui,
        id_salt: &'static str,
        max_height: f32,
    ) {
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                255,
                255,
                255,
                alpha_for_opacity(&self.config, 10),
            ))
            .stroke(subtle_stroke(&self.config))
            .corner_radius(8)
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt(id_salt)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .max_height(max_height)
                    .show(ui, |ui| {
                        if self.ai_conversation_messages.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(18.0);
                                ui.label(
                                    RichText::new("No messages yet")
                                        .color(Color32::from_rgb(146, 157, 166)),
                                );
                                ui.add_space(18.0);
                            });
                        } else {
                            for message in &self.ai_conversation_messages {
                                render_ai_conversation_message(ui, message);
                            }
                        }

                        if self.ai_request_running {
                            render_ai_pending_message(ui);
                        }
                    });
            });
    }

    fn render_ai_compose_row(&mut self, ui: &mut egui::Ui, allow_auto_focus: bool, compact: bool) {
        let ctx = ui.ctx().clone();
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                255,
                255,
                255,
                alpha_for_opacity(&self.config, 14),
            ))
            .stroke(subtle_stroke(&self.config))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let clip_width = if compact || self.ai_request_running {
                        0.0
                    } else {
                        AI_CLIP_BUTTON_WIDTH
                    };
                    let status_width = if self.ai_request_running {
                        if compact {
                            AI_THINKING_STATUS_COMPACT_WIDTH
                        } else {
                            AI_THINKING_STATUS_WIDTH
                        }
                    } else {
                        0.0
                    };
                    let gap_count = 1.0
                        + if clip_width > 0.0 { 1.0 } else { 0.0 }
                        + if status_width > 0.0 { 1.0 } else { 0.0 };
                    let reserved = AI_SEND_BUTTON_WIDTH
                        + clip_width
                        + status_width
                        + gap_count * ui.spacing().item_spacing.x;
                    let input_width = (ui.available_width() - reserved).max(160.0);
                    let input_response = ui.add_sized(
                        [input_width, AI_COMPOSE_INPUT_HEIGHT],
                        TextEdit::singleline(&mut self.ai_conversation_input)
                            .hint_text("Ask a follow-up")
                            .frame(Frame::NONE)
                            .margin(Margin::symmetric(2, 0)),
                    );

                    if allow_auto_focus && self.ai_focus_conversation_input {
                        input_response.request_focus();
                        self.ai_focus_conversation_input = false;
                    }

                    let can_send =
                        !self.ai_request_running && !self.ai_conversation_input.trim().is_empty();
                    let send_requested = input_response.has_focus()
                        && ui
                            .input(|input| input.key_pressed(Key::Enter) && !input.modifiers.shift);
                    if send_requested && can_send {
                        self.send_ai_conversation_prompt(&ctx);
                    } else if send_requested && self.ai_request_running {
                        self.last_status = Some("AI request already running".to_string());
                    }

                    if self.ai_request_running {
                        ui.allocate_ui_with_layout(
                            Vec2::new(status_width, AI_COMPOSE_INPUT_HEIGHT),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.spinner();
                                if !compact {
                                    ui.label(
                                        RichText::new("Thinking")
                                            .size(12.0)
                                            .color(Color32::from_rgb(146, 157, 166)),
                                    );
                                }
                            },
                        );
                    } else if ui
                        .add_visible(
                            !compact,
                            egui::Button::new(RichText::new("Clip").size(12.0)).min_size(
                                Vec2::new(AI_CLIP_BUTTON_WIDTH, AI_COMPOSE_BUTTON_HEIGHT),
                            ),
                        )
                        .on_hover_text("Include current clipboard text with the next AI prompt")
                        .clicked()
                    {
                        self.append_clipboard_context_to_ai_input();
                    }

                    if ui
                        .add_enabled(
                            can_send,
                            egui::Button::new(RichText::new("Send").size(12.0)).min_size(
                                Vec2::new(AI_SEND_BUTTON_WIDTH, AI_COMPOSE_BUTTON_HEIGHT),
                            ),
                        )
                        .on_hover_text("Send follow-up")
                        .clicked()
                    {
                        self.send_ai_conversation_prompt(&ctx);
                    }
                });
            });
    }

    fn append_clipboard_context_to_ai_input(&mut self) {
        if read_clipboard_text().is_some() {
            if self.ai_conversation_input.trim().is_empty() {
                self.ai_conversation_input = "summarize clipboard".to_string();
            } else if !prompt_requests_clipboard_context(&self.ai_conversation_input) {
                self.ai_conversation_input.push_str(" using clipboard");
            }
            self.last_status = Some("Clipboard text will be included with the prompt".to_string());
        } else {
            self.last_status = Some("Clipboard does not contain text".to_string());
        }
    }

    fn render_ai_tool_suggestions(
        &mut self,
        ui: &mut egui::Ui,
        suggestions: &[AiToolSuggestion],
        max_height: f32,
    ) {
        let ctx = ui.ctx().clone();
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                132,
                216,
                228,
                alpha_for_opacity(&self.config, 20),
            ))
            .stroke(Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(132, 216, 228, 58),
            ))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("ai-tool-suggestions")
                    .auto_shrink([false, false])
                    .max_height(max_height)
                    .show(ui, |ui| {
                        for suggestion in suggestions.iter().take(2) {
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    Vec2::new(
                                        (ui.available_width() - AI_TOOL_ROW_TRAILING_WIDTH)
                                            .max(180.0),
                                        AI_TOOL_SUGGESTION_ROW_HEIGHT,
                                    ),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.label(
                                            RichText::new(&suggestion.label)
                                                .strong()
                                                .size(13.0)
                                                .color(Color32::from_rgb(226, 241, 244)),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&suggestion.detail)
                                                    .size(11.0)
                                                    .color(Color32::from_rgb(150, 171, 178)),
                                            )
                                            .wrap(),
                                        );
                                    },
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let enabled =
                                        suggestion.result.is_some() && !self.ai_request_running;
                                    if ui
                                        .add_enabled(
                                            enabled,
                                            egui::Button::new(RichText::new("Run").size(12.0))
                                                .min_size(Vec2::new(
                                                    AI_TOOL_RUN_BUTTON_WIDTH,
                                                    AI_COMPOSE_BUTTON_HEIGHT,
                                                )),
                                        )
                                        .on_hover_text(format!(
                                            "Confirm {}",
                                            suggestion.call.name.trim()
                                        ))
                                        .clicked()
                                    {
                                        self.execute_ai_tool_suggestion(&ctx, suggestion);
                                    }
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });
            });
    }

    fn try_execute_selected_from_keyboard(&mut self, ui: &mut egui::Ui) -> bool {
        let ai_capture_active = self.show_ai_conversation || self.ai_response.is_some();
        if ai_capture_active
            && ui.input(|input| input.key_pressed(Key::Enter) && !input.modifiers.shift)
            && !self.ai_conversation_input.trim().is_empty()
        {
            self.send_ai_conversation_prompt(ui.ctx());
            return true;
        }

        if self.show_ai_conversation || (self.ai_panel_expanded && self.ai_response.is_some()) {
            return false;
        }

        if !ui.input(|input| input.key_pressed(Key::Enter)) {
            return false;
        }

        let results = self.results();
        let shown_count = results.len().min(effective_max_results(&self.config));
        if shown_count == 0 {
            return false;
        }

        self.selected = self.selected.min(shown_count - 1);
        let Some(result) = results.get(self.selected) else {
            return false;
        };
        let confirmed = ui.input(|input| input.modifiers.shift);
        let finished = self.execute_result(ui.ctx(), result, confirmed);
        if finished {
            ui.ctx().request_repaint();
        }

        finished
    }

    fn render_search_box(&mut self, ui: &mut egui::Ui) {
        let mut query_response = None;
        ui.allocate_ui(Vec2::new(ui.available_width(), SEARCH_ROW_HEIGHT), |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(8.0);
                let input_width = (ui.available_width() - 8.0).max(160.0);
                let response = ui.add_sized(
                    [input_width, SEARCH_ROW_HEIGHT],
                    TextEdit::singleline(&mut self.query)
                        .font(egui::TextStyle::Heading)
                        .hint_text("Search, run, or ask AI")
                        .frame(Frame::NONE)
                        .margin(Margin::same(0)),
                );
                if response.changed() {
                    self.invalidate_window_layout();
                    self.selected = 0;
                    self.clear_ai_response();
                    self.schedule_plugin_suggestion_refresh(ui.ctx());
                    ui.ctx().request_repaint();
                }
                query_response = Some(response);
            });
        });

        let ai_capture_active = self.ai_panel_expanded && self.ai_response.is_some();
        if !self.show_ai_conversation
            && !ai_capture_active
            && (self.focus_query || !ui.ctx().egui_wants_keyboard_input())
        {
            if let Some(response) = query_response {
                response.request_focus();
            }
            self.focus_query = false;
        }
    }

    fn render_result_list(
        &mut self,
        ui: &mut egui::Ui,
        results: &[SearchResult],
        shown_count: usize,
    ) {
        if shown_count == 0 {
            let message = if self.query.trim().is_empty() {
                "Ready"
            } else {
                "No matches"
            };
            Frame::new()
                .fill(panel_fill(&self.config))
                .stroke(subtle_stroke(&self.config))
                .corner_radius(8)
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(18.0);
                        ui.label(
                            RichText::new(message)
                                .strong()
                                .color(Color32::from_rgb(176, 185, 194)),
                        );
                        ui.add_space(18.0);
                    });
                });
            return;
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(96.0))
            .show(ui, |ui| {
                for (index, result) in results.iter().take(shown_count).enumerate() {
                    self.render_result(ui, index, result);
                }
            });
    }

    fn render_result(&mut self, ui: &mut egui::Ui, index: usize, result: &SearchResult) {
        let selected = index == self.selected;
        let fill = if selected {
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(&self.config, 34))
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(&self.config, 10))
        };
        let stroke = if selected {
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(&self.config, 62)),
            )
        } else {
            subtle_stroke(&self.config)
        };

        let response = Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(8)
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    category_marker(ui, &result.item);
                    ui.add_space(10.0);
                    let row_width = ui.available_width();
                    let show_trailing = row_width >= 360.0;
                    let trailing_width = if show_trailing {
                        if selected { 116.0 } else { 52.0 }
                    } else {
                        0.0
                    };
                    let text_width = (row_width - trailing_width - 8.0).max(140.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(text_width.min(ui.available_width()), 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            let label_limit = ((text_width / 10.0).floor() as usize).clamp(24, 64);
                            let subtitle_limit =
                                ((text_width / 11.0).floor() as usize).clamp(30, 68);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(truncate_for_label(
                                        &result.item.label,
                                        label_limit,
                                    ))
                                    .strong()
                                    .color(Color32::from_rgb(236, 241, 244)),
                                )
                                .wrap(),
                            );
                            if let Some(subtitle) = &result.item.subtitle {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(truncate_for_label(subtitle, subtitle_limit))
                                            .color(Color32::from_rgb(146, 157, 166)),
                                    )
                                    .wrap(),
                                );
                            }
                        },
                    );
                    if show_trailing {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if selected {
                                ui.label(
                                    RichText::new(result_action_hint(result))
                                        .size(11.0)
                                        .color(Color32::from_rgb(176, 190, 198)),
                                );
                                ui.add_space(8.0);
                            }
                            ui.label(
                                RichText::new(category_label(&result.item))
                                    .size(11.0)
                                    .color(Color32::from_rgb(126, 136, 145)),
                            );
                        });
                    }
                });
            })
            .response;

        if selected {
            response.scroll_to_me(None);
        }
        if response.clicked() {
            self.selected = index;
        }
        if response.double_clicked() {
            self.execute_result(ui.ctx(), result, false);
        }
        ui.add_space(5.0);
    }

    fn execute_result(
        &mut self,
        ctx: &egui::Context,
        result: &SearchResult,
        confirmed: bool,
    ) -> bool {
        let query = self.query.clone();
        self.execute_result_with_query(ctx, result, confirmed, &query)
    }

    fn execute_result_with_query(
        &mut self,
        ctx: &egui::Context,
        result: &SearchResult,
        confirmed: bool,
        query: &str,
    ) -> bool {
        let Some(action) = result.item.actions.first() else {
            self.last_status = Some(format!("No action registered for {}", result.item.label));
            return false;
        };

        let action = self.resolve_action_with_query(action, query);
        if action.requires_confirmation && !confirmed {
            self.last_status = Some(format!(
                "{} requires confirmation; press Shift+Enter to run it",
                result.item.label
            ));
            return false;
        }

        if action.kind == ActionKind::ToolCall
            && action.id == COPY_TO_CLIPBOARD_ACTION_ID
            && let Some(text) = &action.command
        {
            ctx.copy_text(text.clone());
            self.last_status = Some(format!("Copied {text}"));
            self.finish_search_session(ctx);
            return true;
        }

        if action.kind == ActionKind::ToolCall
            && let Some(command) = &action.command
            && let Some(action_ref) = parse_json_rpc_action_command(command)
        {
            return self.execute_json_rpc_plugin_action_with_query(ctx, result, &action_ref, query);
        }

        if action.kind == ActionKind::AiPrompt {
            return self.execute_ai_prompt(ctx, result);
        }

        match execute_action(&action) {
            Ok(()) => {
                self.last_status = Some(format!("Opened {}", result.item.label));
                self.record_successful_launch_for_query(result, query);
                self.finish_search_session(ctx);
                true
            }
            Err(error) => {
                self.last_status = Some(format!("Could not open {}: {}", result.item.label, error));
                false
            }
        }
    }

    fn execute_json_rpc_plugin_action_with_query(
        &mut self,
        ctx: &egui::Context,
        result: &SearchResult,
        action_ref: &veyra_plugin::JsonRpcActionRef,
        query: &str,
    ) -> bool {
        let Some(plugin) = self.config.plugins.iter().find(|plugin| {
            plugin.id == action_ref.plugin_id
                || (plugin.id.trim().is_empty()
                    && format!("plugin.{}", plugin.command) == action_ref.plugin_id)
        }) else {
            self.last_status = Some(format!("Plugin {} is not configured", action_ref.plugin_id));
            return false;
        };

        match execute_json_rpc_action(plugin, action_ref, query) {
            Ok(response) => {
                self.last_status = Some(
                    response
                        .message
                        .unwrap_or_else(|| format!("Ran {}", result.item.label)),
                );
                self.record_successful_launch_for_query(result, query);
                self.finish_search_session(ctx);
                true
            }
            Err(error) => {
                self.last_status = Some(format!("Could not run {}: {error}", result.item.label));
                false
            }
        }
    }

    fn execute_ai_prompt(&mut self, ctx: &egui::Context, _result: &SearchResult) -> bool {
        let Some(provider) = enabled_ai_provider_for_query(&self.config, &self.query) else {
            self.last_status =
                Some("AI is disabled or no enabled provider is configured in ai.toml.".to_string());
            return false;
        };

        let Some(prompt) = ai_prompt_text(&self.query) else {
            self.last_status =
                Some("Type a prompt after ai, ask, chat, kyrphina, or llama.".to_string());
            return false;
        };
        if prompt.trim().is_empty() {
            self.last_status =
                Some("Type a prompt after ai, ask, chat, kyrphina, or llama.".to_string());
            return false;
        }

        let provider = provider.clone();
        self.query.clear();
        self.focus_query = false;
        self.clear_plugin_suggestions();
        self.ai_conversation_input.clear();
        self.reset_ai_session();
        self.start_ai_request(ctx, provider, prompt);
        false
    }

    fn start_ai_request(&mut self, ctx: &egui::Context, provider: AiProvider, prompt: String) {
        if self.ai_request_running {
            self.last_status = Some("AI request already running".to_string());
            return;
        }

        let session_id = self.ai_session_id;
        let turn_index = self.next_ai_turn_index();
        self.ai_session_provider_id = Some(provider.id.clone());

        let proactive_suggestions = self.proactive_ai_tool_suggestions_for_prompt(&prompt);
        if !proactive_suggestions.is_empty() {
            self.start_direct_ai_answer_with_suggestions(
                ctx,
                provider,
                DirectAiAnswer {
                    session_id,
                    turn_index,
                    prompt,
                    answer: "I found a Veyra action for that. Confirm it below.".to_string(),
                    tool_suggestions: proactive_suggestions,
                    status: "Action captured",
                },
            );
            return;
        }

        if let Some(answer) = deterministic_ai_answer(&prompt) {
            self.start_direct_ai_answer(ctx, provider, session_id, turn_index, prompt, answer);
            return;
        }

        let prompt_plan = self.build_ai_model_prompt(&provider, &prompt);
        let request = ai_request_info(
            &provider,
            self.indexed_tool_count(),
            prompt_plan.tool_context_items,
            prompt_plan.message_context_items,
            prompt_plan.estimated_provider_tokens,
        );
        let provider_label = ai_provider_label(&provider);
        let local_only =
            self.config.general.local_only || self.config.ai.local_only || provider.local_only;
        let generation = self.ai_request_generation.wrapping_add(1);
        self.ai_request_generation = generation;
        self.ai_request_running = true;
        self.selected = 0;
        self.ai_panel_expanded = true;
        self.show_ai_conversation = false;
        self.ai_focus_conversation_input = true;
        self.invalidate_window_layout();
        self.record_ai_conversation_user(&prompt);
        self.ai_response = Some(AiResponse {
            generation,
            session_id,
            turn_index,
            prompt: prompt.clone(),
            provider_label: provider_label.clone(),
            request: request.clone(),
            elapsed_ms: None,
            tool_suggestions: Vec::new(),
            eval: None,
            result: AiResponseResult::Pending,
        });
        self.last_status = Some(format!("Asking {provider_label}"));

        let sender = self.ai_response_sender.clone();
        let repaint_ctx = ctx.clone();
        let retry_provider = provider.clone();
        let retry_prompt = trim_text_to_provider_budget(&retry_provider, &prompt);
        let event_prompt = prompt.clone();
        thread::spawn(move || {
            let started = Instant::now();
            let result = match call_ai_provider(provider, prompt_plan.prompt, local_only) {
                Ok(answer) => AiResponseResult::Answer(answer),
                Err(error) if ai_error_is_context_exceeded(&retry_provider, &error) => {
                    match call_ai_provider(retry_provider, retry_prompt, local_only) {
                        Ok(answer) => AiResponseResult::Answer(answer),
                        Err(retry_error) => AiResponseResult::Error(format!(
                            "AI prompt exceeded the provider context budget. Veyra retried with minimal context, but the provider still failed: {retry_error}"
                        )),
                    }
                }
                Err(error) => AiResponseResult::Error(error),
            };
            let _ = sender.send(AiResponse {
                generation,
                session_id,
                turn_index,
                prompt: event_prompt,
                provider_label,
                request,
                elapsed_ms: Some(started.elapsed().as_millis()),
                tool_suggestions: Vec::new(),
                eval: None,
                result,
            });
            repaint_ctx.request_repaint();
        });
        ctx.request_repaint();
    }

    fn start_direct_ai_answer(
        &mut self,
        ctx: &egui::Context,
        provider: AiProvider,
        session_id: u64,
        turn_index: u32,
        prompt: String,
        answer: String,
    ) {
        self.start_direct_ai_answer_with_suggestions(
            ctx,
            provider,
            DirectAiAnswer {
                session_id,
                turn_index,
                prompt,
                answer,
                tool_suggestions: Vec::new(),
                status: "Answered directly",
            },
        );
    }

    fn start_direct_ai_answer_with_suggestions(
        &mut self,
        ctx: &egui::Context,
        provider: AiProvider,
        direct: DirectAiAnswer,
    ) {
        let request = ai_request_info(
            &provider,
            self.indexed_tool_count(),
            direct.tool_suggestions.len(),
            self.ai_conversation_messages
                .iter()
                .rev()
                .take(ai_context_message_limit_for_provider(&provider))
                .count(),
            estimate_token_count(&direct.answer),
        );
        let provider_label = ai_provider_label(&provider);
        let generation = self.ai_request_generation.wrapping_add(1);
        self.ai_request_generation = generation;
        self.ai_request_running = false;
        self.selected = 0;
        self.ai_panel_expanded = true;
        self.show_ai_conversation = false;
        self.ai_focus_conversation_input = true;
        self.invalidate_window_layout();
        self.record_ai_conversation_user(&direct.prompt);

        let mut response = AiResponse {
            generation,
            session_id: direct.session_id,
            turn_index: direct.turn_index,
            prompt: direct.prompt,
            provider_label,
            request,
            elapsed_ms: Some(0),
            tool_suggestions: direct.tool_suggestions,
            eval: None,
            result: AiResponseResult::Answer(direct.answer),
        };
        let eval = evaluate_ai_response(&response);
        response.eval = Some(eval.clone());
        self.last_status = Some(ai_status_with_eval(direct.status, &eval));
        self.record_ai_conversation_response(&response);
        if let Err(error) = append_ai_chat_log(
            &self.profile_dir,
            &response,
            &self.ai_conversation_messages,
            &eval,
        ) {
            self.last_status = Some(format!("{}; chat log failed: {error}", direct.status));
        }
        self.ai_response = Some(response);
        ctx.request_repaint();
    }

    fn send_ai_conversation_prompt(&mut self, ctx: &egui::Context) {
        let prompt = self.ai_conversation_input.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        if self.ai_request_running {
            self.last_status = Some("AI request already running".to_string());
            return;
        }

        let Some(provider) = self.active_ai_conversation_provider() else {
            self.last_status =
                Some("AI is disabled or no enabled provider is configured in ai.toml.".to_string());
            return;
        };

        self.ai_conversation_input.clear();
        self.start_ai_request(ctx, provider, prompt);
    }

    fn active_ai_conversation_provider(&self) -> Option<AiProvider> {
        self.ai_session_provider_id
            .as_deref()
            .and_then(|id| find_enabled_ai_provider(&self.config, id))
            .or_else(|| enabled_ai_provider_for_query(&self.config, ""))
            .cloned()
    }

    fn record_ai_conversation_user(&mut self, prompt: &str) {
        let text = prompt.trim();
        if text.is_empty() {
            return;
        }

        self.ai_conversation_messages.push(AiConversationMessage {
            role: AiConversationRole::User,
            text: text.to_string(),
        });
        self.trim_ai_conversation_history();
    }

    fn record_ai_conversation_response(&mut self, response: &AiResponse) {
        let (role, text) = match &response.result {
            AiResponseResult::Pending => return,
            AiResponseResult::Answer(answer) => (
                AiConversationRole::Assistant,
                ai_answer_display_text(answer),
            ),
            AiResponseResult::Error(error) => {
                (AiConversationRole::System, error.trim().to_string())
            }
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }

        self.ai_conversation_messages.push(AiConversationMessage {
            role,
            text: text.to_string(),
        });
        self.trim_ai_conversation_history();
    }

    fn attach_ai_tool_suggestions(&self, response: &mut AiResponse) {
        response.tool_suggestions.clear();
        let AiResponseResult::Answer(answer) = &response.result else {
            return;
        };

        response.tool_suggestions = parse_ai_function_calls(answer)
            .into_iter()
            .map(|call| self.resolve_ai_tool_suggestion(call))
            .collect();
        if response.tool_suggestions.is_empty() {
            response.tool_suggestions =
                self.proactive_ai_tool_suggestions_for_prompt(&response.prompt);
        }
    }

    fn resolve_ai_tool_suggestion(&self, call: AiToolCall) -> AiToolSuggestion {
        let normalized = normalize_ai_tool_name(&call.name);
        let query = ai_tool_call_param(
            &call,
            &[
                "query",
                "label",
                "name",
                "app",
                "text",
                "url",
                "expression",
                "location",
                "timezone",
            ],
        );

        match normalized.as_str() {
            "open" | "open_result" | "run" | "launch" | "start" | "execute" => {
                let query = query.unwrap_or_default();
                let result = self.best_ai_action_result(&query);
                let (label, detail) = if let Some(result) = &result {
                    (
                        format!("Open {}", result.item.label),
                        format!(
                            "Matched from AI query: {}",
                            non_empty(&query).unwrap_or_default()
                        ),
                    )
                } else {
                    (
                        "Open result".to_string(),
                        format!("No launcher result matched '{}'", query.trim()),
                    )
                };
                AiToolSuggestion {
                    call,
                    label,
                    detail,
                    result,
                    query_context: non_empty(&query),
                }
            }
            "search" | "web_search" | "search_web" => {
                let query = query.unwrap_or_default();
                let result = web_search_result(&query);
                AiToolSuggestion {
                    call,
                    label: format!("Search web for {}", query.trim()),
                    detail: "Veyra will open the configured web search action".to_string(),
                    result,
                    query_context: non_empty(&query),
                }
            }
            "open_url" | "url" | "open_web" => {
                let url = query.unwrap_or_default();
                let result = ai_open_url_tool_result(&url);
                AiToolSuggestion {
                    call,
                    label: "Open URL".to_string(),
                    detail: if result.is_some() {
                        truncate_for_label(&url, 96)
                    } else {
                        format!("Unsupported or invalid URL '{}'", url.trim())
                    },
                    result,
                    query_context: None,
                }
            }
            "calculate" | "calculator" | "math" => {
                let expression = query.unwrap_or_default();
                let value = evaluate_expression(&expression).map(format_number);
                let result = value.as_ref().map(|rendered| ai_copy_tool_result(rendered));
                AiToolSuggestion {
                    call,
                    label: "Calculate".to_string(),
                    detail: value
                        .map(|value| format!("{} = {value}", expression.trim()))
                        .unwrap_or_else(|| format!("Could not calculate '{}'", expression.trim())),
                    result,
                    query_context: None,
                }
            }
            "current_time" | "time" | "date" | "current_date" => {
                let location = query.unwrap_or_default();
                let answer = ai_time_tool_answer(&location);
                let result = answer.as_ref().map(|answer| ai_copy_tool_result(answer));
                AiToolSuggestion {
                    call,
                    label: "Get current time".to_string(),
                    detail: answer.unwrap_or_else(|| {
                        format!("Unsupported time zone/location '{}'", location.trim())
                    }),
                    result,
                    query_context: None,
                }
            }
            "copy" | "copy_to_clipboard" => {
                let text = query.unwrap_or_default();
                let result = non_empty(&text).map(|text| ai_copy_tool_result(&text));
                AiToolSuggestion {
                    call,
                    label: "Copy to clipboard".to_string(),
                    detail: truncate_for_label(&text, 96),
                    result,
                    query_context: None,
                }
            }
            _ => AiToolSuggestion {
                detail: format!("Unsupported AI tool call '{}'", call.name),
                label: "Unsupported tool call".to_string(),
                call,
                result: None,
                query_context: None,
            },
        }
    }

    fn proactive_ai_tool_suggestions_for_prompt(&self, prompt: &str) -> Vec<AiToolSuggestion> {
        let mut suggestions = Vec::new();
        if let Some(query) = extract_open_intent_query(prompt) {
            let call = ai_tool_call("open_result", "query", &query);
            suggestions.push(self.resolve_ai_tool_suggestion(call));
        }

        if let Some(query) = extract_web_search_intent_query(prompt) {
            let call = ai_tool_call("search", "query", &query);
            suggestions.push(self.resolve_ai_tool_suggestion(call));
        }

        if let Some(text) = extract_copy_intent_text(prompt) {
            let call = ai_tool_call("copy_to_clipboard", "text", &text);
            suggestions.push(self.resolve_ai_tool_suggestion(call));
        }

        if let Some(expression) = extract_calculation_intent_query(prompt) {
            let call = ai_tool_call("calculate", "expression", &expression);
            suggestions.push(self.resolve_ai_tool_suggestion(call));
        }

        suggestions
    }

    fn best_ai_action_result(&self, query: &str) -> Option<SearchResult> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return None;
        }

        self.search_index
            .search(&self.catalog, trimmed)
            .into_iter()
            .find(|result| {
                result.item.category != ItemCategory::Ai
                    && result
                        .item
                        .actions
                        .iter()
                        .any(|action| !matches!(action.kind, ActionKind::AiPrompt))
            })
    }

    fn execute_ai_tool_suggestion(
        &mut self,
        ctx: &egui::Context,
        suggestion: &AiToolSuggestion,
    ) -> bool {
        let Some(result) = suggestion.result.clone() else {
            self.last_status = Some(suggestion.detail.clone());
            return false;
        };

        let query = suggestion
            .query_context
            .as_deref()
            .unwrap_or(self.query.as_str())
            .to_string();
        self.execute_result_with_query(ctx, &result, true, &query)
    }

    fn trim_ai_conversation_history(&mut self) {
        let extra = self
            .ai_conversation_messages
            .len()
            .saturating_sub(AI_CONVERSATION_MESSAGE_LIMIT);
        if extra > 0 {
            self.ai_conversation_messages.drain(0..extra);
        }
    }

    fn indexed_tool_count(&self) -> usize {
        self.catalog
            .iter()
            .filter(|item| item.category == ItemCategory::Tool)
            .count()
    }

    fn ai_tool_context_for_prompt(&self, prompt: &str) -> Vec<AiContextItem> {
        let prompt_lower = prompt.to_ascii_lowercase();
        let include_tools = prompt_lower.contains("tool")
            || prompt_lower.contains("run")
            || prompt_lower.contains("open")
            || prompt_lower.contains("fix")
            || prompt_lower.contains("do ");

        let mut items = Vec::new();
        let mut seen = HashSet::new();
        for result in self.search_index.search(&self.catalog, prompt).into_iter() {
            if !matches!(
                result.item.category,
                ItemCategory::Tool
                    | ItemCategory::Command
                    | ItemCategory::Setting
                    | ItemCategory::System
                    | ItemCategory::App
                    | ItemCategory::Web
            ) {
                continue;
            }
            if seen.insert(result.item.id.clone()) {
                items.push(AiContextItem::from_catalog_item(&result.item));
            }
            if items.len() >= AI_TOOL_CONTEXT_LIMIT {
                return items;
            }
        }

        if include_tools {
            for item in self
                .catalog
                .iter()
                .filter(|item| item.category == ItemCategory::Tool)
            {
                if seen.insert(item.id.clone()) {
                    items.push(AiContextItem::from_catalog_item(item));
                }
                if items.len() >= AI_TOOL_CONTEXT_LIMIT {
                    break;
                }
            }
        }

        items
    }

    fn build_ai_model_prompt(&self, provider: &AiProvider, prompt: &str) -> AiPromptPlan {
        let compact = ai_provider_needs_compact_prompt(provider);
        let mut tool_context = self.ai_tool_context_for_prompt(prompt);
        if compact {
            tool_context.truncate(AI_COMPACT_TOOL_CONTEXT_LIMIT);
        }

        let clipboard_context = ai_clipboard_context_for_prompt(prompt);
        let mut message_limit = ai_context_message_limit_for_provider(provider);
        if !prompt_needs_conversation_context(prompt) {
            message_limit = 0;
        }
        loop {
            let model_prompt = format_ai_model_prompt(
                prompt,
                &tool_context,
                clipboard_context.as_deref(),
                &self.ai_conversation_messages,
                message_limit,
                compact,
            );
            if ai_provider_prompt_fits(provider, &model_prompt) {
                let estimated_provider_tokens =
                    estimate_ai_provider_prompt_tokens(provider, &model_prompt);
                return AiPromptPlan::new(
                    model_prompt,
                    tool_context.len(),
                    self.ai_conversation_messages
                        .iter()
                        .rev()
                        .take(message_limit)
                        .count(),
                    estimated_provider_tokens,
                );
            }

            if message_limit > 0 {
                message_limit -= 1;
                continue;
            }
            if !tool_context.is_empty() {
                tool_context.pop();
                continue;
            }

            let fallback_prompt = format_ai_model_prompt(
                &trim_text_to_provider_budget(provider, prompt),
                &[],
                clipboard_context.as_deref(),
                &self.ai_conversation_messages,
                0,
                true,
            );
            let estimated_provider_tokens =
                estimate_ai_provider_prompt_tokens(provider, &fallback_prompt);
            return AiPromptPlan::new(fallback_prompt, 0, 0, estimated_provider_tokens);
        }
    }

    fn resolve_action_with_query(&self, action: &Action, query: &str) -> Action {
        let mut action = action.clone();
        if let Some(command) = &action.command {
            let query = if action.kind == ActionKind::OpenUrl {
                encode_query(action.args.first().map(String::as_str).unwrap_or(query))
            } else {
                query.to_string()
            };
            action.command = Some(command.replace("{query}", &query));
        }
        if let Some(command) = &action.command {
            action.command = Some(expand_env_vars(command));
        }
        action.args = action
            .args
            .iter()
            .map(|arg| arg.replace("{query}", query))
            .map(|arg| expand_env_vars(&arg))
            .collect();
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

    fn open_ai_chat_log(&mut self) {
        match ensure_ai_chat_log_file(&self.profile_dir) {
            Ok(path) => self.open_path(path, "Opened AI chat log"),
            Err(error) => {
                self.last_status = Some(format!("Could not open AI chat log: {error}"));
            }
        }
    }

    fn open_ai_chat_snapshot_dir(&mut self) {
        let path = ai_chat_snapshot_dir(&self.profile_dir);
        if let Err(error) = fs::create_dir_all(&path) {
            self.last_status = Some(format!("Could not create AI chat folder: {error}"));
            return;
        }

        self.open_path(path, "Opened AI chat snapshots");
    }

    fn save_current_ai_chat_snapshot(&mut self) {
        let response = self.ai_response.clone();
        if self.ai_conversation_messages.is_empty() && response.is_none() {
            self.last_status = Some("No AI chat to save".to_string());
            return;
        }

        let evaluation = response.as_ref().map(|response| {
            response
                .eval
                .clone()
                .unwrap_or_else(|| evaluate_ai_response(response))
        });
        match save_ai_chat_snapshot(
            &self.profile_dir,
            self.ai_session_id,
            &self.ai_conversation_messages,
            response.as_ref(),
            evaluation.as_ref(),
        ) {
            Ok(path) => self.open_path(path, "Saved AI chat snapshot"),
            Err(error) => {
                self.last_status = Some(format!("Could not save AI chat snapshot: {error}"));
            }
        }
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
        let available = ui.available_size();
        let sidebar_width = 180.0;
        let gap = 12.0;
        let content_width = (available.x - sidebar_width - gap).max(260.0);

        ui.horizontal(|ui| {
            ui.allocate_ui(Vec2::new(sidebar_width, available.y), |ui| {
                Frame::new()
                    .fill(panel_fill(&self.config))
                    .stroke(subtle_stroke(&self.config))
                    .corner_radius(8)
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(sidebar_width - 24.0);
                        ui.set_height((available.y - 24.0).max(120.0));
                        ScrollArea::vertical()
                            .id_salt("settings-sidebar")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 5.0);
                                ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                                ui.spacing_mut().interact_size = egui::vec2(32.0, 28.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new("Settings")
                                            .strong()
                                            .size(preview_heading_size(&self.config))
                                            .color(Color32::from_rgb(238, 242, 245)),
                                    );
                                    ui.add_space(6.0);
                                    for page in SettingsPage::ALL {
                                        if settings_nav_button(
                                            ui,
                                            page.label(),
                                            self.settings_page == page,
                                        )
                                        .clicked()
                                        {
                                            self.settings_page = page;
                                        }
                                        ui.add_space(2.0);
                                    }
                                });
                            });
                    });
            });

            ui.add_space(gap);

            ui.allocate_ui(Vec2::new(content_width, available.y), |ui| {
                Frame::new()
                    .fill(content_fill(&self.config))
                    .stroke(subtle_stroke(&self.config))
                    .corner_radius(8)
                    .inner_margin(Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width((content_width - 32.0).max(220.0));
                        ui.set_height((available.y - 32.0).max(160.0));
                        ScrollArea::vertical()
                            .id_salt("settings-content")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 5.0);
                                ui.spacing_mut().button_padding = egui::vec2(9.0, 5.0);
                                ui.spacing_mut().interact_size = egui::vec2(34.0, 30.0);
                                ui.vertical(|ui| {
                                    self.render_settings_page(ui);
                                });
                            });
                    });
            });
        });
    }

    fn render_settings_page(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(self.settings_page.label())
                .strong()
                .size(preview_heading_size(&self.config))
                .color(Color32::from_rgb(238, 242, 245)),
        );
        ui.add_space(12.0);

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
                ui.add_space(6.0);
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
                    .add(Slider::new(&mut opacity, 35.0..=100.0).text("Opacity"))
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
                setting_row(ui, "Guarded action", "Shift+Enter");
                setting_row(ui, "Elevated action", "Planned");
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
                ui.add_space(10.0);
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
                let active_provider = enabled_ai_provider_for_query(&self.config, "");
                setting_row(
                    ui,
                    "Enabled",
                    if self.config.ai.enabled { "Yes" } else { "No" },
                );
                setting_row(
                    ui,
                    "Default provider",
                    active_provider
                        .map(ai_provider_label)
                        .unwrap_or_else(|| "Not configured".to_string()),
                );
                setting_row(
                    ui,
                    "Configured providers",
                    self.config.ai.providers.len().to_string(),
                );
                setting_row(
                    ui,
                    "Endpoint",
                    active_provider
                        .map(|provider| provider.base_url.as_str())
                        .unwrap_or("None"),
                );
                setting_row(
                    ui,
                    "Tool calling",
                    active_provider
                        .map(|provider| {
                            if provider.supports_tools {
                                "Enabled"
                            } else {
                                "Provider only"
                            }
                        })
                        .unwrap_or("Unavailable"),
                );
                setting_row(
                    ui,
                    "Chat log",
                    ai_chat_log_path(&self.profile_dir).display().to_string(),
                );
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Open ai.toml").clicked() {
                        self.open_profile_file(ProfileFile::Ai);
                    }
                    if ui.button("Open AI log").clicked() {
                        self.open_ai_chat_log();
                    }
                    if ui.button("Open chat snapshots").clicked() {
                        self.open_ai_chat_snapshot_dir();
                    }
                    if ui.button("Save visible chat").clicked() {
                        self.save_current_ai_chat_snapshot();
                    }
                });
            }
            SettingsPage::Tools => {
                setting_row(
                    ui,
                    "Configured plugins",
                    self.config.plugins.len().to_string(),
                );
                setting_row(
                    ui,
                    "Process tools",
                    self.plugin_process_item_count.to_string(),
                );
                setting_row(
                    ui,
                    "JSON-RPC tools",
                    self.plugin_json_rpc_item_count.to_string(),
                );
                setting_row(
                    ui,
                    "Manifest tools",
                    self.tool_manifest_item_count.to_string(),
                );
                setting_row(ui, "Plugin errors", self.plugin_error_count.to_string());
                setting_row(ui, "Trust model", "Local trusted");
                setting_row(ui, "Confirmation", "Shift+Enter for guarded actions");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Reload plugins").clicked() {
                        self.reload_profile(ui.ctx());
                    }
                    if ui.button("Open plugins.toml").clicked() {
                        self.open_profile_file(ProfileFile::Plugins);
                    }
                });
            }
            SettingsPage::Diagnostics => {
                ui.horizontal(|ui| {
                    if ui.button("Reload").clicked() {
                        self.reload_profile(ui.ctx());
                    }
                    if ui.button("Open config.toml").clicked() {
                        self.open_profile_file(ProfileFile::Config);
                    }
                    if ui.button("Clear history").clicked() {
                        self.clear_launch_history();
                    }
                    if ui.button("Quit Veyra").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(10.0);
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
                setting_row(
                    ui,
                    "History",
                    format!(
                        "{} items / {} launches",
                        self.launch_history.entries.len(),
                        self.launch_history.total_launches()
                    ),
                );
                setting_row(
                    ui,
                    "Refresh",
                    if self.runtime_refreshing {
                        "Running"
                    } else {
                        "Idle"
                    },
                );
                setting_row(ui, "Last load", format!("{} ms", self.runtime_load_ms));
                setting_row(ui, "PATH items", self.path_item_count.to_string());
                setting_row(
                    ui,
                    "Start Menu items",
                    self.start_menu_item_count.to_string(),
                );
                setting_row(
                    ui,
                    "File catalog items",
                    self.file_catalog_item_count.to_string(),
                );
                setting_row(
                    ui,
                    "Plugin suggestions",
                    if self.plugin_suggestion_due_at.is_some() {
                        "Queued"
                    } else if self.plugin_suggestion_refreshing {
                        "Running"
                    } else {
                        "Idle"
                    },
                );
                setting_row(
                    ui,
                    "Skipped paths",
                    self.file_catalog_skipped_paths.to_string(),
                );
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
        profile_dir.join("plugins.toml"),
        ConfigMergeMode::PluginsOnly,
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
    plugin_process_item_count: usize,
    plugin_json_rpc_item_count: usize,
    tool_manifest_item_count: usize,
    plugin_error_count: usize,
    runtime_load_ms: u128,
}

struct RuntimeUpdate {
    runtime: RuntimeState,
    clear_search_on_apply: bool,
}

struct PluginSuggestionBatch {
    generation: u64,
    query: String,
    items: Vec<CatalogItem>,
    diagnostics: Vec<String>,
    error_count: usize,
}

struct AiWarmupEvent {
    generation: u64,
    provider_label: String,
    result: Result<(), String>,
}

#[derive(Debug, Clone)]
struct AiResponse {
    generation: u64,
    session_id: u64,
    turn_index: u32,
    prompt: String,
    provider_label: String,
    request: AiRequestInfo,
    elapsed_ms: Option<u128>,
    tool_suggestions: Vec<AiToolSuggestion>,
    eval: Option<AiEvalReport>,
    result: AiResponseResult,
}

#[derive(Debug, Clone)]
struct AiRequestInfo {
    provider_kind: AiProviderKind,
    model_label: String,
    indexed_tools: usize,
    tool_context_items: usize,
    message_context_items: usize,
    estimated_context_tokens: usize,
    context_limit_tokens: Option<usize>,
    provider_supports_tools: bool,
    native_tool_calls_enabled: bool,
    parsed_tool_calls_enabled: bool,
}

#[derive(Debug, Clone)]
enum AiResponseResult {
    Pending,
    Answer(String),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum AiConversationRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiConversationMessage {
    role: AiConversationRole,
    text: String,
}

struct DirectAiAnswer {
    session_id: u64,
    turn_index: u32,
    prompt: String,
    answer: String,
    tool_suggestions: Vec<AiToolSuggestion>,
    status: &'static str,
}

#[derive(Debug, Clone)]
struct AiToolSuggestion {
    call: AiToolCall,
    label: String,
    detail: String,
    result: Option<SearchResult>,
    query_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AiEvalReport {
    passed: bool,
    summary: String,
    checks: Vec<AiEvalCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AiEvalCheck {
    name: String,
    passed: bool,
    detail: String,
}

fn dedupe_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen_ids = HashSet::new();
    let mut seen_labels = HashSet::new();
    results
        .into_iter()
        .filter(|result| {
            let label_key = normalize_dedupe_label(&result.item.label);
            seen_ids.insert(result.item.id.clone()) && seen_labels.insert(label_key)
        })
        .collect()
}

fn normalize_dedupe_label(label: &str) -> String {
    label
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn ai_clipboard_context_for_prompt(prompt: &str) -> Option<String> {
    if !prompt_requests_clipboard_context(prompt) {
        return None;
    }

    read_clipboard_text().map(|text| truncate_for_label(&text, AI_CLIPBOARD_CONTEXT_CHARS))
}

fn prompt_requests_clipboard_context(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    lowered.contains("clipboard")
        || lowered.contains("selected")
        || lowered.contains("selection")
        || lowered.contains("highlighted")
        || lowered.contains("copied")
        || lowered.contains("this text")
        || lowered.contains("this input")
        || lowered.contains("the input")
        || lowered.contains("word input")
        || lowered.starts_with("fix this")
        || lowered.starts_with("rewrite this")
        || lowered.starts_with("summarize this")
        || lowered.starts_with("explain this")
        || lowered.starts_with("what is this")
        || lowered.starts_with("what does this")
}

#[cfg(windows)]
fn read_clipboard_text() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(WINDOWS_CF_UNICODETEXT) == 0 {
            return None;
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }

        let text = read_open_clipboard_text();
        let _ = CloseClipboard();
        text
    }
}

#[cfg(windows)]
unsafe fn read_open_clipboard_text() -> Option<String> {
    let handle = unsafe { GetClipboardData(WINDOWS_CF_UNICODETEXT) };
    if handle.is_null() {
        return None;
    }

    let ptr = unsafe { GlobalLock(handle) } as *const u16;
    if ptr.is_null() {
        return None;
    }

    let mut len = 0_usize;
    while len < AI_CLIPBOARD_CONTEXT_CHARS && unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    let text = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len)) };
    let _ = unsafe { GlobalUnlock(handle) };

    non_empty(&text)
}

#[cfg(not(windows))]
fn read_clipboard_text() -> Option<String> {
    None
}

fn current_unix_timestamp_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn deterministic_ai_answer(prompt: &str) -> Option<String> {
    let lowered = prompt.to_ascii_lowercase();
    if let Some(answer) = timezone_or_location_answer_for_prompt(&lowered) {
        return Some(answer);
    }

    if let Some(answer) = clock_answer_for_prompt_at(prompt, current_unix_timestamp_i64()) {
        return Some(answer);
    }

    if let Some(answer) = calculator_answer_for_prompt(prompt) {
        return Some(answer);
    }

    if !looks_like_clock_question(&lowered) {
        return None;
    }

    local_clock_answer_for_prompt_with_parts(&lowered, current_local_datetime_parts())
}

fn timezone_or_location_answer_for_prompt(lowered: &str) -> Option<String> {
    let info = local_time_zone_info();
    timezone_or_location_answer_for_prompt_with_info(lowered, info.as_ref())
}

fn timezone_or_location_answer_for_prompt_with_info(
    lowered: &str,
    info: Option<&LocalTimeZoneInfo>,
) -> Option<String> {
    if looks_like_timezone_question(lowered) {
        let zone = format_local_time_zone_info(info);
        return Some(format!("Your system time zone is {zone}."));
    }

    if looks_like_location_question(lowered) {
        let zone = format_local_time_zone_info(info);
        return Some(format!(
            "I cannot determine your physical location from Veyra. I can only see your system time zone, which is {zone}."
        ));
    }

    None
}

fn calculator_answer_for_prompt(prompt: &str) -> Option<String> {
    let expression = extract_calculation_expression(prompt)?;
    evaluate_expression(&expression).map(format_number)
}

fn extract_calculation_expression(prompt: &str) -> Option<String> {
    let mut best = None;
    let mut current = String::new();

    for ch in prompt.chars() {
        if ch.is_ascii_digit()
            || matches!(
                ch,
                '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')' | '.' | ' '
            )
        {
            current.push(ch);
            continue;
        }

        maybe_keep_calculation_candidate(&current, &mut best);
        current.clear();
    }
    maybe_keep_calculation_candidate(&current, &mut best);

    best
}

fn maybe_keep_calculation_candidate(candidate: &str, best: &mut Option<String>) {
    let candidate = candidate.trim();
    if !looks_like_calculation(candidate) || evaluate_expression(candidate).is_none() {
        return;
    }

    if best
        .as_ref()
        .is_none_or(|value| candidate.len() > value.len())
    {
        *best = Some(candidate.to_string());
    }
}

fn clock_answer_for_prompt_at(prompt: &str, utc_seconds: i64) -> Option<String> {
    let lowered = prompt.to_ascii_lowercase();
    if !looks_like_clock_question(&lowered) {
        return None;
    }

    fixed_clock_answer_for_prompt_at(&lowered, utc_seconds)
}

fn fixed_clock_answer_for_prompt_at(lowered: &str, utc_seconds: i64) -> Option<String> {
    let zone = fixed_clock_zone_for_prompt(lowered)?;
    let parts = datetime_parts_for_fixed_offset(utc_seconds, zone.offset_seconds);
    Some(format_clock_answer(
        &parts,
        zone.location,
        Some(zone.abbreviation),
        Some(zone.offset_seconds),
    ))
}

fn local_clock_answer_for_prompt_with_parts(lowered: &str, parts: DateTimeParts) -> Option<String> {
    if fixed_clock_zone_for_prompt(lowered).is_some() {
        return None;
    }

    Some(format_clock_answer(
        &parts,
        "your local time zone",
        None,
        None,
    ))
}

fn looks_like_clock_question(lowered: &str) -> bool {
    let words = prompt_words(lowered);
    let has = |needle: &str| words.contains(&needle);
    let asks_current = has("what")
        || has("which")
        || has("current")
        || has("now")
        || lowered.contains("right now");

    has("time")
        || has("date")
        || has("clock")
        || (has("day") && asks_current)
        || (has("year") && asks_current)
}

fn looks_like_timezone_question(lowered: &str) -> bool {
    lowered.contains("timezone")
        || lowered.contains("time zone")
        || lowered.contains("utc offset")
        || lowered.contains("local zone")
}

fn looks_like_location_question(lowered: &str) -> bool {
    if looks_like_clock_question(lowered) || looks_like_timezone_question(lowered) {
        return false;
    }

    let words = prompt_words(lowered);
    let has = |needle: &str| words.contains(&needle);
    lowered.contains("where am i")
        || lowered.contains("where i am")
        || lowered.contains("my location")
        || (has("what") && has("city") && (has("am") || has("in")))
        || (has("where") && has("located"))
}

fn prompt_words(value: &str) -> Vec<&str> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalTimeZoneInfo {
    name: String,
    offset_seconds: Option<i32>,
}

fn format_local_time_zone_info(info: Option<&LocalTimeZoneInfo>) -> String {
    let Some(info) = info else {
        return "your local time zone".to_string();
    };

    if let Some(offset_seconds) = info.offset_seconds {
        format!("{} ({})", info.name, format_utc_offset(offset_seconds))
    } else {
        info.name.clone()
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedClockZone {
    location: &'static str,
    abbreviation: &'static str,
    offset_seconds: i32,
}

fn fixed_clock_zone_for_prompt(lowered: &str) -> Option<FixedClockZone> {
    if lowered.contains("tokyo") || lowered.contains("japan") || lowered.contains("jst") {
        return Some(FixedClockZone {
            location: "Tokyo",
            abbreviation: "JST",
            offset_seconds: 9 * 60 * 60,
        });
    }
    if (lowered.contains("wilmington") && (lowered.contains("de") || lowered.contains("delaware")))
        || lowered.contains("delware")
        || lowered.contains("wilmingotn")
    {
        return Some(FixedClockZone {
            location: "Wilmington, Delaware",
            abbreviation: "EDT",
            offset_seconds: -4 * 60 * 60,
        });
    }
    if lowered.contains("utc") || lowered.contains("gmt") {
        return Some(FixedClockZone {
            location: "UTC",
            abbreviation: "UTC",
            offset_seconds: 0,
        });
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DateTimeParts {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    weekday: usize,
}

impl DateTimeParts {
    fn month_name(self) -> &'static str {
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        MONTHS
            .get(self.month.saturating_sub(1) as usize)
            .copied()
            .unwrap_or("Unknown")
    }

    fn weekday_name(self) -> &'static str {
        const WEEKDAYS: [&str; 7] = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        WEEKDAYS.get(self.weekday).copied().unwrap_or("Unknown")
    }

    fn format_iso_date(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn format_time(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    fn format_datetime(self) -> String {
        format!("{} {}", self.format_iso_date(), self.format_time())
    }
}

#[cfg(windows)]
fn current_local_datetime_parts() -> DateTimeParts {
    let mut local_time = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    unsafe {
        GetLocalTime(&mut local_time);
    }

    DateTimeParts {
        year: i32::from(local_time.wYear),
        month: u32::from(local_time.wMonth),
        day: u32::from(local_time.wDay),
        hour: u32::from(local_time.wHour),
        minute: u32::from(local_time.wMinute),
        weekday: usize::from(local_time.wDayOfWeek),
    }
}

#[cfg(not(windows))]
fn current_local_datetime_parts() -> DateTimeParts {
    datetime_parts_for_fixed_offset(current_unix_timestamp_i64(), 0)
}

#[cfg(windows)]
fn local_time_zone_info() -> Option<LocalTimeZoneInfo> {
    let mut info: DYNAMIC_TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let status = unsafe { GetDynamicTimeZoneInformation(&mut info) };
    if status == TIME_ZONE_ID_INVALID {
        return None;
    }

    let name = wide_nul_to_string(&info.TimeZoneKeyName)
        .or_else(|| wide_nul_to_string(&info.StandardName))
        .or_else(|| wide_nul_to_string(&info.DaylightName))
        .unwrap_or_else(|| "your local time zone".to_string());
    let status_bias = match status {
        2 => info.DaylightBias,
        1 => info.StandardBias,
        _ => 0,
    };
    let bias_minutes = info.Bias.saturating_add(status_bias);
    Some(LocalTimeZoneInfo {
        name,
        offset_seconds: Some(bias_minutes.saturating_mul(-60)),
    })
}

#[cfg(not(windows))]
fn local_time_zone_info() -> Option<LocalTimeZoneInfo> {
    std::env::var("TZ").ok().and_then(|name| {
        non_empty(&name).map(|name| LocalTimeZoneInfo {
            name,
            offset_seconds: None,
        })
    })
}

#[cfg(windows)]
fn wide_nul_to_string(value: &[u16]) -> Option<String> {
    let len = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    if len == 0 {
        return None;
    }

    non_empty(&String::from_utf16_lossy(&value[..len]))
}

fn datetime_parts_for_fixed_offset(utc_seconds: i64, offset_seconds: i32) -> DateTimeParts {
    let local_seconds = utc_seconds.saturating_add(i64::from(offset_seconds));
    let days = local_seconds.div_euclid(86_400);
    let seconds_of_day = local_seconds.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_from_unix_days(days);
    DateTimeParts {
        year,
        month,
        day,
        hour: seconds_of_day / 3_600,
        minute: (seconds_of_day % 3_600) / 60,
        weekday: (days + 4).rem_euclid(7) as usize,
    }
}

fn civil_from_unix_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let days = days_since_unix_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}

fn format_clock_answer(
    parts: &DateTimeParts,
    location: &str,
    abbreviation: Option<&str>,
    offset_seconds: Option<i32>,
) -> String {
    let zone_label = match (abbreviation, offset_seconds) {
        (Some(abbreviation), Some(offset_seconds)) => {
            format!(
                "{} ({}, {})",
                location,
                abbreviation,
                format_utc_offset(offset_seconds)
            )
        }
        _ => location.to_string(),
    };

    format!(
        "It is {} on {}, {} {}, {} in {}.",
        format_clock_time(parts),
        parts.weekday_name(),
        parts.month_name(),
        parts.day,
        parts.year,
        zone_label
    )
}

fn format_clock_time(parts: &DateTimeParts) -> String {
    let hour_12 = match parts.hour % 12 {
        0 => 12,
        value => value,
    };
    let suffix = if parts.hour < 12 { "AM" } else { "PM" };
    format!("{hour_12}:{:02} {suffix}", parts.minute)
}

fn format_utc_offset(offset_seconds: i32) -> String {
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let abs_seconds = offset_seconds.unsigned_abs();
    let hours = abs_seconds / 3_600;
    let minutes = (abs_seconds % 3_600) / 60;
    format!("UTC{sign}{hours:02}:{minutes:02}")
}

const COPY_TO_CLIPBOARD_ACTION_ID: &str = "copy_to_clipboard";
const SEARCH_ROW_HEIGHT: f32 = 44.0;
const AI_CONVERSATION_MESSAGE_LIMIT: usize = 80;
const AI_CONTEXT_MESSAGE_LIMIT: usize = 10;
const AI_TOOL_CONTEXT_LIMIT: usize = 8;
const AI_COMPACT_CONTEXT_THRESHOLD: usize = 1024;
const AI_COMPACT_TOOL_CONTEXT_LIMIT: usize = 2;
const AI_COMPACT_GENERATION_RESERVE_TOKENS: usize = 192;
const AI_GENERATION_RESERVE_TOKENS: usize = 512;
const AI_CLIPBOARD_CONTEXT_CHARS: usize = 2_000;
#[cfg(windows)]
const WINDOWS_CF_UNICODETEXT: u32 = 13;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileFile {
    Config,
    Commands,
    Plugins,
    Catalogs,
    Ai,
}

impl ProfileFile {
    fn file_name(self) -> &'static str {
        match self {
            ProfileFile::Config => "config.toml",
            ProfileFile::Commands => "commands.toml",
            ProfileFile::Plugins => "plugins.toml",
            ProfileFile::Catalogs => "catalogs.toml",
            ProfileFile::Ai => "ai.toml",
        }
    }

    fn template(self) -> &'static str {
        match self {
            ProfileFile::Config => DEFAULT_CONFIG_TOML,
            ProfileFile::Commands => DEFAULT_COMMANDS_TOML,
            ProfileFile::Plugins => DEFAULT_PLUGINS_TOML,
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
opacity = 0.72
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

const DEFAULT_PLUGINS_TOML: &str = r#"[[plugins]]
id = "sample.echo"
label = "Sample Plugin: Echo Query"
description = "Example JSON-RPC stdio plugin; enable after confirming Python and the script path"
kind = "json_rpc_stdio"
command = "python"
args = ["%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\sample-json-rpc-plugin.py"]
keywords = ["sample", "plugin", "jsonrpc", "echo"]
enabled = false
timeout_ms = 5000

[[plugins]]
id = "kyrphina.ask"
label = "Kyrphina: Ask"
description = "Open the Kyrphina chat panel and send the typed prompt"
kind = "process"
command = "powershell.exe"
args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", "%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\veyra-kyrphina.ps1", "-Mode", "Chat", "-Query", "{query}"]
keywords = ["ai", "assistant", "kyrphina", "chat", "ask"]
enabled = true

[[plugins]]
id = "kyrphina.settings"
label = "Kyrphina: Settings"
description = "Open the Kyrphina settings panel"
kind = "process"
command = "powershell.exe"
args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", "%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\veyra-kyrphina.ps1", "-Mode", "Settings"]
keywords = ["ai", "assistant", "kyrphina", "settings", "config"]
enabled = true

[[plugins]]
id = "kyrphina.doctor"
label = "Kyrphina: Doctor"
description = "Run Kyrphina diagnostics and show the output"
kind = "process"
command = "powershell.exe"
args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", "%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\veyra-kyrphina.ps1", "-Mode", "Doctor"]
keywords = ["ai", "assistant", "kyrphina", "doctor", "diagnostics"]
enabled = true

[[plugins]]
id = "kyrphina.start_llama"
label = "Kyrphina: Start llama.cpp"
description = "Start the Kyrphina llama.cpp backend in the background"
kind = "process"
command = "powershell.exe"
args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", "%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\veyra-kyrphina.ps1", "-Mode", "StartLlama"]
keywords = ["ai", "assistant", "kyrphina", "llama", "backend"]
enabled = true
requires_confirmation = true

[[plugins]]
id = "system.clipboard"
label = "System: Clipboard Settings"
description = "Open Windows clipboard history settings"
kind = "process"
command = "explorer.exe"
args = ["ms-settings:clipboard"]
keywords = ["clipboard", "history", "settings", "plugin"]
enabled = true

[[plugins]]
id = "system.guardian.log"
label = "System Guardian: Log"
description = "Open the System Guardian watchdog log"
kind = "process"
command = "notepad.exe"
args = ["%USERPROFILE%\\system-guardian.log"]
keywords = ["system", "guardian", "watchdog", "health", "log"]
enabled = true

[[plugins]]
id = "system.guardian.run"
label = "System Guardian: Run Task"
description = "Ask the SystemGuardian scheduled task to run now"
kind = "process"
command = "schtasks.exe"
args = ["/Run", "/TN", "SystemGuardian"]
keywords = ["system", "guardian", "watchdog", "task", "run"]
enabled = true
requires_confirmation = true
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
default_provider = "minicpm5_npu"
local_only = true
        warmup_on_startup = false

[[providers]]
id = "minicpm5_npu"
label = "MiniCPM5-1B NPU 16K"
kind = "process"
command = "%USERPROFILE%\\Development\\npu-projects\\npu-chat\\npu_chat.exe"
args = ["%USERPROFILE%\\models\\qualcomm-genie\\minicpm5-1b\\bundle-kvint4-cl16k\\minicpm5_1b_instruct-genie-kvint4-qualcomm_snapdragon_x_plus_8_core", "--temp", "0.3", "--top-k", "40", "--top-p", "0.9", "--seed", "42"]
keep_warm = true
model = "openbmb/MiniCPM5-1B"
local_only = true
enabled = true
timeout_ms = 120000
supports_streaming = false
supports_tools = false
context_limit_tokens = 16384

[[providers]]
id = "pike"
label = "Pike coding agent"
kind = "process"
command = "powershell.exe"
args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", "%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\veyra-pike.ps1", "-PromptFile", "{prompt_file}", "-NoProjectContext", "-Tools", "read,grep,find", "-MaxTurns", "40"]
keep_warm = false
model = "pike default"
local_only = true
enabled = false
timeout_ms = 300000
supports_streaming = false
supports_tools = true
context_limit_tokens = 8000

[[providers]]
id = "pylon"
label = "Pylon router"
kind = "open_ai_compatible"
base_url = "http://127.0.0.1:8088/v1"
model = "pylon-deepseek-v4-flash"
api_key_env = "PYLON_API_KEY"
local_only = true
enabled = false
timeout_ms = 120000
supports_streaming = true
supports_tools = true

[[providers]]
id = "llama"
label = "llama.cpp local"
kind = "open_ai_compatible"
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"
api_key_env = ""
local_only = true
enabled = true
timeout_ms = 60000
supports_streaming = true
supports_tools = true
"#;

fn load_base_runtime_state(profile_dir: &Path) -> RuntimeState {
    let started = Instant::now();
    let (config, loaded_items, load_messages) = load_profile(profile_dir);
    let plugin_process_item_count = config
        .plugins
        .iter()
        .filter(|plugin| process_plugin_item(plugin).is_some())
        .count();
    let mut catalog = seed_catalog();
    catalog.extend(loaded_items);

    RuntimeState {
        config,
        catalog,
        load_messages,
        path_item_count: 0,
        start_menu_item_count: 0,
        file_catalog_item_count: 0,
        file_catalog_skipped_paths: 0,
        plugin_process_item_count,
        plugin_json_rpc_item_count: 0,
        tool_manifest_item_count: 0,
        plugin_error_count: 0,
        runtime_load_ms: started.elapsed().as_millis(),
    }
}

#[cfg(test)]
fn load_runtime_state(profile_dir: &Path) -> RuntimeState {
    load_runtime_state_with_cache(profile_dir, true)
}

fn load_runtime_state_with_cache(profile_dir: &Path, force_refresh: bool) -> RuntimeState {
    let started = Instant::now();
    let mut runtime = load_base_runtime_state(profile_dir);

    let platform_items = if force_refresh {
        let items = discover_platform_catalog_items();
        if let Err(error) = save_cached_platform_catalog_items(profile_dir, &items) {
            runtime
                .load_messages
                .push(format!("Could not save platform cache: {error}"));
        }
        runtime.load_messages.push(format!(
            "Discovered {} PATH executables and {} Start Menu shortcuts",
            items.iter().filter(|item| item.source == "path").count(),
            items
                .iter()
                .filter(|item| item.source == "start_menu")
                .count()
        ));
        items
    } else {
        match load_fresh_cached_platform_catalog_items(
            profile_dir,
            veyra_platform::PLATFORM_CACHE_DEFAULT_TTL_SECONDS,
        ) {
            Some(items) => {
                runtime
                    .load_messages
                    .push(format!("Loaded {} cached platform items", items.len()));
                items
            }
            None => Vec::new(),
        }
    };

    runtime.path_item_count = platform_items
        .iter()
        .filter(|item| item.source == "path")
        .count();
    runtime.start_menu_item_count = platform_items
        .iter()
        .filter(|item| item.source == "start_menu")
        .count();
    runtime.catalog.extend(platform_items);

    let file_catalog = discover_file_catalog_items(&runtime.config.catalogs);
    let file_catalog_item_count = file_catalog.items.len();
    let file_catalog_skipped_paths = file_catalog.skipped_paths;
    runtime.load_messages.push(format!(
        "Indexed {file_catalog_item_count} file catalog items from {} enabled profiles",
        file_catalog.enabled_profiles
    ));
    if file_catalog_skipped_paths > 0 {
        runtime.load_messages.push(format!(
            "Skipped {file_catalog_skipped_paths} missing or unsupported catalog paths"
        ));
    }
    runtime.catalog.extend(file_catalog.items);

    let plugin_extensions = load_plugin_extensions(profile_dir, &runtime.config.plugins);
    runtime.load_messages.extend(plugin_extensions.diagnostics);
    runtime.plugin_json_rpc_item_count = plugin_extensions.json_rpc_item_count;
    runtime.tool_manifest_item_count = plugin_extensions.manifest_item_count;
    runtime.plugin_error_count = plugin_extensions.error_count;
    runtime.catalog.extend(plugin_extensions.items);

    runtime.file_catalog_item_count = file_catalog_item_count;
    runtime.file_catalog_skipped_paths = file_catalog_skipped_paths;
    runtime.runtime_load_ms = started.elapsed().as_millis();

    runtime
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

    if matches!(mode, ConfigMergeMode::Full | ConfigMergeMode::PluginsOnly) {
        target.plugins.extend(incoming.plugins);
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
    PluginsOnly,
    CatalogsOnly,
    AiOnly,
}

fn catalog_items_from_config(config: &VeyraConfig) -> Vec<CatalogItem> {
    config
        .commands
        .iter()
        .filter_map(command_item)
        .chain(config.web_search.iter().filter_map(web_search_item))
        .chain(config.plugins.iter().filter_map(process_plugin_item))
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

fn calculator_search_result(query: &str) -> Option<SearchResult> {
    let expression = query.trim();
    if !looks_like_calculation(expression) {
        return None;
    }

    let value = evaluate_expression(expression)?;
    let rendered = format_number(value);
    let item = CatalogItem::new(
        format!("quick.calculator.{expression}"),
        format!("{expression} = {rendered}"),
        ItemCategory::Tool,
        "quick",
    )
    .subtitle("Calculator result - Enter copies the answer")
    .keywords(["calculator", "calc", "math"])
    .action(Action {
        id: COPY_TO_CLIPBOARD_ACTION_ID.to_string(),
        label: "Copy result".to_string(),
        kind: ActionKind::ToolCall,
        command: Some(rendered),
        args: Vec::new(),
        requires_confirmation: false,
        run_as_admin: false,
    })
    .score_boost(2000);

    Some(SearchResult { item, score: 3000 })
}

fn unit_converter_search_result(query: &str) -> Option<SearchResult> {
    let (value, from, to) = parse_unit_conversion(query)?;
    let converted = convert_units(value, &from, &to)?;
    let rendered_value = format_number(value);
    let rendered_converted = format_number(converted);
    let label = format!("{rendered_value} {from} = {rendered_converted} {to}");

    let item = CatalogItem::new(
        format!("quick.unit.{from}.{to}"),
        label,
        ItemCategory::Tool,
        "quick",
    )
    .subtitle("Unit conversion - Enter copies the result")
    .keywords(["convert", "unit", &from, &to])
    .action(Action {
        id: COPY_TO_CLIPBOARD_ACTION_ID.to_string(),
        label: "Copy result".to_string(),
        kind: ActionKind::ToolCall,
        command: Some(rendered_converted),
        args: Vec::new(),
        requires_confirmation: false,
        run_as_admin: false,
    })
    .score_boost(1900);

    Some(SearchResult { item, score: 2900 })
}

fn snippet_search_result(config: &VeyraConfig, query: &str) -> Option<SearchResult> {
    let trimmed = query.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let keyword = if let Some(_rest) = lowered.strip_prefix("snippet ") {
        &trimmed["snippet ".len()..]
    } else if let Some(_rest) = lowered.strip_prefix("paste ") {
        &trimmed["paste ".len()..]
    } else {
        trimmed
    };

    let keyword = keyword.trim();
    if keyword.is_empty() {
        return None;
    }

    let (id, label, text, keyword_string) = if let Some(entry) = config
        .snippets
        .iter()
        .find(|entry| entry.keyword.to_ascii_lowercase() == keyword)
    {
        (
            entry.id.clone(),
            format!("Paste snippet: {}", entry.label),
            entry.text.clone(),
            entry.keyword.clone(),
        )
    } else {
        let (label, text) = builtin_snippet_text(keyword)?;
        (keyword.to_string(), label, text, keyword.to_string())
    };

    let item = CatalogItem::new(
        format!("quick.snippet.{id}"),
        label,
        ItemCategory::Tool,
        "quick",
    )
    .subtitle(format!("Keyword: {keyword_string}"))
    .keywords(vec!["snippet", "paste", keyword_string.as_str()])
    .action(Action {
        id: COPY_TO_CLIPBOARD_ACTION_ID.to_string(),
        label: "Copy snippet".to_string(),
        kind: ActionKind::ToolCall,
        command: Some(text),
        args: Vec::new(),
        requires_confirmation: false,
        run_as_admin: false,
    })
    .score_boost(1800);

    Some(SearchResult { item, score: 2800 })
}

fn builtin_snippet_text(keyword: &str) -> Option<(String, String)> {
    let parts = current_local_datetime_parts();
    match keyword {
        "date" => Some((
            format!("Date: {}", parts.format_iso_date()),
            parts.format_iso_date(),
        )),
        "time" => Some((
            format!("Time: {}", parts.format_time()),
            parts.format_time(),
        )),
        "datetime" | "now" => Some((
            format!("Date/time: {}", parts.format_datetime()),
            parts.format_datetime(),
        )),
        _ => None,
    }
}

fn komorebi_search_result(query: &str) -> Option<SearchResult> {
    let trimmed = query.trim();
    let lowered = trimmed.to_ascii_lowercase();

    let body = if let Some(_rest) = lowered.strip_prefix("komorebi ") {
        &trimmed["komorebi ".len()..]
    } else if let Some(_rest) = lowered.strip_prefix("kb ") {
        &trimmed["kb ".len()..]
    } else if let Some(_rest) = lowered.strip_prefix("wm ") {
        &trimmed["wm ".len()..]
    } else {
        return None;
    };

    let mut parts = body.split_whitespace();
    let alias = parts.next()?.to_ascii_lowercase();
    let extra: Vec<&str> = parts.collect();

    let args: Vec<String> = match alias.as_str() {
        "start" => vec!["start".to_string()],
        "stop" => vec!["stop".to_string()],
        "pause" => vec!["toggle-pause".to_string()],
        "float" => vec!["toggle-float".to_string()],
        "monocle" => vec!["toggle-monocle".to_string()],
        "max" | "maximized" => vec!["toggle-maximized".to_string()],
        "retile" => vec!["retile".to_string()],
        "layout" | "next-layout" => vec!["cycle-layout".to_string()],
        "prev-layout" => vec!["cycle-layout".to_string(), "previous".to_string()],
        "left" => vec!["focus".to_string(), "left".to_string()],
        "right" => vec!["focus".to_string(), "right".to_string()],
        "up" => vec!["focus".to_string(), "up".to_string()],
        "down" => vec!["focus".to_string(), "down".to_string()],
        "move" => {
            let direction = extra.first()?;
            vec!["move".to_string(), direction.to_ascii_lowercase()]
        }
        "ws" | "workspace" => {
            let n = extra.first()?;
            vec!["focus-workspace".to_string(), n.to_string()]
        }
        "movews" | "move-to-workspace" => {
            let n = extra.first()?;
            vec!["move-to-workspace".to_string(), n.to_string()]
        }
        "prev" | "previous" => vec!["focus-previous-workspace".to_string()],
        "moveprev" | "move-to-previous-workspace" => {
            vec!["move-to-previous-workspace".to_string()]
        }
        "promote" => vec!["promote".to_string()],
        "minimize" => vec!["minimize".to_string()],
        "close" => vec!["close".to_string()],
        "lock" => vec!["toggle-lock".to_string()],
        "swap" => vec!["swap-windows".to_string(), "next".to_string()],
        _ => return None,
    };

    let command_line = args.join(" ");
    let label = format!("Komorebi: {command_line}");

    let item = CatalogItem::new(
        format!("quick.komorebi.{}", args.join(".")),
        label,
        ItemCategory::Tool,
        "quick",
    )
    .subtitle(format!("Run komorebic.exe {command_line}"))
    .keywords(["komorebi", "kb", "wm", "tiling", &alias])
    .action(Action::launch_with_args("komorebic.exe", args.clone()))
    .score_boost(2000);

    Some(SearchResult { item, score: 3000 })
}

fn aurora_search_result(query: &str) -> Option<SearchResult> {
    let trimmed = query.trim();
    let lowered = trimmed.to_ascii_lowercase();

    let body = if let Some(_rest) = lowered.strip_prefix("aurora ") {
        &trimmed["aurora ".len()..]
    } else if let Some(_rest) = lowered.strip_prefix("wp ") {
        &trimmed["wp ".len()..]
    } else {
        return None;
    };

    let mut parts = body.split_whitespace();
    let alias = parts.next()?.to_ascii_lowercase();
    let extra: Vec<&str> = parts.collect();

    let (label, request_json, requires_confirmation) = match alias.as_str() {
        "next" | "n" => (
            "Next wallpaper".to_string(),
            r#"{"type":"next"}"#.to_string(),
            false,
        ),
        "prev" | "previous" | "p" => (
            "Previous wallpaper".to_string(),
            r#"{"type":"prev"}"#.to_string(),
            false,
        ),
        "pause" => (
            "Pause wallpaper cycling".to_string(),
            r#"{"type":"pause","data":{}}"#.to_string(),
            false,
        ),
        "resume" => (
            "Resume wallpaper cycling".to_string(),
            r#"{"type":"resume"}"#.to_string(),
            false,
        ),
        "set" => {
            let path = extra.join(" ");
            if path.is_empty() {
                return None;
            }
            (
                format!("Set wallpaper: {path}"),
                serde_json::json!({"type": "set", "data": {"path": path}}).to_string(),
                false,
            )
        }
        "folder" => {
            let path = extra.join(" ");
            if path.is_empty() {
                return None;
            }
            (
                format!("Set wallpaper folder: {path}"),
                serde_json::json!({"type": "set_folder", "data": {"path": path}}).to_string(),
                false,
            )
        }
        "reload" => (
            "Reload Aurora config".to_string(),
            r#"{"type":"reload"}"#.to_string(),
            false,
        ),
        "quit" | "stop" => (
            "Quit Aurora daemon".to_string(),
            r#"{"type":"quit"}"#.to_string(),
            true,
        ),
        "status" => (
            "Aurora status".to_string(),
            r#"{"type":"status"}"#.to_string(),
            false,
        ),
        "current" => (
            "Show current wallpaper".to_string(),
            r#"{"type":"get_current_wallpaper"}"#.to_string(),
            false,
        ),
        "stats" => (
            "Aurora stats".to_string(),
            r#"{"type":"stats"}"#.to_string(),
            false,
        ),
        _ => return None,
    };

    let item = CatalogItem::new(
        format!("quick.aurora.{alias}"),
        format!("Aurora: {label}"),
        ItemCategory::Tool,
        "quick",
    )
    .subtitle(format!("Send \"{label}\" to aurora daemon"))
    .keywords(["aurora", "wp", "wallpaper", &alias])
    .action(Action {
        id: format!("aurora.{alias}"),
        label: label.clone(),
        kind: ActionKind::AuroraIpc,
        command: Some(request_json),
        args: Vec::new(),
        requires_confirmation,
        run_as_admin: false,
    })
    .score_boost(2000);

    Some(SearchResult { item, score: 3000 })
}

fn web_search_result(query: &str) -> Option<SearchResult> {
    let search_text = query.trim();
    if search_text.is_empty() {
        return None;
    }

    let item = CatalogItem::new(
        "quick.web_search",
        format!("Search web for \"{search_text}\""),
        ItemCategory::Web,
        "quick",
    )
    .subtitle("Browser search - Enter opens your default browser")
    .keywords(["web", "search", "browser", "google"])
    .action(Action::open_url("https://www.google.com/search?q={query}"))
    .score_boost(-250);

    Some(SearchResult { item, score: 1 })
}

fn ai_copy_tool_result(text: &str) -> SearchResult {
    let item = CatalogItem::new(
        "ai.tool.copy_to_clipboard",
        "Copy AI text",
        ItemCategory::Tool,
        "ai",
    )
    .subtitle("Copy text suggested by AI")
    .keywords(["copy", "clipboard", "ai"])
    .action(Action {
        id: COPY_TO_CLIPBOARD_ACTION_ID.to_string(),
        label: "Copy".to_string(),
        kind: ActionKind::ToolCall,
        command: Some(text.to_string()),
        args: Vec::new(),
        requires_confirmation: false,
        run_as_admin: false,
    })
    .score_boost(2500);

    SearchResult { item, score: 2500 }
}

fn ai_open_url_tool_result(url: &str) -> Option<SearchResult> {
    let normalized = normalize_ai_url(url)?;
    let item = CatalogItem::new(
        "ai.tool.open_url",
        format!("Open {}", truncate_for_label(&normalized, 72)),
        ItemCategory::Web,
        "ai",
    )
    .subtitle("Open URL suggested by AI")
    .keywords(["open", "url", "web", "ai"])
    .action(Action::open_url(normalized))
    .score_boost(2400);

    Some(SearchResult { item, score: 2400 })
}

fn normalize_ai_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }

    let with_scheme = if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        trimmed.to_string()
    } else if trimmed.contains('.') && !trimmed.contains(':') {
        format!("https://{trimmed}")
    } else {
        return None;
    };

    let lowered = with_scheme.to_ascii_lowercase();
    (lowered.starts_with("https://") || lowered.starts_with("http://")).then_some(with_scheme)
}

fn ai_time_tool_answer(location: &str) -> Option<String> {
    let location = location.trim();
    let prompt = if location.is_empty() {
        "what time is it".to_string()
    } else {
        format!("what time is it in {location}")
    };
    deterministic_ai_answer(&prompt)
}

fn web_search_alias_result(config: &VeyraConfig, query: &str) -> Option<SearchResult> {
    let (alias, search_text) = query.trim().split_once(char::is_whitespace)?;
    let search_text = search_text.trim();
    if alias.is_empty() || search_text.is_empty() {
        return None;
    }

    let entry = config
        .web_search
        .iter()
        .find(|entry| entry.alias.eq_ignore_ascii_case(alias))?;
    let label = non_empty(&entry.label).unwrap_or_else(|| format!("Web: {}", entry.alias));
    let mut action = Action::open_url(entry.url.clone());
    action.args = vec![search_text.to_string()];

    let item = CatalogItem::new(
        format!("quick.web_alias.{}", entry.id),
        format!("Search {label} for \"{search_text}\""),
        ItemCategory::Web,
        "quick",
    )
    .subtitle(format!("{} - {}", entry.alias, entry.url))
    .keywords([entry.alias.clone(), label])
    .action(action)
    .score_boost(850);

    Some(SearchResult { item, score: 1_850 })
}

fn ai_prompt_search_result(query: &str, provider: &AiProvider) -> Option<SearchResult> {
    let prompt = ai_prompt_text(query)?;
    let provider_label = ai_provider_label(provider);
    let label = if prompt.is_empty() {
        format!("Ask {provider_label}")
    } else {
        format!("Ask {provider_label}: {prompt}")
    };
    let subtitle = match provider.kind {
        AiProviderKind::OpenAiCompatible if provider.base_url.trim().is_empty() => {
            "AI provider is missing a base URL"
        }
        AiProviderKind::Process if provider.command.trim().is_empty() => {
            "AI process provider is missing a command"
        }
        _ if prompt.is_empty() => "Open AI chat",
        _ => "Send this prompt to the configured AI provider",
    };
    let item = CatalogItem::new("quick.ai_prompt", label, ItemCategory::Ai, "quick")
        .subtitle(subtitle)
        .keywords([
            "ai",
            "ask",
            "chat",
            "assistant",
            "kyrphina",
            "llama",
            "pike",
            "pylon",
            "npu",
            "minicpm",
        ])
        .action(Action::ai_prompt())
        .score_boost(1800);

    Some(SearchResult { item, score: 2600 })
}

fn ai_prompt_text(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lowered = trimmed.to_ascii_lowercase();
    for prefix in [
        "ai", "ask", "chat", "kyrphina", "llama", "pike", "pylon", "npu", "minicpm", "minicpm5",
    ] {
        if lowered == prefix {
            return Some(String::new());
        }
        if lowered.starts_with(prefix)
            && trimmed[prefix.len()..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            return Some(trimmed[prefix.len()..].trim().to_string());
        }
    }

    None
}

fn ai_error_is_context_exceeded(provider: &AiProvider, error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();

    for marker in &provider.context_overflow_markers {
        if !marker.is_empty() && lowered.contains(&marker.to_ascii_lowercase()) {
            return true;
        }
    }

    lowered.contains("context size")
        || lowered.contains("context length")
        || lowered.contains("sequence length")
        || lowered.contains("max tokens")
        || lowered.contains("token limit")
        || lowered.contains("kv cache")
        || lowered.contains("prompt too long")
        || lowered.contains("input too long")
        || lowered.contains("exceeds maximum length")
        || (lowered.contains("exceeded") && lowered.contains("context"))
        || (lowered.contains("overflow")
            && (lowered.contains("context")
                || lowered.contains("sequence")
                || lowered.contains("tokens")))
}

fn looks_like_calculation(expression: &str) -> bool {
    if expression.len() < 3 {
        return false;
    }

    let mut has_digit = false;
    let mut has_operator = false;
    for ch in expression.chars() {
        if ch.is_ascii_digit() {
            has_digit = true;
            continue;
        }
        if matches!(
            ch,
            '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')' | '.' | ' '
        ) {
            if matches!(ch, '+' | '-' | '*' | '/' | '%' | '^') {
                has_operator = true;
            }
            continue;
        }
        return false;
    }

    has_digit && has_operator
}

fn evaluate_expression(expression: &str) -> Option<f64> {
    let mut parser = ExpressionParser::new(expression);
    let value = parser.parse_expression()?;
    parser.skip_whitespace();
    (parser.is_finished() && value.is_finite()).then_some(value)
}

fn format_number(value: f64) -> String {
    if (value.fract()).abs() < 0.000_000_001 {
        return format!("{value:.0}");
    }

    let mut rendered = format!("{value:.8}");
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    rendered
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitCategory {
    Length,
    Mass,
    Temperature,
    Data,
    Time,
}

fn parse_unit_conversion(query: &str) -> Option<(f64, String, String)> {
    let trimmed = query.trim();
    let (left, right) = if let Some(split) = trimmed.rsplit_once(" to ") {
        (split.0, split.1)
    } else if let Some(split) = trimmed.rsplit_once(" in ") {
        (split.0, split.1)
    } else {
        return None;
    };

    let left = left.trim();
    let right = right.trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }

    let (value, from_unit) = split_number_and_unit(left)?;
    let to_unit = right.to_ascii_lowercase();
    Some((value, from_unit, to_unit))
}

fn split_number_and_unit(text: &str) -> Option<(f64, String)> {
    let text = text.trim();
    let split_pos = text
        .chars()
        .position(|ch| ch.is_ascii_alphabetic() || ch == '\u{00b0}');
    let split_pos = split_pos?;
    let (number_part, unit_part) = text.split_at(split_pos);
    let value = number_part.trim().parse::<f64>().ok()?;
    let unit = unit_part.trim().to_ascii_lowercase();
    Some((value, unit))
}

fn normalize_unit(unit: &str) -> Option<(UnitCategory, &'static str)> {
    match unit {
        "m" | "meter" | "meters" | "metre" | "metres" => Some((UnitCategory::Length, "m")),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => {
            Some((UnitCategory::Length, "km"))
        }
        "cm" | "centimeter" | "centimeters" => Some((UnitCategory::Length, "cm")),
        "mm" | "millimeter" | "millimeters" => Some((UnitCategory::Length, "mm")),
        "mi" | "mile" | "miles" => Some((UnitCategory::Length, "mi")),
        "ft" | "foot" | "feet" => Some((UnitCategory::Length, "ft")),
        "in" | "inch" | "inches" => Some((UnitCategory::Length, "in")),
        "yd" | "yard" | "yards" => Some((UnitCategory::Length, "yd")),
        "kg" | "kilogram" | "kilograms" => Some((UnitCategory::Mass, "kg")),
        "g" | "gram" | "grams" => Some((UnitCategory::Mass, "g")),
        "mg" | "milligram" | "milligrams" => Some((UnitCategory::Mass, "mg")),
        "lb" | "lbs" | "pound" | "pounds" => Some((UnitCategory::Mass, "lb")),
        "oz" | "ounce" | "ounces" => Some((UnitCategory::Mass, "oz")),
        "st" | "stone" | "stones" => Some((UnitCategory::Mass, "st")),
        "c" | "\u{00b0}c" | "celsius" => Some((UnitCategory::Temperature, "c")),
        "f" | "\u{00b0}f" | "fahrenheit" => Some((UnitCategory::Temperature, "f")),
        "k" | "kelvin" => Some((UnitCategory::Temperature, "k")),
        "b" | "byte" | "bytes" => Some((UnitCategory::Data, "b")),
        "kb" | "kilobyte" | "kilobytes" => Some((UnitCategory::Data, "kb")),
        "mb" | "megabyte" | "megabytes" => Some((UnitCategory::Data, "mb")),
        "gb" | "gigabyte" | "gigabytes" => Some((UnitCategory::Data, "gb")),
        "tb" | "terabyte" | "terabytes" => Some((UnitCategory::Data, "tb")),
        "pb" | "petabyte" | "petabytes" => Some((UnitCategory::Data, "pb")),
        "kib" => Some((UnitCategory::Data, "kib")),
        "mib" => Some((UnitCategory::Data, "mib")),
        "gib" => Some((UnitCategory::Data, "gib")),
        "tib" => Some((UnitCategory::Data, "tib")),
        "s" | "sec" | "second" | "seconds" => Some((UnitCategory::Time, "s")),
        "min" | "minute" | "minutes" => Some((UnitCategory::Time, "min")),
        "h" | "hr" | "hour" | "hours" => Some((UnitCategory::Time, "h")),
        "d" | "day" | "days" => Some((UnitCategory::Time, "d")),
        "wk" | "week" | "weeks" => Some((UnitCategory::Time, "wk")),
        _ => None,
    }
}

fn to_base_value(value: f64, unit: &str, category: UnitCategory) -> Option<f64> {
    match category {
        UnitCategory::Length => Some(
            value
                * match unit {
                    "m" => 1.0,
                    "km" => 1000.0,
                    "cm" => 0.01,
                    "mm" => 0.001,
                    "mi" => 1609.344,
                    "ft" => 0.3048,
                    "in" => 0.0254,
                    "yd" => 0.9144,
                    _ => return None,
                },
        ),
        UnitCategory::Mass => Some(
            value
                * match unit {
                    "kg" => 1.0,
                    "g" => 0.001,
                    "mg" => 0.000_001,
                    "lb" => 0.45359237,
                    "oz" => 0.0283495,
                    "st" => 6.35029,
                    _ => return None,
                },
        ),
        UnitCategory::Data => Some(
            value
                * match unit {
                    "b" => 1.0,
                    "kb" => 1_000.0,
                    "mb" => 1_000_000.0,
                    "gb" => 1_000_000_000.0,
                    "tb" => 1_000_000_000_000.0,
                    "pb" => 1_000_000_000_000_000.0,
                    "kib" => 1024.0,
                    "mib" => 1_048_576.0,
                    "gib" => 1_073_741_824.0,
                    "tib" => 1_099_511_627_776.0,
                    _ => return None,
                },
        ),
        UnitCategory::Time => Some(
            value
                * match unit {
                    "s" => 1.0,
                    "min" => 60.0,
                    "h" => 3600.0,
                    "d" => 86400.0,
                    "wk" => 604_800.0,
                    _ => return None,
                },
        ),
        UnitCategory::Temperature => match unit {
            "c" => Some(value),
            "f" => Some((value - 32.0) * 5.0 / 9.0),
            "k" => Some(value - 273.15),
            _ => None,
        },
    }
}

fn from_base_value(base: f64, unit: &str, category: UnitCategory) -> Option<f64> {
    match category {
        UnitCategory::Length => Some(
            base / match unit {
                "m" => 1.0,
                "km" => 1000.0,
                "cm" => 0.01,
                "mm" => 0.001,
                "mi" => 1609.344,
                "ft" => 0.3048,
                "in" => 0.0254,
                "yd" => 0.9144,
                _ => return None,
            },
        ),
        UnitCategory::Mass => Some(
            base / match unit {
                "kg" => 1.0,
                "g" => 0.001,
                "mg" => 0.000_001,
                "lb" => 0.45359237,
                "oz" => 0.0283495,
                "st" => 6.35029,
                _ => return None,
            },
        ),
        UnitCategory::Data => Some(
            base / match unit {
                "b" => 1.0,
                "kb" => 1_000.0,
                "mb" => 1_000_000.0,
                "gb" => 1_000_000_000.0,
                "tb" => 1_000_000_000_000.0,
                "pb" => 1_000_000_000_000_000.0,
                "kib" => 1024.0,
                "mib" => 1_048_576.0,
                "gib" => 1_073_741_824.0,
                "tib" => 1_099_511_627_776.0,
                _ => return None,
            },
        ),
        UnitCategory::Time => Some(
            base / match unit {
                "s" => 1.0,
                "min" => 60.0,
                "h" => 3600.0,
                "d" => 86400.0,
                "wk" => 604_800.0,
                _ => return None,
            },
        ),
        UnitCategory::Temperature => match unit {
            "c" => Some(base),
            "f" => Some(base * 9.0 / 5.0 + 32.0),
            "k" => Some(base + 273.15),
            _ => None,
        },
    }
}

fn convert_units(value: f64, from: &str, to: &str) -> Option<f64> {
    let (from_category, from_unit) = normalize_unit(from)?;
    let (to_category, to_unit) = normalize_unit(to)?;
    if from_category != to_category {
        return None;
    }
    let base = to_base_value(value, from_unit, from_category)?;
    from_base_value(base, to_unit, to_category)
}

struct ExpressionParser<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> ExpressionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }

    fn parse_expression(&mut self) -> Option<f64> {
        let mut value = self.parse_term()?;
        loop {
            self.skip_whitespace();
            if self.consume('+') {
                value += self.parse_term()?;
            } else if self.consume('-') {
                value -= self.parse_term()?;
            } else {
                return Some(value);
            }
        }
    }

    fn parse_term(&mut self) -> Option<f64> {
        let mut value = self.parse_power()?;
        loop {
            self.skip_whitespace();
            if self.consume('*') {
                value *= self.parse_power()?;
            } else if self.consume('/') {
                let divisor = self.parse_power()?;
                if divisor.abs() < f64::EPSILON {
                    return None;
                }
                value /= divisor;
            } else if self.consume('%') {
                let divisor = self.parse_power()?;
                if divisor.abs() < f64::EPSILON {
                    return None;
                }
                value %= divisor;
            } else {
                return Some(value);
            }
        }
    }

    fn parse_power(&mut self) -> Option<f64> {
        let base = self.parse_factor()?;
        self.skip_whitespace();
        if self.consume('^') {
            let exponent = self.parse_power()?;
            Some(base.powf(exponent))
        } else {
            Some(base)
        }
    }

    fn parse_factor(&mut self) -> Option<f64> {
        self.skip_whitespace();
        if self.consume('-') {
            return Some(-self.parse_factor()?);
        }
        if self.consume('+') {
            return self.parse_factor();
        }
        if self.consume('(') {
            let value = self.parse_expression()?;
            self.skip_whitespace();
            return self.consume(')').then_some(value);
        }
        self.parse_number()
    }

    fn parse_number(&mut self) -> Option<f64> {
        self.skip_whitespace();
        let start = self.cursor;
        let mut has_digit = false;
        let mut has_dot = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                has_digit = true;
                self.cursor += ch.len_utf8();
            } else if ch == '.' && !has_dot {
                has_dot = true;
                self.cursor += ch.len_utf8();
            } else {
                break;
            }
        }

        if !has_digit {
            return None;
        }

        self.input[start..self.cursor].parse::<f64>().ok()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.cursor += 1;
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.cursor..].chars().next()
    }

    fn is_finished(&self) -> bool {
        self.cursor >= self.input.len()
    }
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

fn expand_env_vars(value: &str) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let mut name = String::new();
            let mut closed = false;
            while let Some(&candidate) = chars.peek() {
                chars.next();
                if candidate == '%' {
                    closed = true;
                    break;
                }
                name.push(candidate);
            }

            if closed
                && !name.is_empty()
                && let Ok(replacement) = std::env::var(&name)
            {
                expanded.push_str(&replacement);
            } else {
                expanded.push('%');
                expanded.push_str(&name);
                if closed {
                    expanded.push('%');
                }
            }
            continue;
        }

        if ch == '$' {
            if chars.peek() == Some(&'{') {
                chars.next();
                let mut name = String::new();
                let mut closed = false;
                while let Some(&candidate) = chars.peek() {
                    chars.next();
                    if candidate == '}' {
                        closed = true;
                        break;
                    }
                    name.push(candidate);
                }

                if closed
                    && !name.is_empty()
                    && let Ok(replacement) = std::env::var(&name)
                {
                    expanded.push_str(&replacement);
                } else {
                    expanded.push_str("${");
                    expanded.push_str(&name);
                    if closed {
                        expanded.push('}');
                    }
                }
                continue;
            }

            let mut name = String::new();
            while let Some(&candidate) = chars.peek() {
                if candidate == '_' || candidate.is_ascii_alphanumeric() {
                    chars.next();
                    name.push(candidate);
                } else {
                    break;
                }
            }

            if !name.is_empty()
                && let Ok(replacement) = std::env::var(&name)
            {
                expanded.push_str(&replacement);
            } else {
                expanded.push('$');
                expanded.push_str(&name);
            }
            continue;
        }

        if ch == '~'
            && expanded.is_empty()
            && matches!(chars.peek(), None | Some('/') | Some('\\'))
            && let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
        {
            expanded.push_str(&home);
            continue;
        }

        expanded.push(ch);
    }

    expanded
}

fn enabled_ai_provider_for_query<'a>(
    config: &'a VeyraConfig,
    query: &str,
) -> Option<&'a AiProvider> {
    if !config.ai.enabled {
        return None;
    }

    let requested_id = requested_ai_provider_id(query);
    if let Some(id) = requested_id {
        return find_enabled_ai_provider(config, id);
    }

    let default_id = config.ai.default_provider.trim();
    if !default_id.is_empty()
        && let Some(provider) = find_enabled_ai_provider(config, default_id)
    {
        return Some(provider);
    }

    config
        .ai
        .providers
        .iter()
        .find(|provider| provider_is_runnable(provider))
}

fn ai_warmup_provider(config: &VeyraConfig) -> Option<&AiProvider> {
    if !config.ai.enabled || !config.ai.warmup_on_startup {
        return None;
    }

    let default_id = config.ai.default_provider.trim();
    if !default_id.is_empty()
        && let Some(provider) = find_enabled_ai_provider(config, default_id)
        && provider.kind == AiProviderKind::Process
        && provider.keep_warm
    {
        return Some(provider);
    }

    config.ai.providers.iter().find(|provider| {
        provider.kind == AiProviderKind::Process
            && provider.keep_warm
            && provider_is_runnable(provider)
    })
}

fn requested_ai_provider_id(query: &str) -> Option<&'static str> {
    let trimmed = query.trim_start().to_ascii_lowercase();
    if trimmed == "llama" || trimmed.starts_with("llama ") {
        Some("llama")
    } else if trimmed == "pike" || trimmed.starts_with("pike ") {
        Some("pike")
    } else if trimmed == "pylon" || trimmed.starts_with("pylon ") {
        Some("pylon")
    } else if trimmed == "npu"
        || trimmed.starts_with("npu ")
        || trimmed == "minicpm"
        || trimmed.starts_with("minicpm ")
        || trimmed == "minicpm5"
        || trimmed.starts_with("minicpm5 ")
    {
        Some("minicpm5_npu")
    } else {
        None
    }
}

fn find_enabled_ai_provider<'a>(config: &'a VeyraConfig, id: &str) -> Option<&'a AiProvider> {
    config
        .ai
        .providers
        .iter()
        .find(|provider| provider.id.eq_ignore_ascii_case(id) && provider_is_runnable(provider))
}

fn provider_is_runnable(provider: &AiProvider) -> bool {
    provider.enabled
        && match provider.kind {
            AiProviderKind::OpenAiCompatible => {
                !provider.base_url.trim().is_empty() && !provider.model.trim().is_empty()
            }
            AiProviderKind::Process => !provider.command.trim().is_empty(),
        }
}

fn ai_provider_label(provider: &AiProvider) -> String {
    non_empty(&provider.label)
        .or_else(|| non_empty(&provider.id))
        .unwrap_or_else(|| "AI".to_string())
}

fn effective_font_size(config: &VeyraConfig) -> f32 {
    config.appearance.font_size.clamp(12, 22) as f32
}

fn preview_heading_size(config: &VeyraConfig) -> f32 {
    (effective_font_size(config) + 2.0).min(20.0)
}

fn effective_max_results(config: &VeyraConfig) -> usize {
    if config.appearance.max_results == 0 {
        return 10;
    }

    config.appearance.max_results.clamp(4, 24) as usize
}

fn alpha_for_opacity(config: &VeyraConfig, max_alpha: u8) -> u8 {
    (f32::from(max_alpha) * config.appearance.opacity.clamp(0.35, 1.0)).round() as u8
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SelectionDirection {
    Up,
    Down,
}

/// Steps the keyboard-selected row in `direction`, clamping it back inside the
/// visible result range. `shown_count` is the number of rows actually rendered.
/// When the list is empty the selection stays at 0.
fn step_selection(direction: SelectionDirection, selected: usize, shown_count: usize) -> usize {
    if shown_count == 0 {
        return 0;
    }
    let clamped = selected.min(shown_count - 1);
    match direction {
        SelectionDirection::Up => clamped.saturating_sub(1),
        SelectionDirection::Down => (clamped + 1).min(shown_count - 1),
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

fn ai_response_title(response: &AiResponse) -> String {
    match &response.result {
        AiResponseResult::Pending => format!("Asking {}", response.provider_label),
        AiResponseResult::Answer(_) if !response.tool_suggestions.is_empty() => {
            "Action captured".to_string()
        }
        AiResponseResult::Answer(_) => "Answer captured".to_string(),
        AiResponseResult::Error(_) => "AI request failed".to_string(),
    }
}

fn ai_response_subtitle(response: &AiResponse) -> String {
    let mut subtitle = format!(
        "{} - {}",
        response.provider_label,
        ai_response_title(response)
    );
    if let Some(elapsed_ms) = response.elapsed_ms {
        subtitle.push_str(&format!(" - {} ms", elapsed_ms));
    }
    subtitle
}

fn ai_request_info(
    provider: &AiProvider,
    indexed_tools: usize,
    tool_context_items: usize,
    message_context_items: usize,
    estimated_context_tokens: usize,
) -> AiRequestInfo {
    AiRequestInfo {
        provider_kind: provider.kind,
        model_label: ai_model_label(provider),
        indexed_tools,
        tool_context_items,
        message_context_items,
        estimated_context_tokens,
        context_limit_tokens: ai_provider_context_limit(provider),
        provider_supports_tools: provider.supports_tools,
        native_tool_calls_enabled: false,
        parsed_tool_calls_enabled: indexed_tools > 0,
    }
}

fn evaluate_ai_response(response: &AiResponse) -> AiEvalReport {
    let mut checks = Vec::new();

    match &response.result {
        AiResponseResult::Pending => {
            checks.push(ai_eval_check(
                "completed",
                false,
                "response is still pending",
            ));
        }
        AiResponseResult::Error(error) => {
            checks.push(ai_eval_check("completed", false, error.trim()));
            let fallback_provider = AiProvider::default();
            checks.push(ai_eval_check(
                "context_budget",
                !ai_error_is_context_exceeded(&fallback_provider, error),
                if ai_error_is_context_exceeded(&fallback_provider, error) {
                    "provider reported context size exceeded"
                } else {
                    "no context size error reported"
                },
            ));
        }
        AiResponseResult::Answer(answer) => {
            let display_text = ai_answer_display_text(answer);
            checks.push(ai_eval_check("completed", true, "response completed"));
            checks.push(ai_eval_check(
                "visible_answer",
                !display_text.trim().is_empty(),
                if display_text.trim().is_empty() {
                    "answer display text is empty"
                } else {
                    "answer has visible text"
                },
            ));
            checks.push(ai_eval_check(
                "raw_xml_hidden",
                !display_text.contains("<function"),
                if display_text.contains("<function") {
                    "display text still contains a raw function call"
                } else {
                    "display text hides raw function call markup"
                },
            ));
            let fallback_provider = AiProvider::default();
            checks.push(ai_eval_check(
                "context_budget",
                !ai_error_is_context_exceeded(&fallback_provider, answer),
                if ai_error_is_context_exceeded(&fallback_provider, answer) {
                    "answer contains a context size error"
                } else {
                    "no context size error in answer"
                },
            ));

            if let Some(expected_year) = expected_clock_answer_year(&response.prompt) {
                let year_text = expected_year.to_string();
                checks.push(ai_eval_check(
                    "live_clock_year",
                    display_text.contains(&year_text),
                    if display_text.contains(&year_text) {
                        format!("clock/date answer contains {year_text}")
                    } else {
                        format!("clock/date answer is missing current year {year_text}")
                    },
                ));
            }

            let prompt_lower = response.prompt.to_ascii_lowercase();
            let answer_lower = display_text.to_ascii_lowercase();
            if looks_like_timezone_question(&prompt_lower) {
                let names_zone = local_time_zone_info().is_some_and(|info| {
                    let name = info.name.to_ascii_lowercase();
                    !name.trim().is_empty() && answer_lower.contains(&name)
                });
                let passed = (answer_lower.contains("time zone")
                    || answer_lower.contains("timezone"))
                    && (answer_lower.contains("utc") || names_zone);
                checks.push(ai_eval_check(
                    "timezone_intent",
                    passed,
                    if passed {
                        "timezone answer names a concrete time zone"
                    } else {
                        "timezone answer did not name a concrete time zone"
                    },
                ));
            }
            if looks_like_location_question(&prompt_lower) {
                let passed =
                    answer_lower.contains("cannot") && answer_lower.contains("physical location");
                checks.push(ai_eval_check(
                    "location_privacy",
                    passed,
                    if passed {
                        "location answer states physical location is not available"
                    } else {
                        "location answer did not state that physical location is unavailable"
                    },
                ));
            }
        }
    }

    if response.tool_suggestions.is_empty() {
        checks.push(ai_eval_check("tool_calls", true, "no tool call requested"));
    } else {
        let resolved = response
            .tool_suggestions
            .iter()
            .filter(|suggestion| suggestion.result.is_some())
            .count();
        let total = response.tool_suggestions.len();
        checks.push(ai_eval_check(
            "tool_calls",
            resolved == total,
            format!("{resolved}/{total} tool call(s) resolved to executable launcher actions"),
        ));
    }

    ai_eval_report(checks)
}

fn expected_clock_answer_year(prompt: &str) -> Option<i32> {
    let lowered = prompt.to_ascii_lowercase();
    if !looks_like_clock_question(&lowered) {
        return None;
    }

    if let Some(zone) = fixed_clock_zone_for_prompt(&lowered) {
        return Some(
            datetime_parts_for_fixed_offset(current_unix_timestamp_i64(), zone.offset_seconds).year,
        );
    }

    Some(current_local_datetime_parts().year)
}

fn ai_eval_check(name: impl Into<String>, passed: bool, detail: impl Into<String>) -> AiEvalCheck {
    AiEvalCheck {
        name: name.into(),
        passed,
        detail: detail.into(),
    }
}

fn ai_eval_report(checks: Vec<AiEvalCheck>) -> AiEvalReport {
    let passed_count = checks.iter().filter(|check| check.passed).count();
    let total = checks.len();
    let passed = passed_count == total;
    let status = if passed { "PASS" } else { "FAIL" };
    AiEvalReport {
        passed,
        summary: format!("{status} {passed_count}/{total} checks"),
        checks,
    }
}

fn ai_status_with_eval(status: &str, eval: &AiEvalReport) -> String {
    format!("{status} - {}", eval.summary)
}

fn ai_model_label(provider: &AiProvider) -> String {
    non_empty(&provider.model).unwrap_or_else(|| {
        Path::new(&provider.command)
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .or_else(|| non_empty(&provider.command))
            .unwrap_or_else(|| provider.id.clone())
    })
}

fn ai_provider_context_limit(provider: &AiProvider) -> Option<usize> {
    if let Some(limit) = provider.context_limit_tokens
        && limit > 0
    {
        return Some(limit);
    }

    if let Some(limit) = ai_process_bundle_context_limit(provider) {
        return Some(limit);
    }

    let mut text = String::new();
    text.push_str(&provider.id);
    text.push(' ');
    text.push_str(&provider.label);
    text.push(' ');
    text.push_str(&provider.model);
    text.push(' ');
    text.push_str(&provider.command);
    for arg in &provider.args {
        text.push(' ');
        text.push_str(arg);
    }

    parse_context_limit_from_text(&text)
}

fn ai_process_bundle_context_limit(provider: &AiProvider) -> Option<usize> {
    if provider.kind != AiProviderKind::Process {
        return None;
    }

    for arg in &provider.args {
        if arg.trim_start().starts_with("--") {
            continue;
        }
        let bundle = PathBuf::from(expand_env_vars(arg));
        let config_path = bundle.join("genie_config.json");
        if !config_path.exists() {
            continue;
        }
        let raw = fs::read_to_string(config_path).ok()?;
        let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
        let size = value
            .get("dialog")
            .and_then(|dialog| dialog.get("context"))
            .and_then(|context| context.get("size"))
            .and_then(|size| size.as_u64())?;
        if (128..=262_144).contains(&(size as usize)) {
            return Some(size as usize);
        }
    }

    None
}

fn ai_context_message_limit_for_provider(provider: &AiProvider) -> usize {
    if ai_provider_needs_compact_prompt(provider) {
        return 2;
    }

    AI_CONTEXT_MESSAGE_LIMIT
}

fn ai_provider_needs_compact_prompt(provider: &AiProvider) -> bool {
    ai_provider_context_limit(provider).is_some_and(|limit| limit <= AI_COMPACT_CONTEXT_THRESHOLD)
}

fn ai_provider_prompt_fits(provider: &AiProvider, prompt: &str) -> bool {
    let Some(limit) = ai_provider_context_limit(provider) else {
        return true;
    };
    estimate_ai_provider_prompt_tokens(provider, prompt) <= ai_prompt_budget_tokens(limit)
}

fn ai_prompt_budget_tokens(limit: usize) -> usize {
    let reserve = if limit <= AI_COMPACT_CONTEXT_THRESHOLD {
        AI_COMPACT_GENERATION_RESERVE_TOKENS
    } else {
        AI_GENERATION_RESERVE_TOKENS
    };
    limit.saturating_sub(reserve).max(limit / 2).max(64)
}

fn estimate_ai_provider_prompt_tokens(provider: &AiProvider, prompt: &str) -> usize {
    let provider_prompt = if provider.kind == AiProviderKind::Process {
        format_process_ai_prompt(provider, prompt)
    } else {
        prompt.to_string()
    };
    estimate_token_count(&provider_prompt)
}

fn trim_text_to_provider_budget(provider: &AiProvider, text: &str) -> String {
    let Some(limit) = ai_provider_context_limit(provider) else {
        return text.trim().to_string();
    };
    let max_chars = ai_prompt_budget_tokens(limit).saturating_mul(3).max(96);
    truncate_for_label(text, max_chars)
}

fn parse_context_limit_from_text(text: &str) -> Option<usize> {
    let lowered = text.to_ascii_lowercase();
    let bytes = lowered.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'c' && bytes[index + 1] == b'l' {
            let mut digit_index = index + 2;
            let start = digit_index;
            while digit_index < bytes.len() && bytes[digit_index].is_ascii_digit() {
                digit_index += 1;
            }
            if digit_index > start
                && let Ok(limit) = lowered[start..digit_index].parse::<usize>()
                && (128..=262_144).contains(&limit)
            {
                return Some(limit);
            }
            index = digit_index.max(index + 2);
        } else {
            index += 1;
        }
    }

    None
}

fn ai_provider_kind_label(kind: AiProviderKind) -> &'static str {
    match kind {
        AiProviderKind::OpenAiCompatible => "HTTP",
        AiProviderKind::Process => "Process",
    }
}

fn ai_tool_status_label(request: &AiRequestInfo) -> String {
    if request.native_tool_calls_enabled {
        return format!("Tools native / {}", request.indexed_tools);
    }
    if request.parsed_tool_calls_enabled {
        return format!(
            "Tool calls parsed {} / {}",
            request.tool_context_items, request.indexed_tools
        );
    }
    if request.provider_supports_tools {
        return format!(
            "Tools off {} / {}",
            request.tool_context_items, request.indexed_tools
        );
    }
    if request.indexed_tools > 0 {
        return format!(
            "Tools context {} / {}",
            request.tool_context_items, request.indexed_tools
        );
    }

    "Tools none".to_string()
}

fn render_ai_request_meta(ui: &mut egui::Ui, request: &AiRequestInfo, include_tool_status: bool) {
    ui.horizontal_wrapped(|ui| {
        ai_meta_pill(
            ui,
            &format!(
                "{} / {}",
                ai_provider_kind_label(request.provider_kind),
                short_ai_model_label(&request.model_label)
            ),
        );
        ai_meta_pill(ui, &ai_context_status_label(request));
    });
    if include_tool_status {
        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            ai_meta_pill(ui, &ai_tool_status_label(request));
        });
    }
}

fn short_ai_model_label(model: &str) -> String {
    model
        .rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(model)
        .trim()
        .to_string()
}

fn render_ai_eval_meta(ui: &mut egui::Ui, eval: &AiEvalReport) {
    ui.horizontal_wrapped(|ui| {
        ai_eval_meta_pill(ui, &eval.summary, eval.passed);
        if !eval.passed {
            for check in eval.checks.iter().filter(|check| !check.passed).take(2) {
                ai_meta_pill(ui, &format!("{} failed", check.name));
            }
        }
    });
}

fn ai_context_status_label(request: &AiRequestInfo) -> String {
    if let Some(limit) = request.context_limit_tokens {
        return format_context_status(
            request.estimated_context_tokens,
            Some(limit),
            request.message_context_items,
        );
    }

    format_context_status(
        request.estimated_context_tokens,
        None,
        request.message_context_items,
    )
}

fn format_context_status(estimated_tokens: usize, limit: Option<usize>, messages: usize) -> String {
    let estimated = compact_token_count(estimated_tokens);
    let message_label = if messages == 1 { "msg" } else { "msgs" };
    if let Some(limit) = limit {
        return format!(
            "Context {estimated}/{} / {messages} {message_label}",
            compact_token_count(limit)
        );
    }

    format!("Context {estimated} / {messages} {message_label}")
}

fn compact_token_count(tokens: usize) -> String {
    if tokens >= 1_000 {
        let whole = tokens / 1_000;
        let decimal = (tokens % 1_000) / 100;
        if decimal == 0 {
            format!("{whole}k")
        } else {
            format!("{whole}.{decimal}k")
        }
    } else {
        tokens.to_string()
    }
}

fn ai_eval_meta_pill(ui: &mut egui::Ui, text: &str, passed: bool) {
    let fill = if passed {
        Color32::from_rgba_unmultiplied(142, 210, 132, 38)
    } else {
        Color32::from_rgba_unmultiplied(238, 108, 94, 44)
    };
    let text_color = if passed {
        Color32::from_rgb(174, 232, 166)
    } else {
        Color32::from_rgb(255, 173, 160)
    };
    Frame::new()
        .fill(fill)
        .corner_radius(5)
        .inner_margin(Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).strong().color(text_color));
        });
}

fn ai_toolbar_button(ui: &mut egui::Ui, label: &str, hover: &str) -> egui::Response {
    ui.add_sized(
        [50.0, 26.0],
        egui::Button::new(RichText::new(label).size(12.0)),
    )
    .on_hover_text(hover)
}

fn ai_meta_pill(ui: &mut egui::Ui, text: &str) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 12))
        .corner_radius(5)
        .inner_margin(Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(11.0)
                    .color(Color32::from_rgb(154, 166, 174)),
            );
        });
}

fn estimate_token_count(text: &str) -> usize {
    let non_whitespace = text.chars().filter(|ch| !ch.is_whitespace()).count();
    let words = text.split_whitespace().count();
    non_whitespace.div_ceil(4).max(words).max(1)
}

fn extract_open_intent_query(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    let lowered = trimmed.to_ascii_lowercase();
    for prefix in [
        "please open ",
        "please run ",
        "please launch ",
        "can you open ",
        "could you open ",
        "can u open ",
        "open up ",
        "pull up ",
        "bring up ",
        "open ",
        "run ",
        "launch ",
        "start ",
        "execute ",
    ] {
        if lowered.starts_with(prefix) {
            return clean_agentic_target(&trimmed[prefix.len()..]);
        }
    }

    None
}

fn extract_web_search_intent_query(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    let lowered = trimmed.to_ascii_lowercase();
    for prefix in [
        "search the web for ",
        "search web for ",
        "web search for ",
        "search for ",
        "search ",
        "google ",
        "look up ",
        "find online ",
    ] {
        if lowered.starts_with(prefix) {
            return clean_agentic_target(&trimmed[prefix.len()..]);
        }
    }

    None
}

fn extract_copy_intent_text(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    let lowered = trimmed.to_ascii_lowercase();
    for prefix in [
        "copy to clipboard ",
        "copy this to clipboard ",
        "copy this ",
        "copy ",
    ] {
        if lowered.starts_with(prefix) {
            return clean_copy_text(&trimmed[prefix.len()..]);
        }
    }

    None
}

fn extract_calculation_intent_query(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    let lowered = trimmed.to_ascii_lowercase();
    for prefix in [
        "calculate ",
        "calculator ",
        "compute ",
        "solve ",
        "eval ",
        "evaluate ",
    ] {
        if lowered.starts_with(prefix) {
            let expression = trimmed[prefix.len()..].trim();
            return extract_calculation_expression(expression);
        }
    }

    None
}

fn clean_agentic_target(value: &str) -> Option<String> {
    let mut text = value
        .trim()
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | ':' | ';' | ',' | '.'))
        .trim();
    loop {
        let mut stripped = false;
        for prefix in ["the ", "my ", "app ", "program ", "application "] {
            if text
                .get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
            {
                text = text[prefix.len()..].trim();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    for suffix in [" for me", " please"] {
        if text
            .get(text.len().saturating_sub(suffix.len())..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
        {
            text = text[..text.len() - suffix.len()].trim();
            break;
        }
    }

    non_empty(text)
}

fn clean_copy_text(value: &str) -> Option<String> {
    let text = value
        .trim()
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`'))
        .trim();
    non_empty(text)
}

fn truncate_for_label(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut output = trimmed
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    output.push_str("...");
    output
}

fn copyable_ai_response_text(response: &AiResponse) -> Option<String> {
    match &response.result {
        AiResponseResult::Answer(answer) if !answer.trim().is_empty() => {
            Some(ai_answer_display_text(answer))
        }
        _ => None,
    }
}

fn render_ai_conversation_message(ui: &mut egui::Ui, message: &AiConversationMessage) {
    let (label, color) = ai_conversation_role_style(message.role);
    let is_user = message.role == AiConversationRole::User;
    let available_width = ui.available_width().max(220.0);
    let width_factor = if is_user { 0.68 } else { 0.86 };
    let bubble_width = (available_width * width_factor).clamp(180.0, available_width.min(560.0));
    let fill = match message.role {
        AiConversationRole::User => Color32::from_rgba_unmultiplied(40, 96, 92, 112),
        AiConversationRole::Assistant => Color32::from_rgba_unmultiplied(255, 255, 255, 11),
        AiConversationRole::System => Color32::from_rgba_unmultiplied(130, 48, 42, 96),
    };
    let stroke = match message.role {
        AiConversationRole::User => {
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(132, 232, 216, 54))
        }
        AiConversationRole::Assistant => {
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 18))
        }
        AiConversationRole::System => {
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 165, 150, 48))
        }
    };

    ui.horizontal(|ui| {
        if is_user {
            ui.add_space((available_width - bubble_width).max(0.0));
        }
        ui.allocate_ui_with_layout(
            Vec2::new(bubble_width, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                Frame::new()
                    .fill(fill)
                    .stroke(stroke)
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(11, 8))
                    .show(ui, |ui| {
                        ui.set_width((bubble_width - 22.0).max(120.0));
                        ui.label(RichText::new(label).size(11.0).strong().color(color));
                        ui.add_space(3.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&message.text)
                                    .size(13.0)
                                    .color(Color32::from_rgb(226, 233, 236)),
                            )
                            .wrap(),
                        );
                    });
            },
        );
    });
    ui.add_space(6.0);
}

fn render_ai_pending_message(ui: &mut egui::Ui) {
    let available_width = ui.available_width().max(220.0);
    let bubble_width = (available_width * 0.62).clamp(180.0, available_width.min(420.0));
    ui.allocate_ui_with_layout(
        Vec2::new(bubble_width, 0.0),
        Layout::top_down(Align::Min),
        |ui| {
            Frame::new()
                .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 11))
                .stroke(Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 18),
                ))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(11, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Thinking")
                                .size(13.0)
                                .color(Color32::from_rgb(164, 176, 184)),
                        );
                    });
                });
        },
    );
    ui.add_space(6.0);
}

fn ai_conversation_role_style(role: AiConversationRole) -> (&'static str, Color32) {
    let color = match role {
        AiConversationRole::User => Color32::from_rgb(132, 216, 228),
        AiConversationRole::Assistant => Color32::from_rgb(142, 210, 132),
        AiConversationRole::System => Color32::from_rgb(255, 165, 150),
    };
    (ai_conversation_role_label(role), color)
}

fn ai_conversation_role_label(role: AiConversationRole) -> &'static str {
    match role {
        AiConversationRole::User => "You",
        AiConversationRole::Assistant => "Veyra AI",
        AiConversationRole::System => "System",
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

fn result_action_hint(result: &SearchResult) -> &'static str {
    let Some(action) = result.item.actions.first() else {
        return "No action";
    };

    if action.run_as_admin {
        return "Enter: Run as admin";
    }

    match action.kind {
        ActionKind::AiPrompt => "Enter: Ask",
        ActionKind::OpenUrl => "Enter: Search",
        ActionKind::OpenFile => "Enter: Open",
        ActionKind::Launch => "Enter: Run",
        ActionKind::ShellCommand => "Enter: Run",
        ActionKind::ToolCall if action.id == COPY_TO_CLIPBOARD_ACTION_ID => "Enter: Copy",
        ActionKind::ToolCall => "Enter: Tool",
        ActionKind::AuroraIpc => "Enter: Wallpaper",
    }
}

fn category_color(item: &CatalogItem) -> (u8, u8, u8) {
    match item.category {
        veyra_core::ItemCategory::App => (112, 202, 190),
        veyra_core::ItemCategory::Command => (230, 180, 93),
        veyra_core::ItemCategory::File => (130, 166, 230),
        veyra_core::ItemCategory::Folder => (205, 162, 96),
        veyra_core::ItemCategory::Setting => (170, 145, 220),
        veyra_core::ItemCategory::System => (230, 128, 128),
        veyra_core::ItemCategory::Web => (103, 190, 230),
        veyra_core::ItemCategory::Ai => (142, 210, 132),
        veyra_core::ItemCategory::Tool => (210, 154, 118),
    }
}

fn category_marker(ui: &mut egui::Ui, item: &CatalogItem) {
    let (red, green, blue) = category_color(item);
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(red, green, blue, 210))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(4, 15))
        .show(ui, |ui| {
            ui.allocate_space(Vec2::new(2.0, 1.0));
        });
}

fn panel_fill(config: &VeyraConfig) -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(config, 13))
}

fn content_fill(config: &VeyraConfig) -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(config, 9))
}

fn subtle_stroke(config: &VeyraConfig) -> Stroke {
    Stroke::new(
        1.0,
        Color32::from_rgba_unmultiplied(255, 255, 255, alpha_for_opacity(config, 26)),
    )
}

fn settings_nav_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let color = if selected {
        Color32::from_rgb(238, 242, 245)
    } else {
        Color32::from_rgb(148, 159, 168)
    };
    ui.add_sized(
        [ui.available_width(), 26.0],
        egui::Button::new(RichText::new(label).size(13.0).color(color))
            .selected(selected)
            .frame(true),
    )
}

fn setting_row(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    let value = value.into();
    let needs_wrapping = value.chars().count() > 28
        || value.contains('\\')
        || value.contains('/')
        || value.contains(',');
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 8))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(10, if needs_wrapping { 6 } else { 5 }))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if needs_wrapping {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(label)
                            .size(11.0)
                            .color(Color32::from_rgb(148, 159, 168)),
                    );
                    ui.add_space(1.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(value)
                                .size(13.0)
                                .strong()
                                .color(Color32::from_rgb(196, 205, 214)),
                        )
                        .wrap(),
                    );
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(label)
                            .size(12.0)
                            .color(Color32::from_rgb(148, 159, 168)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(value)
                                .size(13.0)
                                .strong()
                                .color(Color32::from_rgb(196, 205, 214)),
                        );
                    });
                });
            }
        });
    ui.add_space(4.0);
}

fn ai_compose_reserved_height(compact: bool) -> f32 {
    if compact { 58.0 } else { 64.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_tools::AiToolParam;
    use crate::history::recent_launch_results_from;
    use crate::hotkeys::{normalize_global_hotkey, parse_global_hotkey, toggle_hotkey_candidates};
    use eframe::egui::Pos2;
    use std::time::{SystemTime, UNIX_EPOCH};
    use veyra_core::config::PluginEntry;
    use veyra_core::search;

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
        fs::write(
            profile.join("plugins.toml"),
            r#"
                [[plugins]]
                id = "plugin.test"
                label = "Plugin: Test"
                description = "Run plugin"
                command = "plugin.exe"
                args = ["--plugin"]
                keywords = ["plugin"]
            "#,
        )
        .unwrap();

        let (config, items, messages) = load_profile(&profile);

        assert!(!config.general.startup);
        assert_eq!(config.general.history_limit, 42);
        assert_eq!(config.appearance.theme, "test-theme");
        assert_eq!(config.commands.len(), 1);
        assert_eq!(config.web_search.len(), 1);
        assert_eq!(config.plugins.len(), 1);
        assert_eq!(items.len(), 3);
        assert!(items.iter().any(|item| item.id == "plugin.test"));
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
    fn expands_env_vars_in_plugin_paths() {
        let path = std::env::var("PATH").expect("PATH is available for tests");

        assert_eq!(expand_env_vars("%PATH%"), path);
        assert_eq!(expand_env_vars("$PATH"), path);
        assert_eq!(expand_env_vars("${PATH}"), path);
        assert_eq!(
            expand_env_vars("%VEYRA_THIS_ENV_VAR_SHOULD_NOT_EXIST%"),
            "%VEYRA_THIS_ENV_VAR_SHOULD_NOT_EXIST%"
        );
        assert_eq!(expand_env_vars("progress 50%"), "progress 50%");
    }

    #[test]
    fn ai_prompt_result_requires_ai_prefix() {
        let provider = test_ai_provider("genie", "Kyrphina Genie", "http://127.0.0.1:8910/v1");
        let result = ai_prompt_search_result("ai what time is it", &provider).unwrap();
        let action = result.item.actions.first().unwrap();

        assert_eq!(result.item.category, ItemCategory::Ai);
        assert_eq!(result.item.source, "quick");
        assert_eq!(result.item.label, "Ask Kyrphina Genie: what time is it");
        assert_eq!(action.kind, ActionKind::AiPrompt);
        assert_eq!(ai_prompt_text("ask"), Some(String::new()));
        assert_eq!(
            ai_prompt_text("llama summarize this"),
            Some("summarize this".to_string())
        );
        assert_eq!(
            ai_prompt_text("pike review this"),
            Some("review this".to_string())
        );
        assert_eq!(
            ai_prompt_text("pylon summarize this"),
            Some("summarize this".to_string())
        );
        assert_eq!(
            ai_prompt_text("npu count to three"),
            Some("count to three".to_string())
        );
        assert_eq!(
            ai_prompt_text("minicpm5 be brief"),
            Some("be brief".to_string())
        );
        assert!(ai_prompt_search_result("weather tomorrow", &provider).is_none());
    }

    #[test]
    fn ai_provider_selection_uses_config_and_query_prefix() {
        let mut config = VeyraConfig::default();
        config.ai.enabled = true;
        config.ai.default_provider = "genie".to_string();
        config.ai.providers = vec![
            test_ai_provider("genie", "Kyrphina Genie", "http://127.0.0.1:8910/v1"),
            test_ai_provider("llama", "Kyrphina llama.cpp", "http://127.0.0.1:8080/v1"),
        ];
        let mut npu_provider = test_ai_provider("minicpm5_npu", "MiniCPM5 NPU", "");
        npu_provider.kind = AiProviderKind::Process;
        npu_provider.command = "npu_chat.exe".to_string();
        config.ai.providers.push(npu_provider);
        let mut pike_provider = test_ai_provider("pike", "Pike", "");
        pike_provider.kind = AiProviderKind::Process;
        pike_provider.command = "pike.exe".to_string();
        config.ai.providers.push(pike_provider);
        config.ai.providers.push(test_ai_provider(
            "pylon",
            "Pylon",
            "http://127.0.0.1:8088/v1",
        ));

        assert_eq!(
            enabled_ai_provider_for_query(&config, "ai what time is it")
                .unwrap()
                .id,
            "genie"
        );
        assert_eq!(
            enabled_ai_provider_for_query(&config, "llama what time is it")
                .unwrap()
                .id,
            "llama"
        );
        assert_eq!(
            enabled_ai_provider_for_query(&config, "npu what time is it")
                .unwrap()
                .id,
            "minicpm5_npu"
        );
        assert_eq!(
            enabled_ai_provider_for_query(&config, "pike review this")
                .unwrap()
                .id,
            "pike"
        );
        assert_eq!(
            enabled_ai_provider_for_query(&config, "pylon summarize this")
                .unwrap()
                .id,
            "pylon"
        );

        config.ai.enabled = false;
        assert!(enabled_ai_provider_for_query(&config, "ai what time is it").is_none());
    }

    #[test]
    fn explicit_ai_provider_prefix_does_not_fallback_to_default() {
        let mut config = VeyraConfig::default();
        config.ai.enabled = true;
        config.ai.default_provider = "minicpm5_npu".to_string();

        let mut npu_provider = test_ai_provider("minicpm5_npu", "MiniCPM5 NPU", "");
        npu_provider.kind = AiProviderKind::Process;
        npu_provider.command = "npu_chat.exe".to_string();

        let mut llama_provider = test_ai_provider("llama", "llama.cpp", "http://127.0.0.1:8080/v1");
        llama_provider.enabled = false;
        config.ai.providers = vec![npu_provider, llama_provider];

        assert_eq!(
            enabled_ai_provider_for_query(&config, "ai hello")
                .unwrap()
                .id,
            "minicpm5_npu"
        );
        assert!(enabled_ai_provider_for_query(&config, "llama hello").is_none());
    }

    #[test]
    fn ai_conversation_followup_reuses_session_provider() {
        let mut app = test_app_shell();
        app.config.ai.enabled = true;
        app.config.ai.default_provider = "genie".to_string();
        app.config.ai.providers = vec![
            test_ai_provider("genie", "Kyrphina Genie", "http://127.0.0.1:8910/v1"),
            test_ai_provider("llama", "Kyrphina llama.cpp", "http://127.0.0.1:8080/v1"),
        ];
        app.ai_session_provider_id = Some("llama".to_string());

        assert_eq!(app.active_ai_conversation_provider().unwrap().id, "llama");

        app.config.ai.providers[1].enabled = false;
        assert_eq!(app.active_ai_conversation_provider().unwrap().id, "genie");
    }

    #[test]
    fn general_local_only_blocks_remote_ai_provider() {
        let mut provider = test_ai_provider("remote", "Remote", "https://example.com/v1");
        provider.local_only = false;

        let mut app = test_app_shell();
        app.config.general.local_only = true;
        let local_only =
            app.config.general.local_only || app.config.ai.local_only || provider.local_only;
        let error = call_ai_provider(provider, "hello".to_string(), local_only).unwrap_err();

        assert!(error.contains("local_only"));
    }

    #[test]
    fn ai_request_info_reports_context_and_tool_mode() {
        let provider = test_ai_provider("local", "Local", "http://127.0.0.1:8080/v1");
        let info = ai_request_info(&provider, 4, 2, 3, 128);

        assert_eq!(info.model_label, "local:model");
        assert_eq!(info.indexed_tools, 4);
        assert_eq!(info.tool_context_items, 2);
        assert_eq!(info.message_context_items, 3);
        assert_eq!(info.estimated_context_tokens, 128);
        assert_eq!(info.context_limit_tokens, None);
        assert!(info.provider_supports_tools);
        assert!(!info.native_tool_calls_enabled);
        assert!(info.parsed_tool_calls_enabled);
        assert!(ai_tool_status_label(&info).contains("Tool calls parsed"));
    }

    #[test]
    fn ai_history_context_is_only_used_for_clear_followups() {
        assert!(!prompt_needs_conversation_context(
            "what are good npu model settings"
        ));
        assert!(!prompt_needs_conversation_context("what timezone am i"));
        assert!(!prompt_needs_conversation_context(
            "what about MiniCPM settings"
        ));
        assert!(prompt_needs_conversation_context("what about that one"));
        assert!(prompt_needs_conversation_context(
            "continue from the previous answer"
        ));

        let mut app = test_app_shell();
        app.ai_conversation_messages.push(AiConversationMessage {
            role: AiConversationRole::User,
            text: "what timezone am i".to_string(),
        });
        app.ai_conversation_messages.push(AiConversationMessage {
            role: AiConversationRole::Assistant,
            text: "Your system time zone is Pacific Standard Time.".to_string(),
        });
        let provider = test_ai_provider("local", "Local", "http://127.0.0.1:8080/v1");

        let independent = app.build_ai_model_prompt(&provider, "what is MiniCPM good at");
        assert_eq!(independent.message_context_items, 0);
        assert!(!independent.prompt.contains("Pacific Standard Time"));

        let followup = app.build_ai_model_prompt(&provider, "what about that model");
        assert_eq!(followup.message_context_items, 2);
        assert!(followup.prompt.contains("Pacific Standard Time"));

        let fresh_subject = app.build_ai_model_prompt(&provider, "what about MiniCPM settings");
        assert_eq!(fresh_subject.message_context_items, 0);
        assert!(!fresh_subject.prompt.contains("Pacific Standard Time"));
    }

    #[test]
    fn proactive_ai_intent_extractors_are_conservative() {
        assert_eq!(
            extract_open_intent_query("open WireGuard"),
            Some("WireGuard".to_string())
        );
        assert_eq!(
            extract_open_intent_query("can you open the app Firefox for me"),
            Some("Firefox".to_string())
        );
        assert_eq!(
            extract_web_search_intent_query("search web for flow launcher plugins"),
            Some("flow launcher plugins".to_string())
        );
        assert_eq!(
            extract_copy_intent_text("copy hello world"),
            Some("hello world".to_string())
        );
        assert_eq!(
            extract_calculation_intent_query("calculate 17 * 23"),
            Some("17 * 23".to_string())
        );
        assert!(extract_open_intent_query("what is WireGuard").is_none());
        assert!(extract_web_search_intent_query("what should I search for").is_none());
        assert!(extract_calculation_intent_query("what is 17 * 23").is_none());
    }

    #[test]
    fn token_estimate_tracks_words_and_characters() {
        assert_eq!(estimate_token_count("one two three"), 3);
        assert!(estimate_token_count("abcdefghijklmnopqrstuvwxyz") >= 6);
    }

    #[test]
    fn ai_context_limit_is_inferred_from_bundle_paths() {
        let mut provider = test_ai_provider("minicpm5_npu", "MiniCPM5 NPU", "");
        provider.kind = AiProviderKind::Process;
        provider.command = "npu_chat.exe".to_string();
        provider.args = vec![
            "%USERPROFILE%\\models\\qualcomm-genie\\minicpm5-1b\\bundle-cl512\\model".to_string(),
        ];

        assert_eq!(ai_provider_context_limit(&provider), Some(512));
        assert_eq!(
            parse_context_limit_from_text("qwen bundle token_ar1_cl4096"),
            Some(4096)
        );
    }

    #[test]
    fn ai_context_limit_prefers_explicit_provider_override() {
        let mut provider = test_ai_provider("minicpm5_npu", "MiniCPM5 NPU", "");
        provider.kind = AiProviderKind::Process;
        provider.context_limit_tokens = Some(4096);
        provider.args = vec!["C:\\bundle-cl512\\model".to_string()];

        assert_eq!(ai_provider_context_limit(&provider), Some(4096));
    }

    #[test]
    fn ai_context_limit_reads_genie_bundle_config() {
        let profile = temp_profile_dir();
        let bundle = profile.join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(
            bundle.join("genie_config.json"),
            r#"{"dialog":{"context":{"size":2048}}}"#,
        )
        .unwrap();

        let mut provider = test_ai_provider("npu", "NPU", "");
        provider.kind = AiProviderKind::Process;
        provider.args = vec![bundle.to_string_lossy().to_string()];

        assert_eq!(ai_provider_context_limit(&provider), Some(2048));

        fs::remove_dir_all(&profile).ok();
    }

    #[test]
    fn ai_provider_prompt_budget_counts_process_wrapper() {
        let mut provider = test_ai_provider("npu", "NPU", "");
        provider.kind = AiProviderKind::Process;
        provider.context_limit_tokens = Some(512);

        assert!(ai_provider_prompt_fits(&provider, "open wireguard"));
        assert!(!ai_provider_prompt_fits(&provider, &"word ".repeat(600)));
    }

    #[test]
    fn parses_minicpm_xml_function_calls() {
        let calls = parse_ai_function_calls(
            "Opening it.\n<function name=\"open_result\"><param name=\"query\">WireGuard</param></function>",
        );

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "open_result");
        assert_eq!(
            ai_tool_call_param(&calls[0], &["query"]),
            Some("WireGuard".to_string())
        );
    }

    #[test]
    fn parses_function_params_with_cdata_and_entities() {
        let calls = parse_ai_function_calls(
            "<function name='copy_to_clipboard'><param name='text'><![CDATA[A < B & C]]></param><param name=\"note\">&quot;ok&quot;</param></function>",
        );

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].params[0].value, "A < B & C");
        assert_eq!(calls[0].params[1].value, "\"ok\"");
    }

    #[test]
    fn parses_mixed_case_function_markup() {
        let raw = "Done.\n<Function Name=\"open_result\"><Param Name=\"Query\">WireGuard</Param></Function>";
        let calls = parse_ai_function_calls(raw);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "open_result");
        assert_eq!(
            ai_tool_call_param(&calls[0], &["query"]),
            Some("WireGuard".to_string())
        );
        assert_eq!(ai_answer_display_text(raw), "Done.");
    }

    #[test]
    fn resolves_additional_ai_tool_calls() {
        let app = test_app_shell();

        let calc = app.resolve_ai_tool_suggestion(AiToolCall {
            name: "calculate".to_string(),
            params: vec![AiToolParam {
                name: "expression".to_string(),
                value: "2 + 3 * 4".to_string(),
            }],
        });
        assert!(calc.result.is_some());
        assert!(calc.detail.contains("14"));

        let url = app.resolve_ai_tool_suggestion(AiToolCall {
            name: "open_url".to_string(),
            params: vec![AiToolParam {
                name: "url".to_string(),
                value: "example.com".to_string(),
            }],
        });
        assert_eq!(
            url.result
                .unwrap()
                .item
                .actions
                .first()
                .unwrap()
                .command
                .as_deref(),
            Some("https://example.com")
        );

        let time = app.resolve_ai_tool_suggestion(AiToolCall {
            name: "current_time".to_string(),
            params: vec![AiToolParam {
                name: "location".to_string(),
                value: "Tokyo".to_string(),
            }],
        });
        assert!(time.result.is_some());
        assert!(time.detail.contains("Tokyo"));
    }

    #[test]
    fn clipboard_context_detection_is_intentional() {
        assert!(prompt_requests_clipboard_context("fix this"));
        assert!(prompt_requests_clipboard_context("summarize selected text"));
        assert!(prompt_requests_clipboard_context(
            "use clipboard as context"
        ));
        assert!(!prompt_requests_clipboard_context("open wireguard"));
    }

    #[test]
    fn ai_display_text_hides_raw_function_call() {
        assert_eq!(
            ai_answer_display_text(
                "Sure.\n<function name=\"search\"><param name=\"query\">wireguard</param></function>"
            ),
            "Sure."
        );
        assert_eq!(
            ai_answer_display_text(
                "<function name=\"open_result\"><param name=\"query\">WireGuard</param></function>"
            ),
            "Suggested action below."
        );
    }

    #[test]
    fn ai_eval_passes_clean_answer_and_fails_context_error() {
        let clean = test_ai_response("say ok", AiResponseResult::Answer("ok".to_string()));
        let clean_eval = evaluate_ai_response(&clean);
        assert!(clean_eval.passed, "{clean_eval:?}");

        let failed = test_ai_response(
            "what time is it",
            AiResponseResult::Error(
                "AI process returned no answer: Context Size was exceeded".to_string(),
            ),
        );
        let failed_eval = evaluate_ai_response(&failed);
        assert!(!failed_eval.passed);
        assert!(
            failed_eval
                .checks
                .iter()
                .any(|check| check.name == "context_budget" && !check.passed)
        );
    }

    #[test]
    fn ai_eval_fails_unresolved_tool_call() {
        let raw =
            "<function name=\"open_result\"><param name=\"query\">Missing App</param></function>";
        let call = parse_ai_function_calls(raw).pop().unwrap();
        let mut response = test_ai_response(
            "open missing app",
            AiResponseResult::Answer(raw.to_string()),
        );
        response.tool_suggestions.push(AiToolSuggestion {
            call,
            label: "Open result".to_string(),
            detail: "No launcher result matched 'Missing App'".to_string(),
            result: None,
            query_context: Some("Missing App".to_string()),
        });

        let eval = evaluate_ai_response(&response);

        assert!(!eval.passed);
        assert!(
            eval.checks
                .iter()
                .any(|check| check.name == "tool_calls" && !check.passed)
        );
    }

    #[test]
    fn ai_chat_log_and_snapshot_are_written() {
        let profile = temp_profile_dir();
        let mut response = test_ai_response("say ok", AiResponseResult::Answer("ok".to_string()));
        let evaluation = evaluate_ai_response(&response);
        response.eval = Some(evaluation.clone());
        let conversation = vec![
            AiConversationMessage {
                role: AiConversationRole::User,
                text: "say ok".to_string(),
            },
            AiConversationMessage {
                role: AiConversationRole::Assistant,
                text: "ok".to_string(),
            },
        ];

        let log_path = append_ai_chat_log(&profile, &response, &conversation, &evaluation).unwrap();
        let raw_log = fs::read_to_string(&log_path).unwrap();
        let line = raw_log.lines().next().unwrap();
        let value = serde_json::from_str::<serde_json::Value>(line).unwrap();

        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["prompt"], "say ok");
        assert_eq!(value["evaluation"]["passed"], true);
        assert_eq!(value["conversation"].as_array().unwrap().len(), 2);

        let snapshot = save_ai_chat_snapshot(
            &profile,
            response.session_id,
            &conversation,
            Some(&response),
            Some(&evaluation),
        )
        .unwrap();
        let markdown = fs::read_to_string(snapshot).unwrap();

        assert!(markdown.contains("# Veyra AI Chat"));
        assert!(markdown.contains("Eval"));
        assert!(markdown.contains("Veyra AI"));

        fs::remove_dir_all(&profile).ok();
    }

    #[test]
    fn clock_answer_handles_tokyo_time_without_model() {
        let answer = clock_answer_for_prompt_at("hat time is it in tokyo?", 1_780_673_400).unwrap();

        assert!(answer.contains("12:30 AM"));
        assert!(answer.contains("Saturday, June 6, 2026"));
        assert!(answer.contains("Tokyo"));
        assert!(answer.contains("UTC+09:00"));
    }

    #[test]
    fn clock_answer_handles_wilmington_delaware_typo_without_model() {
        let answer =
            clock_answer_for_prompt_at("what time is it in wilmingotn Delware", 1_780_673_400)
                .unwrap();

        assert!(answer.contains("11:30 AM"));
        assert!(answer.contains("Friday, June 5, 2026"));
        assert!(answer.contains("Wilmington, Delaware"));
        assert!(answer.contains("EDT"));
        assert!(answer.contains("UTC-04:00"));
    }

    #[test]
    fn clock_answer_ignores_non_clock_questions() {
        assert!(clock_answer_for_prompt_at("open wireguard", 1_780_673_400).is_none());
    }

    #[test]
    fn clock_answer_ignores_day_inside_other_words() {
        assert!(clock_answer_for_prompt_at("open payday in japan", 1_780_673_400).is_none());
        assert!(clock_answer_for_prompt_at("what about today", 1_780_673_400).is_none());
    }

    #[test]
    fn local_clock_answer_handles_bare_current_year() {
        let answer = local_clock_answer_for_prompt_with_parts(
            "what year is it",
            DateTimeParts {
                year: 2026,
                month: 6,
                day: 5,
                hour: 9,
                minute: 7,
                weekday: 5,
            },
        )
        .unwrap();

        assert!(answer.contains("9:07 AM"));
        assert!(answer.contains("Friday, June 5, 2026"));
        assert!(answer.contains("your local time zone"));
    }

    #[test]
    fn deterministic_answer_handles_arithmetic_before_model() {
        assert_eq!(
            calculator_answer_for_prompt("Answer only the number: 17 * 23."),
            Some("391".to_string())
        );
        assert_eq!(
            deterministic_ai_answer("what is (2 + 3) * 4"),
            Some("20".to_string())
        );
        assert!(calculator_answer_for_prompt("Which is larger, 9.11 or 9.8?").is_none());
    }

    #[test]
    fn timezone_and_location_questions_are_deterministic() {
        let info = LocalTimeZoneInfo {
            name: "Pacific Standard Time".to_string(),
            offset_seconds: Some(-7 * 60 * 60),
        };

        let timezone =
            timezone_or_location_answer_for_prompt_with_info("what is my timezone", Some(&info))
                .unwrap();
        assert!(timezone.contains("Pacific Standard Time"));
        assert!(timezone.contains("UTC-07:00"));

        let location =
            timezone_or_location_answer_for_prompt_with_info("where am i", Some(&info)).unwrap();
        assert!(location.contains("cannot determine your physical location"));
        assert!(location.contains("Pacific Standard Time"));

        assert!(
            timezone_or_location_answer_for_prompt_with_info(
                "what time is it where i am",
                Some(&info)
            )
            .is_none()
        );
    }

    #[test]
    fn ai_eval_fails_location_prompt_that_only_answers_time() {
        let response = test_ai_response(
            "where am i",
            AiResponseResult::Answer(
                "It is 5:59 AM on Friday, June 5, 2026 in your local time zone.".to_string(),
            ),
        );

        let eval = evaluate_ai_response(&response);

        assert!(!eval.passed);
        assert!(
            eval.checks
                .iter()
                .any(|check| check.name == "location_privacy" && !check.passed)
        );
    }

    #[test]
    fn ai_eval_fails_timezone_prompt_that_only_answers_local_time() {
        let response = test_ai_response(
            "what is my timezone",
            AiResponseResult::Answer(
                "It is 5:59 AM on Friday, June 5, 2026 in your local time zone.".to_string(),
            ),
        );

        let eval = evaluate_ai_response(&response);

        assert!(!eval.passed);
        assert!(
            eval.checks
                .iter()
                .any(|check| check.name == "timezone_intent" && !check.passed)
        );
    }

    #[test]
    fn chat_completion_url_accepts_base_or_full_endpoint() {
        assert_eq!(
            chat_completions_url("127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8080/v1").unwrap(),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8080/v1/chat/completions").unwrap(),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
    }

    #[test]
    fn parses_chat_completion_answers() {
        let raw = r#"{
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "  answer text  "
                    }
                }
            ]
        }"#;

        assert_eq!(
            parse_chat_completion_answer(raw).unwrap(),
            "answer text".to_string()
        );
        assert!(parse_chat_completion_answer(r#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn parses_chat_completion_array_content() {
        let raw = r#"{
            "choices": [
                {
                    "message": {
                        "content": [
                            { "type": "text", "text": "first " },
                            { "type": "text", "text": "second" }
                        ]
                    }
                }
            ]
        }"#;

        assert_eq!(
            parse_chat_completion_answer(raw).unwrap(),
            "first second".to_string()
        );
    }

    #[test]
    fn extracts_ai_error_message_from_json_body() {
        assert_eq!(
            response_error_excerpt(r#"{"error":{"message":"model not found"}}"#),
            "model not found"
        );
        assert_eq!(response_error_excerpt(""), "empty response body");
    }

    #[test]
    fn detects_local_ai_endpoints() {
        assert!(is_local_http_endpoint(
            "http://127.0.0.1:8080/v1/chat/completions"
        ));
        assert!(is_local_http_endpoint(
            "http://localhost:8080/v1/chat/completions"
        ));
        assert!(is_local_http_endpoint(
            "http://[::1]:8080/v1/chat/completions"
        ));
        assert!(!is_local_http_endpoint(
            "https://api.openai.com/v1/chat/completions"
        ));
    }

    #[test]
    fn ai_request_rejects_remote_endpoint_when_local_only() {
        let provider = test_ai_provider("cloud", "Cloud AI", "https://api.openai.com/v1");
        let error = call_ai_provider(provider, "hello".to_string(), true).unwrap_err();

        assert!(error.contains("local_only"));
    }

    #[test]
    fn ai_request_requires_configured_api_key_env() {
        let mut provider = test_ai_provider("local", "Local AI", "http://127.0.0.1:8080/v1");
        provider.api_key_env =
            Some("VEYRA_TEST_AI_KEY_SHOULD_NOT_EXIST_47D0AFC1D9E44DD1".to_string());

        let error = call_ai_provider(provider, "hello".to_string(), false).unwrap_err();

        assert!(error.contains("not set"));
    }

    #[test]
    fn process_ai_providers_are_runnable_without_http_url() {
        let mut provider = test_ai_provider("minicpm5_npu", "MiniCPM5 NPU", "");
        provider.kind = AiProviderKind::Process;
        provider.command = "npu_chat.exe".to_string();

        assert!(provider_is_runnable(&provider));
    }

    #[test]
    fn ai_warmup_provider_uses_default_warm_process_provider() {
        let mut config = VeyraConfig::default();
        config.ai.enabled = true;
        config.ai.warmup_on_startup = true;
        config.ai.default_provider = "minicpm5_npu".to_string();

        let mut npu_provider = test_ai_provider("minicpm5_npu", "MiniCPM5 NPU", "");
        npu_provider.kind = AiProviderKind::Process;
        npu_provider.command = "npu_chat.exe".to_string();
        npu_provider.keep_warm = true;
        config.ai.providers = vec![
            test_ai_provider("local_http", "Local HTTP", "http://127.0.0.1:8080/v1"),
            npu_provider,
        ];

        assert_eq!(ai_warmup_provider(&config).unwrap().id, "minicpm5_npu");

        config.ai.warmup_on_startup = false;
        assert!(ai_warmup_provider(&config).is_none());
    }

    #[test]
    fn default_ai_template_does_not_warm_on_startup() {
        let config = VeyraConfig::from_toml_str(DEFAULT_AI_TOML).unwrap();

        assert!(!config.ai.warmup_on_startup);
    }

    #[test]
    fn process_ai_prompt_uses_minicpm_chatml_shape() {
        let provider = AiProvider {
            kind: AiProviderKind::Process,
            ..Default::default()
        };
        let prompt = format_process_ai_prompt(&provider, "Count to three.");

        assert!(prompt.starts_with("<s><|im_start|>system\n"));
        assert!(prompt.contains("<|im_start|>user\nCount to three.<|im_end|>"));
        assert!(prompt.ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\n"));
    }

    #[test]
    fn process_ai_answer_trims_stop_tokens() {
        let provider = AiProvider {
            kind: AiProviderKind::Process,
            ..Default::default()
        };
        assert_eq!(
            clean_process_ai_answer(&provider, "answer text<|im_end|>\nignored"),
            "answer text"
        );
        assert_eq!(
            clean_process_ai_answer(&provider, "<think>\ninternal\n</think>\n\nfinal answer"),
            "final answer"
        );
    }

    #[test]
    fn aurora_quick_result_parses_commands() {
        let next = aurora_search_result("aurora next").unwrap();
        assert_eq!(next.item.id, "quick.aurora.next");
        let next_action = next.item.actions.first().unwrap();
        assert_eq!(next_action.kind, ActionKind::AuroraIpc);
        assert_eq!(next_action.command, Some(r#"{"type":"next"}"#.to_string()));

        let wp_prev = aurora_search_result("wp prev").unwrap();
        assert_eq!(wp_prev.item.id, "quick.aurora.prev");
        assert_eq!(
            wp_prev.item.actions.first().unwrap().command,
            Some(r#"{"type":"prev"}"#.to_string())
        );

        let pause = aurora_search_result("aurora pause").unwrap();
        assert_eq!(
            pause.item.actions.first().unwrap().command,
            Some(r#"{"type":"pause","data":{}}"#.to_string())
        );

        let set = aurora_search_result("aurora set C:\\Pictures\\foo.jpg").unwrap();
        assert_eq!(set.item.id, "quick.aurora.set");
        let set_cmd = set.item.actions.first().unwrap().command.as_ref().unwrap();
        assert!(set_cmd.contains("\"type\":\"set\""));
        assert!(set_cmd.contains("C:\\\\Pictures\\\\foo.jpg"));

        let folder = aurora_search_result("aurora folder C:\\Wallpapers").unwrap();
        let folder_cmd = folder
            .item
            .actions
            .first()
            .unwrap()
            .command
            .as_ref()
            .unwrap();
        assert!(folder_cmd.contains("\"type\":\"set_folder\""));

        assert!(aurora_search_result("aurora unknown").is_none());
        assert!(aurora_search_result("aurora").is_none());
        // `rate` was removed: the daemon has no `Rate` variant (rating is
        // `content_rate`, which needs a content target veyra does not have).
        assert!(aurora_search_result("aurora rate 4").is_none());
        assert!(aurora_search_result("wp rate 5").is_none());
    }

    #[test]
    fn clamps_appearance_values_for_ui() {
        let mut config = VeyraConfig::default();

        config.appearance.font_size = 2;
        config.appearance.max_results = 0;
        config.appearance.opacity = 0.1;
        assert_eq!(effective_font_size(&config), 12.0);
        assert_eq!(effective_max_results(&config), 10);
        assert_eq!(alpha_for_opacity(&config, 200), 70);

        config.appearance.font_size = 80;
        config.appearance.max_results = 90;
        config.appearance.opacity = 2.0;
        assert_eq!(effective_font_size(&config), 22.0);
        assert_eq!(effective_max_results(&config), 24);
        assert_eq!(alpha_for_opacity(&config, 200), 200);
    }

    #[test]
    fn keyboard_selection_steps_and_clamps_within_shown_results() {
        // Down advances and stops at the last visible row.
        assert_eq!(step_selection(SelectionDirection::Down, 0, 3), 1);
        assert_eq!(step_selection(SelectionDirection::Down, 1, 3), 2);
        assert_eq!(step_selection(SelectionDirection::Down, 2, 3), 2);

        // Up moves back but never wraps around or underflows at the top.
        assert_eq!(step_selection(SelectionDirection::Up, 2, 3), 1);
        assert_eq!(step_selection(SelectionDirection::Up, 0, 3), 0);

        // Out-of-range selections (stale after the result list shrank) are
        // pulled back inside the visible range before stepping.
        assert_eq!(step_selection(SelectionDirection::Down, 7, 3), 2);
        assert_eq!(step_selection(SelectionDirection::Up, 7, 3), 1);
        assert_eq!(step_selection(SelectionDirection::Down, 7, 1), 0);

        // A single-row list is a fixed point for both directions.
        assert_eq!(step_selection(SelectionDirection::Down, 0, 1), 0);
        assert_eq!(step_selection(SelectionDirection::Up, 0, 1), 0);

        // An empty list leaves the selection reset to 0.
        assert_eq!(step_selection(SelectionDirection::Down, 2, 0), 0);
        assert_eq!(step_selection(SelectionDirection::Up, 2, 0), 0);
    }

    #[test]
    fn window_layout_scales_with_monitor_size() {
        let small_monitor = Vec2::new(1366.0, 768.0);
        let launcher =
            window_size_for_monitor(WindowLayoutMode::LauncherCompact, small_monitor, 1.0, 0);
        assert_vec2_near(launcher, Vec2::new(680.0, 76.0));
        assert_pos2_near(
            window_position(
                WindowLayoutMode::LauncherCompact,
                small_monitor,
                launcher,
                1.0,
            ),
            Pos2::new(343.0, 96.0),
        );

        let hidpi_monitor = Vec2::new(800.0, 600.0);
        let hidpi_physical_monitor = Vec2::new(1600.0, 1200.0);
        let launcher =
            window_size_for_monitor(WindowLayoutMode::LauncherCompact, hidpi_monitor, 2.0, 0);
        assert_vec2_near(launcher, Vec2::new(680.0, 76.0));
        assert_pos2_near(
            window_position(
                WindowLayoutMode::LauncherCompact,
                hidpi_monitor,
                launcher,
                2.0,
            ),
            Pos2::new(60.0, 96.0),
        );

        let results =
            window_size_for_monitor(WindowLayoutMode::LauncherResults, hidpi_monitor, 2.0, 4);
        assert_vec2_near(results, Vec2::new(680.0, 322.0));

        let ai = window_size_for_monitor(WindowLayoutMode::LauncherAi, hidpi_monitor, 2.0, 0);
        assert_vec2_near(ai, Vec2::new(680.0, 408.0));
        assert!(ai.x * 2.0 <= hidpi_physical_monitor.x - 48.0);
        assert!(ai.y * 2.0 <= hidpi_physical_monitor.y - 48.0);
    }

    #[test]
    #[ignore = "manual diagnostic: run with -- --ignored --nocapture"]
    fn diagnostic_real_runtime_search() {
        let profile_dir = profile_dir("Veyra");
        let started = std::time::Instant::now();
        let runtime = load_runtime_state(&profile_dir);
        let queries = [
            "code",
            "vscode",
            "visual studio code",
            "notepad",
            "calc",
            "terminal",
            "cmd",
            "settings",
            "explorer",
            "task manager",
        ];

        println!(
            "\nLoaded {} catalog items in {} ms\n",
            runtime.catalog.len(),
            started.elapsed().as_millis()
        );
        for query in queries {
            let results = search(&runtime.catalog, query);
            let shown = results.len().min(5);
            println!(
                "Query: '{query}' -> {} results (showing top {shown})",
                results.len()
            );
            for result in results.iter().take(shown) {
                println!(
                    "  [{:>3}] [{:16}] {} -- {}",
                    result.score,
                    result.item.source,
                    result.item.label,
                    result.item.subtitle.as_deref().unwrap_or("")
                );
            }
            println!();
        }
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

    #[test]
    fn evaluates_quick_calculator_expressions() {
        assert_eq!(format_number(evaluate_expression("2+2").unwrap()), "4");
        assert_eq!(
            format_number(evaluate_expression("2 + 3 * 4").unwrap()),
            "14"
        );
        assert_eq!(
            format_number(evaluate_expression("(2 + 3) * 4").unwrap()),
            "20"
        );
        assert_eq!(format_number(evaluate_expression("10 / 4").unwrap()), "2.5");
        assert!(evaluate_expression("10 / 0").is_none());
        assert!(calculator_search_result("notes").is_none());
        assert_eq!(format_number(evaluate_expression("2^10").unwrap()), "1024");
        assert_eq!(
            format_number(evaluate_expression("(1 + 2)^3").unwrap()),
            "27"
        );
    }

    #[test]
    fn calculator_result_copies_answer() {
        let result = calculator_search_result("2+2").unwrap();
        let action = result.item.actions.first().unwrap();

        assert_eq!(result.item.label, "2+2 = 4");
        assert_eq!(action.kind, ActionKind::ToolCall);
        assert_eq!(action.id, COPY_TO_CLIPBOARD_ACTION_ID);
        assert_eq!(action.command.as_deref(), Some("4"));
    }

    #[test]
    fn web_search_result_opens_search_url() {
        let result = web_search_result("launchers for windows").unwrap();
        let action = result.item.actions.first().unwrap();

        assert_eq!(result.item.category, ItemCategory::Web);
        assert_eq!(result.item.source, "quick");
        assert_eq!(action.kind, ActionKind::OpenUrl);
        assert_eq!(
            action.command.as_deref(),
            Some("https://www.google.com/search?q={query}")
        );
        assert!(web_search_result("   ").is_none());
    }

    #[test]
    fn web_search_alias_strips_alias_from_query() {
        let mut config = VeyraConfig::default();
        config.web_search.push(WebSearchEntry {
            id: "github.code".to_string(),
            alias: "gh".to_string(),
            label: "GitHub Code".to_string(),
            url: "https://github.com/search?q={query}&type=code".to_string(),
        });

        let result = web_search_alias_result(&config, "gh rust serde").unwrap();
        let action = result.item.actions.first().unwrap();

        assert_eq!(result.item.label, "Search GitHub Code for \"rust serde\"");
        assert_eq!(
            action.command.as_deref(),
            Some("https://github.com/search?q={query}&type=code")
        );
        assert_eq!(action.args, vec!["rust serde"]);
        assert!(web_search_alias_result(&config, "gh").is_none());
    }

    #[test]
    fn dedupes_results_by_item_id_and_keeps_first_match() {
        let first = SearchResult {
            item: CatalogItem::new("same", "First", ItemCategory::Tool, "test"),
            score: 100,
        };
        let second = SearchResult {
            item: CatalogItem::new("same", "Second", ItemCategory::Tool, "test"),
            score: 500,
        };
        let third = SearchResult {
            item: CatalogItem::new("other", "Other", ItemCategory::Tool, "test"),
            score: 10,
        };

        let results = dedupe_results(vec![first, second, third]);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].item.label, "First");
        assert_eq!(results[1].item.id, "other");
    }

    #[test]
    fn launch_history_records_and_boosts_repeated_items() {
        let item = CatalogItem::new(
            "app.wireguard",
            "WireGuard",
            ItemCategory::App,
            "start_menu",
        );
        let other = CatalogItem::new("app.other", "Other", ItemCategory::App, "start_menu");
        let mut history = LaunchHistory::default();

        history.record(&item, "wire");
        history.record(&item, "wire");

        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].launch_count, 2);
        assert!(history.boost_for(&item, "wire") > history.boost_for(&other, "wire"));
        assert!(history.boost_for(&item, "wire") > history.boost_for(&item, "vpn"));
    }

    #[test]
    fn launch_history_round_trips_to_profile_file() {
        let profile = temp_profile_dir();
        let item = CatalogItem::new(
            "app.wireguard",
            "WireGuard",
            ItemCategory::App,
            "start_menu",
        );
        let mut history = LaunchHistory::default();
        history.record(&item, "wire");

        save_launch_history(&profile, &history).unwrap();
        let loaded = load_launch_history(&profile);

        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].item_id, "app.wireguard");
        assert_eq!(loaded.entries[0].last_query, "wire");

        fs::remove_dir_all(profile).ok();
    }

    #[test]
    fn recent_launch_results_use_current_catalog_items() {
        let mut history = LaunchHistory::default();
        let wireguard = CatalogItem::new(
            "app.wireguard",
            "WireGuard",
            ItemCategory::App,
            "start_menu",
        )
        .subtitle("VPN tunnel manager");
        let missing = CatalogItem::new("app.missing", "Missing", ItemCategory::App, "start_menu");
        history.record(&missing, "missing");
        history.record(&wireguard, "wire");

        let results = recent_launch_results_from(&[wireguard], &history, 12);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].item.id, "app.wireguard");
        assert_eq!(
            results[0].item.subtitle.as_deref(),
            Some("Recent - VPN tunnel manager")
        );
    }

    #[test]
    fn plugin_entries_become_tool_catalog_items() {
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

    fn temp_profile_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("veyra-app-profile-test-{nanos}"));
        path
    }

    fn assert_vec2_near(actual: Vec2, expected: Vec2) {
        assert!((actual.x - expected.x).abs() < 0.01, "{actual:?}");
        assert!((actual.y - expected.y).abs() < 0.01, "{actual:?}");
    }

    fn assert_pos2_near(actual: Pos2, expected: Pos2) {
        assert!((actual.x - expected.x).abs() < 0.01, "{actual:?}");
        assert!((actual.y - expected.y).abs() < 0.01, "{actual:?}");
    }

    fn test_ai_provider(id: &str, label: &str, base_url: &str) -> AiProvider {
        AiProvider {
            id: id.to_string(),
            label: label.to_string(),
            kind: AiProviderKind::OpenAiCompatible,
            base_url: base_url.to_string(),
            model: format!("{id}:model"),
            command: String::new(),
            args: Vec::new(),
            keep_warm: false,
            api_key_env: None,
            local_only: true,
            enabled: true,
            timeout_ms: 60_000,
            supports_streaming: true,
            supports_tools: true,
            context_limit_tokens: None,
            ..Default::default()
        }
    }

    fn test_ai_response(prompt: &str, result: AiResponseResult) -> AiResponse {
        let provider = test_ai_provider("test", "Test AI", "http://127.0.0.1:8910/v1");
        AiResponse {
            generation: 1,
            session_id: 42,
            turn_index: 1,
            prompt: prompt.to_string(),
            provider_label: ai_provider_label(&provider),
            request: ai_request_info(&provider, 0, 0, 0, 8),
            elapsed_ms: Some(12),
            tool_suggestions: Vec::new(),
            eval: None,
            result,
        }
    }

    fn test_app_shell() -> VeyraApp {
        let catalog = seed_catalog();
        let search_index = SearchIndex::new(&catalog);
        let (runtime_sender, runtime_events) = mpsc::channel();
        let (plugin_suggestion_sender, plugin_suggestion_events) = mpsc::channel();
        let (ai_response_sender, ai_response_events) = mpsc::channel();
        let (ai_warmup_sender, ai_warmup_events) = mpsc::channel();
        let (_hotkey_sender, hotkey_events) = mpsc::channel();
        let (_copilot_sender, copilot_events) = mpsc::channel();

        VeyraApp {
            query: String::new(),
            catalog,
            search_index,
            show_settings: false,
            settings_page: SettingsPage::General,
            window_visible: true,
            focus_query: false,
            selected: 0,
            last_status: None,
            profile_dir: temp_profile_dir(),
            config: VeyraConfig::default(),
            launch_history: LaunchHistory::default(),
            load_messages: Vec::new(),
            path_item_count: 0,
            start_menu_item_count: 0,
            file_catalog_item_count: 0,
            file_catalog_skipped_paths: 0,
            plugin_process_item_count: 0,
            plugin_json_rpc_item_count: 0,
            tool_manifest_item_count: 0,
            plugin_error_count: 0,
            runtime_load_ms: 0,
            runtime_refreshing: false,
            runtime_sender,
            runtime_events,
            plugin_suggestion_items: Vec::new(),
            plugin_suggestion_query: String::new(),
            plugin_suggestion_generation: 0,
            plugin_suggestion_refreshing: false,
            plugin_suggestion_pending_query: String::new(),
            plugin_suggestion_due_at: None,
            plugin_suggestion_sender,
            plugin_suggestion_events,
            ai_response: None,
            ai_panel_expanded: false,
            show_ai_conversation: false,
            ai_conversation_input: String::new(),
            ai_focus_conversation_input: false,
            ai_conversation_messages: Vec::new(),
            ai_session_id: 1,
            ai_session_provider_id: None,
            ai_turn_index: 0,
            ai_request_generation: 0,
            ai_request_running: false,
            ai_response_sender,
            ai_response_events,
            ai_warmup_generation: 0,
            ai_warmup_running: false,
            ai_warmup_sender,
            ai_warmup_events,
            hotkeys: HotkeyRuntime::empty_for_tests(hotkey_events, copilot_events),
            layout_mode: None,
            layout_size: None,
            native_center_settle_frames: 0,
        }
    }
}
