use std::process::Command;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;
use veyra_core::{Action, ActionKind, CatalogItem, ItemCategory};

#[cfg(windows)]
const WINDOWS_DEFAULT_PATHEXT: [&str; 11] = [
    ".COM", ".EXE", ".BAT", ".CMD", ".VBS", ".VBE", ".JS", ".JSE", ".WSF", ".WSH", ".MSC",
];

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

pub fn discover_path_executables() -> Vec<CatalogItem> {
    let path_dirs = split_path_entries();
    if path_dirs.is_empty() {
        return Vec::new();
    }

    let extensions = executable_extensions();
    let mut seen_by_path = HashSet::new();
    let mut seen_by_name = HashSet::new();
    let mut items = Vec::new();

    for dir in path_dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !is_executable_file(&path, &extensions) {
                continue;
            }

            let file_name = match path.file_name().and_then(|value| value.to_str()) {
                Some(file_name) => file_name,
                None => continue,
            };

            let normalized_name = normalize_executable_name(file_name);
            if !seen_by_name.insert(normalized_name.clone()) {
                continue;
            }

            let path_key = normalize_path_key(&path);
            if !seen_by_path.insert(path_key.clone()) {
                continue;
            }

            let command = path.to_string_lossy().to_string();
            items.push(
                CatalogItem::new(
                    format!("path_executable:{path_key}"),
                    file_name,
                    ItemCategory::Command,
                    "path",
                )
                .subtitle(command.clone())
                .keywords([normalized_name])
                .action(Action::launch(command)),
            );
        }
    }

    items
}

fn split_path_entries() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|entry| entry.is_dir())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn executable_extensions() -> Vec<String> {
    #[cfg(windows)]
    {
        let configured = std::env::var_os("PATHEXT")
            .as_ref()
            .map(|value| value.to_string_lossy())
            .as_deref()
            .map(parse_executable_extensions)
            .unwrap_or_default();

        if !configured.is_empty() {
            return configured;
        }

        WINDOWS_DEFAULT_PATHEXT
            .iter()
            .map(|item| item.to_string())
            .collect()
    }

    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(any(windows, test))]
fn parse_executable_extensions(value: &str) -> Vec<String> {
    let mut extensions = Vec::new();

    for entry in value.split(';') {
        let extension = entry.trim();
        if extension.is_empty() {
            continue;
        }

        let mut normalized = extension.to_ascii_uppercase();
        if !normalized.starts_with('.') {
            normalized.insert(0, '.');
        }
        if !extensions.contains(&normalized) {
            extensions.push(normalized);
        }
    }

    extensions
}

fn normalize_executable_name(value: &str) -> String {
    #[cfg(windows)]
    return value.to_ascii_lowercase();

    #[cfg(not(windows))]
    return value.to_string();
}

fn normalize_path_key(path: &Path) -> String {
    let key = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    #[cfg(windows)]
    return key.to_ascii_lowercase();

    #[cfg(not(windows))]
    return key;
}

#[cfg(windows)]
fn is_executable_file(path: &Path, extensions: &[String]) -> bool {
    if !path.is_file() {
        return false;
    }

    let extension = match path.extension().and_then(|value| value.to_str()) {
        Some(extension) => extension,
        None => return false,
    };
    let extension = format!(".{}", extension.to_ascii_uppercase());
    extensions.iter().any(|entry| entry == &extension)
}

#[cfg(not(windows))]
fn is_executable_file(path: &Path, _extensions: &[String]) -> bool {
    #[cfg(unix)]
    {
        let Ok(metadata) = fs::metadata(path) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        path.is_file()
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use super::{normalize_executable_name, normalize_path_key, parse_executable_extensions};

    #[test]
    fn parses_pathext_with_case_and_spaces() {
        let extensions = parse_executable_extensions(".exE; .cmd; .Exe;; .bat");
        assert_eq!(extensions, vec![".EXE", ".CMD", ".BAT"]);
    }

    #[test]
    fn parses_empty_pathext_to_empty_list() {
        assert!(parse_executable_extensions("").is_empty());
    }

    #[test]
    fn normalize_executable_name_matches_platform_case_rules() {
        #[cfg(windows)]
        assert_eq!(normalize_executable_name("FooBar.CMD"), "foobar.cmd");
        #[cfg(not(windows))]
        assert_eq!(normalize_executable_name("FooBar.CMD"), "FooBar.CMD");
    }

    #[test]
    fn path_keys_are_stable_for_duplicate_paths() {
        let path = PathBuf::from("a").join("b").join("binary");
        assert_eq!(normalize_path_key(&path), normalize_path_key(&path));
    }

    #[test]
    fn dedupe_logic_prefers_first_occurrence() {
        let candidates = [
            PathBuf::from("first/cmd.exe"),
            PathBuf::from("second/cmd.exe"),
            PathBuf::from("first/cmd.exe"),
        ];

        let mut seen_names = HashSet::new();
        let mut seen_paths = HashSet::new();
        let mut selected = Vec::new();

        for candidate in &candidates {
            let file_name = candidate
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap();
            let name_key = normalize_executable_name(file_name);
            let path_key = normalize_path_key(candidate);

            if seen_names.insert(name_key) && seen_paths.insert(path_key) {
                selected.push(candidate);
            }
        }

        assert_eq!(selected.len(), 1);
    }
}
