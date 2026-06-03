use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;
use veyra_core::{Action, ActionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Macos,
    Other,
}

pub fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Other
    }
}

pub fn profile_dir(app_name: &str) -> PathBuf {
    if let Some(portable) = portable_profile_dir() {
        return portable;
    }

    match current_platform() {
        Platform::Windows => std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(app_name),
        Platform::Linux | Platform::Macos => std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(app_name.to_ascii_lowercase()),
        Platform::Other => PathBuf::from(".").join(app_name),
    }
}

fn portable_profile_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let portable = exe.parent()?.join("portable");
    portable.is_dir().then_some(portable)
}

pub fn execute_action(action: &Action) -> Result<(), PlatformError> {
    match action.kind {
        ActionKind::Launch | ActionKind::ShellCommand | ActionKind::OpenFile => {
            spawn_command(action)
        }
        ActionKind::OpenUrl => {
            let url = action
                .command
                .as_deref()
                .ok_or(PlatformError::MissingCommand)?;
            open_url(url)
        }
        ActionKind::AiPrompt | ActionKind::ToolCall => Err(PlatformError::UnsupportedAction {
            kind: format!("{:?}", action.kind),
        }),
    }
}

fn spawn_command(action: &Action) -> Result<(), PlatformError> {
    let command = action
        .command
        .as_deref()
        .ok_or(PlatformError::MissingCommand)?;

    Command::new(command)
        .args(&action.args)
        .spawn()
        .map(|_| ())
        .map_err(|source| PlatformError::SpawnFailed {
            command: command.to_string(),
            source,
        })
}

fn open_url(url: &str) -> Result<(), PlatformError> {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|source| PlatformError::SpawnFailed {
            command: url.to_string(),
            source,
        })
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("action has no command")]
    MissingCommand,
    #[error("unsupported action kind: {kind}")]
    UnsupportedAction { kind: String },
    #[error("failed to spawn `{command}`")]
    SpawnFailed {
        command: String,
        source: std::io::Error,
    },
}
