use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use eframe::egui;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use veyra_core::config::VeyraConfig;

#[cfg(windows)]
use std::sync::OnceLock;
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

pub(crate) const COPILOT_TOGGLE_HOTKEY: &str = "Win+Shift+F23";
pub(crate) const FALLBACK_TOGGLE_HOTKEY: &str = "Alt+Space";
const COPILOT_HOOK_LABEL: &str = "Copilot hook";

pub(crate) struct HotkeyRuntime {
    manager: Option<GlobalHotKeyManager>,
    registered_hotkeys: Vec<HotKey>,
    toggle_hotkey_ids: Vec<u32>,
    registered_labels: Vec<String>,
    pub(crate) events: mpsc::Receiver<GlobalHotKeyEvent>,
    pub(crate) copilot_events: mpsc::Receiver<()>,
    copilot_hook_registered: bool,
}

impl HotkeyRuntime {
    pub(crate) fn new(ctx: &egui::Context) -> Self {
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

    #[cfg(test)]
    pub(crate) fn empty_for_tests(
        events: mpsc::Receiver<GlobalHotKeyEvent>,
        copilot_events: mpsc::Receiver<()>,
    ) -> Self {
        Self {
            manager: None,
            registered_hotkeys: Vec::new(),
            toggle_hotkey_ids: Vec::new(),
            registered_labels: Vec::new(),
            events,
            copilot_events,
            copilot_hook_registered: false,
        }
    }

    pub(crate) fn register_toggle_hotkeys(&mut self, config: &VeyraConfig) -> Vec<String> {
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

    pub(crate) fn is_toggle_event(&self, event: GlobalHotKeyEvent) -> bool {
        event.state == HotKeyState::Pressed && self.toggle_hotkey_ids.contains(&event.id)
    }

    pub(crate) fn registered_label(&self, label: &str) -> bool {
        self.registered_labels.iter().any(|value| value == label)
    }

    pub(crate) fn registered_labels(&self) -> String {
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

    pub(crate) fn has_registered_toggle(&self) -> bool {
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

pub(crate) fn toggle_hotkey_candidates(configured: &str) -> Vec<String> {
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

pub(crate) fn parse_global_hotkey(
    hotkey: &str,
) -> Result<HotKey, global_hotkey::hotkey::HotKeyParseError> {
    normalize_global_hotkey(hotkey).parse()
}

pub(crate) fn normalize_global_hotkey(hotkey: &str) -> String {
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
